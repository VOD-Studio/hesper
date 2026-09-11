//! Apple I keyboard input model.
//!
//! The keyboard connects to the PIA via Port A:
//! - PA0–PA6: ASCII data (bit 7 = 1 always on Apple I).
//! - CA1: keyboard strobe (asserted when key data is ready).
//!
//! A key stays at the head of the queue until the CPU really reads Port A's
//! peripheral data register; the PIA reports that read through
//! [`Pia6821::take_port_a_read`], so no key is consumed by a DDR read, a
//! control-register read, an output-register write, or a RESET that clears
//! the interrupt flag. The Port A input pins keep the last presented byte,
//! so re-reading without a new keypress returns the same data, exactly as
//! a real keyboard encoder holding its latched output would.

use std::collections::VecDeque;

use crate::pia::Pia6821;

/// Keyboard input model.
pub struct Keyboard {
    /// Keys typed but not yet read by the CPU; the front entry is the one
    /// currently offered to the PIA once presented.
    pending: VecDeque<u8>,
    /// Whether the front key's data is currently on the Port A pins.
    presented: bool,
    /// Whether the CA1 strobe pulse is still asserted from this key.
    strobe: bool,
}

impl Keyboard {
    pub fn new() -> Self {
        Self {
            pending: VecDeque::new(),
            presented: false,
            strobe: false,
        }
    }

    /// Queue a character to be "typed" to the emulated machine.
    /// Masks to seven bits and uppercases ASCII letters before queuing.
    pub fn type_char(&mut self, c: u8) {
        self.pending.push_back((c & 0x7F).to_ascii_uppercase());
    }

    /// Called before each CPU cycle.  Settles any read the CPU performed
    /// since the last tick, then presents pending key data on Port A and
    /// pulses CA1 to create an edge that sets IRQA1.
    ///
    /// Strobe timing: the first tick with pending data asserts CA1 (high);
    /// the next tick de-asserts it (low), allowing the next key edge.
    pub fn tick(&mut self, pia: &mut Pia6821) {
        self.acknowledge_read(pia);
        if self.strobe {
            // De-assert the strobe line.
            pia.set_ca1(false);
            self.strobe = false;
            return;
        }
        // The presented key stays on the pins until it is actually read.
        if self.presented {
            return;
        }
        if let Some(&c) = self.pending.front() {
            self.presented = true;
            // Apple I keyboard: bit 7 = 1 (always).
            pia.set_port_a_inputs(c | 0x80);
            // Assert CA1 — if the PIA is configured for rising edge this
            // sets IRQA1.
            pia.set_ca1(true);
            self.strobe = true;
        }
    }

    /// Consume the PIA's "Port A data register was read" event and, if a
    /// key was presented, retire it. Repeated reads without a new key
    /// retire nothing further: the second read sees the same latched byte
    /// and must not swallow the key behind it.
    fn acknowledge_read(&mut self, pia: &mut Pia6821) {
        if !pia.take_port_a_read() {
            return;
        }
        if self.presented {
            self.pending.pop_front();
            self.presented = false;
        }
    }

    /// Whether any typed key is still waiting to be read, including one
    /// already presented on the Port A pins.
    pub fn has_pending(&self) -> bool {
        !self.pending.is_empty()
    }

    /// Resynchronize with a PIA whose RESET pin has just been asserted
    /// (which cleared CRA and its edge-detect state).
    ///
    /// Real Apple I hardware ties the PIA's RESET pin to the same system
    /// reset line as the 6502, but the external keyboard encoder is not
    /// wired to that line at all — pressing RESET does not erase keys the
    /// user has already typed ahead. This crate models "typed ahead" as a
    /// host-side queue standing in for a live keyboard, so `resync` settles
    /// a read that really completed just before the button was pressed,
    /// then drops the in-flight strobe pulse and takes CA1 low so the
    /// still-unread head key is strobed again after release. Keys already
    /// read are never replayed; keys never read are never lost.
    ///
    /// The caller must have asserted the PIA's RESET line first, so taking
    /// CA1 low here cannot latch a spurious interrupt flag.
    pub fn resync(&mut self, pia: &mut Pia6821) {
        debug_assert!(
            pia.reset_asserted(),
            "resync must run with the PIA's RESET line held"
        );
        self.acknowledge_read(pia);
        self.presented = false;
        self.strobe = false;
        pia.set_ca1(false);
    }
}

impl Default for Keyboard {
    fn default() -> Self {
        Self::new()
    }
}
