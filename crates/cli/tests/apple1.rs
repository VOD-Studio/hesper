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
//! All tests run offline with the bundled Woz Monitor ROM, including normal
//! startup without --rom and validation of optional external overrides.

use std::{
    env, fs,
    io::{Read, Write},
    path::PathBuf,
    process::{self, Command, Output, Stdio},
    thread,
    time::{Duration, Instant},
};

const EXPECTED_SHA256: &str = "e5af0d1c4057bd8e0ef5cb069c208ff7cc0984a7dff53b12c5cf119de8cb5c25";

/// Hang watchdog, not an emulator performance assertion. Debug builds run
/// every master tick, and the long preset tests compete for CPU when the
/// harness runs them in parallel. Allow headroom for that contention while
/// retaining a finite wait; cycle budgets and output/stop assertions below
/// independently check emulated progress and termination.
const DEADLINE: Duration = Duration::from_secs(60);

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
fn invalid_program_addresses_are_rejected_before_loading_resources() {
    let output = run_apple1_cli(&["--program-address"], b"");
    assert_rejected(&output, "--program-address requires an address");
    for address in ["", "-1", "65536", "0x10000", "0x", "$", "E000", "xyz"] {
        let output = run_apple1_cli(&["--program-address", address], b"");
        assert_rejected(&output, "--program-address requires a 16-bit address");
    }
}

#[test]
fn presets_can_be_listed_without_rom_and_conflicting_options_are_rejected() {
    let output = run_apple1_cli(&["--list-presets"], b"");
    assert!(output.status.success());
    let listing = String::from_utf8(output.stdout).unwrap();
    let listed_ids: Vec<_> = listing
        .lines()
        .filter_map(|line| line.split_whitespace().next())
        .filter(|id| hesper::presets::ProgramPreset::find(id).is_some())
        .collect();
    let expected_ids: Vec<_> = hesper::presets::APPLE1_PRESETS
        .iter()
        .map(|preset| preset.id)
        .collect();
    assert_eq!(listed_ids, expected_ids);
    for args in [
        ["--list-presets", "--expansion-ram"],
        ["--expansion-ram", "--list-presets"],
    ] {
        let output = run_apple1_cli(&args, b"");
        assert!(output.status.success());
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains("扩展 RAM 已开启"), "{stdout}");
        assert!(
            !stdout.contains("需开启扩展 RAM") && !stdout.contains("诊断预期报错"),
            "{stdout}"
        );
    }
    assert_rejected(
        &run_apple1_cli(&["--preset"], b""),
        "--preset requires a preset id",
    );
    assert_rejected(
        &run_apple1_cli(&["--preset", "absent"], b""),
        "unknown preset: absent",
    );
    for conflicting in [["--program", "absent.bin"], ["--program-address", "0"]] {
        for args in [
            ["--preset", "basic-huston", conflicting[0], conflicting[1]],
            [conflicting[0], conflicting[1], "--preset", "basic-huston"],
        ] {
            assert_rejected(&run_apple1_cli(&args, b""), "--preset cannot be combined");
        }
    }
}

#[test]
fn bundled_basic_runs_calculations_and_a_numbered_loop() {
    let output = run_apple1_cli(
        &["--preset", "basic-huston", "--max-cycles", "5000000"],
        b"E000R\nPRINT 1+2\n10 FOR I=1 TO 3\n20 PRINT I*I\n30 NEXT I\n40 END\nRUN\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap().replace('\r', "");
    assert!(stdout.contains("\n3\n"), "{stdout:?}");
    assert!(stdout.contains("\n1\n4\n9\n"), "{stdout:?}");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "[stopped]");
}

#[test]
fn bundled_basic_program_runs_after_the_published_warm_entry() {
    // `resistor-calculator` is a BASIC program: the preset carries Huston
    // BASIC at $E000, the tape header at $004A and the tokenized program at
    // $0800, exactly as the site transfers it — and the site's own listing
    // ends with E2B3R, BASIC's warm entry, so the loaded program survives the
    // entry and RUN can execute it.
    let output = run_apple1_cli(
        &["--preset", "resistor-calculator", "--max-cycles", "6000000"],
        b"E2B3R\nRUN\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap().replace('\r', "");
    assert!(stdout.contains("THE RESISTOR CALCULATOR"), "{stdout:?}");
    assert!(stdout.contains("CREATED BY PAOLO DI LEO"), "{stdout:?}");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "[stopped]");
}

