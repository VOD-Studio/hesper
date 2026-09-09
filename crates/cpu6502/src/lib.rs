//! Hesper's machine-independent NMOS 6502 core.
//!
//! Implemented opcodes are listed in `docs/opcodes.md`. Each step executes
//! one instruction or entry via the same single-cycle bus sequencer. Interrupt
//! inputs use cycle sampling with documented NMOS polling/vector rules.
//! Program loading, execution limits, tracing and devices belong to the host.

mod bus;
mod cpu;
mod instruction;

pub use bus::{Bus, LoadError, RAM_SIZE, Ram};
pub use cpu::{
    BusCycle, ClockPhase, Cpu, CpuError, Cycle, Direction, Registers, Status, Step, StepKind,
};
