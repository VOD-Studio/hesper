//! MC6820/6821 Peripheral Interface Adapter (PIA) device model.
//!
//! Apple I uses a single PIA (MC6820 on the original board; this model
//! follows the register behaviour documented in the MC6821 datasheet,
//! which is compatible at the register level) for keyboard input (Port A)
//! and video output (Port B).  The register select lines (RS0, RS1) come
//! from A0 and A1:
//!
//! | Address  | RS1 | RS0 | Register                |
//! |----------|-----|-----|-------------------------|
//! | `$D010`  | 0   | 0   | Port A Data / DDR       |
//! | `$D011`  | 0   | 1   | Control Register A (CRA)|
//! | `$D012`  | 1   | 0   | Port B Data / DDR       |
//! | `$D013`  | 1   | 1   | Control Register B (CRB)|
//!
//! ## Port A vs Port B readback
//!
//! The MC6820/MC6821 datasheets specify different readback behaviour:
//! reading Port A always reads the actual pin level; reading Port B in
//! output mode reads the output latch, not the pin.  The current
//! implementation uses a symmetric `(OR & DDR) | (pins & !DDR)` formula
//! for both ports — this happens to match the Apple I configuration
//! (Port A = input, Port B bits 6–0 = output, bit 7 = input) but is not
//! a correct model of the two ports' distinct read paths.  See
//! `docs/apple1/hardware-evidence.md` H12.
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
//!
//! ## CR bit 2 selects the register for reads too
//!
//! Bit 2 of a control register selects which register the matching data
//! address addresses — for **both** directions. With bit 2 clear, reads and
//! writes at `$D010`/`$D012` see the Data Direction Register; with it set
//! they see the peripheral data register (see `docs/references.md#apple-i`
//! for the MC6821 register reference consulted). Only a real peripheral
//! data-register read clears that port's interrupt flags; reading a DDR or
//! a control register, and writing any register, do not.
//!
//! ## RESET
//!
//! The Apple I board ties the PIA's active-low RESET pin to the same system
//! reset line as the 6502's. [`Pia6821::set_reset_line`] models that pin:
//! entering the asserted state clears the output, data-direction, and
//! control/interrupt registers; while it stays asserted, CPU register
//! writes are ignored and no CA1/CB1 edge can latch an interrupt flag.
//! External input pin levels are not part of the chip's register file and
//! are left alone (the keyboard encoder and video board are not wired to
//! the reset line).

use std::fmt;

