//! Integration tests for the Apple I machine: display output collection,
//! keyboard handshake, physical RESET, CLEAR SCREEN, and batch-boundary
//! independence.

use std::num::NonZeroU64;

use hesper_apple1::{Apple1, Keyboard, Pia6821};
use hesper_cpu6502::{Bus, Cycle, DebugState};

fn cycles(n: u64) -> NonZeroU64 {
    NonZeroU64::new(n).unwrap()
}

/// Build a minimal 256‑byte ROM whose reset vector points to `addr`.
fn rom_with_reset_vector(addr: u16) -> [u8; 256] {
    let mut rom = [0u8; 256];
    rom[0xFC] = addr as u8;
    rom[0xFD] = (addr >> 8) as u8;
    rom
}

/// Keyboard→display echo loop.
///
/// Both control registers select the *peripheral data* registers (CR bit 2
/// set), which is what makes `LDA $D010` read the keyboard and
/// `STA $D012` drive the display's seven data lines; with bit 2 clear the
/// same addresses are the data-direction registers.
///
/// ```text
/// $0000  A9 7F     LDA #$7F
/// $0002  8D 12 D0  STA $D012    ; DDRB = $7F (PB7 input, PB6-0 output)
/// $0005  A9 07     LDA #$07
/// $0007  8D 13 D0  STA $D013    ; CRB = $07 (OR, rising CB1, IRQ on)
/// $000A  A9 07     LDA #$07
/// $000C  8D 11 D0  STA $D011    ; CRA = $07 (OR, rising CA1, IRQ on)
/// $000F  2C 11 D0  POLL BIT $D011
/// $0012  10 FB     BPL POLL
/// $0014  AD 10 D0  LDA $D010    ; read the key (clears IRQA1)
/// $0017  29 7F     AND #$7F
/// $0019  8D 12 D0  STA $D012    ; echo it
/// $001C  4C 0F 00  JMP POLL
/// ```
const ECHO_PROGRAM: &[u8] = &[
    0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000
    0xA9, 0x07, 0x8D, 0x13, 0xD0, // $0005
    0xA9, 0x07, 0x8D, 0x11, 0xD0, // $000A
    0x2C, 0x11, 0xD0, // $000F
    0x10, 0xFB, // $0012
    0xAD, 0x10, 0xD0, // $0014
    0x29, 0x7F, // $0017
    0x8D, 0x12, 0xD0, // $0019
    0x4C, 0x0F, 0x00, // $001C
];

/// Machine loaded with [`ECHO_PROGRAM`], not yet reset.
fn echo_machine(cycles_per_char: u64) -> Apple1 {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(cycles(cycles_per_char))).unwrap();
    machine.bus_mut().load_ram(0x0000, ECHO_PROGRAM).unwrap();
    machine
}

/// Run one cycle at a time until the display finishes a character, and
/// return the characters that completed on that cycle.
fn run_until_output(machine: &mut Apple1, budget: u64) -> Vec<u8> {
    for _ in 0..budget {
        machine.cycle().unwrap();
        let output = machine.drain_output();
        if !output.is_empty() {
            return output;
        }
    }
    panic!("no display output within {budget} cycles");
}

#[test]
fn display_output_collects_character() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(cycles(50))).unwrap();

    // Program at $0000:
    //   LDA #$FF        A9 FF
    //   STA $D012       8D 12 D0   ; DDRB = $FF (all outputs)
    //   LDA #$04        A9 04
    //   STA $D013       8D 13 D0   ; CRB = $04 (select OR, IRQ off)
    //   LDA #'a'        A9 61
    //   STA $D012       8D 12 D0   ; write 'a' to display
    //   JMP $000F       4C 0F 00   ; spinloop
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, 0xA9, 0x04, 0x8D, 0x13, 0xD0, 0xA9, 0x61, 0x8D, 0x12, 0xD0,
        0x4C, 0x0F, 0x00,
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset().unwrap();

    // Reset takes 7 cycles, the program takes ~21 cycles to reach the
    // spinloop after STA $D012. Give plenty of budget for display timing.
    let output = machine.run_cycles(200).unwrap();

    assert_eq!(output, b"A");
    assert_eq!(machine.display().screen()[0][0], b'A');
}

