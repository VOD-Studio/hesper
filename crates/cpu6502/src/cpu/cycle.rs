//! NMOS bus sequencer. Each phase performs exactly one real Bus access.

use super::{Cpu, CpuError, Registers, Status, Step, StepKind};
use crate::{
    Bus,
    instruction::{Mode, Op, decode},
};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    Read,
    Write,
}

/// Data captured by the actual bus transaction, including discarded reads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BusCycle {
    pub address: u16,
    pub data: u8,
    pub direction: Direction,
    pub sync: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cycle {
    pub bus: BusCycle,
    /// Present only when this cycle completes an instruction or entry sequence.
    pub completed: Option<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    Fetch,
    Implied,
    Low,
    High,
    ZeroIndex,
    PointerLow,
    PointerHigh,
    Indexed,
    Memory,
    RmwOld,
    RmwNew,
    Relative,
    BranchTaken,
    BranchCross,
    StackDummyPc,
    StackDummy,
    PushRegister,
    PullRegister,
    ReturnLow,
    ReturnHigh,
    ReturnDummy,
    JsrDummy,
    JsrPushHigh,
    JsrPushLow,
    JsrHigh,
    Padding,
    PushHigh,
    PushLow,
    PushStatus,
    VectorLow,
    VectorHigh,
}

#[derive(Debug)]
pub(super) struct Execution {
    kind: StepKind,
    op: Op,
    mode: Mode,
    phase: Phase,
    before: Registers,
    cycles: u8,
    low: u8,
    value: u8,
    base: u16,
    address: u16,
}

impl Execution {
    fn new(kind: StepKind, before: Registers, op: Op, mode: Mode) -> Self {
        Self {
            kind,
            op,
            mode,
            before,
            phase: Phase::Fetch,
            cycles: 0,
            low: 0,
            value: 0,
            base: 0,
            address: 0,
        }
    }

    fn first_phase(&self) -> Phase {
        use Op::*;
        match self.op {
            Brk => Phase::Padding,
            Pha | Php | Pla | Plp | Rti | Rts => Phase::StackDummyPc,
            _ => match self.mode {
                Mode::Imp | Mode::Acc => Phase::Implied,
                Mode::Rel => Phase::Relative,
                _ => Phase::Low,
            },
        }
    }

    fn store(&self) -> bool {
        matches!(self.op, Op::Sta | Op::Stx | Op::Sty)
    }
    fn modify(&self) -> bool {
        matches!(
            self.op,
            Op::Asl | Op::Lsr | Op::Rol | Op::Ror | Op::Inc | Op::Dec
        )
    }
    fn crossed(&self) -> bool {
        self.base & 0xff00 != self.address & 0xff00
    }
    fn intermediate(&self) -> u16 {
        (self.base & 0xff00) | (self.address & 0xff)
    }
}

fn read(bus: &mut dyn Bus, address: u16) -> BusCycle {
    BusCycle {
        address,
        data: bus.read(address),
        direction: Direction::Read,
        sync: false,
    }
}
fn write(bus: &mut dyn Bus, address: u16, data: u8) -> BusCycle {
    bus.write(address, data);
    BusCycle {
        address,
        data,
        direction: Direction::Write,
        sync: false,
    }
}

impl Cpu {
    /// True when the next cycle starts an instruction or interrupt entry.
    pub fn at_instruction_boundary(&self) -> bool {
        self.execution.is_none()
    }

    /// Begin the seven-cycle RESET entry, discarding any unfinished instruction.
    /// This host request does not yet model the physical RESET pin hold time.
    pub fn begin_reset(&mut self) {
        self.irq_pending = false;
        self.nmi_pending = false;
        self.execution = Some(Execution::new(
            StepKind::Reset,
            self.registers,
            Op::Brk,
            Mode::Imp,
        ));
    }

