use std::fmt;

use crate::instruction::Op;

mod cycle;
use cycle::Execution;
pub use cycle::{
    BusCycle, ClockPhase, Cycle, DebugState, Direction, ExecutionState, InputPins,
    Phase as ExecutionPhase, PinLatches,
};

/// The six stored NMOS status flags. B is not a persistent hardware flag.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Status {
    pub carry: bool,
    pub zero: bool,
    pub interrupt_disable: bool,
    pub decimal: bool,
    pub overflow: bool,
    pub negative: bool,
}

impl Status {
    /// Debugger representation: bit 5 is 1 and B (bit 4) is 0.
    /// PHP/BRK/interrupt stack images encode B separately.
    pub fn bits(self) -> u8 {
        u8::from(self.carry)
            | (u8::from(self.zero) << 1)
            | (u8::from(self.interrupt_disable) << 2)
            | (u8::from(self.decimal) << 3)
            | 0x20
            | (u8::from(self.overflow) << 6)
            | (u8::from(self.negative) << 7)
    }

    /// Restore the six stored flags; ignore bits 4 and 5.
    pub fn from_bits(bits: u8) -> Self {
        Self {
            carry: bits & 0x01 != 0,
            zero: bits & 0x02 != 0,
            interrupt_disable: bits & 0x04 != 0,
            decimal: bits & 0x08 != 0,
            overflow: bits & 0x40 != 0,
            negative: bits & 0x80 != 0,
        }
    }

    fn set_nz(&mut self, value: u8) {
        self.zero = value == 0;
        self.negative = value & 0x80 != 0;
    }
}

/// A value snapshot, also usable for explicit debugger/test state restoration.
/// Default zeros are an emulator convention, NOT hardware power-on guarantees.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Registers {
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub sp: u8,
    pub pc: u16,
    pub status: Status,
}

#[derive(Debug, Default)]
pub struct Cpu {
    registers: Registers,
    execution: Option<Execution>,
    irq_line: bool,
    irq_sample: bool,
    irq_pending: bool,
    nmi_line: bool,
    nmi_sample: bool,
    nmi_edge: bool,
    nmi_pending: bool,
    not_ready: bool,
    so_line: bool,
    so_sample: bool,
    so_pending: bool,
    v_write_pending: [Option<bool>; 2],
    phi2: bool,
    fetch_wait: Option<(Registers, u64)>,
}

/// Hardware interrupt entry is a separate step, not an executed BRK opcode.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StepKind {
    Instruction { opcode: u8 },
    Irq,
    Nmi,
    Reset,
}

/// Trace data captured during execution; formatting it requires no bus reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub address: u16,
    pub kind: StepKind,
    pub before: Registers,
    pub after: Registers,
    pub cycles: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    UnsupportedOpcode { address: u16, opcode: u8 },
    CycleBudgetExceeded { address: u16, budget: u64 },
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOpcode { address, opcode } => {
                write!(f, "unsupported opcode ${opcode:02X} at ${address:04X}")
            }
            Self::CycleBudgetExceeded { address, budget } => {
                write!(
                    f,
                    "cycle budget {budget} exhausted at ${address:04X}; CPU can be resumed"
                )
            }
        }
    }
}

impl std::error::Error for CpuError {}

impl Cpu {
    /// Reproducible initial state: A/X/Y/SP/PC = 0, stored flags clear (P=$20).
    /// Call `reset` with a bus to start through the hardware reset vector.
    pub fn new() -> Self {
        Self::default()
    }

    /// Explicit register injection for debugging/tests, with inactive interrupt
    /// lines and no pending interrupts. This is neither RESET nor a full savestate.
    pub fn from_registers(registers: Registers) -> Self {
        Self {
            registers,
            ..Self::default()
        }
    }

    pub fn registers(&self) -> Registers {
        self.registers
    }

