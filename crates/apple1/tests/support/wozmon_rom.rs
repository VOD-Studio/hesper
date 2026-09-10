//! Loader for a legally-obtained Woz Monitor ROM image.
//!
//! The Apple I's 256-byte Woz Monitor is Apple's own firmware, not this
//! project's code; per `AGENTS.md` and `docs/roadmap.md` (M3.1) it is never
//! downloaded, embedded, or committed to this repository. Tests that need
//! it to exercise real monitor interaction read the image from a path the
//! caller supplies and are `#[ignore]`d so the default `cargo test
//! --workspace` stays self-contained and offline; running them explicitly
//! without the resource is a loud failure, not a silent skip. See
//! `crates/apple1/tests/data/README.md`.

use std::{env, fs};

use sha2::{Digest, Sha256};

/// Environment variable holding the path to a 256-byte Woz Monitor ROM
/// image the caller has the rights to use.
pub const ENV_VAR: &str = "HESPER_APPLE1_ROM";

/// SHA-256 fingerprint of the Woz Monitor ROM as printed in the Apple-1
/// Operation Manual (1976), cross-checked against
/// <https://github.com/alangarf/apple-one/blob/master/roms/wozmon.hex>.
/// This is an integrity check, not a copy of the ROM contents.
pub const EXPECTED_SHA256: &str =
    "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25";

/// Load and verify the externally supplied ROM. Panics with an actionable
/// message when the resource is missing, the wrong size, or does not match
/// the recorded fingerprint.
pub fn load() -> [u8; 256] {
    let path = env::var(ENV_VAR).unwrap_or_else(|_| {
        panic!(
            "{ENV_VAR} is not set. This test requires a Woz Monitor ROM image \
             you have the rights to use; set {ENV_VAR} to its path. \
             See crates/apple1/tests/data/README.md for the resource and \
             licensing notes, and `make wozmon-verify ROM=<path>` to check it \
             before running."
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
