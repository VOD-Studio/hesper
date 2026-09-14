//! Apple-1 program images bundled with the host, independent of the CPU core.

#[derive(Debug, PartialEq, Eq)]
pub struct ProgramPreset {
    pub id: &'static str,
    pub name: &'static str,
    pub address: u16,
    pub entry: u16,
    pub bytes: &'static [u8],
}

pub const APPLE1_PRESETS: &[ProgramPreset] = &[ProgramPreset {
    id: "basic-huston",
    name: "BASIC (Huston)",
    address: 0xE000,
    entry: 0xE000,
    bytes: include_bytes!("../assets/basic-huston.bin"),
}];

#[cfg(test)]
mod tests {
    use super::*;
    use hesper_apple1::Apple1Bus;
    use sha2::{Digest, Sha256};

    #[test]
    fn bundled_basic_matches_the_supplied_image_and_fits_ram() {
        let preset = &APPLE1_PRESETS[0];
        assert_eq!(preset.bytes.len(), 4096);
        assert_eq!(
            format!("{:x}", Sha256::digest(preset.bytes)),
            "311c85f22996e655ae3a0881e0841a547c52f5ec20cd810035ec91ce13a27cbe"
        );
        Apple1Bus::validate_ram_load(preset.address, preset.bytes.len()).unwrap();
        assert!(usize::from(preset.entry - preset.address) < preset.bytes.len());
    }
}