#[test]
fn display_timing_respects_cycles_per_char() {
    let rom = rom_with_reset_vector(0x0000);
    // Short timing: 30 cycles per character.
    let mut machine = Apple1::new(&rom, Some(cycles(30))).unwrap();

    // Same program as above.
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, 0xA9, 0x04, 0x8D, 0x13, 0xD0, 0xA9, 0x41, 0x8D, 0x12, 0xD0,
        0x4C, 0x0F, 0x00,
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();

    machine.reset().unwrap();

    // Run just past the STA $D012 but not enough for display to finish.
    // Reset: 7 cycles. Program:
    //   LDA #$FF:   2 cycles (total 9)
    //   STA $D012:  4 cycles (total 13)
    //   LDA #$04:   2 cycles (total 15)
    //   STA $D013:  4 cycles (total 19)
    //   LDA #'A':   2 cycles (total 21)
    //   STA $D012:  4 cycles (total 25)
    // Display timer started at cycle 25 (after on_write).
    // It needs 30 cycles, so output at cycle 55.
    // Run exactly enough to reach cycle 40 — output should still be empty.
    let output = machine.run_cycles(40).unwrap();
    assert!(
        output.is_empty(),
        "output should be empty before timer expires"
    );

    // Run more — character should now be collected.
    let output2 = machine.run_cycles(50).unwrap();
    assert_eq!(output2, b"A", "character should appear after timer expires");
}

#[test]
fn keyboard_echo_flow() {
    // Short display time so the echo is collected quickly.
    let mut machine = echo_machine(20);
    machine.reset().unwrap();

    // Complete reset + PIA setup (18 cycles) = exactly 25 cycles.
    // After 25 cycles the CPU just finished STA $D011 and is about to
    // fetch BIT $D011 at $000F.
    let _ = machine.run_cycles(25).unwrap();

    // Now queue a key while the CPU is at the BIT instruction.
    machine.type_char(b'H');

    // Run cycles.  Keyboard tick 1 asserts CA1 → IRQA1 set.
    // BIT $D011 detects it, BPL falls through, LDA $D010 clears
    // IRQA1 and returns $C8.  AND #$7F → $48.  STA $D012 starts
    // the display timer (20 cycles).  Total ≈ 50 cycles needed.
    let output = machine.run_cycles(50).unwrap();

    assert_eq!(
        output, b"H",
        "echoed character 'H' should appear in display output, got {output:?}"
    );
}

