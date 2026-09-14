//! Apple I machine Bus: two 4 KiB RAM banks, 256-byte ROM, and MC6821 PIA.
//!
//! ## Address map
//!
//! | Range             | Device            |
//! |-------------------|-------------------|
//! | `$0000–$0FFF`     | 4 KiB RAM         |
//! | `$Dxxx` with A4=1 | MC6821 PIA, register selected by A1/A0 |
//! | `$E000–$EFFF`     | 4 KiB RAM         |
//! | `$FF00–$FFFF`     | 256-byte Woz Monitor ROM |
//! | everything else  | open bus          |
//!
//! PIA selection is `(addr & 0xF010) == 0xD010` (Apple-1 Operation Manual
//! hardware notes and Sheet 2/3; see `docs/apple1/hardware-evidence.md` H04).
//! Thus $D0F2 (used by BASIC) and $D012 access the same Port B register.
//!
//! Open bus deterministically returns the last value driven on the data bus
//! (initialised to zero). This is a **simulation convention** for
//! reproducibility; real unmapped Apple I addresses float and can return
//! noise. ROM writes are silently ignored. The RAM banks are independent,
//! not mirrored.

use std::{fmt, ops::Range};

use hesper_cpu6502::Bus;

use crate::pia::Pia6821;

/// Apple I machine bus.
#[derive(Debug)]
pub struct Apple1Bus {
    ram: [u8; Self::RAM_SIZE],
    rom: [u8; Self::ROM_SIZE],
    pia: Pia6821,
    last_read: u8,
}

/// Error returned when a ROM image has the wrong size.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RomSizeError {
    pub expected: usize,
    pub got: usize,
}

impl fmt::Display for RomSizeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ROM must be {} bytes, got {}", self.expected, self.got)
    }
}

impl std::error::Error for RomSizeError {}

/// Error returned when a host RAM load does not fit entirely inside
/// one of the Apple I's 4 KiB RAM banks (`$0000–$0FFF`, `$E000–$EFFF`).
/// A load cannot cross a bank boundary or an unmapped region.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamLoadError {
    pub start: u16,
    pub len: usize,
}

impl fmt::Display for RamLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot load {} bytes at ${:04X}: must fit within one Apple I RAM bank ($0000–$0FFF or $E000–$EFFF)",
            self.len, self.start
        )
    }
}

impl std::error::Error for RamLoadError {}

impl Apple1Bus {
    pub const RAM_BANK_SIZE: usize = 4 * 1024;
    pub const RAM_SIZE: usize = 2 * Self::RAM_BANK_SIZE;
    pub const HIGH_RAM_BASE: u16 = 0xE000;
    pub const ROM_SIZE: usize = 256;
    pub const PIA_BASE: u16 = 0xD010;
    pub const ROM_BASE: u16 = 0xFF00;

    /// Create a new Apple I bus with the given 256‑byte ROM image.
    ///
    /// RAM is zero‑filled.  The PIA is in its reset state (all registers zero,
    /// interrupts disabled).
    pub fn new(rom: &[u8]) -> Result<Self, RomSizeError> {
        if rom.len() != Self::ROM_SIZE {
            return Err(RomSizeError {
                expected: Self::ROM_SIZE,
                got: rom.len(),
            });
        }
        let mut bytes = [0u8; Self::ROM_SIZE];
        bytes.copy_from_slice(rom);
        Ok(Self {
            ram: [0u8; Self::RAM_SIZE],
            rom: bytes,
            pia: Pia6821::new(),
            last_read: 0,
        })
    }

    /// Host load into RAM (non‑wrapping, transactional).  Returns an error
    /// if the start address is outside RAM or the region does not fit
    /// entirely within one 4 KiB bank; nothing is copied on error.
    /// Empty loads at a bank's exclusive end ($1000 or $F000) are allowed.
    pub fn load_ram(&mut self, start: u16, bytes: &[u8]) -> Result<(), RamLoadError> {
        let range = Self::ram_range(start, bytes.len())?;
        self.ram[range].copy_from_slice(bytes);
        Ok(())
    }

    /// Validate a host load before creating a machine or changing memory.
    pub fn validate_ram_load(start: u16, len: usize) -> Result<(), RamLoadError> {
        Self::ram_range(start, len).map(|_| ())
    }

    fn ram_range(start: u16, len: usize) -> Result<Range<usize>, RamLoadError> {
        let error = RamLoadError { start, len };
        let (bank, offset) = match start {
            0x0000..=0x1000 => (0, usize::from(start)),
            0xE000..=0xF000 => (
                Self::RAM_BANK_SIZE,
                usize::from(start - Self::HIGH_RAM_BASE),
            ),
            _ => return Err(error),
        };
        if len > Self::RAM_BANK_SIZE - offset {
            return Err(error);
        }
        let begin = bank + offset;
        Ok(begin..begin + len)
    }

