//! Integration tests for the `hesper apple1` subcommand against the real
//! built binary and a real OS pipe — not the `Apple1` library API that
//! `crates/apple1/tests/wozmon.rs` exercises directly.
//!
//! These specifically cover the interactive stdin loop in
//! `crates/cli/src/apple1.rs`, including line-ending handling: a canonical
//! terminal delivers the user's Enter key (CR, 0x0D) to a reading process
//! as LF (0x0A) — POSIX `ICRNL` translates CR to NL on input before the
//! line discipline hands the buffered line to `read_line`. Piping a
//! bare-LF-terminated line reproduces exactly what the CLI sees from a real
//! terminal session, without needing a PTY in the test harness.
//!
//! They require a real Woz Monitor ROM image supplied externally (see
//! `crates/cli/tests/support/wozmon_rom.rs` and
//! `crates/apple1/tests/data/README.md`) and are `#[ignore]`d so the
//! default `cargo test --workspace` stays self-contained and offline. Run
//! explicitly with the resource present:
//!
//! ```sh
//! HESPER_APPLE1_ROM=/path/to/wozmon.bin cargo test -p hesper --test apple1 -- --ignored
//! ```

#[path = "support/wozmon_rom.rs"]
mod wozmon_rom;

use std::{
    io::Write,
    process::{Command, Stdio},
};

use wozmon_rom::RomFile;

/// Run the built `hesper apple1` binary, feed it `stdin_input`, close
/// stdin (EOF), and return its captured stdout as text.
fn run_apple1_cli(rom: &RomFile, stdin_input: &[u8]) -> String {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hesper"))
        .args(["apple1", "--rom", rom.path()])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn hesper apple1");
    child.stdin.take().unwrap().write_all(stdin_input).unwrap();
    let output = child.wait_with_output().unwrap();
    String::from_utf8_lossy(&output.stdout).into_owned()
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn apple1_cli_boots_to_prompt() {
    let rom = RomFile::from_env("boot");
    let stdout = run_apple1_cli(&rom, b"");
    assert!(
        stdout.contains('\\'),
        "expected the Woz Monitor prompt in boot output, got: {stdout:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn apple1_cli_examine_command_terminated_by_bare_lf_is_executed() {
    // Regression test: a bare-LF line ending (what a real canonical
    // terminal actually delivers for Enter) must still be recognized as
    // the Woz Monitor's line terminator, not silently swallowed.
    let rom = RomFile::from_env("examine");
    let stdout = run_apple1_cli(&rom, b"FF00.FF0F\n");
    assert!(
        stdout.contains("FF00: D8 58"),
        "expected a ROM dump starting with the Woz Monitor's own CLD/CLI \
         bytes, got: {stdout:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn apple1_cli_write_and_examine_ram_terminated_by_bare_lf() {
    let rom = RomFile::from_env("write-examine");
    let stdout = run_apple1_cli(&rom, b"300: AB CD EF\n300.302\n");
    assert!(
        stdout.contains("0300: AB CD EF"),
        "expected the deposited bytes to read back after bare-LF-terminated \
         lines, got: {stdout:?}"
    );
}
