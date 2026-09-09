use std::fmt;

use crate::{
    Bus,
    instruction::{Mode, Op, decode},
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
    /// Future PHP/BRK/interrupt stack images must encode B separately.
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
}

/// Trace data captured during execution; formatting it requires no bus reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Step {
    pub address: u16,
    pub opcode: u8,
    pub before: Registers,
    pub after: Registers,
    pub cycles: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CpuError {
    UnsupportedOpcode { address: u16, opcode: u8 },
}

impl fmt::Display for CpuError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnsupportedOpcode { address, opcode } => {
                write!(f, "unsupported opcode ${opcode:02X} at ${address:04X}")
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

    /// Explicit state injection for debugging/tests. This does not perform RESET.
    pub fn from_registers(registers: Registers) -> Self {
        Self { registers }
    }

    pub fn registers(&self) -> Registers {
        self.registers
    }

    /// Instruction-level NMOS RESET: set I, decrement SP by 3, fetch $FFFC/$FFFD.
    /// Preserve A/X/Y and N/V/D/Z/C. No stack writes; dummy reads are omitted.
    /// Returns 7 cycles, excluding the first instruction and reset-pin hold time.
    pub fn reset(&mut self, bus: &mut dyn Bus) -> u8 {
        self.registers.status.interrupt_disable = true;
        self.registers.sp = self.registers.sp.wrapping_sub(3);
        let lo = bus.read(0xfffc);
        let hi = bus.read(0xfffd);
        self.registers.pc = u16::from_le_bytes([lo, hi]);
        7
    }

