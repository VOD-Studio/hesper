use crate::{
    DRAM_SIZE, DataDisk,
    interrupt_devices::{BLOCK, Clint, Plic},
    uart::Uart,
    virtio_blk::VirtioBlk,
};
use std::collections::TryReserveError;

const CLINT: u64 = 0x0200_0000;
const PLIC: u64 = 0x0c00_0000;
const UART: u64 = 0x1000_0000;
const VIRTIO_BLK: u64 = 0x1000_1000;
const SIFIVE_TEST: u64 = 0x0010_0000;

/// Terminal requests from the SiFive test device on the system-disk board.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum GuestExit {
    /// Poweroff with the guest's 16-bit exit code.
    PowerOff(u16),
    /// Guest reset request. The host may start a new boot.
    Reboot,
    /// Guest failure with the device's 16-bit exit code.
    Failure(u16),
}

#[derive(Default)]
pub(crate) struct SifiveTest {
    pub(crate) exit: Option<GuestExit>,
}

impl SifiveTest {
    fn write(&mut self, offset: u64, width: usize, value: u64) {
        if offset != 0 {
            return;
        }
        let code = if width == 4 { (value >> 16) as u16 } else { 0 };
        self.exit = match value as u16 {
            0x5555 => Some(GuestExit::PowerOff(code)),
            0x7777 => Some(GuestExit::Reboot),
            0x3333 => Some(GuestExit::Failure(code)),
            _ => None,
        };
    }
}

pub(crate) struct Board {
    pub(crate) ram: Vec<u8>,
    pub(crate) uart: Uart,
    clint: Clint,
    plic: Plic,
    pub(crate) block: Option<VirtioBlk>,
    pub(crate) test: Option<SifiveTest>,
    pub(crate) ticks: u64,
}

impl Board {
    pub(crate) fn new() -> Self {
        Self::with_ram(vec![0; DRAM_SIZE], None)
    }

    pub(crate) fn try_new(disk: Option<DataDisk>) -> Result<Self, TryReserveError> {
        let mut ram = Vec::new();
        ram.try_reserve_exact(DRAM_SIZE)?;
        ram.resize(DRAM_SIZE, 0);
        Ok(Self::with_ram(ram, disk))
    }

    fn with_ram(ram: Vec<u8>, disk: Option<DataDisk>) -> Self {
        Self {
            ram,
            uart: Uart::new(),
            clint: Clint::new(),
            plic: Plic::new(),
            block: disk.map(VirtioBlk::new),
            test: None,
            ticks: 0,
        }
    }

    pub(crate) fn time(&self) -> u64 {
        self.clint.time(self.ticks)
    }

    pub(crate) fn reset(&mut self) {
        self.ram.fill(0);
        self.uart = Uart::new();
        self.clint = Clint::new();
        self.plic = Plic::new();
        if let Some(block) = &mut self.block {
            block.reset();
        }
        if let Some(test) = &mut self.test {
            test.exit = None;
        }
        self.ticks = 0;
    }

    pub(crate) fn advance(&mut self, ticks: u64) {
        self.ticks = ticks;
        self.uart.advance(ticks);
        self.plic.set_uart_line(self.uart.interrupt());
    }

    pub(crate) fn pending(&self) -> u64 {
        self.clint.pending(self.ticks) | self.plic.pending()
    }

    pub(crate) fn next_event(&self) -> Option<u64> {
        match (
            self.clint.next_event(self.ticks),
            self.uart.next_event(self.ticks),
        ) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        }
    }

    pub(crate) fn read_mmio(&mut self, address: u64, width: usize) -> Option<u64> {
        let value = if let Some(offset) = offset(address, width, CLINT, 0x1_0000) {
            self.clint.read(offset, width, self.ticks)?
        } else if width == 4
            && let Some(offset) = offset(address, width, PLIC, 0x60_0000)
        {
            u64::from(self.plic.read(offset))
        } else if width == 1
            && let Some(offset) = offset(address, width, UART, 0x100)
        {
            u64::from(self.uart.read(offset, self.ticks))
        } else if self.test.is_some()
            && matches!(width, 2 | 4)
            && offset(address, width, SIFIVE_TEST, 0x1000).is_some()
        {
            0
        } else {
            let offset = offset(address, width, VIRTIO_BLK, 0x1000)?;
            self.block.as_ref()?.read(offset, width)?
        };
        Some(value)
    }

    /// Returns whether a device also wrote DRAM via DMA.
    pub(crate) fn write_mmio(&mut self, address: u64, width: usize, value: u64) -> Option<bool> {
        if let Some(offset) = offset(address, width, CLINT, 0x1_0000) {
            self.clint.write(offset, width, value, self.ticks)?;
        } else if width == 4
            && let Some(offset) = offset(address, width, PLIC, 0x60_0000)
        {
            self.plic.write(offset, value as u32);
        } else if width == 1
            && let Some(offset) = offset(address, width, UART, 0x100)
        {
            self.uart.write(offset, value as u8, self.ticks);
        } else if let Some(test) = &mut self.test
            && matches!(width, 2 | 4)
            && let Some(offset) = offset(address, width, SIFIVE_TEST, 0x1000)
        {
            test.write(offset, width, value);
        } else {
            let offset = offset(address, width, VIRTIO_BLK, 0x1000)?;
            let block = self.block.as_mut()?;
            let dma = block.write(offset, width, value as u32, &mut self.ram)?;
            self.plic.set_line(BLOCK, block.interrupt());
            return Some(dma);
        }
        Some(false)
    }
}

pub(crate) fn read_le(bytes: &[u8]) -> u64 {
    match bytes {
        [byte] => u64::from(*byte),
        [a, b] => u64::from(u16::from_le_bytes([*a, *b])),
        [a, b, c, d] => u64::from(u32::from_le_bytes([*a, *b, *c, *d])),
        _ => u64::from_le_bytes(bytes.try_into().expect("eight-byte access")),
    }
}

fn offset(address: u64, width: usize, base: u64, length: u64) -> Option<u64> {
    let offset = address.checked_sub(base)?;
    (offset.checked_add(width as u64)? <= length).then_some(offset)
}
