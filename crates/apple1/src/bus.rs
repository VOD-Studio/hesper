//! Apple I machine Bus: 4 KiB RAM, 256-byte ROM, PIA at `$D010‑$D013`.
//!
//! ## Address map
//!
//! | Range             | Device            |
//! |-------------------|-------------------|
//! | `$0000–$0FFF`     | 4 KiB RAM         |
//! | `$1000–$CFFF`     | open bus          |
//! | `$D010–$D013`     | MC6821 PIA        |
//! | `$D014–$FEFF`     | open bus          |
//! | `$FF00–$FFFF`     | 256-byte Woz Monitor ROM |
//!
//! Open bus returns the last value driven on the data bus (the high byte
//! of the address if nothing was driven since the last opcode fetch).
//! ROM writes are silently ignored.  The 4 KiB RAM is not mirrored.

use std::fmt;

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

/// Error returned when a host RAM load does not fit entirely inside the
/// Apple I's 4 KiB RAM. Distinct from the CPU crate's fixed 64 KiB
/// `LoadError`: this bus decodes RAM only at `$0000–$0FFF`, so both the
/// start address and the length are checked against 4 KiB.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamLoadError {
    pub start: u16,
    pub len: usize,
}

impl fmt::Display for RamLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot load {} bytes at ${:04X}: exceeds 4 KiB Apple I RAM",
            self.len, self.start
        )
    }
}

impl std::error::Error for RamLoadError {}

impl Apple1Bus {
    pub const RAM_SIZE: usize = 4 * 1024;
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
    /// entirely within the 4 KiB RAM; nothing is copied on error.
    pub fn load_ram(&mut self, start: u16, bytes: &[u8]) -> Result<(), RamLoadError> {
        let offset = usize::from(start);
        if offset > Self::RAM_SIZE || bytes.len() > Self::RAM_SIZE - offset {
            return Err(RamLoadError {
                start,
                len: bytes.len(),
            });
        }
        self.ram[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Side‑effect‑free host inspection of RAM.
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
            0x0000..=0x0FFF => Device::Ram,
            0xD010..=0xD013 => Device::Pia,
            0xFF00..=0xFFFF => Device::Rom,
            _ => Device::Open,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Device {
    Ram,
    Rom,
    Pia,
    Open,
}

impl Bus for Apple1Bus {
    fn read(&mut self, addr: u16) -> u8 {
        let value = match Self::decode(addr) {
            Device::Ram => self.ram[usize::from(addr)],
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
            Device::Ram => self.ram[usize::from(addr)] = value,
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
    fn ram_load_error_names_the_apple_i_ram_size() {
        let err = RamLoadError {
            start: 0x0FFE,
            len: 4,
        };
        assert_eq!(
            err.to_string(),
            "cannot load 4 bytes at $0FFE: exceeds 4 KiB Apple I RAM"
        );
    }
}
