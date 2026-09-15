//! Apple I bus: 8 KiB RAM, optional 4 KiB expansion, ROM, and MC6821 PIA.
//!
//! ## Address map
//!
//! | Range             | Device            |
//! |-------------------|-------------------|
//! | `$0000–$0FFF`     | 4 KiB RAM         |
//! | `$1000–$1FFF`   | optional 4 KiB RAM |
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
    ram: Vec<u8>,
    expansion_ram: bool,
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

/// A host load does not fit within a contiguous installed RAM region.
/// The optional expansion joins low RAM into $0000–$1FFF.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RamLoadError {
    pub start: u16,
    pub len: usize,
    pub expansion_ram: bool,
}

impl fmt::Display for RamLoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.expansion_ram {
            return write!(
                f,
                "cannot load {} bytes at ${:04X}: must fit within installed Apple I RAM ($0000–$1FFF or $E000–$EFFF)",
                self.len, self.start
            );
        }
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
    /// Default RAM capacity; expansion adds another RAM_BANK_SIZE bytes.
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
        Self::with_expansion_ram(rom, false)
    }

    /// Create a bus with optional 4 KiB expansion RAM at $1000–$1FFF.
    /// This is an address-map option, not an electrical expansion-card model.
    pub fn with_expansion_ram(rom: &[u8], expansion_ram: bool) -> Result<Self, RomSizeError> {
        if rom.len() != Self::ROM_SIZE {
            return Err(RomSizeError {
                expected: Self::ROM_SIZE,
                got: rom.len(),
            });
        }
        let mut bytes = [0u8; Self::ROM_SIZE];
        bytes.copy_from_slice(rom);
        let mut pia = Pia6821::new();
        // The board ties PA7 to +5V even before the keyboard presents a byte.
        pia.set_port_a_inputs(0x80);
        Ok(Self {
            ram: vec![0; Self::low_ram_size(expansion_ram) + Self::RAM_BANK_SIZE],
            expansion_ram,
            rom: bytes,
            pia,
            last_read: 0,
        })
    }

    /// Host load into RAM (non‑wrapping, transactional).  Returns an error
    /// unless the entire region is installed contiguous RAM. Nothing is copied
    /// on error. Empty loads at a region's exclusive end are allowed.
    pub fn load_ram(&mut self, start: u16, bytes: &[u8]) -> Result<(), RamLoadError> {
        let range = Self::ram_range(start, bytes.len(), self.expansion_ram)?;
        self.ram[range].copy_from_slice(bytes);
        Ok(())
    }

    /// Validate a host load before creating a machine or changing memory.
    pub fn validate_ram_load(start: u16, len: usize) -> Result<(), RamLoadError> {
        Self::validate_ram_load_with_expansion(start, len, false)
    }

    /// Validate against the selected RAM mapping without mutating a machine.
    pub fn validate_ram_load_with_expansion(
        start: u16,
        len: usize,
        expansion_ram: bool,
    ) -> Result<(), RamLoadError> {
        Self::ram_range(start, len, expansion_ram).map(|_| ())
    }

    /// Whether RAM at $1000–$1FFF is installed.
    pub fn expansion_ram(&self) -> bool {
        self.expansion_ram
    }

    fn low_ram_size(expansion_ram: bool) -> usize {
        Self::RAM_BANK_SIZE * if expansion_ram { 2 } else { 1 }
    }

    fn ram_range(
        start: u16,
        len: usize,
        expansion_ram: bool,
    ) -> Result<Range<usize>, RamLoadError> {
        let error = RamLoadError {
            start,
            len,
            expansion_ram,
        };
        let low_size = Self::low_ram_size(expansion_ram);
        let address = usize::from(start);
        let (begin, available) = if address <= low_size {
            (address, low_size - address)
        } else if (Self::HIGH_RAM_BASE..=0xF000).contains(&start) {
            let offset = usize::from(start - Self::HIGH_RAM_BASE);
            (low_size + offset, Self::RAM_BANK_SIZE - offset)
        } else {
            return Err(error);
        };
        if len > available {
            return Err(error);
        }
        Ok(begin..begin + len)
    }

    /// Side-effect-free host inspection of packed low and high RAM: the first
    /// 4 KiB (8 KiB with expansion) corresponds to low RAM, followed by
    /// 4 KiB at $E000–$EFFF.
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
    fn decode(&self, addr: u16) -> Device {
        match addr {
            0x0000..=0x0FFF => Device::Ram(usize::from(addr)),
            0x1000..=0x1FFF if self.expansion_ram => Device::Ram(usize::from(addr)),
            0xE000..=0xEFFF => Device::Ram(
                Self::low_ram_size(self.expansion_ram) + usize::from(addr - Self::HIGH_RAM_BASE),
            ),
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
        let value = match self.decode(addr) {
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
        match self.decode(addr) {
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
    fn expansion_maps_contiguous_low_ram_without_aliasing_high_ram() {
        let mut bus = Apple1Bus::with_expansion_ram(&dummy_rom(), true).unwrap();
        assert!(bus.expansion_ram());
        bus.load_ram(0x0FFF, &[0x12, 0x34]).unwrap();
        bus.write(0x1FFF, 0x56);
        bus.write(0xE000, 0x78);
        bus.write(0xEFFF, 0x9A);
        for (address, value) in [
            (0x0FFF, 0x12),
            (0x1000, 0x34),
            (0x1FFF, 0x56),
            (0xE000, 0x78),
            (0xEFFF, 0x9A),
        ] {
            assert_eq!(bus.read(address), value);
        }
        let before = bus.ram_slice().to_vec();
        for (start, len) in [
            (0x1FFF, 2),
            (0x2000, 1),
            (0x2001, 0),
            (0xD010, 1),
            (0xEFFF, 2),
            (0xFFFF, 2),
            (0, usize::MAX),
        ] {
            assert!(Apple1Bus::validate_ram_load_with_expansion(start, len, true).is_err());
        }
        assert!(bus.load_ram(0x1FFF, &[0xFF; 2]).is_err());
        bus.load_ram(0x2000, &[]).unwrap();
        bus.write(0x2000, 0xCC);
        bus.write(0xFF00, 0xCC);
        assert_eq!(bus.read(0xFF00), 0xD8);
        assert_eq!(
            bus.read(0x2000),
            0xD8,
            "unmapped addresses still read open bus"
        );
        assert_eq!(bus.ram_slice(), before);
        let mut default_bus = Apple1Bus::new(&dummy_rom()).unwrap();
        assert!(!default_bus.expansion_ram());
        assert!(default_bus.load_ram(0x0FFF, &[0x12, 0x34]).is_err());
        assert!(default_bus.load_ram(0x1000, &[0x34]).is_err());
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
                len: 2,
                expansion_ram: false,
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
            assert_eq!(
                err,
                RamLoadError {
                    start,
                    len: 0,
                    expansion_ram: false
                }
            );
            let err = bus.load_ram(start, &[0x99]).unwrap_err();
            assert_eq!(
                err,
                RamLoadError {
                    start,
                    len: 1,
                    expansion_ram: false
                }
            );
        }
        assert_eq!(bus.ram_slice(), &[0u8; Apple1Bus::RAM_SIZE]);
    }

    #[test]
    fn ram_load_error_names_the_apple_i_ram_banks() {
        let err = RamLoadError {
            start: 0x0FFE,
            len: 4,
            expansion_ram: false,
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
                Err(RamLoadError {
                    start,
                    len,
                    expansion_ram: false
                })
            );
            assert_eq!(bus.ram_slice(), before);
        }
        assert!(Apple1Bus::validate_ram_load(0xE000, usize::MAX).is_err());
    }
}
