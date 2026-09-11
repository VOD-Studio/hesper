//! Apple I machine: wraps CPU, bus, display, and keyboard into a cycle‑batch
//! run loop.

use std::num::NonZeroU64;

use hesper_cpu6502::{Cpu, CpuError, Cycle, Direction, StepKind};

use crate::bus::{Apple1Bus, RomSizeError};
use crate::display::{DEFAULT_CYCLES_PER_CHAR, Display};
use crate::keyboard::Keyboard;

/// Cycles to hold the physical RESET line asserted before releasing it.
/// The falling clock phase samples the line once per cycle; a handful of
/// cycles gives comfortable margin over the single sample a real edge
/// needs, well short of modeling how long a human actually holds the
/// button.
pub const RESET_HOLD_CYCLES: u64 = 4;

/// Upper bound on cycles to run after releasing RESET while waiting for
/// the CPU's physical reset sequence to report completion. Matches the
/// fixed 64-cycle window used for the CPU's own revD RESET fixtures
/// (`crates/cpu6502/tests/data/README.md#物理-reset-对照`), which is far
/// more than the ~13-20 cycles the sequence actually takes.
pub const RESET_COMPLETION_BUDGET: u64 = 64;

/// A complete Apple I emulator: CPU, memory bus, display shift register,
/// and keyboard input.
pub struct Apple1 {
    cpu: Cpu,
    bus: Apple1Bus,
    display: Display,
    keyboard: Keyboard,
    total_cycles: u64,
    reset_line: bool,
}

impl Apple1 {
    /// Create an Apple I machine with the given 256‑byte Woz Monitor ROM and
    /// display speed (in CPU cycles per character).  Uses
    /// [`DEFAULT_CYCLES_PER_CHAR`] if `None`.
    pub fn new(rom: &[u8], cycles_per_char: Option<NonZeroU64>) -> Result<Self, RomSizeError> {
        Ok(Self {
            cpu: Cpu::new(),
            bus: Apple1Bus::new(rom)?,
            display: Display::new(cycles_per_char.unwrap_or(DEFAULT_CYCLES_PER_CHAR)),
            keyboard: Keyboard::new(),
            total_cycles: 0,
            reset_line: false,
        })
    }

    /// Run every device and the CPU through exactly one cycle: tick devices
    /// → update PIA input pins → execute one CPU cycle → deliver a `$D012`
    /// output-register write to the display. Returns the raw CPU
    /// [`hesper_cpu6502::Cycle`] so callers can inspect bus activity and
    /// completion; the machine itself keeps no execution history.
    ///
    /// While the physical RESET line is held the keyboard is not advanced
    /// (its strobe negotiation with a reset PIA is meaningless), but the
    /// display timer, the PIA's display input pins, and the CPU's real
    /// reset sequence all still run.
    ///
    /// The cycle counter increments before the CPU executes, so a cycle
    /// that ends in [`CpuError::UnsupportedOpcode`] still counts: its bus
    /// read really happened.
    pub fn cycle(&mut self) -> Result<Cycle, CpuError> {
        if !self.bus.pia().reset_asserted() {
            self.keyboard.tick(self.bus.pia_mut());
        }
        self.display.tick(self.bus.pia_mut());
        self.display.update_pia(self.bus.pia_mut());

        self.total_cycles += 1;
        let cycle = self.cpu.cycle(&mut self.bus)?;

        // A write to the Port B data address only reaches the video board
        // when the PIA is actually driving the seven display data lines;
        // the display never sees the raw CPU byte (see
        // `Pia6821::display_data`).
        if cycle.bus.address == 0xD012
            && cycle.bus.direction == Direction::Write
            && let Some(data) = self.bus.pia().display_data()
        {
            self.display.on_write(data);
        }

        Ok(cycle)
    }

