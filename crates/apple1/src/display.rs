//! Apple I display shift-register model.
//!
//! Models the video output Port B protocol:
//! - CPU writes a character to `$D012` (Port B data).
//! - The shift register becomes busy (PB7 = 1).
//! - After `cycles_per_char` cycles, the character is fully sent,
//!   PB7 returns to 0, and a rising edge on CB1 sets IRQB1.

use crate::pia::Pia6821;

/// Default cycles per character (~1 ms at 1 MHz).
pub const DEFAULT_CYCLES_PER_CHAR: u64 = 1000;

/// Display shift-register model.
pub struct Display {
    pub cycles_per_char: u64,
    cycles_remaining: u64,
    /// Characters that have been fully sent.
    output: Vec<u8>,
    /// Last character latched into the display.
    latch: u8,
    /// Whether the display is currently busy sending.
    busy: bool,
}

impl Display {
    pub fn new(cycles_per_char: u64) -> Self {
        Self {
            cycles_per_char,
            cycles_remaining: 0,
            output: Vec::new(),
            latch: 0,
            busy: false,
        }
    }

    /// Advance the display timer by one cycle.  When the timer expires the
    /// character is collected and the display becomes ready (CB1 rising edge).
    pub fn tick(&mut self, pia: &mut Pia6821) {
        if !self.busy {
            return;
        }
        self.cycles_remaining -= 1;
        if self.cycles_remaining == 0 {
            self.busy = false;
            self.output.push(self.latch);
            // Rising edge on CB1 → sets IRQB1 if enabled.
            pia.set_cb1(true);
            pia.set_cb1(false);
        }
    }

    /// Update Port B input pins before the PIA is read by the CPU.
    /// PB7 = busy flag; PB6–PB0 = last character written for read-back.
    pub fn update_pia(&self, pia: &mut Pia6821) {
        let value = if self.busy {
            0x80 | (self.latch & 0x7F)
        } else {
            self.latch & 0x7F
        };
        pia.set_port_b_inputs(value);
    }

    /// Notify the display that the CPU wrote to Port B (`$D012`).
    /// Starts the shift-register busy timer.  Each write resets the timer.
    pub fn on_write(&mut self, ch: u8) {
        self.busy = true;
        self.latch = ch;
        self.cycles_remaining = self.cycles_per_char;
    }

    /// Drain all completed output characters since the last drain.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Reset display state (stop any in-progress transmission).
    pub fn reset(&mut self) {
        self.busy = false;
        self.latch = 0;
        self.cycles_remaining = 0;
        self.output.clear();
    }
}