#[test]
fn bundled_assembly_program_runs_from_its_published_entry() {
    let output = run_apple1_cli(
        &["--preset", "15-puzzle", "--max-cycles", "3000000"],
        b"0300R\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8(output.stdout).unwrap().replace('\r', "");
    assert!(stdout.contains("15 PUZZLE - BY JEFF JETTON"), "{stdout:?}");
    assert!(stdout.contains("INSTRUCTIONS (Y/N)?"), "{stdout:?}");
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "[stopped]");
}

#[test]
fn presets_longer_than_a_ram_bank_are_rejected_with_that_reason() {
    // `little-tower` runs from $0300 to $14CD in the published listing, so it
    // needs the optional $1000-$1FFF expansion, disabled by default. The host
    // must say so through the bank error rather than boot a truncated image.
    let output = run_apple1_cli(&["--preset", "little-tower"], b"");
    assert!(!output.status.success());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("must fit within one Apple I RAM bank"),
        "{stderr}"
    );
}

#[test]
fn expansion_ram_allows_little_tower_to_reach_its_menu() {
    let output = run_apple1_cli(
        &[
            "--preset",
            "little-tower",
            "--expansion-ram",
            "--max-cycles",
            "8000000",
        ],
        b"0300R\n",
    );
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(
        stdout.contains("LITTLE TOWER") && stdout.contains("1] PLAY  2] HELP"),
        "{stdout}"
    );
    assert_eq!(String::from_utf8_lossy(&output.stderr).trim(), "[stopped]");
    let output = run_apple1_cli(
        &[
            "--preset",
            "little-tower",
            "--expansion-ram",
            "--no-expansion-ram",
        ],
        b"",
    );
    assert_rejected(&output, "must fit within one Apple I RAM bank");
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
        stderr.contains(EXPECTED_SHA256),
        "expected the pinned fingerprint in the message, got: {stderr:?}"
    );
}

/// Generous cycle ceiling for the real-ROM runs below: every one of them
/// finishes its monitor commands long before this, and the ceiling keeps a
/// regression from hanging the suite.
///
/// What it has to cover, in real CPU cycles: a boot of ~50 000, 2 000 per
/// typed character, one full video frame (~238 000 master ticks, ~17 000
/// CPU cycles) for every character the monitor prints — the video board
/// clocks out at most one character per frame, ~60 characters/second as on
/// the real machine — and, after each line, a drain that waits for three
/// quiet frames (~51 000). The longest run here prints under 60 characters
/// across two lines, so it needs roughly 1.2 million of these 2 million.
const ROM_TEST_MAX_CYCLES: &str = "2000000";

