//! Apple I machine: wraps CPU, bus, display, keyboard, and board-level
//! timing into a master-tick run loop driven by the original 14.31818 MHz
//! crystal model.
//!
//! One [`Apple1::tick`] advances exactly one master tick.  Only real CPU
//! Φ2 edges that are not suppressed by refresh produce a
//! [`hesper_cpu6502::Cycle`]; the CPU holds Φ1 and performs no bus access
//! during refresh.

use hesper_cpu6502::{ClockPhase, Cpu, CpuError, StepKind};

use crate::bus::{Apple1Bus, RomSizeError};
use crate::display::Display;
use crate::keyboard::Keyboard;
use crate::timing::Timing;

/// Number of real CPU cycles to hold the physical RESET line asserted
/// before releasing it.  The CPU samples the line once per cycle; four
/// cycles gives comfortable margin.
pub const RESET_HOLD_CYCLES: u64 = 4;

/// Upper bound on real CPU cycles to run after releasing RESET while
/// waiting for the CPU's physical reset sequence to report completion.
pub const RESET_COMPLETION_BUDGET: u64 = 64;

/// Length of the B3 one-shot's CB1 pulse, in master ticks: the schematic
/// labels it 3.5 µs, quantised up to the next crystal period
/// (`ceil(3.5e-6 * 14_318_180) = 51`).
///
/// This is a nominal digital stand-in for the 74123's RC timing, not a
/// tolerance or temperature characterisation of the real part.
pub const B3_PULSE_TICKS: u64 = 51;

/// What happened on one master tick.
#[derive(Debug, Clone)]
pub struct Tick {
    /// A real CPU bus cycle completed on this tick, or `None` when no CPU
    /// bus access occurred (Φ1-only tick, refresh-suppressed Φ2, or
    /// between-cycle idle).
    pub cpu: Option<hesper_cpu6502::Cycle>,
    /// The board's RF line is asserted (refresh cycle suppressing Φ2).
    pub refresh: bool,
    /// A full video frame completed on this tick.
    pub frame_completed: bool,
}

/// A complete Apple I emulator: CPU, memory bus, display, keyboard, and
/// board-level clock model.
pub struct Apple1 {
    cpu: Cpu,
    bus: Apple1Bus,
    display: Display,
    keyboard: Keyboard,
    timing: Timing,
    /// Real CPU bus cycles completed (excluding refresh-suppressed Φ2).
    cpu_cycle_count: u64,
    reset_line: bool,
    /// Master ticks left in the B3 one-shot's CB1 pulse; zero when idle.
    b3_ticks: u64,
}

impl Apple1 {
    /// Create an Apple I machine with the given 256-byte Woz Monitor ROM.
    pub fn new(rom: &[u8]) -> Result<Self, RomSizeError> {
        Ok(Self {
            cpu: Cpu::new(),
            bus: Apple1Bus::new(rom)?,
            display: Display::new(),
            keyboard: Keyboard::new(),
            timing: Timing::new(),
            cpu_cycle_count: 0,
            reset_line: false,
            b3_ticks: 0,
        })
    }

