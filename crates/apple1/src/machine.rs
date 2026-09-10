//! Apple I machine: wraps CPU, bus, display, and keyboard into a cycle‑batch
//! run loop.

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
}

impl Apple1 {
    /// Create an Apple I machine with the given 256‑byte Woz Monitor ROM and
    /// display speed (in CPU cycles per character).  Uses
    /// [`DEFAULT_CYCLES_PER_CHAR`] if `None`.
    pub fn new(rom: &[u8], cycles_per_char: Option<u64>) -> Result<Self, RomSizeError> {
        Ok(Self {
            cpu: Cpu::new(),
            bus: Apple1Bus::new(rom)?,
            display: Display::new(cycles_per_char.unwrap_or(DEFAULT_CYCLES_PER_CHAR)),
            keyboard: Keyboard::new(),
            total_cycles: 0,
        })
    }

    /// Run every device and the CPU through exactly one cycle: tick devices
    /// → update PIA input pins → execute one CPU cycle → detect `$D012`
    /// writes. Returns the raw CPU [`hesper_cpu6502::Cycle`] so callers can
    /// inspect completion (used by both `run_cycles` and `reset`).
    fn tick_one_cycle(&mut self) -> Result<Cycle, CpuError> {
        self.keyboard.tick(self.bus.pia_mut());
        self.display.tick(self.bus.pia_mut());
        self.display.update_pia(self.bus.pia_mut());

        let cycle = self.cpu.cycle(&mut self.bus)?;

        if cycle.bus.address == 0xD012
            && cycle.bus.direction == Direction::Write
            && self.bus.pia().port_b_or_selected()
        {
            self.display.on_write(cycle.bus.data);
        }

        self.total_cycles += 1;
        Ok(cycle)
    }

    /// Advance the machine by up to `budget` CPU cycles.
    ///
    /// Each cycle runs: tick devices → update PIA input pins → execute one CPU
    /// cycle → detect `$D012` writes → collect completed display output.
    ///
    /// Returns any display characters that completed during this batch.
    /// Splitting the same total budget across several calls (mid-instruction,
    /// mid-RDY-wait, or mid physical RESET hold) yields the same device and
    /// CPU state as one call with the combined budget: the CPU's own
    /// sequencer state persists across calls, and every device is ticked
    /// exactly once per cycle regardless of batch boundaries.
    pub fn run_cycles(&mut self, budget: u64) -> Result<Vec<u8>, CpuError> {
        let mut output = Vec::new();
        for _ in 0..budget {
            self.tick_one_cycle()?;
            output.extend(self.display.drain_output());
        }
        Ok(output)
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

    /// Physical RESET: assert the shared reset line the 6502 and the PIA
    /// are both tied to on real Apple I hardware, hold it, then release and
    /// run the CPU's real reset sequence (sync, hold, dummy fetch, three
    /// stack reads, `$FFFC/D` vector read) to completion — not the
    /// immediate host `begin_reset` entry point. RAM is preserved.
    ///
    /// The PIA's own registers are cleared (its RESET pin shares the same
    /// line). The video screen and the keyboard's queued-ahead keys are
    /// left alone: neither is wired to the Apple I's system reset line (see
    /// `crates/apple1/src/lib.rs`).
    ///
    /// Returns [`CpuError::CycleBudgetExceeded`] if the reset sequence does
    /// not report completion within [`RESET_COMPLETION_BUDGET`] cycles of
    /// release; this can only happen if the CPU itself is broken, since the
    /// sequence is fixed-length.
    pub fn reset(&mut self) -> Result<(), CpuError> {
        self.bus.pia_mut().reset();
        self.keyboard.resync();
        self.display.reset();

        self.cpu.set_reset_line(true);
        for _ in 0..RESET_HOLD_CYCLES {
            self.tick_one_cycle()?;
        }
        self.cpu.set_reset_line(false);

        for _ in 0..RESET_COMPLETION_BUDGET {
            let cycle = self.tick_one_cycle()?;
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