    /// Perform the same seven bus cycles exposed by `begin_reset` + `cycle`.
    /// Preserves A/X/Y and N/V/D/Z/C; reads (never writes) three stack locations.
    pub fn reset(&mut self, bus: &mut dyn Bus) -> u8 {
        self.begin_reset();
        let mut count = 0;
        // RESET cannot encounter an opcode error; it never decodes memory.
        while let Some(mut execution) = self.execution.take() {
            let (_, done) = self.advance(bus, &mut execution);
            count += 1;
            if !done {
                self.execution = Some(execution);
            }
        }
        count
    }

    /// Advance one CPU bus cycle. No speculative or logging-only bus accesses.
    /// IRQ/NMI sampling is still the M1 boundary convention until the pin model
    /// is upgraded; the instruction bus sequences themselves are cycle driven.
    pub fn cycle(&mut self, bus: &mut dyn Bus) -> Result<Cycle, CpuError> {
        let mut execution = if let Some(execution) = self.execution.take() {
            execution
        } else if self.nmi_pending || self.irq_pending {
            let kind = if self.nmi_pending {
                StepKind::Nmi
            } else {
                StepKind::Irq
            };
            self.nmi_pending = false;
            self.irq_pending = false;
            Execution::new(kind, self.registers, Op::Brk, Mode::Imp)
        } else {
            let mut access = read(bus, self.registers.pc);
            access.sync = true;
            let opcode = access.data;
            let Some((op, mode, _)) = decode(opcode) else {
                return Err(CpuError::UnsupportedOpcode {
                    address: access.address,
                    opcode,
                });
            };
            let mut execution =
                Execution::new(StepKind::Instruction { opcode }, self.registers, op, mode);
            self.registers.pc = self.registers.pc.wrapping_add(1);
            execution.cycles = 1;
            execution.phase = execution.first_phase();
            self.execution = Some(execution);
            return Ok(Cycle {
                bus: access,
                completed: None,
            });
        };
        let (access, done) = self.advance(bus, &mut execution);
        execution.cycles += 1;
        let completed = if done {
            // Preserved M1 interrupt convention; bus sequencing is independent.
            if matches!(execution.kind, StepKind::Instruction { .. }) {
                let i = if matches!(execution.op, Op::Cli | Op::Sei | Op::Plp) {
                    execution.before.status.interrupt_disable
                } else {
                    self.registers.status.interrupt_disable
                };
                self.irq_pending = execution.op != Op::Brk && self.irq_line && !i;
            }
            Some(Step {
                address: execution.before.pc,
                kind: execution.kind,
                before: execution.before,
                after: self.registers,
                cycles: execution.cycles,
            })
        } else {
            self.execution = Some(execution);
            None
        };
        Ok(Cycle {
            bus: access,
            completed,
        })
    }

    /// Finish the current instruction/entry, or start one at a boundary.
    /// The returned count and `before` snapshot cover the entire operation even
    /// when the caller has already advanced some of its cycles individually.
    pub fn step(&mut self, bus: &mut dyn Bus) -> Result<Step, CpuError> {
        loop {
            if let Some(step) = self.cycle(bus)?.completed {
                return Ok(step);
            }
        }
    }

    fn stack_address(&self) -> u16 {
        0x100 | u16::from(self.registers.sp)
    }

    fn fetch_cycle(&mut self, bus: &mut dyn Bus) -> BusCycle {
        let access = read(bus, self.registers.pc);
        self.registers.pc = self.registers.pc.wrapping_add(1);
        access
    }

