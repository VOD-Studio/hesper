//! MC6821 Peripheral Interface Adapter (PIA) device model.
//!
//! Apple I uses a single PIA for keyboard input (Port A) and video output
//! (Port B).  The register select lines (RS0, RS1) come from A0 and A1:
//!
//! | Address  | RS1 | RS0 | Register                |
//! |----------|-----|-----|-------------------------|
//! | `$D010`  | 0   | 0   | Port A Data / DDR       |
//! | `$D011`  | 0   | 1   | Control Register A (CRA)|
//! | `$D012`  | 1   | 0   | Port B Data / DDR       |
//! | `$D013`  | 1   | 1   | Control Register B (CRB)|
//!
//! ## Control register bits
//!
//! | Bit | Name      | R/W | Description                              |
//! |-----|-----------|-----|------------------------------------------|
//! | 7   | IRQA/B1   | R   | Set by active transition on CA1/CB1      |
//! | 6   | IRQA/B2   | R   | Set by active transition on CA2/CB2      |
//! | 5   | CA2/CB2 ctl | R/W | Determines CA2/CB2 mode                 |
//! | 4   | CA2/CB2 ctl | R/W |                                          |
//! | 3   | CA2/CB2 ctl | R/W |                                          |
//! | 2   | DDR access | R/W | 0 = DDR selected, 1 = OR selected        |
//! | 1   | CA1/CB1 ctl | R/W | Active transition (0 = H→L, 1 = L→H)    |
//! | 0   | CA1/CB1 ctl | R/W | Interrupt enable (0 = disable, 1 = enable)|

use std::fmt;

/// Four register addresses decoded from RS1/RS0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reg {
    PortAData = 0,
    ControlA = 1,
    PortBData = 2,
    ControlB = 3,
}

impl Reg {
    fn from_addr(addr: u16) -> Self {
        match addr & 0x03 {
            0 => Reg::PortAData,
            1 => Reg::ControlA,
            2 => Reg::PortBData,
            3 => Reg::ControlB,
            _ => unreachable!(),
        }
    }
}

pub struct Pia6821 {
    // Port A (keyboard — all inputs on Apple I)
    ora: u8,
    ddra: u8,
    cra: u8,
    /// Input pin state for Port A.  Read data returns (ora & ddra) | (pins & !ddra).
    pins_a: u8,

    // Port B (video — outputs on Apple I)
    orb: u8,
    ddrb: u8,
    crb: u8,
    pins_b: u8,

    // Control line input states (for edge detection)
    ca1: bool,
    _ca2_input: bool,
    cb1: bool,
    _cb2_input: bool,
}

impl Pia6821 {
    /// All registers zero, all control lines inactive.
    pub fn new() -> Self {
        Self {
            ora: 0,
            ddra: 0,
            cra: 0,
            pins_a: 0,
            orb: 0,
            ddrb: 0,
            crb: 0,
            pins_b: 0,
            ca1: false,
            _ca2_input: false,
            cb1: false,
            _cb2_input: false,
        }
    }

    // --- Port A input pins (keyboard data + strobe) ---

    /// Set the Port A input pin levels.  On Apple I these come from the keyboard.
    pub fn set_port_a_inputs(&mut self, value: u8) {
        self.pins_a = value;
    }

    /// Set the CA1 input pin.  A rising or falling edge (per CRA bits 1,0)
    /// sets IRQA1 (bit 7 of CRA) — per the MC6821 datasheet the status flag
    /// latches on the qualifying transition regardless of the interrupt
    /// enable bit (CRA bit 0); that bit only gates whether the flag would
    /// also assert the PIA's external IRQ output pin, which this crate
    /// does not model (the base Apple I configuration does not wire the
    /// PIA's IRQ output to the 6502's IRQ input either). Software can
    /// therefore always poll the flag, enabled or not — this is how the
    /// Woz Monitor itself reads it (`BIT`/`BPL`, no interrupt handler).
    pub fn set_ca1(&mut self, asserted: bool) {
        let prev = self.ca1;
        self.ca1 = asserted;
        let rising = asserted && !prev;
        let falling = !asserted && prev;
        let active_high = self.cra & 0x02 != 0;

        let edge = if active_high { rising } else { falling };
        if edge {
            self.cra |= 0x80; // set IRQA1
        }
    }

    // --- Port B input pins (display acknowledge on CB1, PB7 display ready) ---

    /// Set the Port B input pin levels.  On Apple I, PB7 reflects the display
    /// ready state (0 = ready, 1 = busy).
    pub fn set_port_b_inputs(&mut self, value: u8) {
        self.pins_b = value;
    }

