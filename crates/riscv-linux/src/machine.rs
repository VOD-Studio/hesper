use crate::{
    BootError, BootImages, DataDisk, DiskFault, FDT_ADDRESS, PrivilegeMode, UartDiagnostics,
    board::Board, boot::Validated, dram_range, hart::Hart,
};
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
    pub satp: u64,
    pub last_trap: Option<Trap>,
    pub retired: u64,
    pub time: u64,
}

/// Side-effect-free observation of a stalled or paused board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Diagnostics {
    pub cpu: DebugState,
    pub mstatus: u64,
    pub mie: u64,
    pub mip: u64,
    pub machine_trap: Trap,
    pub supervisor_trap: Trap,
    pub stalled_traps: u32,
    pub waiting: bool,
    pub next_device_event: Option<u64>,
    pub uart: UartDiagnostics,
}

/// Fuel spent and instructions retired by a single batch, plus cumulative virtual time.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Progress {
    pub spent: u32,
    pub retired: u32,
    pub time: u64,
}

/// Fixed hardware capabilities for the board's RV64 hart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HartIsa {
    /// The original integer, multiplication, atomic and compressed extensions.
    Imac,
    /// IMAC plus single- and double-precision floating point.
    Imafdc,
}

/// A single hart and one contiguous DRAM allocation. Starts in M-mode, Bare.
pub struct Machine {
    hart: Hart,
    board: Board,
    retired: u64,
    cycle: u64,
    external_lines: u64,
    stalled_traps: u32,
}

impl Machine {
    /// Create an empty board with the hart at the DRAM base.
    pub fn new() -> Self {
        Self::with_board(Board::new(), HartIsa::Imac)
    }

    /// Create an empty board, returning an error if DRAM cannot be allocated.
    pub fn try_new() -> Result<Self, BootError> {
        Self::try_with_isa(HartIsa::Imac)
    }

    /// Create an empty board with the specified fixed hart capabilities.
    pub fn try_with_isa(isa: HartIsa) -> Result<Self, BootError> {
        Self::try_new_board(None, isa)
    }

    /// Create an empty board with a data disk attached at the modern MMIO slot.
    pub fn try_with_disk(disk: DataDisk) -> Result<Self, BootError> {
        Self::try_new_board(Some(disk), HartIsa::Imac)
    }

    fn try_new_board(disk: Option<DataDisk>, isa: HartIsa) -> Result<Self, BootError> {
        Board::try_new(disk)
            .map(|board| Self::with_board(board, isa))
            .map_err(|_| BootError::Allocation)
    }

    fn with_board(board: Board, isa: HartIsa) -> Self {
        Self {
            hart: Hart::new(isa),
            board,
            retired: 0,
            cycle: 0,
            external_lines: 0,
            stalled_traps: 0,
        }
    }

    /// Validate every image and boot a new board in M-mode at the OpenSBI entry.
    pub fn boot(images: BootImages<'_>) -> Result<Self, BootError> {
        Self::boot_with_isa(images, HartIsa::Imac)
    }

    /// Validate and cold boot the default images with the specified hart ISA.
    /// The host authenticates the images and chooses the matching hardware.
    pub fn boot_with_isa(images: BootImages<'_>, isa: HartIsa) -> Result<Self, BootError> {
        Self::boot_board(Validated::new(images, None)?, None, isa)
    }

    /// Validate boot images and start a cold board owning the supplied disk.
    pub fn boot_with_disk(images: BootImages<'_>, disk: DataDisk) -> Result<Self, BootError> {
        Self::boot_board(Validated::new(images, None)?, Some(disk), HartIsa::Imac)
    }

    /// Cold boot with a replacement `images.initramfs` and an optional disk.
    /// The host must authenticate the original firmware, kernel and `images.fdt`
    /// and supply the locked initramfs length, not the replacement length.
    pub fn boot_with_initramfs(
        images: BootImages<'_>,
        locked_initramfs_len: usize,
        disk: Option<DataDisk>,
    ) -> Result<Self, BootError> {
        Self::boot_board(
            Validated::new(images, Some(locked_initramfs_len))?,
            disk,
            HartIsa::Imac,
        )
    }

    fn boot_board(
        images: Validated<'_>,
        disk: Option<DataDisk>,
        isa: HartIsa,
    ) -> Result<Self, BootError> {
        let mut machine = Self::try_new_board(disk, isa)?;
        machine.install_boot(images);
        Ok(machine)
    }

    /// Cold boot on the existing allocation, retaining data-disk sectors.
    /// Validation failure preserves the old RAM, devices and hart observation.
    pub fn reboot(&mut self, images: BootImages<'_>) -> Result<(), BootError> {
        self.reboot_board(Validated::new(images, None)?);
        Ok(())
    }

    /// Cold boot with a replacement `images.initramfs`, retaining the current
    /// data disk. The host has the same authentication obligations as for
    /// [`Self::boot_with_initramfs`]. Validation failure leaves this machine intact.
    pub fn reboot_with_initramfs(
        &mut self,
        images: BootImages<'_>,
        locked_initramfs_len: usize,
    ) -> Result<(), BootError> {
        self.reboot_board(Validated::new(images, Some(locked_initramfs_len))?);
        Ok(())
    }

