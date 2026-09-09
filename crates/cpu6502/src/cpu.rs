use std::fmt;

use crate::Bus;

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
        let cycles = match opcode {
            0xa9 => {
                let value = self.fetch(bus);
                self.load_a(value);
                2
            }
            0xa5 => {
                let addr = u16::from(self.fetch(bus));
                self.load_a(bus.read(addr));
                3
            }
            0xad => {
                let addr = self.fetch_word(bus);
                self.load_a(bus.read(addr));
                4
            }
            0xa2 => {
                self.registers.x = self.fetch(bus);
                self.registers.status.set_nz(self.registers.x);
                2
            }
            0x85 => {
                let addr = u16::from(self.fetch(bus));
                bus.write(addr, self.registers.a);
                3
            }
            0x8d => {
                let addr = self.fetch_word(bus);
                bus.write(addr, self.registers.a);
                4
            }
            0x9d => {
                let base = self.fetch_word(bus);
                let addr = base.wrapping_add(u16::from(self.registers.x));
                bus.write(addr, self.registers.a);
                // Stores always take 5 cycles, even without a page crossing.
                5
            }
            0xaa => {
                self.registers.x = self.registers.a;
                self.registers.status.set_nz(self.registers.x);
                2
            }
            0x8a => {
                self.load_a(self.registers.x);
                2
            }
            0x9a => {
                self.registers.sp = self.registers.x;
                2
            }
            0xe8 => {
                self.registers.x = self.registers.x.wrapping_add(1);
                self.registers.status.set_nz(self.registers.x);
                2
            }
            0xca => {
                self.registers.x = self.registers.x.wrapping_sub(1);
                self.registers.status.set_nz(self.registers.x);
                2
            }
            0xe0 => {
                let operand = self.fetch(bus);
                let result = self.registers.x.wrapping_sub(operand);
                self.registers.status.carry = self.registers.x >= operand;
                self.registers.status.set_nz(result);
                2
            }
            0xd0 => self.branch(bus, !self.registers.status.zero),
            0xf0 => self.branch(bus, self.registers.status.zero),
            0x4c => {
                self.registers.pc = self.fetch_word(bus);
                3
            }
            0x6c => {
                let pointer = self.fetch_word(bus);
                let lo = bus.read(pointer);
                // NMOS: only the pointer's low byte increments ($xxFF -> $xx00).
                let hi_addr = (pointer & 0xff00) | (pointer.wrapping_add(1) & 0x00ff);
                let hi = bus.read(hi_addr);
                self.registers.pc = u16::from_le_bytes([lo, hi]);
                5
            }
            0x20 => {
                let lo = self.fetch(bus);
                // PC now points to JSR's final byte, which is the saved address.
                let [return_lo, return_hi] = self.registers.pc.to_le_bytes();
                self.push(bus, return_hi);
                self.push(bus, return_lo);
                // Fetch high after stack writes: matters if code overlaps stack.
                let hi = self.fetch(bus);
                self.registers.pc = u16::from_le_bytes([lo, hi]);
                6
            }
            0x60 => {
                let lo = self.pull(bus);
                let hi = self.pull(bus);
                self.registers.pc = u16::from_le_bytes([lo, hi]).wrapping_add(1);
                6
            }
            0x48 => {
                self.push(bus, self.registers.a);
                3
            }
            0x68 => {
                let value = self.pull(bus);
                self.load_a(value);
                4
            }
            0x18 => {
                self.registers.status.carry = false;
                2
            }
            0x38 => {
                self.registers.status.carry = true;
                2
            }
            0xd8 => {
                self.registers.status.decimal = false;
                2
            }
            0xea => 2,
            _ => {
                self.registers.pc = before.pc;
                return Err(CpuError::UnsupportedOpcode {
                    address: before.pc,
                    opcode,
                });
            }
        };
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