    /// Advance exactly one master tick.  Returns what happened on that
    /// tick: a real CPU bus cycle (if Φ2 was not suppressed), the refresh
    /// line state, and whether a video frame completed.
    ///
    /// # Ordering
    ///
    /// Each master tick samples old state, identifies edges, executes at
    /// most one CPU bus access, and then commits new state — following the
    /// original board's synchronous edge order rather than assigning one
    /// stage's new output through another:
    ///
    /// 1. The board clock advances; Φ1, Φ2, MEMΦ, LINEΦ, and the
    ///    character clock edges are identified from the old timing state.
    /// 2. On a character-clock edge the keyboard presents its key, and the
    ///    terminal advances: MEMΦ shifts the carousel, LINEΦ reloads the
    ///    line buffer, and the C7 stage samples DA.
    /// 3. An accepted character asserts RDA, which starts the B3 one-shot
    ///    and pulls CB1 low. The one-shot runs on board time, so it
    ///    finishes even while the CPU is stopped for refresh.
    /// 4. On a Φ1 edge the CPU enters Φ2 (no bus access). A Φ1 already
    ///    held from a refresh is not re-entered.
    /// 5. On a Φ2 edge the PIA's enable rises — firing any strobe an ORB
    ///    write asked for — PB7 is refreshed with the terminal's DA line,
    ///    and the CPU performs its one bus access.
    /// 6. During refresh the Φ2 edge is skipped: the CPU stays in Φ2, the
    ///    PIA sees no enable, and no bus access occurs.
    pub fn tick(&mut self) -> Result<Tick, CpuError> {
        let ev = self.timing.tick();

        // --- B3 one-shot ---
        //
        // Counted before the video board runs, so a pulse triggered on this
        // tick lasts exactly its nominal length rather than one tick less.
        if self.b3_ticks > 0 {
            self.b3_ticks -= 1;
            if self.b3_ticks == 0 {
                self.bus.pia_mut().set_cb1(false);
            }
        }

        // --- Character-clock edge: keyboard and video board ---
        if ev.char_edge {
            if !self.bus.pia().reset_asserted() {
                self.keyboard.tick(self.bus.pia_mut());
            }

            // The terminal's DA input is CB2 through the board's inverter,
            // and its data inputs are the seven character lines.
            let (data, da) = {
                let pia = self.bus.pia();
                (pia.data_lines(), !pia.cb2_level())
            };
            self.display.clock(&ev, data, da);

            if self.display.take_rda() {
                // The terminal took the character: B3 pulses CB1, which is
                // the acknowledge software waits for (directly, or through
                // PB7 once CB1 releases CB2).
                self.b3_ticks = B3_PULSE_TICKS;
                self.bus.pia_mut().set_cb1(true);
            }
        }

        // --- Φ1 edge: the CPU enters Φ2, no bus access ---
        if ev.phi1_edge && self.cpu.next_clock_phase() == ClockPhase::Phi1 {
            self.cpu.half_cycle(&mut self.bus)?;
        }

        // --- Φ2 edge ---
        let mut cpu_cycle = None;
        if ev.phi2_edge && !ev.refresh {
            // The PIA's enable rises with Φ2. A strobe requested by an
            // earlier ORB write fires now, before this cycle's access.
            self.bus.pia_mut().e_rising_edge();

            // PB7 carries DA, so the cycle's Port B read sees the current
            // handshake state.
            let da = !self.bus.pia().cb2_level();
            self.bus.pia_mut().set_display_ready(da);

            if self.cpu.next_clock_phase() == ClockPhase::Phi2 {
                let result = self.cpu.half_cycle(&mut self.bus);
                // The bus access really happened whichever way the opcode
                // turned out, so the cycle counts either way.
                self.cpu_cycle_count += 1;
                if let Some(cycle) = result? {
                    cpu_cycle = Some(cycle);
                }
            }
        }

        Ok(Tick {
            cpu: cpu_cycle,
            refresh: ev.refresh,
            frame_completed: ev.frame_completed,
        })
    }

    /// Advance the machine by up to `ticks` master ticks.  Returns any
    /// display characters that completed during this batch.  On error the
    /// completed-output queue is preserved.
    pub fn run_ticks(&mut self, ticks: u64) -> Result<Vec<u8>, CpuError> {
        for _ in 0..ticks {
            self.tick()?;
        }
        Ok(self.display.drain_output())
    }

    /// Take the display characters that have finished shifting out since
    /// the last drain.
    pub fn drain_output(&mut self) -> Vec<u8> {
        self.display.drain_output()
    }

    /// Push a single character into the keyboard queue.
    /// Masks to seven bits and uppercases ASCII letters.
    pub fn type_char(&mut self, c: u8) {
        self.keyboard.type_char(c);
    }