    fn reboot_board(&mut self, images: Validated<'_>) {
        self.board.reset();
        self.hart = Hart::new(self.hart.privilege.isa());
        self.retired = 0;
        self.cycle = 0;
        self.external_lines = 0;
        self.stalled_traps = 0;
        self.install_boot(images);
    }

    fn install_boot(&mut self, images: Validated<'_>) {
        images.install(&mut self.board.ram);
        // Hart 0 is a0=0 at reset; a1 points to the mutable tree in RAM.
        self.hart.x[11] = FDT_ADDRESS;
    }

    /// Copy a complete host span into DRAM or reject it without modifying RAM.
    pub fn load(&mut self, address: u64, data: &[u8]) -> Result<(), LoadError> {
        let range = dram_range(address, data.len()).ok_or(LoadError {
            address,
            bytes: data.len(),
        })?;
        self.board.ram[range].copy_from_slice(data);
        self.hart.invalidate_reservation(address, data.len());
        Ok(())
    }

    /// Observe DRAM bytes without making a guest bus access.
    pub fn inspect(&self, address: u64, bytes: usize) -> Option<&[u8]> {
        self.board.ram.get(dram_range(address, bytes)?)
    }

    /// Borrow the current raw data disk for explicit export. This does not
    /// promise that the guest has flushed or unmounted its filesystem.
    pub fn data_disk(&self) -> Option<&[u8]> {
        Some(self.board.block.as_ref()?.disk.bytes())
    }

    /// Whether the guest wrote sectors since attachment or the last export.
    pub fn disk_dirty(&self) -> bool {
        self.board
            .block
            .as_ref()
            .is_some_and(|block| block.disk.dirty())
    }

    /// Record successful export at the host boundary, while execution is paused.
    pub fn mark_disk_exported(&mut self) {
        if let Some(block) = &mut self.board.block {
            block.disk.mark_exported();
        }
    }

    /// Observe a transport failure that requires device reset.
    pub fn disk_fault(&self) -> Option<DiskFault> {
        self.board.block.as_ref()?.fault()
    }

    /// Observe integer architectural state and cumulative counters.
    pub fn debug_state(&self) -> DebugState {
        DebugState {
            pc: self.hart.pc,
            x: self.hart.x,
            mode: self.hart.privilege.mode,
            satp: self.hart.privilege.satp,
            last_trap: self.hart.last_trap,
            retired: self.retired,
            time: self.board.time(),
        }
    }

    /// Snapshot CSRs and queues without consuming IIR, LSR or PLIC claim.
    pub fn diagnostics(&self) -> Diagnostics {
        let state = &self.hart.privilege;
        Diagnostics {
            cpu: self.debug_state(),
            mstatus: state.status(),
            mie: state.mie,
            mip: state.observe_pending(self.external_lines | self.board.pending()),
            machine_trap: state.machine.observation(),
            supervisor_trap: state.supervisor.observation(),
            stalled_traps: self.stalled_traps,
            waiting: self.hart.waiting,
            next_device_event: self.board.next_event(),
            uart: self.board.uart.diagnostics(),
        }
    }

    /// Add externally driven interrupt lines to the board's device levels.
    pub fn set_interrupt_lines(&mut self, asserted: u64) {
        self.external_lines = asserted;
    }

    /// Queue host serial bytes for arrival at the UART; returns bytes accepted.
    pub fn serial_input(&mut self, bytes: &[u8]) -> usize {
        self.board.uart.queue_input(bytes, self.board.ticks)
    }

    /// Remove completed UART output bytes, in order, into the caller's buffer.
    pub fn serial_output(&mut self, bytes: &mut [u8]) -> usize {
        self.board.uart.drain_output(bytes)
    }

    /// Execute at most `fuel` transitions. A trap consumes a tick but does not
    /// retire. WFI jumps to the next device event or returns when none exists.
    /// A full serial output queue also stops the batch until the host drains it.
    pub fn run(&mut self, fuel: u32) -> Progress {
        let (mut spent, mut retired) = (0, 0);
        let mut stalled_traps = self.stalled_traps;
        while spent < fuel && !self.board.uart.output_full() {
            let lines = self.external_lines | self.board.pending();
            self.hart.privilege.set_interrupt_lines(lines);
            if let Some(did_retire) = self.hart.step(&mut self.board, self.cycle, self.retired) {
                self.cycle = self.cycle.wrapping_add(1);
                self.board.advance(self.board.ticks.wrapping_add(1));
                if did_retire {
                    retired += 1;
                    self.retired = self.retired.wrapping_add(1);
                    stalled_traps = 0;
                } else {
                    stalled_traps = stalled_traps.saturating_add(1);
                }
            } else if let Some(ticks) = self.board.next_event() {
                self.board.advance(ticks);
            } else {
                break;
            }
            spent += 1;
        }
        self.stalled_traps = stalled_traps;
        Progress {
            spent,
            retired,
            time: self.board.time(),
        }
    }
}

impl Default for Machine {
    fn default() -> Self {
        Self::new()
    }
}
