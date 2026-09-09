//! Hesper's machine-independent NMOS 6502 core.
//!
//! Implemented opcodes are listed in `docs/opcodes.md`. Each step executes
//! one instruction or entry via the same single-cycle bus sequencer. Interrupt
//! input sampling still uses a documented boundary approximation.
//! Program loading, execution limits, tracing and devices belong to the host.

mod bus;
mod cpu;
mod instruction;

pub use bus::{Bus, LoadError, RAM_SIZE, Ram};
pub use cpu::{BusCycle, Cpu, CpuError, Cycle, Direction, Registers, Status, Step, StepKind};
