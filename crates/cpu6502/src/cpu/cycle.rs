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
    /// RDY held a read: the sequencer did not advance. Pending SP transfers
    /// can still commit; the bus transaction and pin sampling still occur.
    pub stalled: bool,
    /// Present only when this cycle completes an instruction or entry sequence.
    pub completed: Option<Step>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Phase {
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
    ResetHold,
}

/// Read-only sequencer snapshot. `phase` names the next bus operation.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExecutionState {
    pub kind: StepKind,
    pub instruction_address: u16,
    pub phase: Phase,
    pub cycles: u64,
    pub base_address: u16,
    pub effective_address: u16,
    pub low_byte: u8,
    pub data: u8,
    /// Internal stack address cursor, distinct from the visible SP register.
    pub stack_address: u16,
}

/// Logical inputs: IRQ/NMI/RESET/SO true means asserted (electrically low).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct InputPins {
    pub irq: bool,
    pub nmi: bool,
    pub reset: bool,
    pub ready: bool,
    pub so: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PinLatches {
    /// Sampled IRQ after applying the sampled I flag.
    pub irq_sample: bool,
    pub irq_pending: bool,
    pub nmi_sample: bool,
    pub nmi_edge: bool,
    pub nmi_pending: bool,
    pub reset_sample: bool,
    pub reset_stop: bool,
    pub reset_active: bool,
    pub so_sample: bool,
    pub so_pending: bool,
}

/// Diagnostic observation, not a restorable CPU savestate or transistor state.
/// Taking this snapshot never accesses the Bus.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugState {
    pub registers: Registers,
    pub next_clock_phase: ClockPhase,
    pub execution: Option<ExecutionState>,
    pub fetch_wait_cycles: u64,
    pub pins: InputPins,
    pub latches: PinLatches,
    pub pending_v_writes: [Option<bool>; 2],
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
    stack: u8,
    branch_irq: bool,
    branch_nmi: bool,
}

#[derive(Debug)]
pub(super) struct ResetSequence {
    before: Registers,
    cycles: u64,
    op: Op,
    phase: Phase,
    transfers: u64,
    fetch_stalled: bool,
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
            stack: before.sp,
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

