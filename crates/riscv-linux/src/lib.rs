//! Bare-metal execution and physical memory for Hesper's single-hart RISC-V board.

use core::ops::Range;

mod atomic;
mod board;
mod compressed;
mod decode;
mod hart;
mod interrupt_devices;
mod machine;
mod mmu;
mod pmp;
mod privilege;
mod uart;

pub use machine::{DebugState, LoadError, Machine, Progress, Trap};
pub use privilege::PrivilegeMode;

/// Guest physical address of the first DRAM byte.
pub const DRAM_BASE: u64 = 0x8000_0000;
/// Size of the board's contiguous DRAM in bytes.
pub const DRAM_SIZE: usize = 128 * 1024 * 1024;

/// Maps a complete guest physical span to byte offsets in the board's DRAM.
/// An empty span may start one past the last byte; no nonempty span may do so.
pub fn dram_range(address: u64, length: usize) -> Option<Range<usize>> {
    let start = usize::try_from(address.checked_sub(DRAM_BASE)?).ok()?;
    let end = start.checked_add(length)?;
    (end <= DRAM_SIZE).then_some(start..end)
}
