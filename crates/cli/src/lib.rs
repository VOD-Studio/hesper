//! Host-side demo shared by the CLI and its integration tests, not a machine model.

use std::fmt;

use hesper_cpu6502::{Cpu, CpuError, Cycle, DebugState, LoadError, Ram, Registers, Step};

pub const DEMO_START: u16 = 0x8000;
pub const DEMO_DONE: u16 = 0x800f;
pub const DEFAULT_MAX_STEPS: u64 = 1_000;

/// Original hand-encoded program; readable listing in `examples/count.asm`.
pub const DEMO_PROGRAM: &[u8] = &[
    0xd8, // $8000 CLD
    0xa2, 0xff, // $8001 LDX #$FF
    0x9a, // $8003 TXS
    0xa2, 0x00, // $8004 LDX #$00
    0x8a, // $8006 TXA
    0x9d, 0x00, 0x02, // $8007 STA $0200,X
    0xe8, // $800A INX
    0xe0, 0x0a, // $800B CPX #$0A
    0xd0, 0xf7, // $800D BNE $8006 (-9 from $800F)
    0xea, // $800F NOP: host stops BEFORE executing this marker
];

pub struct DemoRun {
    pub ram: Ram,
    pub registers: Registers,
    pub steps: u64,
    pub instruction_cycles: u64,
    pub reset_cycles: u64,
}

#[derive(Debug)]
pub enum DemoError {
    Load(LoadError),
    Cpu(CpuError),
    StepLimit { limit: u64, pc: u16 },
}

impl fmt::Display for DemoError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Load(err) => err.fmt(f),
            Self::Cpu(err) => err.fmt(f),
            Self::StepLimit { limit, pc } => {
                write!(f, "step limit {limit} reached at ${pc:04X}")
            }
        }
    }
}

impl std::error::Error for DemoError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Load(err) => Some(err),
            Self::Cpu(err) => Some(err),
            Self::StepLimit { .. } => None,
        }
    }
}

/// Run through RESET to the agreed completion PC, with a host instruction budget.
/// The trace callback receives accumulated cycles including RESET's 7 cycles.
pub fn run_demo(max_steps: u64, mut trace: impl FnMut(&Step, u64)) -> Result<DemoRun, DemoError> {
    run_demo_with_trace(max_steps, |event, total| {
        if let DemoEvent::Instruction(step) = event {
            trace(&step, total);
        }
    })
}

#[derive(Debug, Clone, Copy)]
pub enum DemoEvent {
    Cycle { cycle: Cycle, state: DebugState },
    Instruction(Step),
}

/// Observe actual execution, including the seven RESET bus reads. Callbacks
/// require no bus access; the host decides how much history to retain.
pub fn run_demo_with_trace(
    max_steps: u64,
    mut trace: impl FnMut(DemoEvent, u64),
) -> Result<DemoRun, DemoError> {
    let mut ram = Ram::new();
    ram.load(DEMO_START, DEMO_PROGRAM)
        .map_err(DemoError::Load)?;
    ram.load(0xfffc, &DEMO_START.to_le_bytes())
        .map_err(DemoError::Load)?;
    let mut cpu = Cpu::new();
    let mut total = 0;
    let mut finish_step = |cpu: &mut Cpu, ram: &mut Ram, trace: &mut dyn FnMut(DemoEvent, u64)| {
        for _ in 0..7 {
            let cycle = cpu.cycle(ram).map_err(DemoError::Cpu)?;
            total += 1;
            trace(
                DemoEvent::Cycle {
                    cycle,
                    state: cpu.debug_state(),
                },
                total,
            );
            if let Some(step) = cycle.completed {
                return Ok(step);
            }
        }
        Err(DemoError::Cpu(CpuError::CycleBudgetExceeded {
            address: cpu.registers().pc,
            budget: 7,
        }))
    };
    cpu.begin_reset();
    let reset_cycles = finish_step(&mut cpu, &mut ram, &mut trace)?.cycles;
    let mut steps = 0;
    let mut instruction_cycles = 0;
    while cpu.registers().pc != DEMO_DONE {
        if steps == max_steps {
            return Err(DemoError::StepLimit {
                limit: max_steps,
                pc: cpu.registers().pc,
            });
        }
        let step = finish_step(&mut cpu, &mut ram, &mut trace)?;
        steps += 1;
        instruction_cycles += step.cycles;
        trace(
            DemoEvent::Instruction(step),
            instruction_cycles + reset_cycles,
        );
    }
    Ok(DemoRun {
        ram,
        registers: cpu.registers(),
        steps,
        instruction_cycles,
        reset_cycles,
    })
}

pub mod apple1;
