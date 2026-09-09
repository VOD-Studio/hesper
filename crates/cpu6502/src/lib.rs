//! Hesper's machine-independent NMOS 6502 core.
//!
//! M0 implements the 25 opcodes listed in `docs/opcodes.md`. Each step executes
//! one instruction and reports its cycle count; bus accesses are NOT cycle exact.
//! Program loading, execution limits, tracing and devices belong to the host.

mod bus;
mod cpu;

pub use bus::{Bus, LoadError, RAM_SIZE, Ram};
pub use cpu::{Cpu, CpuError, Registers, Status, Step};