    /// Side-effect-free host inspection of both packed RAM banks: the first
    /// 4 KiB corresponds to $0000–$0FFF, the second to $E000–$EFFF.
    /// Slice indices are not CPU addresses for the second bank.
    pub fn ram_slice(&self) -> &[u8] {
        &self.ram
    }

    /// Side‑effect‑free host inspection of ROM.
    pub fn rom_slice(&self) -> &[u8] {
        &self.rom
    }

    /// Borrow the PIA for input injection.
    pub fn pia(&self) -> &Pia6821 {
        &self.pia
    }

    /// Mutably borrow the PIA for input injection.
    pub fn pia_mut(&mut self) -> &mut Pia6821 {
        &mut self.pia
    }

    /// Decode an address to determine which device handles it.
    fn decode(addr: u16) -> Device {
        match addr {
            0x0000..=0x0FFF => Device::Ram(usize::from(addr)),
            0xE000..=0xEFFF => {
                Device::Ram(Self::RAM_BANK_SIZE + usize::from(addr - Self::HIGH_RAM_BASE))
            }
            _ if addr & 0xF010 == Self::PIA_BASE => Device::Pia,
            0xFF00..=0xFFFF => Device::Rom,
            _ => Device::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Ram(usize),
    Rom,
    Pia,
    Open,
}

impl Bus for Apple1Bus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match Self::decode(addr) {
            Device::Ram(offset) => self.ram[offset],
            Device::Rom => self.rom[usize::from(addr - Self::ROM_BASE)],
            Device::Pia => self.pia.read(addr),
            Device::Open => self.last_read,
        };
        self.last_read = value;
        value
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.last_read = value;
        match Self::decode(addr) {
            Device::Ram(offset) => self.ram[offset] = value,
            Device::Rom => { /* ROM is read‑only */ }
            Device::Pia => self.pia.write(addr, value),
            Device::Open => { /* open bus: write is ignored */ }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_rom() -> [u8; 256] {
        let mut rom = [0u8; 256];
        rom[0] = 0xD8; // CLD — matches Woz Monitor entry
        rom
    }

    #[test]
    fn rom_wrong_size_rejected() {
        let err = Apple1Bus::new(&[0; 128]).unwrap_err();
        assert_eq!(err.expected, 256);
        assert_eq!(err.got, 128);
    }

    #[test]
    fn ram_read_write() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.write(0x0000, 0x42);
        bus.write(0x0FFF, 0xFF);
        assert_eq!(bus.read(0x0000), 0x42);
        assert_eq!(bus.read(0x0FFF), 0xFF);
    }

    #[test]
    fn rom_read_only() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        assert_eq!(bus.read(0xFF00), 0xD8); // CLD
        bus.write(0xFF00, 0x00); // should be ignored
        assert_eq!(bus.read(0xFF00), 0xD8); // unchanged
    }

    #[test]
    fn pia_basic_access() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        // Write DDRB, then switch to OR and write data.
        bus.write(0xD012, 0x7F); // DDRB = $7F (bits 0-6 output, bit 7 input)
        bus.write(0xD013, 0x04); // CRB = $04: select OR
        bus.write(0xD012, 0x2A); // ORB = $2A