    fn read_operation(&mut self, op: Op, value: u8) {
        use Op::*;
        match op {
            Adc => self.adc(value),
            Sbc => self.sbc(value),
            Lda => self.load_a(value),
            Ldx => {
                self.registers.x = value;
                self.registers.status.set_nz(value);
            }
            Ldy => {
                self.registers.y = value;
                self.registers.status.set_nz(value);
            }
            And => self.load_a(self.registers.a & value),
            Ora => self.load_a(self.registers.a | value),
            Eor => self.load_a(self.registers.a ^ value),
            Bit => {
                self.registers.status.zero = self.registers.a & value == 0;
                self.registers.status.negative = value & 0x80 != 0;
                self.registers.status.overflow = value & 0x40 != 0;
            }
            Cmp => self.compare(self.registers.a, value),
            Cpx => self.compare(self.registers.x, value),
            Cpy => self.compare(self.registers.y, value),
            _ => unreachable!("private decoder sent non-read operation to ALU"),
        }
        if matches!(op, Adc | Sbc) {
            self.v_write_pending[1] = Some(self.registers.status.overflow);
        } else if op == Bit {
            self.v_write_pending[0] = Some(self.registers.status.overflow);
        }
    }

    fn modify(&mut self, op: Op, value: u8) -> u8 {
        use Op::*;
        let carry = u8::from(self.registers.status.carry);
        let result = match op {
            Asl => {
                self.registers.status.carry = value & 0x80 != 0;
                value.wrapping_shl(1)
            }
            Lsr => {
                self.registers.status.carry = value & 1 != 0;
                value >> 1
            }
            Rol => {
                self.registers.status.carry = value & 0x80 != 0;
                value.wrapping_shl(1) | carry
            }
            Ror => {
                self.registers.status.carry = value & 1 != 0;
                (value >> 1) | (carry << 7)
            }
            Inc => value.wrapping_add(1),
            Dec => value.wrapping_sub(1),
            _ => unreachable!("private decoder sent non-modifying operation to ALU"),
        };
        self.registers.status.set_nz(result);
        result
    }

    fn implied(&mut self, op: Op) {
        use Op::*;
        match op {
            Tax | Tsx => {
                self.registers.x = if op == Tax {
                    self.registers.a
                } else {
                    self.registers.sp
                };
                self.registers.status.set_nz(self.registers.x);
            }
            Tay => {
                self.registers.y = self.registers.a;
                self.registers.status.set_nz(self.registers.y);
            }
            Txa => self.load_a(self.registers.x),
            Tya => self.load_a(self.registers.y),
            Txs => self.registers.sp = self.registers.x,
            Inx | Dex => {
                self.registers.x = if op == Inx {
                    self.registers.x.wrapping_add(1)
                } else {
                    self.registers.x.wrapping_sub(1)
                };
                self.registers.status.set_nz(self.registers.x);
            }
            Iny | Dey => {
                self.registers.y = if op == Iny {
                    self.registers.y.wrapping_add(1)
                } else {
                    self.registers.y.wrapping_sub(1)
                };
                self.registers.status.set_nz(self.registers.y);
            }
            Clc => self.registers.status.carry = false,
            Cld => self.registers.status.decimal = false,
            Cli => self.registers.status.interrupt_disable = false,
            Clv => self.registers.status.overflow = false,
            Sec => self.registers.status.carry = true,
            Sed => self.registers.status.decimal = true,
            Sei => self.registers.status.interrupt_disable = true,
            Nop => {}
            _ => unreachable!("private decoder sent non-implied operation to ALU"),
        }
        if op == Clv {
            self.v_write_pending = [Some(false); 2];
        }
    }

    fn branch_taken(&self, op: Op) -> bool {
        use Op::*;
        let p = self.registers.status;
        match op {
            Bcc => !p.carry,
            Bcs => p.carry,
            Beq => p.zero,
            Bne => !p.zero,
            Bmi => p.negative,
            Bpl => !p.negative,
            Bvc => !p.overflow,
            Bvs => p.overflow,
            _ => unreachable!("private decoder sent non-branch operation to branch condition"),
        }
    }