    /// Advance the machine by up to `budget` CPU cycles.
    ///
    /// Each cycle runs [`Apple1::cycle`]. Returns any display characters
    /// that completed during this batch (`run_cycles(0)` runs no cycle and
    /// only collects what was already complete). Splitting the same total
    /// budget across several calls (mid-instruction, mid-RDY-wait, or mid
    /// physical RESET hold) yields the same device and CPU state as one
    /// call with the combined budget: the CPU's own sequencer state
    /// persists across calls, and every device is ticked exactly once per
    /// cycle regardless of batch boundaries.
    ///
    /// On error the completed-output queue is preserved; the caller can
    /// still take it with [`Apple1::drain_output`].
    pub fn run_cycles(&mut self, budget: u64) -> Result<Vec<u8>, CpuError> {
        for _ in 0..budget {
            self.cycle()?;
        }
        Ok(self.drain_output())
    }

    /// Take the display characters that have finished shifting out since
    /// the last drain.
    pub fn drain_output(&mut self) -> Vec<u8> {
        self.display.drain_output()
    }

    /// Push a single character into the keyboard queue.
    pub fn type_char(&mut self, c: u8) {
        self.keyboard.type_char(c);
    }

    /// Push each byte of `s` into the keyboard queue.
    pub fn type_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.keyboard.type_char(b);
        }
    }

    /// Drive the shared physical RESET line that the 6502's `RES` pin and
    /// the PIA's own RESET pin are both tied to on real Apple I hardware.
    ///
    /// Asserting it clears the PIA's registers, resynchronizes the keyboard
    /// strobe against that cleared PIA (queued-ahead keys survive; see
    /// [`Keyboard::resync`]), and takes the CPU's reset line low. RAM, the
    /// video screen, and in-flight display timing are untouched: neither
    /// the 40x24 screen nor the keyboard encoder is wired to the reset line
    /// (see `crates/apple1/src/lib.rs`), and this line is not the CLEAR
    /// SCREEN button ([`Apple1::clear_screen`]).
    ///
    /// The caller advances the machine with [`Apple1::cycle`] /
    /// [`Apple1::run_cycles`] while the line is held and after releasing
    /// it, so a host can hold RESET across any number of batches and stay
    /// inside its own cycle budget. Setting the level it already has is a
    /// no-op.
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
    /// [`RESET_HOLD_CYCLES`], release it, then run until the CPU reports
    /// its physical reset sequence complete (sync, hold, dummy fetch, three
    /// stack reads, `$FFFC/D` vector read).
    ///
    /// This runs cycles to completion without yielding, so a host that
    /// needs to enforce its own cycle budget mid-RESET drives
    /// [`Apple1::set_reset_line`] and [`Apple1::cycle`] directly instead
    /// (as `crates/cli/src/apple1.rs` does).
    ///
    /// Returns [`CpuError::CycleBudgetExceeded`] if the reset sequence does
    /// not report completion within [`RESET_COMPLETION_BUDGET`] cycles of
    /// release; this can only happen if the CPU itself is broken, since the
    /// sequence is fixed-length.
    pub fn reset(&mut self) -> Result<(), CpuError> {
        self.set_reset_line(true);
        for _ in 0..RESET_HOLD_CYCLES {
            self.cycle()?;
        }
        self.set_reset_line(false);

        for _ in 0..RESET_COMPLETION_BUDGET {
            let cycle = self.cycle()?;
            if let Some(step) = cycle.completed
                && step.kind == StepKind::Reset
            {
                return Ok(());
            }
        }
        Err(CpuError::CycleBudgetExceeded {
            address: self.cpu.registers().pc,
            budget: RESET_COMPLETION_BUDGET,
        })
    }

    /// CLEAR SCREEN: the Apple I keyboard's second pushbutton. Blanks the
    /// 40x24 screen and homes the cursor, running no CPU cycle and touching
    /// no other state — not RAM, the CPU, the PIA, the keyboard queue, the
    /// cycle count, or a character still shifting out (see
    /// [`Display::clear_screen`]).
    pub fn clear_screen(&mut self) {
        self.display.clear_screen();
    }

    /// Total cycles executed since creation.
    pub fn total_cycles(&self) -> u64 {
        self.total_cycles
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