/// Four register addresses decoded from RS1/RS0.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Reg {
    PortAData,
    ControlA,
    PortBData,
    ControlB,
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

    /// Whether the RESET pin is currently held asserted.
    reset_held: bool,

    /// Set by an actual Port A peripheral data-register read, consumed by
    /// [`Pia6821::take_port_a_read`]. Not a hardware register: it is how
    /// the machine tells the keyboard model that the CPU really took the
    /// presented byte, instead of guessing from the interrupt flag.
    port_a_read: bool,
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
            reset_held: false,
            port_a_read: false,
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
    ///
    /// While the RESET pin is held asserted the pin level is still tracked
    /// (it is an external line) but no flag can latch.
    pub fn set_ca1(&mut self, asserted: bool) {
        let prev = self.ca1;
        self.ca1 = asserted;
        if self.reset_held {
            return;
        }
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
    /// regardless of the interrupt enable bit, and never while RESET is
    /// held.
    pub fn set_cb1(&mut self, asserted: bool) {
        let prev = self.cb1;
        self.cb1 = asserted;
        if self.reset_held {
            return;
        }
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
    /// Cleared by a Port A **peripheral data register** read (CRA bit 2
    /// set), not by a DDRA read and not by any write.
    pub fn irqa1_active(&self) -> bool {
        self.cra & 0x80 != 0
    }

    /// Drive the PIA's active-low RESET pin (`asserted == true` means the
    /// board's reset line is pulled, i.e. the pin is low).
    ///
    /// The false→true transition clears the register file: both output
    /// registers, both data-direction registers (all lines become inputs),
    /// and both control registers including the interrupt flags. Holding it
    /// keeps the chip in that state: CPU writes are dropped and no CA1/CB1
    /// edge latches a flag. Releasing it only ends the hold — it does not
    /// clear the registers a second time. Redundant asserts are no-ops.
    ///
    /// External input pin levels and a pending
    /// [`Pia6821::take_port_a_read`] notification survive: they are not
    /// chip registers, and the host must still be able to settle a read
    /// that really happened just before the button was pressed.
    pub(crate) fn set_reset_line(&mut self, asserted: bool) {
        if !asserted {
            self.reset_held = false;
            return;
        }
        if self.reset_held {
            return;
        }
        self.reset_held = true;
        self.ora = 0;
        self.ddra = 0;
        self.cra = 0;
        self.orb = 0;
        self.ddrb = 0;
        self.crb = 0;
    }

    /// Whether the RESET pin is currently held asserted.
    pub(crate) fn reset_asserted(&self) -> bool {
        self.reset_held
    }

    /// Consume the "the CPU read Port A's peripheral data register" event.
    pub(crate) fn take_port_a_read(&mut self) -> bool {
        std::mem::take(&mut self.port_a_read)
    }

    /// The byte the Port B output pins are actually driving to the video
    /// board, or `None` when nothing valid is being driven: RESET held,
    /// CRB not selecting the output register, or PB0–PB6 not all
    /// configured as outputs (the Apple I display takes seven data lines,
    /// so a partly-input DDRB is not driving a character).
    pub(crate) fn display_data(&self) -> Option<u8> {
        if self.reset_held || self.crb & 0x04 == 0 || self.ddrb & 0x7F != 0x7F {
            return None;
        }
        Some(self.orb & 0x7F)
    }

    // --- Bus-facing read / write ---

    /// Read a PIA register.  `addr` should be in `$D010..$D013`.
    /// Only the lowest two address bits matter.
    ///
    /// CR bit 2 selects the data register vs. the DDR for reads as well as
    /// writes; only a peripheral data-register read has the side effects
    /// (interrupt flags cleared, read event latched).
    pub fn read(&mut self, addr: u16) -> u8 {
        match Reg::from_addr(addr) {
            Reg::PortAData => {
                if self.cra & 0x04 != 0 {
                    self.read_port_a_data()
                } else {
                    self.ddra
                }
            }
            Reg::ControlA => self.cra,
            Reg::PortBData => {
                if self.crb & 0x04 != 0 {
                    self.read_port_b_data()
                } else {
                    self.ddrb
                }
            }
            Reg::ControlB => self.crb,
        }
    }

    /// Write a PIA register.  Ignored entirely while RESET is held.
    pub fn write(&mut self, addr: u16, value: u8) {
        if self.reset_held {
            return;
        }
        match Reg::from_addr(addr) {
            Reg::PortAData => self.write_port_a_data(value),
            Reg::ControlA => self.cra = self.write_cr(self.cra, value),
            Reg::PortBData => self.write_port_b_data(value),
            Reg::ControlB => self.crb = self.write_cr(self.crb, value),
        }
    }

    // --- Internal helpers ---

    /// Read Port A's peripheral data register.  The returned value mixes
    /// ORA for output bits with input pin state for input bits.  Reading
    /// clears IRQA1/IRQA2 and latches the read event for the host.
    fn read_port_a_data(&mut self) -> u8 {
        let value = (self.ora & self.ddra) | (self.pins_a & !self.ddra);
        self.cra &= !0xC0;
        self.port_a_read = true;
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
    }

    fn read_port_b_data(&mut self) -> u8 {
        let value = (self.orb & self.ddrb) | (self.pins_b & !self.ddrb);
        self.crb &= !0xC0;
        value
    }

    fn write_port_b_data(&mut self, value: u8) {
        if self.crb & 0x04 != 0 {
            self.orb = value;
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
            .field("reset_held", &self.reset_held)
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
    fn ddr_reads_back_from_the_data_address_when_cr_bit2_is_clear() {
        // Regression: reading `$D010`/`$D012` with CR bit 2 clear used to
        // return the peripheral data register (and clear interrupt flags),
        // so software could never read back the DDR it had just written —
        // writing DDRB = $7F read back as $00.
        let mut pia = Pia6821::new();
        pia.write(0xD010, 0x3C); // DDRA
        pia.write(0xD012, 0x7F); // DDRB
        pia.set_port_a_inputs(0xFF);
        pia.set_port_b_inputs(0xFF);

        assert_eq!(pia.read(0xD010), 0x3C, "DDRA must read back");
        assert_eq!(pia.read(0xD012), 0x7F, "DDRB must read back");
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
    fn only_a_peripheral_data_read_clears_that_ports_flags() {
        let mut pia = Pia6821::new();
        // Both ports: rising-edge active, DDR selected (bit 2 clear).
        pia.write(0xD011, 0x02);
        pia.write(0xD013, 0x02);
        pia.set_ca1(true);
        pia.set_cb1(true);
        assert_eq!(pia.cra & 0x80, 0x80);
        assert_eq!(pia.crb & 0x80, 0x80);

        // Reading the DDR through the data address must not clear flags.
        let _ = pia.read(0xD010);
        let _ = pia.read(0xD012);
        // Reading the control registers must not clear flags.
        let _ = pia.read(0xD011);
        let _ = pia.read(0xD013);
        // Writing the DDR must not clear flags.
        pia.write(0xD010, 0x00);
        pia.write(0xD012, 0x7F);
        assert_eq!(
            pia.cra & 0x80,
            0x80,
            "DDR/control access must not clear IRQA1"
        );
        assert_eq!(
            pia.crb & 0x80,
            0x80,
            "DDR/control access must not clear IRQB1"
        );

        // Select the output registers and write them: still no clearing.
        pia.write(0xD011, 0x06);
        pia.write(0xD013, 0x06);
        pia.write(0xD010, 0x00);
        pia.write(0xD012, 0x41);
        assert_eq!(pia.cra & 0x80, 0x80, "an OR write must not clear IRQA1");
        assert_eq!(pia.crb & 0x80, 0x80, "an OR write must not clear IRQB1");

        // A real Port A data read clears Port A's flags only.
        pia.set_port_a_inputs(0xC8);
        assert_eq!(pia.read(0xD010), 0xC8);
        assert_eq!(pia.cra & 0x80, 0);
        assert_eq!(pia.crb & 0x80, 0x80, "the other port keeps its flag");

        // ...and symmetrically for Port B.
        let _ = pia.read(0xD012);
        assert_eq!(pia.crb & 0x80, 0);
    }

    #[test]
    fn only_a_peripheral_data_read_reports_a_port_a_read_event() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x02); // DDR selected
        let _ = pia.read(0xD010); // DDRA read
        let _ = pia.read(0xD011); // control read
        pia.write(0xD010, 0x00); // DDRA write
        assert!(!pia.take_port_a_read(), "no data register was read");

        pia.write(0xD011, 0x06); // select OR
        pia.write(0xD010, 0x00); // ORA write
        assert!(!pia.take_port_a_read(), "an OR write is not a read");

        let _ = pia.read(0xD010);
        assert!(pia.take_port_a_read(), "the data register read must report");
        assert!(!pia.take_port_a_read(), "the event is consumed once");
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

    #[test]
    fn held_reset_clears_registers_once_and_blocks_writes() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x07);
        pia.write(0xD013, 0x07);
        pia.write(0xD010, 0x55); // ORA
        pia.write(0xD012, 0x41); // ORB
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80);

        pia.set_reset_line(true);
        assert!(pia.reset_asserted());
        assert_eq!(pia.cra, 0);
        assert_eq!(pia.crb, 0);
        assert_eq!(pia.ora, 0);
        assert_eq!(pia.orb, 0);
        assert_eq!(pia.ddra, 0);
        assert_eq!(pia.ddrb, 0);

        // Writes while held are dropped, and no edge can latch a flag.
        pia.write(0xD011, 0x07);
        pia.write(0xD010, 0x55);
        pia.write(0xD012, 0x7F);
        pia.set_ca1(false);
        pia.set_ca1(true);
        assert_eq!(pia.cra, 0, "CRA must stay cleared while RESET is held");
        assert_eq!(pia.ora, 0);
        assert_eq!(pia.ddrb, 0);

        // A redundant assert changes nothing; release ends the hold and
        // software can reconfigure the chip again.
        pia.set_reset_line(true);
        assert!(pia.reset_asserted());
        pia.set_reset_line(false);
        assert!(!pia.reset_asserted());
        pia.write(0xD011, 0x07);
        assert_eq!(pia.cra, 0x07);
        pia.set_ca1(false);
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80, "edges latch again after release");
    }

    #[test]
    fn held_reset_keeps_input_pin_levels_and_a_pending_read_event() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x06); // select ORA
        pia.set_port_a_inputs(0xC1);
        assert_eq!(pia.read(0xD010), 0xC1);

        pia.set_reset_line(true);
        // The keyboard encoder and video board are not on the reset line.
        assert!(pia.port_a_bit7(), "external pin levels are untouched");
        assert!(
            pia.take_port_a_read(),
            "a read that already happened must still settle"
        );
    }

    #[test]
    fn display_data_requires_or_select_and_seven_output_lines() {
        let mut pia = Pia6821::new();
        pia.write(0xD012, 0x7F); // DDRB: PB0-PB6 outputs
        assert_eq!(pia.display_data(), None, "DDR still selected, no OR write");

        pia.write(0xD013, 0x04); // select ORB
        pia.write(0xD012, 0xC1);
        assert_eq!(pia.display_data(), Some(0x41), "bit 7 is not a data line");

        // A port line configured as input is not driving the display.
        pia.write(0xD013, 0x00); // select DDRB
        pia.write(0xD012, 0x3F); // PB6 becomes an input
        pia.write(0xD013, 0x04);
        assert_eq!(pia.display_data(), None);

        // Held RESET drives nothing.
        pia.write(0xD013, 0x00);
        pia.write(0xD012, 0x7F);
        pia.write(0xD013, 0x04);
        pia.write(0xD012, 0x41);
        assert_eq!(pia.display_data(), Some(0x41));
        pia.set_reset_line(true);
        assert_eq!(pia.display_data(), None);
    }
}
