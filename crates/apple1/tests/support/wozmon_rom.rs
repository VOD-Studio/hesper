//! The CLI's bundled firmware is also the machine integration fixture.
use sha2::{Digest, Sha256};

pub fn load() -> [u8; 256] {
    let rom = include_bytes!("../../../cli/assets/wozmon.bin");
    assert_eq!(
        format!("{:x}", Sha256::digest(rom)),
        "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25",
        "bundled Woz Monitor fingerprint changed"
    );
    *rom
}