#[test]
fn queued_keys_survive_repeated_resets_and_are_echoed_once_each() {
    // Real Apple I hardware ties the PIA's RESET pin to the same system
    // reset line as the 6502 (clearing its registers), but the external
    // keyboard encoder is not wired to that line at all — pressing RESET
    // does not erase keys the user already typed ahead, and must not
    // duplicate them either.
    let mut machine = echo_machine(1);
    machine.reset().unwrap();

    machine.type_str("XY");
    assert!(machine.keyboard().has_pending());

    // Two RESETs land on top of the untouched queue; the program restarts
    // from the ROM vector and reconfigures the PIA each time.
    machine.reset().unwrap();
    machine.reset().unwrap();

    let output = machine.run_cycles(3_000).unwrap();
    assert_eq!(
        output, b"XY",
        "both queued keys must be echoed exactly once, got {output:?}"
    );
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn physical_reset_never_replays_a_key_the_cpu_already_read() {
    let mut machine = echo_machine(1);
    machine.reset().unwrap();
    machine.type_str("XY");

    // Stop the instant 'X' has been echoed: the CPU really read it.
    assert_eq!(run_until_output(&mut machine, 3_000), b"X");

    machine.reset().unwrap();
    let output = machine.run_cycles(3_000).unwrap();
    assert_eq!(
        output, b"Y",
        "a key already read must not be replayed by RESET, got {output:?}"
    );
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn held_reset_line_keeps_the_pia_reset_and_the_queue_intact_across_batches() {
    let mut machine = echo_machine(1);
    machine.reset().unwrap();
    let _ = machine.run_cycles(40).unwrap();
    assert_eq!(
        machine.bus_mut().read(0xD011),
        0x07,
        "the program configured CRA before RESET"
    );

    machine.type_char(b'Z');
    machine.set_reset_line(true);
    assert!(machine.reset_line_asserted());

    // The hold spans several host batches: the PIA stays reset (CPU writes
    // are dropped) and the unread key stays queued.
    for _ in 0..3 {
        let _ = machine.run_cycles(7).unwrap();
        assert_eq!(
            machine.bus_mut().read(0xD011),
            0,
            "CRA must stay cleared while RESET is held"
        );
        assert_eq!(
            machine.bus_mut().read(0xD012),
            0,
            "DDRB must stay cleared while RESET is held"
        );
        assert!(
            machine.keyboard().has_pending(),
            "an unread key must survive the hold"
        );
    }

    machine.set_reset_line(false);
    assert!(!machine.reset_line_asserted());
    let output = machine.run_cycles(3_000).unwrap();
    assert_eq!(
        output, b"Z",
        "the still-unread key is strobed again after release, got {output:?}"
    );
}

#[test]
fn the_display_only_sees_characters_the_pia_actually_drives() {
    let rom = rom_with_reset_vector(0x0000);

    // PB6 left as an input: the PIA is not driving all seven display data
    // lines, so writing the output register drives no character. The DDRB
    // write itself must not produce one either.
    let partial: &[u8] = &[
        0xA9, 0x3F, 0x8D, 0x12, 0xD0, // $0000 DDRB = $3F
        0xA9, 0x04, 0x8D, 0x13, 0xD0, // $0005 CRB = $04 (select OR)
        0xA9, 0x41, 0x8D, 0x12, 0xD0, // $000A ORB = 'A'
        0x4C, 0x0F, 0x00, // $000F spin
    ];
    let mut machine = Apple1::new(&rom, Some(cycles(5))).unwrap();
    machine.bus_mut().load_ram(0x0000, partial).unwrap();
    machine.reset().unwrap();
    assert!(
        machine.run_cycles(200).unwrap().is_empty(),
        "a partly-input DDRB drives no display character"
    );
    assert_eq!(machine.display().screen()[0][0], b' ');

    // All seven data lines as outputs: the same write is delivered once.
    let full: &[u8] = &[
        0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000 DDRB = $7F
        0xA9, 0x04, 0x8D, 0x13, 0xD0, // $0005 CRB = $04 (select OR)
        0xA9, 0x41, 0x8D, 0x12, 0xD0, // $000A ORB = 'A'
        0x4C, 0x0F, 0x00, // $000F spin
    ];
    let mut machine = Apple1::new(&rom, Some(cycles(5))).unwrap();
    machine.bus_mut().load_ram(0x0000, full).unwrap();
    machine.reset().unwrap();
    assert_eq!(machine.run_cycles(200).unwrap(), b"A");
}

#[test]
fn keyboard_no_key_leaves_irqa1_clear() {
    let mut machine = echo_machine(50);
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x07);

    let _ = machine.run_cycles(20).unwrap();
    assert_eq!(
        machine.bus_mut().read(0xD011) & 0x80,
        0,
        "IRQA1 must stay clear when no key is queued"
    );
}

#[test]
fn keyboard_repeated_read_without_new_key_returns_same_data() {
    let mut machine = echo_machine(50);
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x07);
    machine.type_char(b'q');
    let _ = machine.run_cycles(5).unwrap();

    let first = machine.bus_mut().read(0xD010);
    let second = machine.bus_mut().read(0xD010);
    assert_eq!(
        first, second,
        "repeated reads without a new key must return the same data"
    );
    assert_eq!(first, 0xD1);
}

#[test]
fn keyboard_continuous_input_delivers_keys_in_fifo_order() {
    let mut machine = echo_machine(50);
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x07);
    machine.type_str("aB");

    // The second key must wait for the first to be read before it is
    // presented — no skipping or reordering.
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010), 0xC1);
    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010), 0xC2);
}

