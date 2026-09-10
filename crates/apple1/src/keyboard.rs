//! Apple I keyboard input model.
//!
//! The keyboard connects to the PIA via Port A:
//! - PA0–PA6: ASCII data (bit 7 = 1 always on Apple I).
//! - CA1: keyboard strobe (asserted when key data is ready).
//!
//! The CPU reads `$D010` to get the character; this read also clears
//! IRQA1 automatically (already handled by the PIA register model).

use std::collections::VecDeque;

use crate::pia::Pia6821;

/// Keyboard input model.
pub struct Keyboard {
    pending: VecDeque<u8>,
    current: u8,
    strobe: bool,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            current: 0,
            strobe: false,
        }
    }

    /// Queue a character to be "typed" to the emulated machine.
    pub fn type_char(&mut self, c: u8) {
        self.pending.push_back(c);
    }

    /// Called before each CPU cycle.  Presents pending key data on Port A
    /// and pulses CA1 to create a rising edge that sets IRQA1.
    ///
    /// Strobe timing: the first tick with pending data asserts CA1 (high);
    /// the next tick de-asserts it (low), allowing the next key edge.
    pub fn tick(&mut self, pia: &mut Pia6821) {
        if self.strobe {
            // De-assert the strobe line.
            pia.set_ca1(false);
            self.strobe = false;
            return;
        }
        // Don't present next character until the previous one has been read
        // (IRQA1 is cleared when the CPU reads Port A data).
        if pia.irqa1_active() {
            return;
        }
        if let Some(c) = self.pending.pop_front() {
            self.current = c;
            // Apple I keyboard: bit 7 = 1 (always).
            pia.set_port_a_inputs(c | 0x80);
            // Assert CA1 — if the PIA is configured for rising edge this
            // sets IRQA1.
            pia.set_ca1(true);
            self.strobe = true;
        }
    }

    /// Whether more keys are queued.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty() || self.strobe
    }

    /// Reset keyboard state (clear queue and strobe).
    pub fn reset(&mut self) {
        self.pending.clear();
        self.current = 0;
        self.strobe = false;
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}