        // Read back: ORB & DDRB = $2A & $7F = $2A
        let val = bus.read(0xD012);
        assert_eq!(val, 0x2A);
    }

    #[test]
    fn pia_aliases_share_registers_and_read_side_effects() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.write(0xD0F2, 0x7F); // BASIC's display alias: DDRB
        assert_eq!(bus.read(0xD012), 0x7F);
        bus.write(0xDFFF, 0x04); // CRB alias selects the data register
        bus.write(0xD012, 0x2A);
        bus.pia_mut().set_port_b_inputs(0x80);
        bus.pia_mut().set_cb1(true);
        assert_eq!(bus.read(0xD017), 0x84); // canonical flag visible in an alias
        assert_eq!(bus.read(0xD0F2), 0xAA);
        assert_eq!(bus.read(0xD013), 0x04); // alias read clears the same IRQ flag
        bus.write(0xD0F1, 0x04);
        bus.pia_mut().set_port_a_inputs(0xC1);
        bus.pia_mut().set_ca1(true);
        assert_eq!(bus.read(0xD015), 0x84);
        assert_eq!(bus.read(0xDFFC), 0xC1);
        assert!(bus.pia_mut().take_port_a_read());
        assert_eq!(bus.read(0xD011), 0x04);
        for address in [0xC0F2, 0xD002, 0xD0E2] {
            bus.write(address, 0x55);
            bus.read(0xFF00);
            assert_eq!(bus.read(address), 0xD8, "${address:04X} is not selected");
        }
    }

    #[test]
    fn open_bus_returns_last_read() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.write(0x0000, 0xAB);
        let _ = bus.read(0x0000); // last_read = AB
        assert_eq!(bus.read(0x2000), 0xAB); // open bus returns last read
    }

    #[test]
    fn open_bus_write_ignored_but_updates_last_read() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.write(0x1000, 0x55);
        assert_eq!(bus.last_read, 0x55);
        // Value not stored anywhere.
    }

    #[test]
    fn load_ram_ok_and_error() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.load_ram(0x0FF0, &[0xAA; 16]).unwrap();
        assert_eq!(bus.read(0x0FF0), 0xAA);
        assert_eq!(bus.read(0x0FFF), 0xAA);

        // Oversized load.
        let err = bus.load_ram(0x0FF0, &[0xBB; 17]).unwrap_err();
        assert_eq!(err.start, 0x0FF0);
        assert_eq!(err.len, 17);
        // RAM untouched.
        assert_eq!(bus.read(0x0FF0), 0xAA);
    }

    #[test]
    fn load_ram_last_byte_fits_and_crossing_the_end_changes_nothing() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.load_ram(0x0FFF, &[0x11]).unwrap();
        assert_eq!(bus.read(0x0FFF), 0x11);

        let err = bus.load_ram(0x0FFF, &[0x22, 0x33]).unwrap_err();
        assert_eq!(
            err,
            RamLoadError {
                start: 0x0FFF,
                len: 2
            }
        );
        assert_eq!(
            bus.read(0x0FFF),
            0x11,
            "a rejected load must leave RAM exactly as it was"
        );
    }

    #[test]
    fn load_ram_rejects_start_addresses_outside_ram_without_panicking() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        // The first address past RAM accepts an empty load (nothing is
        // written), matching a zero-length region ending exactly at the
        // 4 KiB boundary.
        bus.load_ram(0x1000, &[]).unwrap();

        for start in [0x1001u16, 0xFFFF] {
            let err = bus.load_ram(start, &[]).unwrap_err();
            assert_eq!(err, RamLoadError { start, len: 0 });
            let err = bus.load_ram(start, &[0x99]).unwrap_err();
            assert_eq!(err, RamLoadError { start, len: 1 });
        }
        assert_eq!(bus.ram_slice(), &[0u8; Apple1Bus::RAM_SIZE]);
    }

    #[test]
    fn ram_load_error_names_the_apple_i_ram_banks() {
        let err = RamLoadError {
            start: 0x0FFE,
            len: 4,
        };
        assert_eq!(
            err.to_string(),
            "cannot load 4 bytes at $0FFE: must fit within one Apple I RAM bank ($0000–$0FFF or $E000–$EFFF)"
        );
    }

    #[test]
    fn high_ram_is_independent_writable_and_not_mirrored() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.load_ram(0xE000, &[0xEA; Apple1Bus::RAM_BANK_SIZE])
            .unwrap();
        bus.write(0x0000, 0x12);
        bus.write(0x0FFF, 0x34);
        bus.write(0xEFFF, 0x56);
        assert_eq!(bus.read(0x0000), 0x12);
        assert_eq!(bus.read(0x0FFF), 0x34);
        assert_eq!(bus.read(0xE000), 0xEA);
        assert_eq!(bus.read(0xEFFF), 0x56);
        for address in [0x1000, 0xDFEF, 0xF000, 0xFEFF] {
            bus.write(address, 0x99);
            bus.read(0x0000);
            assert_eq!(
                bus.read(address),
                0x12,
                "${address:04X} must remain open bus"
            );
        }
    }

    #[test]
    fn bank_crossing_and_unmapped_loads_are_transactional() {
        let mut bus = Apple1Bus::new(&dummy_rom()).unwrap();
        bus.load_ram(0x0000, &[0x11; Apple1Bus::RAM_BANK_SIZE])
            .unwrap();
        bus.load_ram(0xE000, &[0x22; Apple1Bus::RAM_BANK_SIZE])
            .unwrap();
        bus.load_ram(0xEFFF, &[0x33]).unwrap();
        bus.load_ram(0xF000, &[]).unwrap();
        let before = bus.ram_slice().to_vec();
        for (start, len) in [
            (0x0FFF, 2),
            (0x1000, 1),
            (0xD010, 1),
            (0xDFFF, 2),
            (0xE000, 4097),
            (0xEFFF, 2),
            (0xF000, 1),
            (0xFFFF, 2),
        ] {
            assert_eq!(
                bus.load_ram(start, &vec![0xFF; len]),
                Err(RamLoadError { start, len })
            );
            assert_eq!(bus.ram_slice(), before);
        }
        assert!(Apple1Bus::validate_ram_load(0xE000, usize::MAX).is_err());
    }
}