#[test]
fn keyboard_normalizes_high_bit_letters_without_changing_symbols() {
    let mut keyboard = Keyboard::new();
    let mut pia = Pia6821::new();
    pia.write(0xD011, 0x07);
    for byte in [
        0xE1, 0xFA, b'0', b'9', b'@', b'[', 0x60, b'{', b'~', b'_', b'\r', 0x1B,
    ] {
        keyboard.type_char(byte);
    }
    for expected in [
        0xC1, 0xDA, 0xB0, 0xB9, 0xC0, 0xDB, 0xE0, 0xFB, 0xFE, 0xDF, 0x8D, 0x9B,
    ] {
        keyboard.tick(&mut pia);
        assert_eq!(pia.read(0xD010), expected);
        keyboard.tick(&mut pia);
    }
    assert!(!keyboard.has_pending());
}

#[test]
fn keyboard_control_characters_pass_through_unfiltered() {
    // CR ($0D) is an ordinary data byte to the PIA; the keyboard model
    // does not interpret or filter control characters.
    let mut machine = echo_machine(50);
    machine.reset().unwrap();
    machine.bus_mut().write(0xD011, 0x07);
    machine.type_char(0x0D);

    let _ = machine.run_cycles(5).unwrap();
    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, 0x0D);
}

#[test]
fn a_key_is_only_consumed_by_a_real_data_register_read() {
    let mut machine = echo_machine(50);
    machine.reset().unwrap();
    machine.type_char(b'K');
    let _ = machine.run_cycles(5).unwrap();

    // DDR reads, control reads and output-register writes are not the
    // CPU taking the key.
    machine.bus_mut().write(0xD011, 0x00); // select DDRA
    let _ = machine.bus_mut().read(0xD010); // DDRA read
    let _ = machine.bus_mut().read(0xD011); // control read
    machine.bus_mut().write(0xD011, 0x04); // select ORA
    machine.bus_mut().write(0xD010, 0x00); // ORA write
    let _ = machine.run_cycles(5).unwrap();
    assert!(
        machine.keyboard().has_pending(),
        "only a data-register read may consume a key"
    );

    assert_eq!(machine.bus_mut().read(0xD010) & 0x7F, b'K');
    let _ = machine.run_cycles(5).unwrap();
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn display_write_while_busy_via_cpu_drops_pending_character() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(cycles(50))).unwrap();

    // DDRB=$FF, CRB=$04 (OR select), write 'A' ($41), write 'B' ($42)
    // before 'A' finishes its 50-cycle timer, then spin.
    let program: &[u8] = &[
        0xA9, 0xFF, 0x8D, 0x12, 0xD0, // $0000 DDRB
        0xA9, 0x04, 0x8D, 0x13, 0xD0, // $0005 CRB
        0xA9, 0x41, 0x8D, 0x12, 0xD0, // $000A write 'A' (starts timer)
        0xA9, 0x42, 0x8D, 0x12, 0xD0, // $000F write 'B' (overwrites 'A')
        0x4C, 0x14, 0x00, // $0014 JMP $0014 (self-loop)
    ];
    machine.bus_mut().load_ram(0x0000, program).unwrap();
    machine.reset().unwrap();

    let output = machine.run_cycles(100).unwrap();
    assert_eq!(
        output, b"B",
        "'A' must be dropped by the overwrite; only 'B' is ever delivered"
    );
}

/// Two characters separated by a delay loop, so the first completes before
/// the second starts shifting out.
///
/// ```text
/// $0000  A9 7F     LDA #$7F
/// $0002  8D 12 D0  STA $D012    ; DDRB = $7F
/// $0005  A9 04     LDA #$04
/// $0007  8D 13 D0  STA $D013    ; CRB = $04 (select OR)
/// $000A  A9 41     LDA #'A'
/// $000C  8D 12 D0  STA $D012    ; 'A' starts shifting out
/// $000F  A2 08     LDX #$08
/// $0011  CA        DEX
/// $0012  D0 FD     BNE $0011    ; ~39 cycles, outlasting 'A'
/// $0014  A9 42     LDA #'B'
/// $0016  8D 12 D0  STA $D012    ; 'B' starts shifting out
/// $0019  4C 19 00  JMP $0019
/// ```
const TWO_CHAR_PROGRAM: &[u8] = &[
    0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000
    0xA9, 0x04, 0x8D, 0x13, 0xD0, // $0005
    0xA9, 0x41, 0x8D, 0x12, 0xD0, // $000A
    0xA2, 0x08, // $000F
    0xCA, // $0011
    0xD0, 0xFD, // $0012
    0xA9, 0x42, 0x8D, 0x12, 0xD0, // $0014
    0x4C, 0x19, 0x00, // $0019
];