    /// Set the CB1 input pin (Display Acknowledge on Apple I). See
    /// [`Pia6821::set_ca1`]: the flag sets on the qualifying edge
    /// regardless of the interrupt enable bit.
    pub fn set_cb1(&mut self, asserted: bool) {
        let prev = self.cb1;
        self.cb1 = asserted;
        let rising = asserted && !prev;
        let falling = !asserted && prev;
        let active_high = self.crb & 0x02 != 0;

        let edge = if active_high { rising } else { falling };
        if edge {
            self.crb |= 0x80; // set IRQB1
        }
    }

    /// Whether Port A input pins are currently driven (bit 7 of all inputs).
    /// On Apple I the keyboard sets bit 7; this returns true when keyboard
    /// data has been set on the pins.
    pub fn port_a_bit7(&self) -> bool {
        self.pins_a & 0x80 != 0
    }

    /// Whether the IRQA1 interrupt flag (CRA bit 7) is currently set.
    /// This is cleared when the CPU reads Port A data.
    pub fn irqa1_active(&self) -> bool {
        self.cra & 0x80 != 0
    }

    /// Reset to power-on state: all registers zero, all control lines inactive.
    pub fn reset(&mut self) {
        self.ora = 0;
        self.ddra = 0;
        self.cra = 0;
        self.pins_a = 0;
        self.orb = 0;
        self.ddrb = 0;
        self.crb = 0;
        self.pins_b = 0;
        self.ca1 = false;
        self._ca2_input = false;
        self.cb1 = false;
        self._cb2_input = false;
    }

    /// Whether writes to Port B go to the Output Register (CRB bit 2 = 1)
    /// rather than the Data Direction Register.
    pub fn port_b_or_selected(&self) -> bool {
        self.crb & 0x04 != 0
    }

    // --- Bus-facing read / write ---

    /// Read a PIA register.  `addr` should be in `$D010..$D013`.
    /// Only the lowest two address bits matter.
    pub fn read(&mut self, addr: u16) -> u8 {
        match Reg::from_addr(addr) {
            Reg::PortAData => self.read_port_a_data(),
            Reg::ControlA => self.cra,
            Reg::PortBData => self.read_port_b_data(),
            Reg::ControlB => self.crb,
        }
    }

    /// Write a PIA register.
    pub fn write(&mut self, addr: u16, value: u8) {
        match Reg::from_addr(addr) {
            Reg::PortAData => self.write_port_a_data(value),
            Reg::ControlA => self.cra = self.write_cr(self.cra, value),
            Reg::PortBData => self.write_port_b_data(value),
            Reg::ControlB => self.crb = self.write_cr(self.crb, value),
        }
    }

    // --- Internal helpers ---

    /// Read Port A data.  The returned value mixes ORA for output bits with
    /// input pin state for input bits.  Reading clears IRQA1 and IRQA2.
    fn read_port_a_data(&mut self) -> u8 {
        let value = (self.ora & self.ddra) | (self.pins_a & !self.ddra);
        // Reading the data register clears interrupt flags.
        self.cra &= !0xC0;
        value
    }

    fn write_port_a_data(&mut self, value: u8) {
        if self.cra & 0x04 != 0 {
            // CR bit 2 = 1 → write Output Register
            self.ora = value;
        } else {
            // CR bit 2 = 0 → write Data Direction Register
            self.ddra = value;
        }
        // Writing the data register also clears interrupt flags (side effect
        // when CR bit 2 = 1, but consistent peripherals clear on write too).
        if self.cra & 0x04 != 0 {
            self.cra &= !0xC0;
        }
    }

    fn read_port_b_data(&mut self) -> u8 {
        let value = (self.orb & self.ddrb) | (self.pins_b & !self.ddrb);
        self.crb &= !0xC0;
        value
    }

    fn write_port_b_data(&mut self, value: u8) {
        if self.crb & 0x04 != 0 {
            self.orb = value;
            self.crb &= !0xC0;
        } else {
            self.ddrb = value;
        }
    }

    /// Apply a control register write.  Bits 7 and 6 are read-only flags
    /// (IRQA1/B1 and IRQA2/B2) and are preserved.
    fn write_cr(&self, old: u8, value: u8) -> u8 {
        // Preserve read-only flags.
        let flags = old & 0xC0;
        (value & 0x3F) | flags
    }
}