    /// Set the IRQ input level (`true` means the active-low pin is asserted).
    /// Sampled each cycle; hold through a sampling point for a valid request.
    /// Deasserting the line does not cancel an interrupt already polled.
    pub fn set_irq_line(&mut self, asserted: bool) {
        self.irq_line = asserted;
    }

    /// Set the NMI input level (`true` means the active-low pin is asserted).
    /// A sampled false-to-true transition latches an edge. Instruction polling
    /// and vector selection consume it; a pulse entirely between cycles is missed.
    pub fn set_nmi_line(&mut self, asserted: bool) {
        self.nmi_line = asserted;
    }

    /// RDY high allows progress. Low repeats reads; NMOS writes keep progressing.
    pub fn set_ready(&mut self, ready: bool) {
        self.not_ready = !ready;
    }

    /// Active-low SO. Sampled at phi1; a new falling edge sets V one cycle later.
    pub fn set_so_line(&mut self, asserted: bool) {
        self.so_line = asserted;
    }

    fn sample_interrupt_pins(&mut self, interrupt_disable: bool) {
        self.irq_sample = self.irq_line && !interrupt_disable;
        if self.nmi_line && !self.nmi_sample {
            self.nmi_edge = true;
        }
        self.nmi_sample = self.nmi_line;
    }

    fn adc(&mut self, operand: u8) {
        let a = self.registers.a;
        let carry = u16::from(self.registers.status.carry);
        let binary = u16::from(a) + u16::from(operand) + carry;
        if self.registers.status.decimal {
            // NMOS: low-digit correction precedes N/V, high correction follows.
            // Even invalid BCD digits produce only ONE low-digit carry.
            let low = u16::from(a & 0x0f) + u16::from(operand & 0x0f) + carry;
            let low = if low > 9 {
                ((low + 6) & 0x0f) | 0x10
            } else {
                low
            };
            let intermediate = u16::from(a & 0xf0) + u16::from(operand & 0xf0) + low;
            self.registers.status.zero = binary as u8 == 0;
            self.registers.status.negative = intermediate & 0x80 != 0;
            self.registers.status.overflow =
                (!(a ^ operand) & (a ^ intermediate as u8) & 0x80) != 0;
            let corrected = if intermediate >= 0xa0 {
                intermediate + 0x60
            } else {
                intermediate
            };
            self.registers.status.carry = corrected > 0xff;
            self.registers.a = corrected as u8;
        } else {
            self.registers.status.carry = binary > 0xff;
            self.registers.status.overflow = (!(a ^ operand) & (a ^ binary as u8) & 0x80) != 0;
            self.load_a(binary as u8);
        }
    }

    fn sbc(&mut self, operand: u8) {
        let a = self.registers.a;
        let borrow = i16::from(!self.registers.status.carry);
        let binary = i16::from(a) - i16::from(operand) - borrow;
        // All four SBC flags are taken from the binary subtraction, even in D=1.
        self.registers.status.carry = binary >= 0;
        self.registers.status.overflow = ((a ^ operand) & (a ^ binary as u8) & 0x80) != 0;
        self.registers.status.set_nz(binary as u8);
        self.registers.a = if self.registers.status.decimal {
            let mut low = i16::from(a & 0x0f) - i16::from(operand & 0x0f) - borrow;
            let mut high = i16::from(a >> 4) - i16::from(operand >> 4);
            if low < 0 {
                low -= 6;
                high -= 1;
            }
            if high < 0 {
                high -= 6;
            }
            (((high << 4) & 0xf0) | (low & 0x0f)) as u8
        } else {
            binary as u8
        };
    }

    fn compare(&mut self, register: u8, operand: u8) {
        self.registers.status.carry = register >= operand;
        self.registers.status.set_nz(register.wrapping_sub(operand));
    }

    fn load_a(&mut self, value: u8) {
        self.registers.a = value;
        self.registers.status.set_nz(value);
    }
}