    fn advance(&mut self, bus: &mut dyn Bus, e: &mut Execution) -> (BusCycle, bool) {
        use Phase::*;
        let mut done = false;
        let access = match e.phase {
            Fetch => {
                let mut access = read(bus, self.registers.pc);
                access.sync = true;
                e.phase = Padding;
                access
            }
            Implied => {
                let access = read(bus, self.registers.pc);
                if e.mode == Mode::Acc {
                    self.registers.a = self.modify(e.op, self.registers.a);
                } else {
                    self.implied(e.op);
                }
                done = true;
                access
            }
            Low => {
                let access = self.fetch_cycle(bus);
                e.low = access.data;
                e.base = u16::from(e.low);
                e.address = e.base;
                e.phase = match e.mode {
                    Mode::Imm => {
                        self.read_operation(e.op, e.low);
                        done = true;
                        Low
                    }
                    Mode::Zp => Memory,
                    Mode::Zpx | Mode::Zpy | Mode::Izx => ZeroIndex,
                    Mode::Izy => PointerLow,
                    _ if e.op == Op::Jsr => JsrDummy,
                    _ => High,
                };
                access
            }
            High => {
                let access = self.fetch_cycle(bus);
                e.base = u16::from_le_bytes([e.low, access.data]);
                e.address = e.base;
                if e.op == Op::Jmp && e.mode == Mode::Abs {
                    self.registers.pc = e.address;
                    done = true;
                } else if e.mode == Mode::Ind {
                    e.phase = PointerLow;
                } else if matches!(e.mode, Mode::Abx | Mode::Aby) {
                    let index = if e.mode == Mode::Abx {
                        self.registers.x
                    } else {
                        self.registers.y
                    };
                    e.address = e.base.wrapping_add(u16::from(index));
                    e.phase = Indexed;
                } else {
                    e.phase = Memory;
                }
                access
            }
            ZeroIndex => {
                let access = read(bus, e.base);
                let index = if e.mode == Mode::Zpy {
                    self.registers.y
                } else {
                    self.registers.x
                };
                e.base = u16::from(e.low.wrapping_add(index));
                e.address = e.base;
                e.phase = if e.mode == Mode::Izx {
                    PointerLow
                } else {
                    Memory
                };
                access
            }
            PointerLow => {
                let access = read(bus, e.base);
                e.low = access.data;
                e.phase = PointerHigh;
                access
            }
            PointerHigh => {
                // Both the zero-page pointer and NMOS JMP indirect wrap in-page.
                let access = read(bus, (e.base & 0xff00) | (e.base.wrapping_add(1) & 0xff));
                e.base = u16::from_le_bytes([e.low, access.data]);
                e.address = e.base;
                if e.mode == Mode::Ind {
                    self.registers.pc = e.address;
                    done = true;
                } else if e.mode == Mode::Izy {
                    e.address = e.base.wrapping_add(u16::from(self.registers.y));
                    e.phase = Indexed;
                } else {
                    e.phase = Memory;
                }
                access
            }
            Indexed => {
                let access = read(bus, e.intermediate());
                if !e.store() && !e.modify() && !e.crossed() {
                    self.read_operation(e.op, access.data);
                    done = true;
                } else {
                    e.phase = Memory;
                }
                access
            }
            Memory => {
                if e.store() {
                    let value = match e.op {
                        Op::Sta => self.registers.a,
                        Op::Stx => self.registers.x,
                        _ => self.registers.y,
                    };
                    done = true;
                    write(bus, e.address, value)
                } else {
                    let access = read(bus, e.address);
                    if e.modify() {
                        e.value = access.data;
                        e.phase = RmwOld;
                    } else {
                        self.read_operation(e.op, access.data);
                        done = true;
                    }
                    access
                }
            }
            RmwOld => {
                let access = write(bus, e.address, e.value);
                e.value = self.modify(e.op, e.value);
                e.phase = RmwNew;
                access
            }
            RmwNew => {
                done = true;
                write(bus, e.address, e.value)
            }
            Relative => {
                let access = self.fetch_cycle(bus);
                e.base = self.registers.pc;
                e.address = e.base.wrapping_add_signed(i16::from(access.data as i8));
                if self.branch_taken(e.op) {
                    e.phase = BranchTaken;
                } else {
                    done = true;
                }
                access
            }
            BranchTaken => {
                let access = read(bus, e.base);
                if e.crossed() {
                    self.registers.pc = e.intermediate();
                    e.phase = BranchCross;
                } else {
                    self.registers.pc = e.address;
                    done = true;
                }
                access
            }
            BranchCross => {
                let access = read(bus, self.registers.pc);
                self.registers.pc = e.address;
                done = true;
                access
            }
            StackDummyPc => {
                let access = read(bus, self.registers.pc);
                e.phase = if matches!(e.op, Op::Pha | Op::Php) {
                    PushRegister
                } else {
                    StackDummy
                };
                access
            }
            StackDummy => {
                let access = read(bus, self.stack_address());
                self.registers.sp = self.registers.sp.wrapping_add(1);
                e.phase = if e.op == Op::Rts {
                    ReturnLow
                } else {
                    PullRegister
                };
                access
            }
            PushRegister => {
                let value = if e.op == Op::Pha {
                    self.registers.a
                } else {
                    self.registers.status.bits() | 0x10
                };
                let access = write(bus, self.stack_address(), value);
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                done = true;
                access
            }
            PullRegister => {
                let access = read(bus, self.stack_address());
                if e.op == Op::Pla {
                    self.load_a(access.data);
                } else {
                    self.registers.status = Status::from_bits(access.data);
                }
                if e.op == Op::Rti {
                    self.registers.sp = self.registers.sp.wrapping_add(1);
                    e.phase = ReturnLow;
                } else {
                    done = true;
                }
                access
            }
            ReturnLow => {
                let access = read(bus, self.stack_address());
                e.low = access.data;
                self.registers.sp = self.registers.sp.wrapping_add(1);
                e.phase = ReturnHigh;
                access
            }
            ReturnHigh => {
                let access = read(bus, self.stack_address());
                self.registers.pc = u16::from_le_bytes([e.low, access.data]);
                if e.op == Op::Rts {
                    e.phase = ReturnDummy;
                } else {
                    done = true;
                }
                access
            }
            ReturnDummy => {
                done = true;
                self.fetch_cycle(bus)
            }
            JsrDummy => {
                e.phase = JsrPushHigh;
                read(bus, self.stack_address())
            }
            JsrPushHigh | JsrPushLow => {
                let [lo, hi] = self.registers.pc.to_le_bytes();
                let access = write(
                    bus,
                    self.stack_address(),
                    if e.phase == JsrPushHigh { hi } else { lo },
                );
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                e.phase = if e.phase == JsrPushHigh {
                    JsrPushLow
                } else {
                    JsrHigh
                };
                access
            }
            JsrHigh => {
                let access = read(bus, self.registers.pc);
                self.registers.pc = u16::from_le_bytes([e.low, access.data]);
                done = true;
                access
            }
            Padding => {
                let access = if matches!(e.kind, StepKind::Instruction { .. }) {
                    self.fetch_cycle(bus)
                } else {
                    read(bus, self.registers.pc)
                };
                e.phase = PushHigh;
                access
            }
            PushHigh | PushLow | PushStatus => {
                let [lo, hi] = self.registers.pc.to_le_bytes();
                let value = match e.phase {
                    PushHigh => hi,
                    PushLow => lo,
                    _ => {
                        self.registers.status.bits()
                            | if matches!(e.kind, StepKind::Instruction { .. }) {
                                0x10
                            } else {
                                0
                            }
                    }
                };
                let access = if e.kind == StepKind::Reset {
                    read(bus, self.stack_address())
                } else {
                    write(bus, self.stack_address(), value)
                };
                self.registers.sp = self.registers.sp.wrapping_sub(1);
                e.phase = match e.phase {
                    PushHigh => PushLow,
                    PushLow => PushStatus,
                    _ => {
                        self.registers.status.interrupt_disable = true;
                        e.address = match e.kind {
                            StepKind::Nmi => 0xfffa,
                            StepKind::Reset => 0xfffc,
                            _ => 0xfffe,
                        };
                        VectorLow
                    }
                };
                access
            }
            VectorLow => {
                let access = read(bus, e.address);
                e.low = access.data;
                e.phase = VectorHigh;
                access
            }
            VectorHigh => {
                let access = read(bus, e.address.wrapping_add(1));
                self.registers.pc = u16::from_le_bytes([e.low, access.data]);
                done = true;
                access
            }
        };
        (access, done)
    }
}
