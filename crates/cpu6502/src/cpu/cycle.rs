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

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClockPhase {
    Phi1,
    Phi2,
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
    /// RDY held a read: the bus transaction occurred, execution did not advance.
    pub stalled: bool,
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
    cycles: u64,
    low: u8,
    value: u8,
    base: u16,
    address: u16,
    branch_irq: bool,
    branch_nmi: bool,
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
            branch_irq: false,
            branch_nmi: false,
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

    // Only needed when RDY is low. A write returns None and must run normally.
    fn read_address(&self, cpu: &Cpu) -> Option<u16> {
        use Phase::*;
        Some(match self.phase {
            Fetch | Implied | Low | High | Relative | StackDummyPc | Padding | JsrHigh
            | ReturnDummy | BranchCross => cpu.registers.pc,
            ZeroIndex | PointerLow | BranchTaken => self.base,
            PointerHigh => (self.base & 0xff00) | (self.base.wrapping_add(1) & 0xff),
            Indexed => self.intermediate(),
            Memory if !self.store() => self.address,
            StackDummy | JsrDummy | PullRegister | ReturnLow | ReturnHigh => cpu.stack_address(),
            PushHigh | PushLow | PushStatus if self.kind == StepKind::Reset => cpu.stack_address(),
            VectorLow => self.address,
            VectorHigh => self.address.wrapping_add(1),
            _ => return None,
        })
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
        self.execution.is_none() && self.fetch_wait.is_none() && !self.phi2
    }

    /// Begin the seven-cycle RESET entry, discarding any unfinished instruction.
    /// This host request does not yet model the physical RESET pin hold time.
    pub fn begin_reset(&mut self) {
        self.phi2 = false;
        self.fetch_wait = None;
        self.irq_pending = false;
        self.irq_sample = false;
        self.nmi_pending = false;
        self.nmi_edge = false;
        self.nmi_sample = self.nmi_line;
        self.v_write_pending = [None; 2];
        self.execution = Some(Execution::new(
            StepKind::Reset,
            self.registers,
            Op::Brk,
            Mode::Imp,
        ));
    }

    /// Begin RESET and spend at most seven cycles in the same cycle engine.
    /// RDY can exhaust this budget; resume with `cycle` or `step`, not `reset`.
    /// Without external pin activity, preserves A/X/Y and N/V/D/Z/C and reads
    /// (never writes) three stack locations.
    pub fn reset(&mut self, bus: &mut dyn Bus) -> Result<Step, CpuError> {
        self.begin_reset();
        self.step(bus)
    }

    pub fn next_clock_phase(&self) -> ClockPhase {
        if self.phi2 {
            ClockPhase::Phi2
        } else {
            ClockPhase::Phi1
        }
    }

    /// Advance one phase. Phi1 samples SO; phi2 performs the actual bus transfer,
    /// samples IRQ/NMI/RDY and finishes the cycle. The host may change inputs in
    /// between. This is digital phase scheduling, not an electrical pin simulator.
    pub fn half_cycle(&mut self, bus: &mut dyn Bus) -> Result<Option<Cycle>, CpuError> {
        if !self.phi2 {
            if self.so_pending {
                self.registers.status.overflow = true;
            }
            // V writes overlap the next opcode fetch: BIT/pulls commit at the
            // next phi1, ADC/SBC one later, and CLV covers both. They win a
            // coincident SO update. See the fixed revD SO sweep, not a new edge
            // when SO remains low. Architectural results remain visible to step.
            if let Some(value) = self.v_write_pending[0] {
                self.registers.status.overflow = value;
            }
            self.v_write_pending = [self.v_write_pending[1], None];
            self.so_pending = self.so_line && !self.so_sample;
            self.so_sample = self.so_line;
            self.phi2 = true;
            Ok(None)
        } else {
            self.phi2 = false;
            self.finish_phi2(bus).map(Some)
        }
    }

    /// Advance/finish one CPU bus cycle. No speculative or logging-only reads.
    /// Interrupt inputs are sampled each cycle; instruction polling uses the
    /// previous sample, with the NMOS taken-branch polling exceptions.
    pub fn cycle(&mut self, bus: &mut dyn Bus) -> Result<Cycle, CpuError> {
        if !self.phi2 {
            self.half_cycle(bus)?;
        }
        self.phi2 = false;
        self.finish_phi2(bus)
    }

    fn finish_phi2(&mut self, bus: &mut dyn Bus) -> Result<Cycle, CpuError> {
        let sampled_i = self.registers.status.interrupt_disable;
        let polled_irq = self.irq_sample;
        let polled_nmi = self.nmi_edge;
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
            if self.not_ready {
                let (before, waits) = self.fetch_wait.unwrap_or((self.registers, 0));
                self.fetch_wait = Some((before, waits + 1));
                self.sample_interrupt_pins(sampled_i);
                return Ok(Cycle {
                    bus: access,
                    stalled: true,
                    completed: None,
                });
            }
            let opcode = access.data;
            let Some((op, mode, _)) = decode(opcode) else {
                return Err(CpuError::UnsupportedOpcode {
                    address: access.address,
                    opcode,
                });
            };
            let (before, waits) = self.fetch_wait.take().unwrap_or((self.registers, 0));
            let mut execution = Execution::new(StepKind::Instruction { opcode }, before, op, mode);
            self.registers.pc = self.registers.pc.wrapping_add(1);
            execution.cycles = waits + 1;
            execution.phase = execution.first_phase();
            self.execution = Some(execution);
            self.sample_interrupt_pins(sampled_i);
            return Ok(Cycle {
                bus: access,
                stalled: false,
                completed: None,
            });
        };
        if self.not_ready
            && let Some(address) = execution.read_address(self)
        {
            let mut access = read(bus, address);
            access.sync = execution.phase == Phase::Fetch;
            execution.cycles += 1;
            self.execution = Some(execution);
            self.sample_interrupt_pins(sampled_i);
            return Ok(Cycle {
                bus: access,
                stalled: true,
                completed: None,
            });
        }
        let phase = execution.phase;
        if phase == Phase::Relative {
            execution.branch_irq = polled_irq;
            execution.branch_nmi = polled_nmi;
        }
        let (access, done) = self.advance(bus, &mut execution);
        execution.cycles += 1;
        let completed = if done {
            if matches!(execution.kind, StepKind::Instruction { .. }) {
                let (irq, nmi) = match phase {
                    // A taken same-page branch retains the operand-cycle poll.
                    Phase::BranchTaken => (execution.branch_irq, execution.branch_nmi),
                    Phase::BranchCross => (
                        execution.branch_irq || polled_irq,
                        execution.branch_nmi || polled_nmi,
                    ),
                    _ => (polled_irq, polled_nmi),
                };
                self.irq_pending = execution.op != Op::Brk && irq;
                self.nmi_pending = execution.op != Op::Brk && nmi;
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
        self.sample_interrupt_pins(sampled_i);
        Ok(Cycle {
            bus: access,
            stalled: false,
            completed,
        })
    }

    /// Finish the current instruction/entry, or start one at a boundary.
    /// The returned count and `before` snapshot cover the entire operation even
    /// when the caller has already advanced some of its cycles individually.
    pub fn step(&mut self, bus: &mut dyn Bus) -> Result<Step, CpuError> {
        self.step_with_cycle_budget(bus, 7)
    }

    /// Limit the number of cycles spent in this call, retaining state on timeout.
    /// `step` uses seven; hosts with RDY waits can choose a larger finite budget.
    pub fn step_with_cycle_budget(
        &mut self,
        bus: &mut dyn Bus,
        budget: u64,
    ) -> Result<Step, CpuError> {
        for _ in 0..budget {
            if let Some(step) = self.cycle(bus)?.completed {
                return Ok(step);
            }
        }
        Err(CpuError::CycleBudgetExceeded {
            address: self.registers.pc,
            budget,
        })
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
                    self.v_write_pending[0] = Some(self.registers.status.overflow);
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
                        e.address = if e.kind == StepKind::Reset {
                            0xfffc
                        } else if e.kind == StepKind::Nmi || self.nmi_edge {
                            0xfffa
                        } else {
                            0xfffe
                        };
                        // An edge sampled after this selection remains pending.
                        self.nmi_edge = false;
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
