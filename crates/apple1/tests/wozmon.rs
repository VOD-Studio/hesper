//! Woz Monitor integration tests.
//!
//! These require a real Woz Monitor ROM image supplied externally (see
//! `crates/apple1/tests/support/wozmon_rom.rs` and
//! `crates/apple1/tests/data/README.md`); they are `#[ignore]`d so the
//! default `cargo test --workspace` stays self-contained and offline.
//! Run explicitly with the resource present:
//!
//! ```sh
//! HESPER_APPLE1_ROM=/path/to/wozmon.bin cargo test -p hesper-apple1 --test wozmon -- --ignored
//! ```
//!
//! Every wait is for a *specific complete result* — a full prompt, a full
//! dump line, bytes actually present in RAM — never for "output stopped for
//! a while", which passes just as happily when the machine printed a
//! command echo and then hung.

#[path = "support/wozmon_rom.rs"]
mod wozmon_rom;

use std::{collections::VecDeque, fmt::Write as _, num::NonZeroU64};

use hesper_apple1::Apple1;
use hesper_cpu6502::{Cycle, DebugState};

/// Cycles allowed for any single wait. ~1 second of real Apple I time,
/// orders of magnitude more than any monitor command needs.
const WAIT_BUDGET: u64 = 1_000_000;

/// Cycles between output drains / predicate checks.
const CHECK_INTERVAL: u64 = 1_000;

/// Cycle records kept for failure reporting.
const TRACE_KEEP: usize = 64;

/// Run the machine until `predicate` accepts the machine state and the
/// output accumulated so far. Panics with the recent bus trace, CPU state
/// and screen contents on a CPU error or an exhausted budget — a wait that
/// never completes is a failure, never a silent pass.
fn run_until(
    machine: &mut Apple1,
    budget: u64,
    mut predicate: impl FnMut(&Apple1, &[u8]) -> bool,
) -> Vec<u8> {
    let mut output = Vec::new();
    let mut recent: VecDeque<(u64, Cycle, DebugState)> = VecDeque::with_capacity(TRACE_KEEP);
    let mut executed = 0;

    while executed < budget {
        let chunk = CHECK_INTERVAL.min(budget - executed);
        for _ in 0..chunk {
            match machine.cycle() {
                Ok(cycle) => {
                    if recent.len() == TRACE_KEEP {
                        recent.pop_front();
                    }
                    recent.push_back((machine.total_cycles(), cycle, machine.cpu().debug_state()));
                }
                Err(err) => {
                    output.extend(machine.drain_output());
                    panic!(
                        "{}",
                        report(machine, &output, &recent, &format!("CPU error: {err}"))
                    );
                }
            }
        }
        executed += chunk;
        output.extend(machine.drain_output());
        if predicate(machine, &output) {
            return output;
        }
    }

    panic!(
        "{}",
        report(
            machine,
            &output,
            &recent,
            &format!("wait budget of {budget} cycles exhausted")
        )
    );
}

fn report(
    machine: &Apple1,
    output: &[u8],
    recent: &VecDeque<(u64, Cycle, DebugState)>,
    why: &str,
) -> String {
    let mut text = String::new();
    let _ = writeln!(text, "{why}");
    let _ = writeln!(text, "total cycles: {}", machine.total_cycles());
    let _ = writeln!(text, "registers: {:?}", machine.cpu().registers());
    let _ = writeln!(text, "output so far: {:?}", String::from_utf8_lossy(output));
    let _ = writeln!(text, "screen:");
    for (row, line) in machine.display().screen().iter().enumerate() {
        let _ = writeln!(text, "  {row:02}|{}|", String::from_utf8_lossy(line));
    }
    let (cursor_row, cursor_col) = machine.display().cursor();
    let _ = writeln!(text, "cursor: ({cursor_row}, {cursor_col})");
    let _ = writeln!(text, "last {} cycles:", recent.len());
    for (total, cycle, state) in recent {
        let _ = writeln!(text, "  C{total} {cycle:?} | {state:?}");
    }
    text
}

/// The lines the display has finished: the trailing fragment after the last
/// CR is dropped, so a predicate never accepts a half-printed line.
fn complete_lines(output: &[u8]) -> Vec<String> {
    let mut lines: Vec<String> = output
        .split(|&byte| byte == b'\r')
        .map(|line| String::from_utf8_lossy(line).into_owned())
        .collect();
    lines.pop();
    lines
}