#[test]
fn system_reset_preserves_the_screen_and_lets_an_in_flight_character_finish() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom, Some(cycles(20))).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, TWO_CHAR_PROGRAM)
        .unwrap();
    machine.reset().unwrap();

    // 'A' completed at cycle ~38; 'B' started at cycle ~65.
    let early = machine.run_cycles(70).unwrap();
    assert_eq!(early, b"A", "'A' completed; 'B' is still shifting out");
    assert_eq!(machine.display().screen()[0][0], b'A');
    assert_eq!(machine.display().cursor(), (0, 1));

    // RESET is not a video reset: the screen keeps 'A' and the in-flight
    // 'B' still lands.
    machine.reset().unwrap();
    let late = machine.run_cycles(5).unwrap();
    assert_eq!(late, b"B", "the in-flight character must still complete");
    assert_eq!(
        &machine.display().screen()[0][..2],
        b"AB".as_slice(),
        "RESET must not clear the screen"
    );
    assert_eq!(machine.display().cursor(), (0, 2));
}

#[test]
fn clear_screen_touches_nothing_but_the_screen() {
    let mut machine = echo_machine(1);
    machine.reset().unwrap();
    machine.type_str("AB");
    assert_eq!(run_until_output(&mut machine, 3_000), b"A");
    assert_eq!(machine.display().screen()[0][0], b'A');

    let before_total = machine.total_cycles();
    let before_registers = machine.cpu().registers();
    let before_debug = machine.cpu().debug_state();
    let before_ram = machine.bus().ram_slice().to_vec();
    let before_pending = machine.keyboard().has_pending();

    machine.clear_screen();

    assert_eq!(machine.display().screen(), &[[b' '; 40]; 24]);
    assert_eq!(machine.display().cursor(), (0, 0));
    assert_eq!(
        machine.total_cycles(),
        before_total,
        "CLEAR SCREEN runs no CPU cycle"
    );
    assert_eq!(machine.cpu().registers(), before_registers);
    assert_eq!(machine.cpu().debug_state(), before_debug);
    assert_eq!(machine.bus().ram_slice(), before_ram.as_slice());
    assert_eq!(machine.keyboard().has_pending(), before_pending);

    // The PIA configuration and the queued key are untouched: the running
    // echo loop keeps working and 'B' lands on the cleared screen.
    let output = machine.run_cycles(3_000).unwrap();
    assert_eq!(output, b"B");
    assert_eq!(machine.display().screen()[0][0], b'B');
    assert_eq!(machine.display().cursor(), (0, 1));
}

#[test]
fn continuous_advance_matches_split_batch_advance() {
    // Same program and keyboard input, driven once with a single large
    // budget and once as 300 one-cycle batches: final RAM, registers,
    // display output and total cycle count must match exactly. Splitting a
    // batch must not change device or CPU state — the sequencer persists
    // across `run_cycles` calls.
    fn run(batch_sizes: &[u64]) -> (Vec<u8>, Vec<u8>, u64) {
        let mut machine = echo_machine(20);
        machine.reset().unwrap();
        machine.type_str("HI");

        let mut output = Vec::new();
        for &batch in batch_sizes {
            output.extend(machine.run_cycles(batch).unwrap());
        }
        (
            output,
            machine.bus().ram_slice().to_vec(),
            machine.total_cycles(),
        )
    }

    let continuous = run(&[300]);
    let split: Vec<u64> = std::iter::repeat_n(1u64, 300).collect();
    let split = run(&split);

    assert_eq!(continuous, split);
    // Sanity: something must actually happen (not two silently-idle runs).
    // 'H' and 'I' are both queued before the CPU starts polling, and the
    // display's 20-cycle busy timer outlasts the echo loop's turnaround
    // between them, so 'H' is overwritten before it is ever sent (see
    // `display_write_while_busy_via_cpu_drops_pending_character`) — only
    // 'I' is delivered. That drop is deterministic and identical across
    // batchings, which the assertion above already covers; this only
    // guards against a change that silently produces no output at all.
    assert_eq!(continuous.0, b"I");
}

