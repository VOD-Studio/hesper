//! Integration tests for the `hesper apple1` subcommand against the real
//! built binary and a real OS pipe — not the `Apple1` library API that
//! `crates/apple1/tests/wozmon.rs` exercises directly.
//!
//! These specifically cover the batch stdin loop in
//! `crates/cli/src/apple1.rs`, including line-ending handling: a canonical
//! terminal delivers the user's Enter key (CR, 0x0D) to a reading process
//! as LF (0x0A) — POSIX `ICRNL` translates CR to NL on input before the
//! line discipline hands the buffered line to `read_line`. Piping a
//! bare-LF-terminated line reproduces exactly what the CLI sees from a real
//! terminal session, without needing a PTY in the test harness.
//!
//! Argument and file validation is checked offline. The tests that need a
//! real Woz Monitor ROM image (supplied externally; see
//! `crates/cli/tests/support/wozmon_rom.rs` and
//! `crates/apple1/tests/data/README.md`) are `#[ignore]`d so the default
//! `cargo test --workspace` stays self-contained and offline. Run them
//! explicitly with the resource present:
//!
//! ```sh
//! HESPER_APPLE1_ROM=/path/to/wozmon.bin cargo test -p hesper --test apple1 -- --ignored
//! ```

#[path = "support/wozmon_rom.rs"]
mod wozmon_rom;

use std::{
    env, fs,
    io::{Read, Write},
    path::PathBuf,
    process::{self, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

use wozmon_rom::RomFile;

/// Wall-clock ceiling for one CLI run. Every emulator run in these tests is
/// bounded by `--max-cycles` or by stdin EOF, so hitting this means the
/// host loop failed to stop — a failure, never a silent hang.
const DEADLINE: Duration = Duration::from_secs(10);

/// Run the built `hesper apple1` binary with `args`, feed it `stdin_input`,
/// close stdin (EOF), and return its exit status and captured streams.
///
/// stdout and stderr are drained concurrently: a run that produces more
/// output than a pipe buffer holds would otherwise deadlock against a
/// parent that only reads after `wait`.
fn run_apple1_cli(args: &[&str], stdin_input: &[u8]) -> Output {
    let mut child = Command::new(env!("CARGO_BIN_EXE_hesper"))
        .arg("apple1")
        .args(args)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn hesper apple1");

    let mut stdin = child.stdin.take().expect("piped stdin");
    let input = stdin_input.to_vec();
    let writer = thread::spawn(move || {
        // A run stopped by its cycle budget may never read all of stdin;
        // the broken pipe is expected, not a test failure.
        let _ = stdin.write_all(&input);
    });
    let mut stdout_pipe = child.stdout.take().expect("piped stdout");
    let mut stderr_pipe = child.stderr.take().expect("piped stderr");
    let stdout_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stdout_pipe.read_to_end(&mut buffer);
        buffer
    });
    let stderr_reader = thread::spawn(move || {
        let mut buffer = Vec::new();
        let _ = stderr_pipe.read_to_end(&mut buffer);
        buffer
    });

    let deadline = Instant::now() + DEADLINE;
    let status = loop {
        match child.try_wait().expect("wait on hesper apple1") {
            Some(status) => break status,
            None if Instant::now() >= deadline => {
                let _ = child.kill();
                let status = child.wait().expect("reap killed hesper apple1");
                let stdout = stdout_reader.join().expect("stdout reader");
                let stderr = stderr_reader.join().expect("stderr reader");
                panic!(
                    "hesper apple1 {args:?} did not exit within {DEADLINE:?} ({status:?})\n\
                     stdout: {:?}\nstderr: {:?}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
            }
            None => thread::sleep(Duration::from_millis(10)),
        }
    };
    let _ = writer.join();

    Output {
        status,
        stdout: stdout_reader.join().expect("stdout reader"),
        stderr: stderr_reader.join().expect("stderr reader"),
    }
}

/// A scratch file removed when the test ends.
struct TempFile(PathBuf);

impl TempFile {
    fn new(tag: &str, bytes: &[u8]) -> Self {
        let path = env::temp_dir().join(format!("hesper-apple1-cli-{tag}-{}.bin", process::id()));
        fs::write(&path, bytes).expect("write scratch file");
        Self(path)
    }

    fn path(&self) -> &str {
        self.0.to_str().expect("utf-8 scratch path")
    }
}

impl Drop for TempFile {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

/// Assert a run failed before starting the emulator: non-zero exit, the
/// expected diagnostic on stderr, and nothing at all on stdout.
fn assert_rejected(output: &Output, expected: &str) {
    assert!(
        !output.status.success(),
        "expected a non-zero exit, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(expected),
        "expected stderr to contain {expected:?}, got: {stderr:?}"
    );
    assert!(
        output.stdout.is_empty(),
        "a rejected run must produce no machine output, got: {:?}",
        String::from_utf8_lossy(&output.stdout)
    );
}

#[test]
fn zero_cycles_per_char_is_rejected() {
    // A zero-cycle display timer used to make the busy countdown underflow
    // and panic on the first character written.
    let rom = TempFile::new("zero-delay-rom", &[0u8; 256]);
    let output = run_apple1_cli(&["--rom", rom.path(), "--cycles-per-char", "0"], b"");
    assert_rejected(&output, "--cycles-per-char requires a positive integer");
}

#[test]
fn non_numeric_cycles_per_char_is_rejected() {
    let rom = TempFile::new("bad-delay-rom", &[0u8; 256]);
    let output = run_apple1_cli(&["--rom", rom.path(), "--cycles-per-char", "fast"], b"");
    assert_rejected(&output, "--cycles-per-char requires a positive integer");
}

#[test]
fn out_of_range_trace_limit_is_rejected() {
    let rom = TempFile::new("trace-limit-rom", &[0u8; 256]);
    for limit in ["0", "4097", "many"] {
        let output = run_apple1_cli(&["--rom", rom.path(), "--trace-limit", limit], b"");
        assert_rejected(&output, "--trace-limit requires 1..4096");
    }
}

#[test]
fn non_numeric_max_cycles_is_rejected() {
    let rom = TempFile::new("max-cycles-rom", &[0u8; 256]);
    let output = run_apple1_cli(&["--rom", rom.path(), "--max-cycles", "lots"], b"");
    assert_rejected(&output, "--max-cycles requires an unsigned integer");
}

#[test]
fn missing_rom_file_is_reported() {
    let missing = env::temp_dir().join(format!("hesper-apple1-absent-{}.bin", process::id()));
    let output = run_apple1_cli(&["--rom", missing.to_str().unwrap()], b"");
    assert_rejected(&output, "cannot read ROM file");
}

#[test]
fn wrong_size_rom_is_rejected() {
    for (tag, size) in [("rom-255", 255usize), ("rom-257", 257)] {
        let rom = TempFile::new(tag, &vec![0u8; size]);
        let output = run_apple1_cli(&["--rom", rom.path()], b"");
        assert_rejected(&output, "is exactly 256 bytes");
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("is {size} bytes")),
            "expected the actual size in the message, got: {stderr:?}"
        );
    }
}