    /// Execute one instruction and count cycles, without cycle-exact bus timing.
    /// Unsupported opcodes (including BRK in M0) leave registers unchanged.
    /// The opcode fetch has already occurred and its bus side effects remain.
    pub fn step(&mut self, bus: &mut dyn Bus) -> Result<Step, CpuError> {
        let before = self.registers;
        let opcode = self.fetch(bus);
        let Some((op, mode, mut cycles)) = decode(opcode) else {
            self.registers.pc = before.pc;
            return Err(CpuError::UnsupportedOpcode {
                address: before.pc,
                opcode,
            });
        };
        use Op::*;
        match op {
            Lda | Ldx | Ldy | And | Ora | Eor | Bit | Cmp | Cpx | Cpy => {
                let (value, crossed) = self.read_operand(bus, mode);
                cycles += u8::from(crossed);
                match op {
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
                    _ => unreachable!("read-operation dispatch is exhaustive"),
                }
            }
            Sta | Stx | Sty => {
                let (addr, _) = self.address(bus, mode);
                let value = match op {
                    Sta => self.registers.a,
                    Stx => self.registers.x,
                    _ => self.registers.y,
                };
                bus.write(addr, value);
            }
            Asl | Lsr | Rol | Ror | Inc | Dec => {
                let addr = if mode == Mode::Acc {
                    None
                } else {
                    Some(self.address(bus, mode).0)
                };
                let value = match addr {
                    Some(addr) => bus.read(addr),
                    None => self.registers.a,
                };
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
                    _ => unreachable!("modify-operation dispatch is exhaustive"),
                };
                if let Some(addr) = addr {
                    // NMOS read/modify/write writes the old value before the new one.
                    bus.write(addr, value);
                    bus.write(addr, result);
                } else {
                    self.registers.a = result;
                }
                self.registers.status.set_nz(result);
            }
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
            Bcc => cycles = self.branch(bus, !self.registers.status.carry),
            Bcs => cycles = self.branch(bus, self.registers.status.carry),
            Beq => cycles = self.branch(bus, self.registers.status.zero),
            Bmi => cycles = self.branch(bus, self.registers.status.negative),
            Bne => cycles = self.branch(bus, !self.registers.status.zero),
            Bpl => cycles = self.branch(bus, !self.registers.status.negative),
            Bvc => cycles = self.branch(bus, !self.registers.status.overflow),
            Bvs => cycles = self.branch(bus, self.registers.status.overflow),
            Jmp => {
                let addr = self.fetch_word(bus);
                self.registers.pc = if mode == Mode::Ind {
                    let lo = bus.read(addr);
                    let hi_addr = (addr & 0xff00) | (addr.wrapping_add(1) & 0x00ff);
                    u16::from_le_bytes([lo, bus.read(hi_addr)])
                } else {
                    addr
                };
            }
            Jsr => {
                let lo = self.fetch(bus);
                // Save the final operand address, high first; read high AFTER writes.
                let [return_lo, return_hi] = self.registers.pc.to_le_bytes();
                self.push(bus, return_hi);
                self.push(bus, return_lo);
                let hi = self.fetch(bus);
                self.registers.pc = u16::from_le_bytes([lo, hi]);
            }
            Rts => {
                let lo = self.pull(bus);
                let hi = self.pull(bus);
                self.registers.pc = u16::from_le_bytes([lo, hi]).wrapping_add(1);
            }
            Pha => self.push(bus, self.registers.a),
            Php => self.push(bus, self.registers.status.bits() | 0x10),
            Pla => {
                let value = self.pull(bus);
                self.load_a(value);
            }
            Plp => self.registers.status = Status::from_bits(self.pull(bus)),
            Clc => self.registers.status.carry = false,
            Cld => self.registers.status.decimal = false,
            Cli => self.registers.status.interrupt_disable = false,
            Clv => self.registers.status.overflow = false,
            Sec => self.registers.status.carry = true,
            Sed => self.registers.status.decimal = true,
            Sei => self.registers.status.interrupt_disable = true,
            Nop => {}
        }
        Ok(Step {
            address: before.pc,
            opcode,
            before,
            after: self.registers,
            cycles,
        })
    }

    fn fetch(&mut self, bus: &mut dyn Bus) -> u8 {
        let value = bus.read(self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        value
    }

    fn fetch_word(&mut self, bus: &mut dyn Bus) -> u16 {
        let lo = self.fetch(bus);
        let hi = self.fetch(bus);
        u16::from_le_bytes([lo, hi])
    }

    fn read_operand(&mut self, bus: &mut dyn Bus, mode: Mode) -> (u8, bool) {
        if mode == Mode::Imm {
            return (self.fetch(bus), false);
        }
        let (addr, crossed) = self.address(bus, mode);
        (bus.read(addr), crossed)
    }

    fn address(&mut self, bus: &mut dyn Bus, mode: Mode) -> (u16, bool) {
        let (base, index) = match mode {
            Mode::Zp => return (u16::from(self.fetch(bus)), false),
            Mode::Zpx | Mode::Zpy => {
                let index = if mode == Mode::Zpx {
                    self.registers.x
                } else {
                    self.registers.y
                };
                return (u16::from(self.fetch(bus).wrapping_add(index)), false);
            }
            Mode::Abs => return (self.fetch_word(bus), false),
            Mode::Abx => (self.fetch_word(bus), self.registers.x),
            Mode::Aby => (self.fetch_word(bus), self.registers.y),
            Mode::Izx => {
                let pointer = self.fetch(bus).wrapping_add(self.registers.x);
                return (Self::zero_page_word(bus, pointer), false);
            }
            Mode::Izy => {
                let pointer = self.fetch(bus);
                (Self::zero_page_word(bus, pointer), self.registers.y)
            }
            // Private decoder + execution match only call this for memory modes.
            // All 256 opcode bytes are checked by the independent contract tests.
            _ => unreachable!("non-memory mode sent to address resolver"),
        };
        let addr = base.wrapping_add(u16::from(index));
        (addr, (base & 0xff00) != (addr & 0xff00))
    }

    fn zero_page_word(bus: &mut dyn Bus, pointer: u8) -> u16 {
        let lo = bus.read(u16::from(pointer));
        let hi = bus.read(u16::from(pointer.wrapping_add(1)));
        u16::from_le_bytes([lo, hi])
    }

    fn compare(&mut self, register: u8, operand: u8) {
        self.registers.status.carry = register >= operand;
        self.registers.status.set_nz(register.wrapping_sub(operand));
    }

    fn load_a(&mut self, value: u8) {
        self.registers.a = value;
        self.registers.status.set_nz(value);
    }

    fn branch(&mut self, bus: &mut dyn Bus, taken: bool) -> u8 {
        let offset = self.fetch(bus) as i8;
        if !taken {
            return 2;
        }
        let next_pc = self.registers.pc;
        self.registers.pc = next_pc.wrapping_add_signed(i16::from(offset));
        3 + u8::from((next_pc & 0xff00) != (self.registers.pc & 0xff00))
    }

    fn push(&mut self, bus: &mut dyn Bus, value: u8) {
        bus.write(0x0100 | u16::from(self.registers.sp), value);
        self.registers.sp = self.registers.sp.wrapping_sub(1);
    }

    fn pull(&mut self, bus: &mut dyn Bus) -> u8 {
        self.registers.sp = self.registers.sp.wrapping_add(1);
        bus.read(0x0100 | u16::from(self.registers.sp))
    }
}