impl Default for Pia6821 {
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Pia6821 {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Pia6821")
            .field("ddra", &self.ddra)
            .field("ora", &self.ora)
            .field("cra", &format_args!("{:02X}", self.cra))
            .field("ddrb", &self.ddrb)
            .field("orb", &self.orb)
            .field("crb", &format_args!("{:02X}", self.crb))
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reset_state_all_zero() {
        let pia = Pia6821::new();
        assert_eq!(pia.ddra, 0);
        assert_eq!(pia.ddrb, 0);
        assert_eq!(pia.cra, 0);
        assert_eq!(pia.crb, 0);
        // Reading data registers returns 0 (no input pins set).
    }

    #[test]
    fn ddr_or_selection_via_cr_bit2() {
        let mut pia = Pia6821::new();
        let addr_a = 0xD010;

        // CR bit 2 = 0 → DDR selected.
        pia.write(addr_a, 0x0F);
        assert_eq!(pia.ddra, 0x0F);
        assert_eq!(pia.ora, 0);

        // Switch to OR.
        pia.write(0xD011, 0x04); // CRA = $04: bit2=1, rest 0
        pia.write(addr_a, 0x55);
        assert_eq!(pia.ora, 0x55);
        assert_eq!(pia.ddra, 0x0F); // DDR unchanged

        // Switch back to DDR.
        pia.write(0xD011, 0x00);
        pia.write(addr_a, 0xF0);
        assert_eq!(pia.ddra, 0xF0);
        assert_eq!(pia.ora, 0x55); // ORA unchanged
    }

    #[test]
    fn data_read_mixes_output_and_input_pins() {
        let mut pia = Pia6821::new();
        // Set DDR: lower nibble output, upper nibble input.
        pia.write(0xD010, 0x0F); // DDRA = $0F
        pia.write(0xD011, 0x04); // CRA bit2=1 → select OR
        pia.write(0xD010, 0x3C); // ORA = $3C
        pia.set_port_a_inputs(0xA0); // input pins upper nibble

        // Read should be: (ORA & DDR) | (pins & !DDR) = $3C & $0F | $A0 & $F0 = $0C | $A0 = $AC
        let val = pia.read(0xD010);
        assert_eq!(val, 0xAC);
    }

    #[test]
    fn ca1_edge_sets_irqa1_flag() {
        let mut pia = Pia6821::new();
        // CRA: bit2=0, bit1=1 (active on rising edge), bit0=1 (IRQ enabled)
        pia.write(0xD011, 0x03);

        // Prime ca1 high so the next edge is falling.
        // The initial false→true IS a rising edge, so we accept the flag.
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80); // initial rising edge sets flag
        pia.cra &= !0x80; // clear it manually for this test

        // H→L edge: no flag (active edge is rising).
        pia.set_ca1(false);
        assert_eq!(pia.cra & 0x80, 0);

        // L→H edge: flag set.
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80);
    }

    #[test]
    fn ca1_edge_sets_irqa1_flag_even_when_interrupt_disabled() {
        // MC6821 datasheet: the status flag latches on the qualifying
        // transition regardless of the interrupt-enable bit; the enable
        // bit only gates the (unmodeled) external IRQ output pin. A
        // regression for a prior bug where this crate gated the flag
        // itself on the enable bit, silently losing status edges whenever
        // software polled without enabling interrupts — as the Woz
        // Monitor and this crate's `Keyboard`/`Display` models do.
        let mut pia = Pia6821::new();
        // CRA: bit1=1 (active on rising edge), bit0=0 (IRQ disabled).
        pia.write(0xD011, 0x02);

        pia.set_ca1(true);
        assert_eq!(
            pia.cra & 0x80,
            0x80,
            "flag must set on the edge even with interrupts disabled"
        );
    }

    #[test]
    fn reading_data_clears_irq_flags() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x03); // rising edge, IRQ enabled
        pia.set_ca1(false);
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80);

        // Switch to OR so we read data, not DDR.
        pia.write(0xD011, 0x07); // bit2=1, bit1=1, bit0=1
        let _ = pia.read(0xD010);
        assert_eq!(pia.cra & 0x80, 0); // flags cleared
    }

    #[test]
    fn control_register_preserves_readonly_flags_on_write() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x03); // rising edge, IRQ enabled
        pia.set_ca1(false);
        pia.set_ca1(true);
        assert_eq!(pia.cra, 0x83); // flag set + config

        // Write only the DDR select bit — flags must survive.
        pia.write(0xD011, 0x04);
        assert_eq!(pia.cra, 0x84); // $80 (flag) | $04 (DDR select)
    }
}