#[test]
fn right_size_but_wrong_rom_is_rejected() {
    // A 256-byte file that is not the Woz Monitor: accepting it would boot
    // into unexplained garbage instead of naming the real problem.
    let rom = TempFile::new("rom-wrong-hash", &[0x42u8; 256]);
    let output = run_apple1_cli(&["--rom", rom.path()], b"");
    assert_rejected(&output, "ROM SHA-256 mismatch");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains(wozmon_rom::EXPECTED_SHA256),
        "expected the pinned fingerprint in the message, got: {stderr:?}"
    );
}

/// Generous cycle ceiling for the real-ROM runs below: every one of them
/// finishes its monitor commands long before this, and the ceiling keeps a
/// regression from hanging the suite.
const ROM_TEST_MAX_CYCLES: &str = "2000000";

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn apple1_cli_boots_to_prompt() {
    let rom = RomFile::from_env("boot");
    let output = run_apple1_cli(
        &["--rom", rom.path(), "--max-cycles", ROM_TEST_MAX_CYCLES],
        b"",
    );
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
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
    let output = run_apple1_cli(
        &["--rom", rom.path(), "--max-cycles", ROM_TEST_MAX_CYCLES],
        b"FF00.FF0F\n",
    );
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
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
    let output = run_apple1_cli(
        &["--rom", rom.path(), "--max-cycles", ROM_TEST_MAX_CYCLES],
        b"300: AB CD EF\n300.302\n",
    );
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("0300: AB CD EF"),
        "expected the deposited bytes to read back after bare-LF-terminated \
         lines, got: {stdout:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn oversized_program_is_rejected() {
    let rom = RomFile::from_env("big-program");
    let program = TempFile::new("program-4097", &vec![0xEAu8; 4097]);
    let output = run_apple1_cli(&["--rom", rom.path(), "--program", program.path()], b"");
    assert_rejected(&output, "program exceeds 4 KiB Apple I RAM");
}