/// Count the two-digit hex values a monitor dump line lists after its
/// `ADDR:` prefix.
fn dump_byte_count(line: &str, prefix: &str) -> usize {
    match line.split_once(prefix) {
        Some((_, rest)) => rest
            .split_whitespace()
            .take_while(|token| token.len() == 2 && token.chars().all(|c| c.is_ascii_hexdigit()))
            .count(),
        None => 0,
    }
}

fn has_full_dump_line(output: &[u8], prefix: &str, bytes: usize) -> bool {
    complete_lines(output)
        .iter()
        .any(|line| dump_byte_count(line, prefix) == bytes)
}

/// Boot the machine and return the boot output, which must contain the
/// monitor's complete prompt: backslash followed by CR.
fn boot() -> (Apple1, Vec<u8>) {
    let rom = wozmon_rom::load();
    let mut machine = Apple1::new(&rom, NonZeroU64::new(100)).unwrap();
    machine.reset().unwrap();
    let output = run_until(&mut machine, WAIT_BUDGET, |_, out| {
        out.windows(2).any(|pair| pair == [0x5C, 0x0D])
    });
    (machine, output)
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_boots_and_shows_prompt() {
    let (_machine, output) = boot();
    // The Woz Monitor outputs backslash (prompt) with bit 7 set ($DC)
    // followed by CR ($8D); the Apple I display strips bit 7, so the
    // machine's own output bytes are $5C, $0D.
    assert!(
        output.windows(2).any(|pair| pair == [0x5C, 0x0D]),
        "expected the prompt bytes [5C, 0D], got: {:?}",
        String::from_utf8_lossy(&output)
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_memory_examine_dumps_rom() {
    let (mut machine, _output) = boot();

    // Examine ROM range $FF00-$FF0F: the monitor dumps eight bytes per
    // line, so both complete lines must arrive.
    machine.type_str("FF00.FF0F\r");
    let output = run_until(&mut machine, WAIT_BUDGET, |_, out| {
        has_full_dump_line(out, "FF00:", 8) && has_full_dump_line(out, "FF08:", 8)
    });

    let text = String::from_utf8_lossy(&output);
    // The Woz Monitor's own first bytes are D8 58 (CLD, CLI).
    assert!(
        text.contains("FF00: D8 58"),
        "expected the ROM's own opening bytes in the dump, got: {text:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_write_and_examine_ram() {
    let (mut machine, _output) = boot();

    // Write a known pattern to $0300 (outside the Woz Monitor input buffer
    // at $0200-$027F) and wait for the bytes to really be in RAM, not for
    // the command echo.
    machine.type_str("300: AB CD EF\r");
    run_until(&mut machine, WAIT_BUDGET, |machine, _| {
        machine.bus().ram_slice()[0x0300..0x0303] == [0xAB, 0xCD, 0xEF]
    });

    // Examine it back and wait for the complete three-byte dump line.
    machine.type_str("300.302\r");
    let output = run_until(&mut machine, WAIT_BUDGET, |_, out| {
        complete_lines(out)
            .iter()
            .any(|line| line.contains("0300: AB CD EF"))
    });

    let text = String::from_utf8_lossy(&output);
    assert!(
        text.contains("0300: AB CD EF"),
        "expected the deposited bytes to read back, got: {text:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_write_and_run_program() {
    let (mut machine, _output) = boot();

    // Program at $0300: write '*' to the display, then spin.
    //   A9 2A     LDA #'*'
    //   8D 12 D0  STA $D012
    //   4C 05 03  JMP $0305   (spin on the JMP itself)
    const PROGRAM: [u8; 8] = [0xA9, 0x2A, 0x8D, 0x12, 0xD0, 0x4C, 0x05, 0x03];
    machine.type_str("300: A9 2A 8D 12 D0 4C 05 03\r");
    run_until(&mut machine, WAIT_BUDGET, |machine, _| {
        machine.bus().ram_slice()[0x0300..0x0308] == PROGRAM
    });

    // Run it. The '*' must come from the program, not from a command echo,
    // so only bytes after the echoed command's CR count.
    machine.type_str("300R\r");
    let output = run_until(&mut machine, WAIT_BUDGET, |_, out| {
        match out.iter().position(|&byte| byte == b'\r') {
            Some(cr) => out[cr + 1..].contains(&b'*'),
            None => false,
        }
    });

    let cr = output
        .iter()
        .position(|&byte| byte == b'\r')
        .expect("the command echo's CR must be delivered");
    assert!(
        output[cr + 1..].contains(&b'*'),
        "expected the program's asterisk after the echoed command, got: {:?}",
        String::from_utf8_lossy(&output)
    );
}