    /// Push each byte of `s` into the keyboard queue.
    /// Each byte is masked to seven bits and ASCII letters are uppercased.
    pub fn type_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.keyboard.type_char(b);
        }
    }

    /// Drive the shared physical RESET line that the 6502's `RES` pin and
    /// the PIA's own RESET pin are both tied to on real Apple I hardware.
    ///
    /// Asserting it clears the PIA's registers, resynchronizes the
    /// keyboard strobe against that cleared PIA, and takes the CPU's reset
    /// line low.  RAM, the video screen, and in-flight display timing are
    /// untouched.
    pub fn set_reset_line(&mut self, asserted: bool) {
        if self.reset_line == asserted {
            return;
        }
        self.reset_line = asserted;
        if asserted {
            self.bus.pia_mut().set_reset_line(true);
            self.keyboard.resync(self.bus.pia_mut());
            self.cpu.set_reset_line(true);
        } else {
            self.bus.pia_mut().set_reset_line(false);
            self.cpu.set_reset_line(false);
        }
    }

    /// Whether the physical RESET line is currently held asserted.
    pub fn reset_line_asserted(&self) -> bool {
        self.reset_line
    }

    /// Synchronous convenience RESET: assert the line for
    /// [`RESET_HOLD_CYCLES`] real CPU cycles, release it, then run until
    /// the CPU reports its physical reset sequence complete.
    ///
    /// Counts only real CPU bus cycles (not master ticks); refresh slots
    /// extend the wall-clock time without advancing the hold count.
    ///
    /// Returns [`CpuError::CycleBudgetExceeded`] if the reset sequence
    /// does not report completion within [`RESET_COMPLETION_BUDGET`] real
    /// CPU cycles of release.
    pub fn reset(&mut self) -> Result<(), CpuError> {
        self.set_reset_line(true);

        // Hold for RESET_HOLD_CYCLES real CPU cycles.
        let mut held = 0u64;
        while held < RESET_HOLD_CYCLES {
            let tick = self.tick()?;
            if tick.cpu.is_some() {
                held += 1;
            }
        }

        self.set_reset_line(false);

        // Wait for completion: up to RESET_COMPLETION_BUDGET real CPU
        // cycles after release.  Refresh clocks and Φ1 halves extend the
        // wall-clock time without advancing the count.
        let mut elapsed = 0u64;
        while elapsed < RESET_COMPLETION_BUDGET {
            let tick = self.tick()?;
            if let Some(cycle) = tick.cpu {
                elapsed += 1;
                if let Some(step) = cycle.completed
                    && step.kind == StepKind::Reset
                {
                    return Ok(());
                }
            }
        }
        Err(CpuError::CycleBudgetExceeded {
            address: self.cpu.registers().pc,
            budget: RESET_COMPLETION_BUDGET,
        })
    }

    /// CLEAR SCREEN: the Apple I keyboard's second pushbutton.  Blanks
    /// the 40x24 screen and homes the cursor, running no CPU cycle and
    /// touching no other state.
    pub fn clear_screen(&mut self) {
        self.display.clear_screen();
    }

    /// Total master ticks since creation or last machine recreate.
    pub fn master_ticks(&self) -> u64 {
        self.timing.master_ticks()
    }

    /// Real CPU bus cycles completed (excluding refresh-suppressed Φ2
    /// and Φ1-only ticks).
    pub fn cpu_cycles(&self) -> u64 {
        self.cpu_cycle_count
    }

    /// Completed video frames since creation.
    pub fn video_frames(&self) -> u64 {
        self.timing.frames()
    }

    /// Whether any I/O is still in flight: unread keyboard input, an
    /// unfinished PIA output handshake, the B3 one-shot's CB1 pulse, or a
    /// video control sequence (carriage-return fill, pending scroll).
    /// Text already on the screen and the free-running scan are not
    /// pending work.
    pub fn io_pending(&self) -> bool {
        self.keyboard.has_pending()
            || self.bus.pia().output_strobe_pending()
            || self.b3_ticks > 0
            || self.display.io_pending()
    }

    /// Read-only access to the CPU.
    pub fn cpu(&self) -> &Cpu {
        &self.cpu
    }

    /// Read-only access to the bus.
    pub fn bus(&self) -> &Apple1Bus {
        &self.bus
    }

    /// Mutable access to the bus (for loading programs, inspecting RAM).
    pub fn bus_mut(&mut self) -> &mut Apple1Bus {
        &mut self.bus
    }

    /// Read-only access to the display (screen contents, cursor position).
    pub fn display(&self) -> &Display {
        &self.display
    }

    /// Read-only access to the keyboard (pending input).
    pub fn keyboard(&self) -> &Keyboard {
        &self.keyboard
    }
}
