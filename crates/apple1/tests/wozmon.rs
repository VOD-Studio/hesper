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

#[path = "support/wozmon_rom.rs"]
mod wozmon_rom;

use hesper_apple1::Apple1;

/// Run the machine until output stops (machine is idle, waiting for input).
/// Returns accumulated display output.
fn run_until_idle(machine: &mut Apple1) -> Vec<u8> {
    let mut output = Vec::new();
    for _ in 0..100 {
        match machine.run_cycles(10000) {
            Ok(out) => {
                if out.is_empty() {
                    break;
                }
                output.extend(&out);
            }
            Err(e) => {
                panic!("machine error: {e}");
            }
        }
    }
    output
}

/// Boot the machine and return accumulated boot output.
fn boot() -> (Apple1, Vec<u8>) {
    let rom = wozmon_rom::load();
    let mut machine = Apple1::new(&rom, Some(100)).unwrap();
    machine.reset().unwrap();
    let output = run_until_idle(&mut machine);
    (machine, output)
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_boots_and_shows_prompt() {
    let (_machine, output) = boot();
    let s = String::from_utf8_lossy(&output);
    // The Woz Monitor outputs backslash (prompt) with bit 7 set ($DC);
    // the Apple I display strips bit 7, so the output byte is $5C.
    assert!(
        output.contains(&0x5C),
        "expected prompt byte 0x5C (backslash) in boot output, got: {s:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_memory_examine_dumps_rom() {
    let (mut machine, _output) = boot();

    // Examine ROM range $FF00-$FF0F
    machine.type_str("FF00.FF0F\r");
    let output = run_until_idle(&mut machine);

    // Output should contain hex digits from the ROM
    let s = String::from_utf8_lossy(&output);
    // The ROM starts with D8 58 A0 7F — these bytes should appear
    // in the hex dump somewhere
    assert!(s.contains("D8"), "expected D8 in hex dump, got: {s}");
    assert!(s.contains("58"), "expected 58 in hex dump, got: {s}");
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_write_and_run_program() {
    let (mut machine, _output) = boot();

    // Write a program at $0300 that outputs a character and spins:
    // LDA #'*'  ($2A)   A9 2A
    // STA $D012          8D 12 D0
    // JMP $0305          4C 05 03  (spin on the JMP instruction)
    machine.type_str("300: A9 2A 8D 12 D0 4C 05 03\r");
    run_until_idle(&mut machine);

    // Run the program
    machine.type_str("300R\r");
    let output = run_until_idle(&mut machine);

    // The program writes '*' to the display
    assert!(
        output.contains(&b'*'),
        "expected asterisk in output from test program, got: {output:?}"
    );
}

#[test]
#[ignore = "requires HESPER_APPLE1_ROM; see crates/apple1/tests/data/README.md"]
fn wozmon_write_and_examine_ram() {
    let (mut machine, _output) = boot();

    // Write known pattern to $0300 (outside the Woz Monitor input buffer
    // at $0200-$027F).
    machine.type_str("300: AB CD EF\r");
    run_until_idle(&mut machine);

    // Examine it back
    machine.type_str("300.302\r");
    let output = run_until_idle(&mut machine);

    let s = String::from_utf8_lossy(&output);
    assert!(s.contains("AB"), "expected AB in examine output, got: {s}");
    assert!(s.contains("CD"), "expected CD in examine output, got: {s}");
    assert!(s.contains("EF"), "expected EF in examine output, got: {s}");
}