/// A host input event applied at an exact absolute cycle index.
#[derive(Debug, Clone, Copy)]
enum Event {
    Type(u8),
    ResetLine(bool),
}

/// Scripted timeline: power-on RESET driven through the physical line, a key
/// read and echoed, a second RESET mid-run, a character still shifting out
/// at the end, and a key that is never read.
const TIMELINE: &[(u64, Event)] = &[
    (0, Event::ResetLine(true)),
    (8, Event::ResetLine(false)),
    (30, Event::Type(b'A')),
    (120, Event::ResetLine(true)),
    (129, Event::ResetLine(false)),
    (370, Event::Type(b'B')),
    (397, Event::Type(b'C')),
];

const TIMELINE_TOTAL: u64 = 400;

/// Everything a host can observe after running the timeline.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    cycles: Vec<Cycle>,
    debug: DebugState,
    ram: Vec<u8>,
    output: Vec<u8>,
    screen: Vec<Vec<u8>>,
    cursor: (usize, usize),
    total: u64,
    port_b: u8,
    pending: bool,
}

/// Run [`TIMELINE`] for [`TIMELINE_TOTAL`] cycles, drained at every
/// `batch`-cycle boundary. Events always land on their exact cycle: a batch
/// is cut short at an event rather than the event being deferred to the
/// batch boundary.
fn run_timeline(batch: u64) -> Outcome {
    let mut machine = echo_machine(20);
    let mut cycles = Vec::new();
    let mut output = Vec::new();
    let mut executed = 0u64;

    while executed < TIMELINE_TOTAL {
        for &(at, event) in TIMELINE {
            if at == executed {
                match event {
                    Event::Type(byte) => machine.type_char(byte),
                    Event::ResetLine(asserted) => machine.set_reset_line(asserted),
                }
            }
        }
        let next_event = TIMELINE
            .iter()
            .map(|&(at, _)| at)
            .filter(|&at| at > executed)
            .min()
            .unwrap_or(TIMELINE_TOTAL);
        let next_batch = executed - executed % batch + batch;
        let stop = TIMELINE_TOTAL.min(next_event).min(next_batch);
        for _ in executed..stop {
            cycles.push(machine.cycle().unwrap());
        }
        executed = stop;
        output.extend(machine.drain_output());
    }

    // Reading Port B's data register reports PB7 (display busy) alongside
    // the last character driven.
    let port_b = machine.bus_mut().read(0xD012);
    Outcome {
        cycles,
        debug: machine.cpu().debug_state(),
        ram: machine.bus().ram_slice().to_vec(),
        output,
        screen: machine
            .display()
            .screen()
            .iter()
            .map(|row| row.to_vec())
            .collect(),
        cursor: machine.display().cursor(),
        total: machine.total_cycles(),
        port_b,
        pending: machine.keyboard().has_pending(),
    }
}

#[test]
fn scripted_reset_and_input_timeline_is_batch_boundary_independent() {
    let single = run_timeline(1);

    // The scenario really exercises what it claims: a character echoed, a
    // character still shifting out, and a key never read.
    assert_eq!(single.total, TIMELINE_TOTAL);
    assert_eq!(single.cycles.len() as u64, TIMELINE_TOTAL);
    assert_eq!(single.output, b"A", "'A' was read and echoed");
    assert_eq!(
        single.port_b & 0x80,
        0x80,
        "a character must still be shifting out at the end"
    );
    assert!(single.pending, "the last key must still be unread");

    for batch in [7u64, 31, 64, TIMELINE_TOTAL] {
        assert_eq!(
            run_timeline(batch),
            single,
            "batch size {batch} changed the observable outcome"
        );
    }
}
