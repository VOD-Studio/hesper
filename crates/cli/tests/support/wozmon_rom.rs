//! Loader for a legally-obtained Woz Monitor ROM image.
//!
//! Mirrors `crates/apple1/tests/support/wozmon_rom.rs`: the Woz Monitor is
//! Apple's own firmware and is never embedded or committed here. Tests never
//! download it; those needing it read the image from a caller-supplied path and
//! are `#[ignore]`d; running them explicitly without the resource fails
//! loudly instead of skipping silently. See
//! `crates/apple1/tests/data/README.md`.

use std::{env, fs};

use sha2::{Digest, Sha256};

pub const ENV_VAR: &str = "HESPER_APPLE1_ROM";

pub const EXPECTED_SHA256: &str =
    "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25";

pub fn load() -> [u8; 256] {
    let path = env::var(ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{ENV_VAR} is not set. This test requires a Woz Monitor ROM image \
             you have the rights to use; set {ENV_VAR} to its path. \
             See crates/apple1/tests/data/README.md."
        )
    });
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("cannot read {ENV_VAR}={path}: {e}"));
    assert_eq!(
        bytes.len(),
        256,
        "{path} is {} bytes; expected a 256-byte Woz Monitor ROM image",
        bytes.len()
    );
    let digest = format!("{:x}", Sha256::digest(&bytes));
    assert_eq!(
        digest, EXPECTED_SHA256,
        "{path} does not match the recorded Woz Monitor fingerprint \
         (got {digest}, expected {EXPECTED_SHA256}); wrong file or corrupted image"
    );
    let mut rom = [0u8; 256];
    rom.copy_from_slice(&bytes);
    rom
}

/// Path to a ROM file for the CLI's `--rom` flag: writes the loaded and
/// verified image to a fresh temp file (the CLI reads ROMs from disk, not
/// from memory) and returns its path alongside a guard that removes it.
pub struct RomFile(std::path::PathBuf);

impl RomFile {
    pub fn from_env(tag: &str) -> Self {
        let rom = load();
        let path = std::env::temp_dir().join(format!(
            "hesper-apple1-cli-test-{tag}-{}.rom",
            std::process::id()
        ));
        fs::write(&path, rom).unwrap();
        Self(path)
    }

    pub fn path(&self) -> &str {
        self.0.to_str().unwrap()
    }
}

impl Drop for RomFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}