    fn stack_address(&self) -> u16 {
        0x100 | u16::from(self.stack)
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
            Memory if !self.store() || cpu.reset_active => self.address,
            StackDummy | JsrDummy | PullRegister | ReturnLow | ReturnHigh => self.stack_address(),
            PushHigh | PushLow | PushStatus if self.kind == StepKind::Reset || cpu.reset_active => {
                self.stack_address()
            }
            PushRegister | JsrPushHigh | JsrPushLow if cpu.reset_active => self.stack_address(),
            RmwOld | RmwNew if cpu.reset_active => self.address,
            VectorLow | ResetHold => self.address,
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
    pub fn debug_state(&self) -> DebugState {
        DebugState {
            registers: self.registers,
            next_clock_phase: self.next_clock_phase(),
            execution: self.execution.as_ref().map(|e| ExecutionState {
                kind: e.kind,
                instruction_address: e.before.pc,
                phase: e.phase,
                cycles: e.cycles,
                base_address: e.base,
                effective_address: e.address,
                low_byte: e.low,
                data: e.value,
                stack_address: e.stack_address(),
            }),
            fetch_wait_cycles: self.fetch_wait.map_or(0, |(_, cycles)| cycles),
            pins: InputPins {
                irq: self.irq_line,
                nmi: self.nmi_line,
                reset: self.reset_line,
                ready: !self.not_ready,
                so: self.so_line,
            },
            latches: PinLatches {
                irq_sample: self.irq_sample,
                irq_pending: self.irq_pending,
                nmi_sample: self.nmi_sample,
                nmi_edge: self.nmi_edge,
                nmi_pending: self.nmi_pending,
                reset_sample: self.reset_sample,
                reset_stop: self.reset_stop,
                reset_active: self.reset_active,
                so_sample: self.so_sample,
                so_pending: self.so_pending,
            },
            pending_v_writes: self.v_write_pending,
        }
    }

    /// True when the next cycle starts an instruction or interrupt entry.
    pub fn at_instruction_boundary(&self) -> bool {
        self.execution.is_none()
            && self.fetch_wait.is_none()
            && self.reset_sequence.is_none()
            && !self.phi2
    }

    /// Begin the seven-cycle RESET entry, discarding any unfinished instruction.
    /// Starts from the visible registers, not an abandoned stack address latch.
    /// This host request does not model the physical RESET pin hold time.
    pub fn begin_reset(&mut self) {
        self.phi2 = false;
        self.fetch_wait = None;
        self.reset_sample = false;
        self.reset_stop = false;
        self.reset_active = false;
        self.reset_sequence = None;
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
        // RES is sampled on the falling phase. Its timing-chain stop and write
        // inhibition take another cycle to reach the already-launched bus phase.
        if self.reset_sample && !self.reset_stop {
            let op = self.execution.as_ref().map_or(Op::Brk, |e| {
                if e.phase == Phase::ResetHold {
                    Op::Brk
                } else {
                    e.op
                }
            });
            if let Some(sequence) = &mut self.reset_sequence {
                sequence.op = op;
            } else {
                self.reset_sequence = Some(ResetSequence {
                    before: self.registers,
                    cycles: 0,
                    op,
                    phase: Phase::Fetch,
                    transfers: 0,
                    fetch_stalled: self.fetch_wait.is_some(),
                });
            }
        }
        let mut holding = self.reset_stop;
        if holding {
            let mut e = self.execution.take().unwrap_or_else(|| {
                Execution::new(StepKind::Reset, self.registers, Op::Brk, Mode::Imp)
            });
            if e.phase != Phase::ResetHold {
                let sequence = self.reset_sequence.as_mut().expect("sampled RESET");
                sequence.phase = e.phase;
                sequence.transfers = 0;
                e.op = sequence.op;
                e.before = sequence.before;
                e.cycles = sequence.cycles;
                e.address = match e.phase {
                    Phase::VectorLow => 0xfffc,
                    Phase::VectorHigh => 0xfffd,
                    _ => e.read_address(self).expect("RESET inhibits writes"),
                };
                e.phase = Phase::ResetHold;
                e.kind = StepKind::Reset;
            }
            self.fetch_wait = None;
            self.execution = Some(e);
        } else if let Some(e) = &mut self.execution
            && e.phase == Phase::ResetHold
            && !self.not_ready_sample
        {
            e.phase = Phase::Fetch;
            e.op = Op::Brk;
            e.kind = StepKind::Reset;
            e.stack = self.registers.sp;
        }
        holding |= self
            .execution
            .as_ref()
            .is_some_and(|e| e.phase == Phase::ResetHold);
        let clearing_nmi = self.reset_active
            && self
                .execution
                .as_ref()
                .is_none_or(|e| !matches!(e.phase, Phase::VectorLow | Phase::VectorHigh));
        let result = self.execute_cycle(bus);
        self.reset_active |= self.reset_sample;
        self.reset_stop = self.reset_sample;
        self.reset_sample = self.reset_line;
        self.not_ready_sample = self.not_ready;
        let mut cycle = result?;
        if !holding {
            self.data_latch = cycle.bus.data;
        }
        if clearing_nmi {
            self.nmi_edge = false;
            self.nmi_pending = false;
            self.irq_pending = false;
        }
        if let Some(sequence) = &mut self.reset_sequence {
            sequence.cycles += 1;
            if !self.reset_sample
                && !self.reset_stop
                && let Some(step) = &mut cycle.completed
                && step.kind == StepKind::Reset
            {
                step.before = sequence.before;
                step.address = sequence.before.pc;
                step.cycles = sequence.cycles;
                self.reset_sequence = None;
                self.reset_active = false;
            } else {
                cycle.completed = None;
            }
        }
        Ok(cycle)
    }

    fn execute_cycle(&mut self, bus: &mut dyn Bus) -> Result<Cycle, CpuError> {
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
            // RESET clears the fetched IR independently of the timing chain.
            // The actual external byte is still retained in the bus trace.
            let (kind, op, mode) = if self.reset_sample {
                (StepKind::Reset, Op::Brk, Mode::Imp)
            } else if let Some((op, mode, _)) = decode(access.data) {
                (
                    StepKind::Instruction {
                        opcode: access.data,
                    },
                    op,
                    mode,
                )
            } else {
                return Err(CpuError::UnsupportedOpcode {
                    address: access.address,
                    opcode: access.data,
                });
            };
            let (before, waits) = self.fetch_wait.take().unwrap_or((self.registers, 0));
            let mut execution = Execution::new(kind, before, op, mode);
            if self.alu_double_on_fetch {
                self.alu_latch = self.alu_latch.wrapping_add(self.alu_latch);
                self.alu_double_on_fetch = false;
            }
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
        // SP transfers are not gated by RDY. Keep the address cursor unchanged
        // until its bus phase advances, so a repeated read never increments it
        // again. These commit points follow fixed revD falling-edge observations.
        match execution.phase {
            Phase::StackDummy if matches!(execution.op, Op::Pla | Op::Plp) => {
                self.registers.sp = execution.stack.wrapping_add(1);
            }
            Phase::ReturnLow => self.registers.sp = execution.stack.wrapping_add(1),
            Phase::PushStatus => self.registers.sp = execution.stack.wrapping_sub(1),
            Phase::JsrHigh => self.registers.sp = execution.stack,
            _ => {}
        }
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
        if phase != Phase::ResetHold {
            match phase {
                Phase::StackDummy | Phase::ReturnLow => {
                    self.alu_latch = execution.stack;
                }
                Phase::PushRegister
                | Phase::JsrPushHigh
                | Phase::JsrPushLow
                | Phase::PushHigh
                | Phase::PushLow
                | Phase::PushStatus => {
                    self.alu_latch = execution.stack;
                }
                _ => {}
            }
            if done {
                self.alu_zero_input = false;
                self.alu_carry_input = false;
                self.alu_double_on_fetch = false;
                match execution.op {
                    Op::Pha | Op::Php => self.alu_latch = access.data,
                    Op::Pla | Op::Plp => {
                        self.alu_latch = access.data.wrapping_add(1);
                        self.alu_zero_input = true;
                        self.alu_carry_input = true;
                    }
                    _ if execution.modify() => self.alu_latch = self.data_latch,
                    Op::Rts | Op::Rti => {
                        self.alu_latch = access.data;
                        self.alu_zero_input = true;
                    }
                    Op::Nop => {
                        // Undriven internal buses precharge high.
                        self.alu_latch = u8::MAX.wrapping_add(u8::MAX);
                    }
                    Op::Jsr => self.alu_latch = access.data,
                    Op::Brk => {
                        self.alu_latch = u8::MAX.wrapping_add(access.data);
                    }
                    Op::Jmp => {}
                    _ => {
                        self.alu_latch = access.data;
                        self.alu_double_on_fetch = true;
                    }
                }
            }
        }
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
                // A physical release presents the held address latch at SYNC;
                // its stored PC can already differ after overlapping transfers.
                let address = if e.kind == StepKind::Reset && self.reset_sequence.is_some() {
                    e.address
                } else {
                    self.registers.pc
                };
                let mut access = read(bus, address);
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
                    _ if e.op == Op::Jsr => {
                        // JSR borrows SP for the target low byte while its
                        // original stack address is retained separately.
                        self.registers.sp = e.low;
                        JsrDummy
                    }
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
                let access = read(bus, e.stack_address());
                e.stack = e.stack.wrapping_add(1);
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
                let access = write(bus, e.stack_address(), value);
                e.stack = e.stack.wrapping_sub(1);
                self.registers.sp = e.stack;
                done = true;
                access
            }
            PullRegister => {
                let access = read(bus, e.stack_address());
                if e.op == Op::Pla {
                    self.load_a(access.data);
                } else {
                    self.registers.status = Status::from_bits(access.data);
                    self.v_write_pending[0] = Some(self.registers.status.overflow);
                }
                if e.op == Op::Rti {
                    e.stack = e.stack.wrapping_add(1);
                    e.phase = ReturnLow;
                } else {
                    done = true;
                }
                access
            }
            ReturnLow => {
                let access = read(bus, e.stack_address());
                e.low = access.data;
                e.stack = e.stack.wrapping_add(1);
                e.phase = ReturnHigh;
                access
            }
            ReturnHigh => {
                let access = read(bus, e.stack_address());
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
                read(bus, e.stack_address())
            }
            JsrPushHigh | JsrPushLow => {
                let [lo, hi] = self.registers.pc.to_le_bytes();
                let access = write(
                    bus,
                    e.stack_address(),
                    if e.phase == JsrPushHigh { hi } else { lo },
                );
                e.stack = e.stack.wrapping_sub(1);
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
                    read(bus, e.stack_address())
                } else {
                    write(bus, e.stack_address(), value)
                };
                e.stack = e.stack.wrapping_sub(1);
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
            ResetHold => {
                let access = read(bus, e.address);
                self.reset_transfers(e, access.data);
                access
            }
        };
        (access, done)
    }

    /// The timing chain is quiescent, but the selected instruction's T0
    /// transfers and the separate RMW tail still drive the live internal buses.
    fn reset_transfers(&mut self, e: &mut Execution, data: u8) {
        let sequence = self.reset_sequence.as_mut().expect("RESET transfer");
        let first = sequence.transfers == 0;
        let phase = sequence.phase;
        sequence.transfers += 1;
        let previous_data = self.data_latch;
        let mut next_data = data;
        match e.op {
            Op::Brk | Op::Rti | Op::Jsr => {
                let mut high = data;
                let low = if e.op == Op::Jsr {
                    let low = self.registers.sp;
                    self.registers.sp = if first && phase != Phase::Fetch {
                        if matches!(phase, Phase::JsrPushHigh | Phase::JsrPushLow) {
                            e.stack.wrapping_sub(1)
                        } else {
                            e.stack
                        }
                    } else {
                        previous_data
                    };
                    low
                } else if first {
                    match phase {
                        Phase::PushHigh | Phase::PushLow | Phase::PushStatus => {
                            e.stack.wrapping_sub(1)
                        }
                        Phase::StackDummy | Phase::PullRegister | Phase::ReturnLow => {
                            e.stack.wrapping_add(1)
                        }
                        Phase::VectorLow => (e.address as u8).wrapping_add(1),
                        Phase::Padding => {
                            let a = if self.alu_zero_input {
                                0
                            } else {
                                self.alu_latch
                            };
                            a.wrapping_add(self.alu_latch)
                                .wrapping_add(u8::from(self.alu_carry_input))
                        }
                        Phase::Fetch if sequence.fetch_stalled => {
                            (self.registers.pc as u8).wrapping_add(1)
                        }
                        Phase::Fetch if e.op == Op::Brk => self.alu_latch,
                        _ => previous_data,
                    }
                } else if e.op == Op::Brk && !sequence.fetch_stalled && !self.alu_zero_input {
                    previous_data.wrapping_sub(1)
                } else {
                    previous_data
                };
                // Vector-low's zero-source transfer retires one phase later
                // than the ordinary FF + DB (decrement) feedback path.
                self.alu_zero_input = first && phase == Phase::VectorLow;
                if first && matches!(phase, Phase::PushHigh | Phase::JsrPushHigh) {
                    // PCL/DB and DL/ADH overlap: the external high-address
                    // latch sees the wired bus, while PCH captures PCL itself.
                    // A short pulse exposes this distinction at release SYNC.
                    next_data = self.registers.pc as u8;
                    high &= next_data;
                    self.registers.pc = u16::from_le_bytes([low, next_data]);
                } else {
                    self.registers.pc = u16::from_le_bytes([low, high]);
                }
                e.address = u16::from_le_bytes([low, high]);
            }
            Op::Rts => {
                if first && !matches!(phase, Phase::ReturnDummy | Phase::Fetch) {
                    self.registers.pc = self.registers.pc.wrapping_add(1);
                }
                e.address = self.registers.pc;
            }
            Op::Pha | Op::Php => {
                self.registers.sp = previous_data;
                e.address = self.registers.pc;
            }
            Op::Plp => {
                self.registers.status = Status::from_bits(data);
                self.v_write_pending[0] = Some(self.registers.status.overflow);
                e.address = self.registers.pc;
            }
            Op::Pla
            | Op::Lda
            | Op::Ldx
            | Op::Ldy
            | Op::Adc
            | Op::Sbc
            | Op::And
            | Op::Ora
            | Op::Eor
            | Op::Bit
            | Op::Cmp
            | Op::Cpx
            | Op::Cpy => {
                self.read_operation(if e.op == Op::Pla { Op::Lda } else { e.op }, data);
                e.address = self.registers.pc;
            }
            _ => {
                e.address = self.registers.pc;
                if e.modify() {
                    let sequence = self.reset_sequence.as_mut().expect("RESET RMW tail");
                    match sequence.phase {
                        Phase::Memory | Phase::RmwOld => {
                            e.address = (self.registers.pc & 0xff00) | (e.base & 0xff);
                            sequence.phase = if sequence.phase == Phase::Memory {
                                Phase::RmwOld
                            } else {
                                Phase::RmwNew
                            };
                        }
                        _ => {}
                    }
                }
            }
        }
        self.data_latch = next_data;
    }
}
