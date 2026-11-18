use crate::{DRAM_SIZE, PrivilegeMode, dram_range, hart::Hart};
use core::fmt;

/// Failure to load a host byte span into the board's DRAM.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadError {
    pub address: u64,
    pub bytes: usize,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} bytes at {:#018x} do not fit in board DRAM",
            self.bytes, self.address
        )
    }
}

impl std::error::Error for LoadError {}

/// Observation of the last trap. Bit 63 of `cause` marks an interrupt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Trap {
    pub epc: u64,
    pub cause: u64,
    pub value: u64,
}

/// Architectural observations; not a savestate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DebugState {
    pub pc: u64,
    pub x: [u64; 32],
    pub mode: PrivilegeMode,
    pub last_trap: Option<Trap>,
    pub retired: u64,
    pub time: u64,
}

/// Fuel spent and instructions retired by a single batch, plus cumulative virtual time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub spent: u32,
    pub retired: u32,
    pub time: u64,
}

/// A single hart and one contiguous DRAM allocation. Starts in M-mode, Bare.
pub struct Machine {
    hart: Hart,
    ram: Box<[u8]>,
    retired: u64,
    time: u64,
}

impl Machine {
    /// Create an empty board with the hart at the DRAM base.
    pub fn new() -> Self {
        Self {
            hart: Hart::new(),
            ram: vec![0; DRAM_SIZE].into_boxed_slice(),
            retired: 0,
            time: 0,
        }
    }

    /// Copy a complete host span into DRAM or reject it without modifying RAM.
    pub fn load(&mut self, address: u64, data: &[u8]) -> Result<(), LoadError> {
        let range = dram_range(address, data.len()).ok_or(LoadError {
            address,
            bytes: data.len(),
        })?;
        self.ram[range].copy_from_slice(data);
        self.hart.invalidate_reservation(address, data.len());
        Ok(())
    }

    /// Observe DRAM bytes without making a guest bus access.
    pub fn inspect(&self, address: u64, bytes: usize) -> Option<&[u8]> {
        self.ram.get(dram_range(address, bytes)?)
    }

    /// Observe integer architectural state and cumulative counters.
    pub fn debug_state(&self) -> DebugState {
        DebugState {
            pc: self.hart.pc,
            x: self.hart.x,
            mode: self.hart.privilege.mode,
            last_trap: self.hart.last_trap,
            retired: self.retired,
            time: self.time,
        }
    }

    /// Drive the board's local and external interrupt lines. Set one bit per cause.
    pub fn set_interrupt_lines(&mut self, asserted: u64) {
        self.hart.privilege.set_interrupt_lines(asserted);
    }

    /// Execute at most `fuel` transitions. A trap consumes a tick but does not
    /// retire. WFI with no enabled pending interrupt returns without busy-waiting.
    pub fn run(&mut self, fuel: u32) -> Progress {
        let (mut spent, mut retired) = (0, 0);
        while spent < fuel {
            let Some(did_retire) = self.hart.step(&mut self.ram, self.time, self.retired) else {
                break;
            };
            spent += 1;
            self.time = self.time.wrapping_add(1);
            if did_retire {
                retired += 1;
                self.retired = self.retired.wrapping_add(1);
            }
        }
        Progress {
            spent,
            retired,
            time: self.time,
        }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}
