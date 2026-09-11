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

/// CB2 control codes from CRB bits 5-3 (MC6820 Table 5 / MC6821 figures
/// 10-12).  The Apple I terminal uses `100`: CB2 strobes on the first
/// enable after an ORB write and is released by the CB1 acknowledge.
const CB2_WRITE_STROBE_CB1: u8 = 0b100;
const CB2_WRITE_STROBE_E: u8 = 0b101;
const CB2_MANUAL_LOW: u8 = 0b110;
const CB2_MANUAL_HIGH: u8 = 0b111;

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

    /// CB2 output level driven by the write-strobe handshake (CRB bits
    /// 5-3 = 100/101).  Manual modes take their level straight from the
    /// control bits and input modes from the pin, so this only holds the
    /// handshake flip-flop's state.
    cb2_out: bool,
    /// An ORB write asked for a strobe; it fires on the next E edge, not
    /// during the write itself.
    cb2_armed: bool,
    /// CRB bits 5-3 = 101: CB2 is low and the next E edge returns it high.
    cb2_clear_on_e: bool,
    /// CB2 pin level when CB2 is configured as an input.  The original
    /// board leaves the pin pulled high when nothing drives it.
    cb2_input: bool,

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
            cb2_out: true,
            cb2_armed: false,
            cb2_clear_on_e: false,
            cb2_input: true,
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
    /// `asserted == true` means the pin is at its active **low** level, so
    /// CRA bit 1 selects which transition of that level latches the flag:
    /// 0 = into the active level (high-to-low), 1 = out of it
    /// (low-to-high), exactly as the datasheet specifies.
    ///
    /// While the RESET pin is held asserted the pin level is still tracked
    /// (it is an external line) but no flag can latch.
    pub fn set_ca1(&mut self, asserted: bool) {
        let prev = self.ca1;
        self.ca1 = asserted;
        if self.reset_held {
            return;
        }
        let into_active = asserted && !prev;
        let out_of_active = !asserted && prev;
        let active_high = self.cra & 0x02 != 0;

        let edge = if active_high {
            out_of_active
        } else {
            into_active
        };
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
    ///
    /// With the write-strobe handshake selected (CRB bits 5-3 = 100) the
    /// active edge is also what returns CB2 high. Software acknowledges
    /// the handshake by reading the Port B data register, which clears the
    /// flag; an acknowledge that arrives while the previous one is still
    /// unread does not advance the handshake.
    pub fn set_cb1(&mut self, asserted: bool) {
        let prev = self.cb1;
        self.cb1 = asserted;
        if self.reset_held {
            return;
        }
        let into_active = asserted && !prev;
        let out_of_active = !asserted && prev;
        let active_high = self.crb & 0x02 != 0;

        let edge = if active_high {
            out_of_active
        } else {
            into_active
        };
        if edge && self.crb & 0x80 == 0 {
            self.crb |= 0x80; // set IRQB1
            if self.cb2_mode() == CB2_WRITE_STROBE_CB1 {
                self.cb2_out = true;
            }
        }
    }

    /// The CB2 handshake mode selected by CRB bits 5-3.
    fn cb2_mode(&self) -> u8 {
        (self.crb >> 3) & 0x07
    }

    /// The level present on the CB2 pin: driven by the handshake
    /// flip-flop in the write-strobe modes, fixed by the control bits in
    /// the manual modes, and taken from the pin in the input modes.
    pub(crate) fn cb2_level(&self) -> bool {
        match self.cb2_mode() {
            CB2_MANUAL_LOW => false,
            CB2_MANUAL_HIGH => true,
            CB2_WRITE_STROBE_CB1 | CB2_WRITE_STROBE_E => self.cb2_out,
            _ => self.cb2_input,
        }
    }

    /// The PIA's E (enable) clock rose.  The machine calls this once per
    /// real CPU Φ2 cycle, before that cycle's bus access, so a strobe
    /// requested by an ORB write lands on the *next* enable.
    pub(crate) fn e_rising_edge(&mut self) {
        if self.reset_held {
            return;
        }
        match self.cb2_mode() {
            // Write strobe with CB1: CB2 goes low on the first E after an
            // ORB write and CB1's active edge returns it high.
            CB2_WRITE_STROBE_CB1 => {
                if self.cb2_armed {
                    self.cb2_armed = false;
                    self.cb2_out = false;
                }
            }
            // Write strobe with E: CB2 goes low on the first E after an
            // ORB write and returns high on the following E.
            CB2_WRITE_STROBE_E => {
                if self.cb2_armed {
                    self.cb2_armed = false;
                    self.cb2_out = false;
                    self.cb2_clear_on_e = true;
                } else if self.cb2_clear_on_e {
                    self.cb2_clear_on_e = false;
                    self.cb2_out = true;
                }
            }
            _ => {
                self.cb2_armed = false;
                self.cb2_clear_on_e = false;
            }
        }
    }

    /// Present the video board's DA line on PB7.
    ///
    /// The Apple I wires CB2 through an inverter to the terminal's DA
    /// input and loops DA back to PB7, which is why the monitor polls bit
    /// 7 of `$D012` to know when the terminal has taken the character.
    pub(crate) fn set_display_ready(&mut self, da: bool) {
        if da {
            self.pins_b |= 0x80;
        } else {
            self.pins_b &= !0x80;
        }
    }

    /// Whether an output handshake is still in flight: a strobe waiting
    /// for its enable, or CB2 held low because the terminal has not taken
    /// the character yet.
    pub(crate) fn output_strobe_pending(&self) -> bool {
        self.cb2_armed || !self.cb2_level()
    }

    /// The seven character data lines as the terminal sees them.
    ///
    /// Only lines configured as outputs are driven, and the original board
    /// leaves several 7400 inputs open, so an undriven line reads as a
    /// TTL high rather than as a bus fault.
    pub(crate) fn data_lines(&self) -> u8 {
        (self.orb & self.ddrb) | (!self.ddrb & 0x7f)
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
        // A strobe waiting for an enable that will never come is dropped,
        // and CB2 is released to its idle level (CRB now selects input
        // mode, so the pin floats high and PB7 reads "not ready").
        self.cb2_armed = false;
        self.cb2_clear_on_e = false;
        self.cb2_out = true;
    }

    /// Whether the RESET pin is currently held asserted.
    pub(crate) fn reset_asserted(&self) -> bool {
        self.reset_held
    }

    /// Consume the "the CPU read Port A's peripheral data register" event.
    pub(crate) fn take_port_a_read(&mut self) -> bool {
        std::mem::take(&mut self.port_a_read)
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
            Reg::ControlB => self.write_control_b(value),
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
            // A write to the output register asks the CB2 handshake to
            // strobe.  The strobe itself waits for the next E edge.
            if matches!(self.cb2_mode(), CB2_WRITE_STROBE_CB1 | CB2_WRITE_STROBE_E) {
                self.cb2_armed = true;
            }
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

    /// Apply a CRB write, restarting the CB2 handshake when the mode
    /// changes: a strobe armed under the old mode must not fire under the
    /// new one, and entering a write-strobe mode leaves CB2 idle high so
    /// software sees the terminal as ready.
    fn write_control_b(&mut self, value: u8) {
        let new = self.write_cr(self.crb, value);
        if (self.crb >> 3) & 0x07 != (new >> 3) & 0x07 {
            self.cb2_armed = false;
            self.cb2_clear_on_e = false;
            if matches!((new >> 3) & 0x07, CB2_WRITE_STROBE_CB1 | CB2_WRITE_STROBE_E) {
                self.cb2_out = true;
            }
        }
        self.crb = new;
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
        // CRA: bit 2 = 0 (DDR selected), bit 1 = 1 (flag on the active-low
        // pin's low-to-high transition), bit 0 = 1 (IRQ enabled).
        pia.write(0xD011, 0x03);

        // Taking the pin to its active low level is not the qualifying
        // transition under this selection.
        pia.set_ca1(true);
        assert_eq!(
            pia.cra & 0x80,
            0,
            "entering the active level does not latch"
        );
        assert_eq!(pia.cra, 0x03);

        // Returning high is.
        pia.set_ca1(false);
        assert_eq!(pia.cra & 0x80, 0x80);

        // The opposite selection latches on the other transition.
        pia.cra = 0x00;
        pia.set_ca1(true);
        assert_eq!(pia.cra & 0x80, 0x80, "bit 1 = 0 latches on the low edge");
        pia.cra = 0x00;
        pia.set_ca1(false);
        assert_eq!(pia.cra & 0x80, 0, "leaving the active level does not latch");
    }

    #[test]
    fn ca1_edge_sets_irqa1_flag_even_when_interrupt_disabled() {
        // MC6821 datasheet: the status flag latches on the qualifying
        // transition regardless of the interrupt-enable bit; the enable
        // bit only gates the (unmodeled) external IRQ output pin. A
        // regression for a prior bug where this crate gated the flag
        // itself on the enable bit, silently losing status edges whenever
        // software polled without enabling interrupts — as the Woz
        // Monitor and this crate's `Keyboard` model do.
        let mut pia = Pia6821::new();
        // CRA: bit1=1 (active on the low-to-high transition), bit0=0 (IRQ
        // disabled).
        pia.write(0xD011, 0x02);

        pia.set_ca1(true);
        pia.set_ca1(false);
        assert_eq!(
            pia.cra & 0x80,
            0x80,
            "flag must set on the edge even with interrupts disabled"
        );
    }

    #[test]
    fn reading_data_clears_irq_flags() {
        let mut pia = Pia6821::new();
        pia.write(0xD011, 0x03); // low-to-high active, IRQ enabled
        pia.set_ca1(true);
        pia.set_ca1(false);
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
        pia.set_ca1(false);
        pia.set_cb1(true);
        pia.set_cb1(false);
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
        pia.write(0xD011, 0x03); // low-to-high active, IRQ enabled
        pia.set_ca1(true);
        pia.set_ca1(false);
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
        pia.set_ca1(false);
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
        pia.set_ca1(true);
        pia.set_ca1(false);
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
        pia.set_ca1(true);
        pia.set_ca1(false);
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

    /// Apple I display setup: PB0-PB6 outputs, PB7 input, CRB selecting
    /// the output register with the write-strobe-with-CB1 handshake and a
    /// rising-edge CB1.
    fn configured(pia: &mut Pia6821) {
        pia.write(0xD012, 0x7F); // DDRB: PB0-PB6 outputs
        pia.write(0xD013, 0x27); // CRB: mode 100, ORB, rising CB1
    }

    #[test]
    fn write_strobe_waits_for_the_enable_after_the_orb_write() {
        let mut pia = Pia6821::new();
        configured(&mut pia);
        assert!(pia.cb2_level(), "the terminal starts ready");

        pia.write(0xD012, 0x41);
        assert!(
            pia.cb2_level(),
            "writing the output register must not pull CB2 down by itself"
        );
        assert!(pia.output_strobe_pending(), "a strobe is waiting");

        pia.e_rising_edge();
        assert!(
            !pia.cb2_level(),
            "the first enable after the write pulls CB2 down"
        );
        assert!(
            pia.output_strobe_pending(),
            "CB2 low is still an unfinished handshake"
        );
    }

    #[test]
    fn a_control_or_ddr_access_does_not_ask_for_a_strobe() {
        let mut pia = Pia6821::new();
        pia.write(0xD012, 0x7F); // DDRB write
        pia.write(0xD013, 0x27); // CRB write
        let _ = pia.read(0xD012); // DDRB read
        let _ = pia.read(0xD013); // CRB read
        pia.e_rising_edge();
        assert!(pia.cb2_level(), "only an ORB write arms the handshake");
        assert!(!pia.output_strobe_pending());
    }

    #[test]
    fn cb1_active_edge_releases_cb2_and_sets_the_flag() {
        let mut pia = Pia6821::new();
        configured(&mut pia);
        pia.write(0xD012, 0x41);
        pia.e_rising_edge();
        assert!(!pia.cb2_level());

        // The terminal's B3 pulse: CB1 goes low, then back high.
        pia.set_cb1(true);
        assert!(
            !pia.cb2_level(),
            "CB2 is released by the active edge, not the level"
        );
        pia.set_cb1(false);
        assert!(pia.cb2_level(), "the rising CB1 edge returned CB2 high");
        assert_eq!(pia.crb & 0x80, 0x80, "IRQB1 latched");
        assert!(!pia.output_strobe_pending());
    }

    #[test]
    fn an_unread_acknowledge_does_not_advance_the_handshake() {
        let mut pia = Pia6821::new();
        configured(&mut pia);
        pia.write(0xD012, 0x41);
        pia.e_rising_edge();

        // First acknowledge: flag sets and CB2 returns high.
        pia.set_cb1(true);
        pia.set_cb1(false);
        assert!(pia.cb2_level());

        // Software has not read $D012, so the flag is still set: a second
        // acknowledge must not be consumed.
        pia.write(0xD012, 0x42);
        pia.e_rising_edge();
        assert!(!pia.cb2_level());
        pia.set_cb1(true);
        pia.set_cb1(false);
        assert!(
            !pia.cb2_level(),
            "an acknowledge while the previous one is unread must not release CB2"
        );

        // Reading the data register clears the flag and lets the next
        // acknowledge through.
        let _ = pia.read(0xD012);
        assert_eq!(pia.crb & 0x80, 0);
        pia.set_cb1(true);
        pia.set_cb1(false);
        assert!(pia.cb2_level());
    }

    #[test]
    fn write_strobe_with_e_returns_high_on_the_following_enable() {
        let mut pia = Pia6821::new();
        pia.write(0xD012, 0x7F);
        pia.write(0xD013, 0x2F); // CRB: mode 101, ORB, rising CB1
        pia.write(0xD012, 0x41);

        pia.e_rising_edge();
        assert!(!pia.cb2_level(), "the strobe enable pulls CB2 down");
        pia.e_rising_edge();
        assert!(pia.cb2_level(), "the next enable returns it high");
        // This mode does not use CB1 to release the strobe.
        pia.e_rising_edge();
        assert!(pia.cb2_level());
    }

    #[test]
    fn manual_cb2_modes_take_their_level_from_the_control_bits() {
        let mut pia = Pia6821::new();
        pia.write(0xD013, 0x37); // mode 110: manual low
        assert!(!pia.cb2_level());
        pia.write(0xD013, 0x3F); // mode 111: manual high
        assert!(pia.cb2_level());

        // A write strobe left over from a previous mode must not survive
        // the mode change.
        pia.write(0xD013, 0x27); // mode 100
        pia.write(0xD012, 0x41);
        pia.write(0xD013, 0x37); // switch to manual low before the enable
        pia.e_rising_edge();
        pia.write(0xD013, 0x3F); // manual high
        assert!(pia.cb2_level());
    }

    #[test]
    fn input_mode_leaves_cb2_to_the_pin_and_never_strobes() {
        let mut pia = Pia6821::new();
        pia.write(0xD012, 0x7F);
        pia.write(0xD013, 0x07); // CRB: mode 000, ORB, rising CB1
        assert!(pia.cb2_level(), "an undriven CB2 pin floats high");

        pia.write(0xD012, 0x41);
        pia.e_rising_edge();
        assert!(pia.cb2_level(), "input mode has no output strobe");
        assert!(!pia.output_strobe_pending());
    }

    #[test]
    fn data_lines_drive_from_the_output_register_and_float_high_otherwise() {
        let mut pia = Pia6821::new();
        // No outputs configured: every line floats high.
        assert_eq!(pia.data_lines(), 0x7F);

        pia.write(0xD012, 0x0F); // PB0-PB3 outputs
        pia.write(0xD013, 0x04); // select ORB
        pia.write(0xD012, 0x55);
        assert_eq!(
            pia.data_lines(),
            0x55 & 0x0F | 0x7F & !0x0F,
            "only the driven lines follow the output register"
        );

        // Held RESET releases the output drivers.
        pia.set_reset_line(true);
        assert_eq!(pia.data_lines(), 0x7F);
        assert!(pia.cb2_level());
        assert!(!pia.output_strobe_pending());
    }
}