#[test]
fn apple1_cli_boots_to_prompt() {
    let output = run_apple1_cli(&["--max-cycles", ROM_TEST_MAX_CYCLES], b"");
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
fn a_tiny_budget_stops_exactly_where_it_says() {
    // The ROM is still fully validated with a zero budget; what must not
    // happen is any emulated cycle beyond the ceiling. Before the session
    // budget existed, `--max-cycles 1` ran 50 013 cycles.
    for budget in ["0", "1"] {
        let output = run_apple1_cli(&["--max-cycles", budget], b"");
        assert!(
            output.status.success(),
            "a budget stop is graceful, got {:?}",
            output.status
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(
            stderr.contains(&format!("[max cycles reached: {budget}]")),
            "expected the exact budget in the stop message, got: {stderr:?}"
        );
        assert!(
            output.stdout.is_empty(),
            "no character can complete in {budget} cycles, got: {:?}",
            String::from_utf8_lossy(&output.stdout)
        );
    }
}

#[test]
fn repeated_input_cannot_push_the_run_past_its_budget() {
    // 100 lines of input used to buy 100 more batches: the same run
    // reported 262 013 cycles against a 51 000-cycle ceiling.
    let input: Vec<u8> = b"F\n".repeat(100);
    let output = run_apple1_cli(&["--max-cycles", "51000"], &input);
    assert!(
        output.status.success(),
        "a budget stop is graceful, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("[max cycles reached: 51000]"),
        "expected the run to stop at exactly 51000 cycles, got: {stderr:?}"
    );
}

#[test]
fn apple1_cli_examine_command_terminated_by_bare_lf_is_executed() {
    // Regression test: a bare-LF line ending (what a real canonical
    // terminal actually delivers for Enter) must still be recognized as
    // the Woz Monitor's line terminator, not silently swallowed.
    let output = run_apple1_cli(&["--max-cycles", ROM_TEST_MAX_CYCLES], b"FF00.FF0F\n");
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
fn apple1_cli_write_and_examine_ram_terminated_by_bare_lf() {
    let output = run_apple1_cli(
        &["--max-cycles", ROM_TEST_MAX_CYCLES],
        b"300: aB cD eF\n300.302\n",
    );
    assert!(
        output.status.success(),
        "expected exit 0, got {:?}",
        output.status
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("300: AB CD EF"), "got: {stdout:?}");
    assert!(
        stdout.contains("0300: AB CD EF"),
        "expected the deposited bytes to read back after bare-LF-terminated \
         lines, got: {stdout:?}"
    );
}

#[test]
fn oversized_program_is_rejected() {
    let program = TempFile::new("program-4097", &vec![0xEAu8; 4097]);
    let output = run_apple1_cli(&["--program", program.path()], b"");
    assert_rejected(&output, "must fit within one Apple I RAM bank");
}

#[test]
fn program_address_loads_and_runs_in_both_ram_banks() {
    // Original code: print '*' through the same PIA alias used by BASIC,
    // wait for the character to finish, then return to Woz Monitor.
    let program = TempFile::new(
        "program-address",
        &[
            0xA9, 0xAA, // LDA #$AA
            0x2C, 0xF2, 0xD0, 0x30, 0xFB, // BIT $D0F2 / BMI (wait ready)
            0x8D, 0xF2, 0xD0, // STA $D0F2
            0x2C, 0xF2, 0xD0, 0x30, 0xFB, // BIT $D0F2 / BMI (wait done)
            0x4C, 0x00, 0xFF, // JMP $FF00
        ],
    );
    for (address, command, dump) in [
        (None, "0R\n", "0000: A9"),
        (Some("768"), "300R\n", "0300: A9"),
        (Some("0xE000"), "E000R\n", "E000: A9"),
        (Some("0XE000"), "E000R\n", "E000: A9"),
        (Some("$E000"), "E000R\n", "E000: A9"),
        (Some("57344"), "E000R\n", "E000: A9"),
    ] {
        let mut args = vec![
            "--program",
            program.path(),
            "--max-cycles",
            ROM_TEST_MAX_CYCLES,
        ];
        if let Some(address) = address {
            args.extend(["--program-address", address]);
        }
        let output = run_apple1_cli(&args, command.as_bytes());
        assert!(output.status.success(), "{output:?}");
        let stdout = String::from_utf8_lossy(&output.stdout);
        assert!(stdout.contains(dump), "{stdout:?}");
        assert!(stdout.contains('*'), "the program must execute: {stdout:?}");
        assert!(
            stdout.ends_with("\\\r\n"),
            "must return to Woz Monitor: {stdout:?}"
        );
        assert!(!String::from_utf8_lossy(&output.stderr).contains("max cycles reached"));
    }
}

#[test]
fn program_load_cannot_cross_ram_banks_or_target_io_and_rom() {
    let program = TempFile::new("program-range", &[0xEA, 0xEA]);
    for address in [
        "0x0FFF", "0x1000", "0xD010", "0xDFFF", "0xEFFF", "0xF000", "0xFF00", "0xFFFF",
    ] {
        let output = run_apple1_cli(
            &["--program", program.path(), "--program-address", address],
            b"",
        );
        assert_rejected(&output, "must fit within one Apple I RAM bank");
    }
}

/// Trace records the CLI kept, one per line on stderr, above the stop
/// message. Bus records start with the board-clock `M=` marker, instruction
/// records with `$`.
fn trace_lines(output: &Output) -> Vec<String> {
    String::from_utf8_lossy(&output.stderr)
        .lines()
        .filter(|line| line.starts_with('M') || line.starts_with('$'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn a_bus_trace_reports_every_executed_cycle_including_the_reset_vector() {
    let output = run_apple1_cli(
        &["--max-cycles", "20", "--bus-trace", "--trace-limit", "4096"],
        b"",
    );
    assert!(
        output.status.success(),
        "a budget stop is graceful, got {:?}",
        output.status
    );
    let lines = trace_lines(&output);
    assert_eq!(
        lines.len(),
        20,
        "20 executed cycles must produce 20 bus records, got: {lines:?}"
    );
    // Each bus record carries the board's master-tick marker ahead of the
    // session's CPU-cycle index.
    assert!(
        lines.iter().all(|line| line.starts_with("M=")),
        "every bus record must start with the board-clock marker, got: {lines:?}"
    );
    // The physical RESET sequence's own vector reads are real bus cycles.
    assert!(
        lines.iter().any(|line| line.contains("$FFFC=")),
        "expected the reset vector low read, got: {lines:?}"
    );
    assert!(
        lines.iter().any(|line| line.contains("$FFFD=")),
        "expected the reset vector high read, got: {lines:?}"
    );
}

#[test]
fn the_trace_limit_bounds_both_kinds_together() {
    let bus_only = run_apple1_cli(
        &["--max-cycles", "20", "--bus-trace", "--trace-limit", "4"],
        b"",
    );
    let bus_lines = trace_lines(&bus_only);
    assert_eq!(
        bus_lines.len(),
        4,
        "only the last four records may be kept, got: {bus_lines:?}"
    );

    let both = run_apple1_cli(
        &[
            "--max-cycles",
            "20",
            "--bus-trace",
            "--trace",
            "--trace-limit",
            "4",
        ],
        b"",
    );
    let both_lines = trace_lines(&both);
    assert_eq!(
        both_lines.len(),
        4,
        "two trace kinds share one limit, got: {both_lines:?}"
    );

    // Unbounded enough to keep everything: the instruction trace reports
    // the completed physical RESET sequence, the bus trace does not.
    let instructions = run_apple1_cli(
        &["--max-cycles", "20", "--trace", "--trace-limit", "4096"],
        b"",
    );
    let instruction_lines = trace_lines(&instructions);
    assert!(
        instruction_lines.iter().any(|line| line.contains("RESET")),
        "expected the completed RESET sequence record, got: {instruction_lines:?}"
    );
    assert!(
        instruction_lines.len() < 20,
        "instruction records are per completed step, not per cycle, got: \
         {instruction_lines:?}"
    );
}

#[test]
fn tracing_does_not_change_what_the_machine_does() {
    // Both runs get the same generous budget so that the comparison covers
    // the deposit and the examine dump actually completing, not two runs
    // cut off at the same early point.
    let plain = run_apple1_cli(
        &["--max-cycles", ROM_TEST_MAX_CYCLES],
        b"300: AB CD EF\n300.302\n",
    );
    let traced = run_apple1_cli(
        &[
            "--max-cycles",
            ROM_TEST_MAX_CYCLES,
            "--trace",
            "--bus-trace",
        ],
        b"300: AB CD EF\n300.302\n",
    );
    assert_eq!(
        plain.stdout, traced.stdout,
        "the same input and budget must produce the same machine output"
    );
    assert!(!trace_lines(&traced).is_empty(), "the traced run recorded");
    assert!(
        trace_lines(&plain).is_empty(),
        "the plain run recorded none"
    );
}

#[test]
fn an_unsupported_opcode_fails_with_the_trace_that_led_to_it() {
    // $02 is not an official NMOS opcode; the monitor's own RUN command
    // jumps straight into it.
    //
    // The budget must cover the run up to that jump. The video terminal
    // takes one character per carousel lap, so every character the monitor
    // prints costs about a frame (~17 000 real CPU cycles): the prompt and
    // the two echoed command lines run to roughly twenty characters, the
    // boot costs ~50 000, and the batch loop's three-frame quiet drain
    // runs twice. Measured: the monitor jumps into $0300 at about 514 500
    // CPU cycles, so 800 000 leaves roughly 1.5x margin.
    let output = run_apple1_cli(
        &[
            "--max-cycles",
            "800000",
            "--bus-trace",
            "--trace-limit",
            "8",
        ],
        b"300: 02\n300R\n",
    );
    assert!(
        !output.status.success(),
        "a real CPU error must exit non-zero, got {:?}",
        output.status
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        stderr.contains("unsupported opcode $02 at $0300"),
        "expected the failing address and opcode, got: {stderr:?}"
    );
    let lines = trace_lines(&output);
    assert_eq!(
        lines.len(),
        8,
        "the records leading to the error must still be reported, got: {lines:?}"
    );
}

#[test]
fn external_rom_override_matches_bundled_firmware() {
    let rom = TempFile::new(
        "bundled-rom-override",
        include_bytes!("../assets/wozmon.bin"),
    );
    let input = b"FF00.FF0F\n";
    let bundled = run_apple1_cli(&["--max-cycles", ROM_TEST_MAX_CYCLES], input);
    let external = run_apple1_cli(
        &["--rom", rom.path(), "--max-cycles", ROM_TEST_MAX_CYCLES],
        input,
    );
    assert!(bundled.status.success());
    assert_eq!(bundled.status.code(), external.status.code());
    assert_eq!(bundled.stdout, external.stdout);
    assert_eq!(bundled.stderr, external.stderr);
}
