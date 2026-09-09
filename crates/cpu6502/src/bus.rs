use std::fmt;

/// Every emulated CPU memory access passes through this interface.
pub trait Bus {
    /// A read may have side effects, as on a memory-mapped device.
    fn read(&mut self, addr: u16) -> u8;
    fn write(&mut self, addr: u16, value: u8);
}

pub const RAM_SIZE: usize = 65_536;

/// Zero-filled, unmapped 64 KiB RAM for tests and simple hosts.
pub struct Ram {
    bytes: [u8; RAM_SIZE],
}

impl Ram {
    pub fn new() -> Self {
        Self {
            bytes: [0; RAM_SIZE],
        }
    }

    /// Host loading never wraps. A failed load leaves all RAM unchanged.
    pub fn load(&mut self, start: u16, bytes: &[u8]) -> Result<(), LoadError> {
        let offset = usize::from(start);
        // Subtract first so even an oversized host slice cannot overflow usize.
        if bytes.len() > RAM_SIZE - offset {
            return Err(LoadError {
                start,
                len: bytes.len(),
            });
        }
        self.bytes[offset..offset + bytes.len()].copy_from_slice(bytes);
        Ok(())
    }

    /// Side-effect-free host inspection of this RAM, not an emulated bus read.
    pub fn as_slice(&self) -> &[u8] {
        &self.bytes
    }
}

impl Default for Ram {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus for Ram {
    fn read(&mut self, addr: u16) -> u8 {
        self.bytes[usize::from(addr)]
    }

    fn write(&mut self, addr: u16, value: u8) {
        self.bytes[usize::from(addr)] = value;
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LoadError {
    pub start: u16,
    pub len: usize,
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "cannot load {} bytes at ${:04X}: exceeds 64 KiB RAM",
            self.len, self.start
        )
    }
}

impl std::error::Error for LoadError {}
