//! Apple I machine: wraps CPU, bus, display, and keyboard into a cycle‑batch
//! run loop.

use hesper_cpu6502::{Cpu, CpuError, Direction};

use crate::bus::{Apple1Bus, RomSizeError};
use crate::display::{DEFAULT_CYCLES_PER_CHAR, Display};
use crate::keyboard::Keyboard;

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

    /// Advance the machine by up to `budget` CPU cycles.
    ///
    /// Each cycle runs: tick devices → update PIA input pins → execute one CPU
    /// cycle → detect `$D012` writes → collect completed display output.
    ///
    /// Returns any display characters that completed during this batch.
    pub fn run_cycles(&mut self, budget: u64) -> Result<Vec<u8>, CpuError> {
        let mut output = Vec::new();
        for _ in 0..budget {
            // 1. Update device state before the cycle.
            self.keyboard.tick(self.bus.pia_mut());
            self.display.tick(self.bus.pia_mut());

            // 2. Set Port B input pins to reflect display busy state.
            self.display.update_pia(self.bus.pia_mut());

            // 3. Execute one CPU cycle.
            let cycle = self.cpu.cycle(&mut self.bus)?;

            // 4. Detect CPU write to Port B Output Register ($D012 when CRB bit 2 = 1).
            if cycle.bus.address == 0xD012
                && cycle.bus.direction == Direction::Write
                && self.bus.pia().port_b_or_selected()
            {
                self.display.on_write(cycle.bus.data);
            }

            // 5. Collect any fully-sent display output.
            output.extend(self.display.drain_output());

            self.total_cycles += 1;
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

    /// Reset the machine (physical RESET line).  Preserves RAM contents.
    pub fn reset(&mut self) {
        self.bus.pia_mut().reset();
        self.display.reset();
        self.keyboard.reset();
        self.cpu.begin_reset();
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
}
