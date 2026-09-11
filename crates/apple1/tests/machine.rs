//! Integration tests for the Apple I machine over the board clock model:
//! the real PIA handshake to the video terminal, keyboard input, physical
//! RESET, CLEAR SCREEN, and batch-boundary independence.

use hesper_apple1::Apple1;
use hesper_cpu6502::{Bus, Cycle, DebugState};

/// Master ticks in one complete video frame: 262 scan lines of 65
/// character clocks, each 14 crystal periods.
const FRAME_TICKS: u64 = 262 * 65 * 14;

/// Build a minimal 256‑byte ROM whose reset vector points to `addr`.
fn rom_with_reset_vector(addr: u16) -> [u8; 256] {
    let mut rom = [0u8; 256];
    rom[0xFC] = addr as u8;
    rom[0xFD] = (addr >> 8) as u8;
    rom
}

/// Keyboard→display echo loop over the real handshake.
///
/// `CRB = $27` selects the output register, the write-strobe-with-CB1
/// handshake, and a rising-edge CB1 — the configuration the Woz Monitor
/// uses. The terminal only takes a character while the cursor's slot is
/// exposed, so the loop waits on PB7 (`DA`, which the board wires back to
/// that pin) before the next character.
///
/// ```text
/// $0000  A9 7F     LDA #$7F
/// $0002  8D 12 D0  STA $D012    ; DDRB = $7F (PB7 input, PB6-0 output)
/// $0005  A9 27     LDA #$27
/// $0007  8D 13 D0  STA $D013    ; CRB = $27 (ORB, write strobe, rising CB1)
/// $000A  A9 07     LDA #$07
/// $000C  8D 11 D0  STA $D011    ; CRA = $07 (ORA, rising CA1, IRQ on)
/// $000F  2C 11 D0  POLL BIT $D011
/// $0012  10 FB     BPL POLL
/// $0014  AD 10 D0  LDA $D010    ; read the key (clears IRQA1)
/// $0017  29 7F     AND #$7F
/// $0019  8D 12 D0  STA $D012    ; offer it to the terminal
/// $001C  2C 12 D0  WAIT BIT $D012
/// $001F  30 FB     BMI WAIT     ; wait for DA (PB7) to clear
/// $0021  4C 0F 00  JMP POLL
/// ```
const ECHO_PROGRAM: &[u8] = &[
    0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000
    0xA9, 0x27, 0x8D, 0x13, 0xD0, // $0005
    0xA9, 0x07, 0x8D, 0x11, 0xD0, // $000A
    0x2C, 0x11, 0xD0, // $000F
    0x10, 0xFB, // $0012
    0xAD, 0x10, 0xD0, // $0014
    0x29, 0x7F, // $0017
    0x8D, 0x12, 0xD0, // $0019
    0x2C, 0x12, 0xD0, // $001C
    0x30, 0xFB, // $001F
    0x4C, 0x0F, 0x00, // $0021
];

/// Machine loaded with [`ECHO_PROGRAM`], not yet reset.
fn echo_machine() -> Apple1 {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine.bus_mut().load_ram(0x0000, ECHO_PROGRAM).unwrap();
    machine
}

/// Machine whose program offers `ch` once and then waits, using the
/// handshake properly.
///
/// ```text
/// $0000  A9 7F     LDA #$7F
/// $0002  8D 12 D0  STA $D012    ; DDRB = $7F
/// $0005  A9 27     LDA #$27
/// $0007  8D 13 D0  STA $D013    ; CRB = $27
/// $000A  A9 <ch>   LDA #ch
/// $000C  8D 12 D0  STA $D012    ; offer it to the terminal
/// $000F  2C 12 D0  WAIT BIT $D012
/// $0012  30 FB     BMI WAIT     ; wait for DA (PB7) to clear
/// $0014  4C 0F 00  JMP WAIT
/// ```
fn one_char_program(ch: u8) -> Vec<u8> {
    vec![
        0xA9, 0x7F, 0x8D, 0x12, 0xD0, // DDRB = $7F
        0xA9, 0x27, 0x8D, 0x13, 0xD0, // CRB = $27
        0xA9, ch, 0x8D, 0x12, 0xD0, // offer the character
        0x2C, 0x12, 0xD0, // WAIT: BIT $D012
        0x30, 0xFB, // BMI WAIT
        0x4C, 0x0F, 0x00, // JMP WAIT
    ]
}

/// Run master ticks until `predicate` accepts the machine, or panic.
fn run_until(machine: &mut Apple1, budget: u64, what: &str, predicate: impl Fn(&Apple1) -> bool) {
    for _ in 0..budget {
        machine.tick().unwrap();
        if predicate(machine) {
            return;
        }
    }
    panic!("{what} did not happen within {budget} master ticks");
}

#[test]
fn a_character_reaches_the_screen_through_the_handshake() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &one_char_program(b'a'))
        .unwrap();
    machine.reset().unwrap();

    // The terminal only takes the character when the cursor slot passes,
    // so the wait is up to one whole frame.
    machine.run_ticks(2 * FRAME_TICKS).unwrap();

    assert_eq!(machine.display().screen()[0][0], b'A');
    assert_eq!(machine.display().cursor(), (0, 1));
    assert!(
        !machine.io_pending(),
        "the handshake completed and nothing else is queued"
    );
}

#[test]
fn the_terminal_holds_the_cpu_until_it_takes_the_character() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &one_char_program(b'Z'))
        .unwrap();
    machine.reset().unwrap();

    // The program reaches its write within a few hundred ticks, long
    // before the terminal's next turn with the cursor slot. The character
    // must not be on the screen yet, and PB7 must read busy: the board
    // wires DA back to that pin, which is how software waits.
    machine.run_ticks(2_000).unwrap();
    assert_eq!(
        machine.display().screen()[0][0],
        b' ',
        "the terminal cannot have taken it this early"
    );
    assert!(machine.io_pending(), "the handshake is still in flight");
    assert_eq!(
        machine.bus_mut().read(0xD012) & 0x80,
        0x80,
        "PB7 carries DA, so software sees the terminal as busy"
    );

    // Once the terminal has taken it, the screen shows the character and
    // DA is released.
    machine.run_ticks(2 * FRAME_TICKS).unwrap();
    assert_eq!(machine.display().screen()[0][0], b'Z');
    assert_eq!(
        machine.bus_mut().read(0xD012) & 0x80,
        0,
        "DA released once the character was taken"
    );
}

#[test]
fn keyboard_echo_flow() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_str("HI");

    // Each character costs about a frame, so two characters need several.
    let output = machine.run_ticks(12 * FRAME_TICKS).unwrap();

    assert_eq!(output, b"HI", "both keys must be read and echoed in order");
    assert_eq!(&machine.display().screen()[0][..2], b"HI");
    assert_eq!(machine.display().cursor(), (0, 2));
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn keyboard_no_key_leaves_irqa1_clear() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.run_ticks(FRAME_TICKS).unwrap();
    assert!(
        !machine.bus().pia().irqa1_active(),
        "no key means no strobe flag"
    );
    assert_eq!(machine.display().cursor(), (0, 0));
}

#[test]
fn keyboard_repeated_read_without_new_key_returns_same_data() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'K');
    machine.run_ticks(FRAME_TICKS).unwrap();

    // The port pins keep the latched byte, so reading again without a new
    // keypress returns the same data.
    let first = machine.bus_mut().read(0xD010);
    let second = machine.bus_mut().read(0xD010);
    assert_eq!(first & 0x7F, b'K');
    assert_eq!(second & 0x7F, b'K');
}

#[test]
fn keyboard_continuous_input_delivers_keys_in_fifo_order() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_str("ABC");
    let output = machine.run_ticks(20 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"ABC");
}

#[test]
fn keyboard_normalizes_high_bit_letters_without_changing_symbols() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(0xE1); // 'a' with bit 7 set
    machine.type_char(b'9');
    let output = machine.run_ticks(12 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"A9");
}

#[test]
fn keyboard_control_characters_pass_through_unfiltered() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(0x03); // ctrl-C
    machine.run_ticks(4 * FRAME_TICKS).unwrap();
    // The keyboard hands the byte over unchanged; the terminal is what
    // decides a control code is not a printable cell.
    assert_eq!(machine.display().screen()[0][0], b' ');
    assert_eq!(machine.display().cursor(), (0, 0));
}

#[test]
fn a_key_is_only_consumed_by_a_real_data_register_read() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'Q');
    run_until(
        &mut machine,
        4 * FRAME_TICKS,
        "the key to be offered",
        |m| m.bus().pia().irqa1_active(),
    );
    assert!(machine.keyboard().has_pending());

    // Reading the direction register and the control register through the
    // same address block must not retire the key.
    let _ = machine.bus_mut().read(0xD012);
    let _ = machine.bus_mut().read(0xD011);
    machine.run_ticks(200).unwrap();
    assert!(
        machine.keyboard().has_pending(),
        "only a peripheral data read settles the key"
    );

    // The echo program's own data read does retire it.
    let output = machine.run_ticks(8 * FRAME_TICKS).unwrap();
    assert!(!machine.keyboard().has_pending());
    assert_eq!(output, b"Q");
}

#[test]
fn queued_keys_survive_repeated_resets_and_are_echoed_once_each() {
    // Keys already typed live in the keyboard encoder, which is not wired
    // to the reset line, so they survive RESET — as long as the CPU has
    // not already taken them. One key per reset, echoed exactly once.
    let mut machine = echo_machine();
    machine.reset().unwrap();

    let mut output = Vec::new();
    for key in b"OK" {
        machine.type_char(*key);
        output.extend(machine.run_ticks(4 * FRAME_TICKS).unwrap());
        machine.set_reset_line(true);
        output.extend(machine.run_ticks(FRAME_TICKS / 8).unwrap());
        machine.set_reset_line(false);
    }
    output.extend(machine.run_ticks(4 * FRAME_TICKS).unwrap());

    assert_eq!(
        output, b"OK",
        "each queued key must be echoed exactly once across resets"
    );
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn reset_clears_a_character_offered_but_not_yet_taken() {
    // The character lives in the PIA's output register while the terminal
    // waits for its cursor slot. RESET clears that register: the hardware
    // holds no copy of the character anywhere else, so it is gone rather
    // than delivered late.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'Z');
    machine.run_ticks(1_000).unwrap();
    assert!(
        machine.io_pending(),
        "the key has been read and offered, and the terminal has not taken it"
    );

    machine.set_reset_line(true);
    machine.run_ticks(FRAME_TICKS / 8).unwrap();
    machine.set_reset_line(false);
    let output = machine.run_ticks(2 * FRAME_TICKS).unwrap();

    assert_eq!(
        output, b"",
        "nothing is left in the output register to take"
    );
    assert_eq!(machine.display().screen()[0][0], b' ');
}

#[test]
fn physical_reset_never_replays_a_key_the_cpu_already_read() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'A');
    let output = machine.run_ticks(4 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"A");

    // RESET while nothing is pending must not resurrect the key.
    machine.set_reset_line(true);
    machine.run_ticks(FRAME_TICKS / 4).unwrap();
    machine.set_reset_line(false);
    let output = machine.run_ticks(8 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"");
    assert!(!machine.keyboard().has_pending());
}

#[test]
fn held_reset_line_keeps_the_pia_reset_and_the_queue_intact_across_batches() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'X');

    machine.set_reset_line(true);
    assert!(machine.reset_line_asserted());
    // Splitting the hold across batches must not change anything.
    for _ in 0..3 {
        machine.run_ticks(FRAME_TICKS / 8).unwrap();
        assert!(!machine.bus().pia().irqa1_active());
        assert!(
            machine.keyboard().has_pending(),
            "queued keys survive RESET"
        );
    }
    machine.set_reset_line(false);
    let output = machine.run_ticks(4 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"X");
}

#[test]
fn system_reset_preserves_the_screen() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'S');
    machine.run_ticks(4 * FRAME_TICKS).unwrap();
    assert_eq!(machine.display().screen()[0][0], b'S');

    machine.set_reset_line(true);
    machine.run_ticks(FRAME_TICKS / 4).unwrap();
    machine.set_reset_line(false);
    machine.run_ticks(4 * FRAME_TICKS).unwrap();

    assert_eq!(
        machine.display().screen()[0][0],
        b'S',
        "RESET is not the CLEAR SCREEN button"
    );
}

#[test]
fn clear_screen_touches_nothing_but_the_screen() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'B');
    machine.run_ticks(4 * FRAME_TICKS).unwrap();
    assert_eq!(machine.display().screen()[0][0], b'B');

    machine.type_char(b'C');
    let before_master = machine.master_ticks();
    let before_registers = machine.cpu().registers();
    let before_debug = machine.cpu().debug_state();
    let before_ram = machine.bus().ram_slice().to_vec();

    machine.clear_screen();

    assert_eq!(machine.display().screen(), &[[b' '; 40]; 24]);
    assert_eq!(machine.display().cursor(), (0, 0));
    assert_eq!(
        machine.master_ticks(),
        before_master,
        "CLEAR SCREEN runs no machine time"
    );
    assert_eq!(machine.cpu().registers(), before_registers);
    assert_eq!(machine.cpu().debug_state(), before_debug);
    assert_eq!(machine.bus().ram_slice(), before_ram.as_slice());
    assert!(machine.keyboard().has_pending());

    // The running echo loop keeps working and 'C' lands on the cleared
    // screen.
    let output = machine.run_ticks(8 * FRAME_TICKS).unwrap();
    assert_eq!(output, b"C");
    assert_eq!(machine.display().screen()[0][0], b'C');
}

#[test]
fn batch_boundaries_do_not_change_the_machine() {
    // The same program and keyboard input, driven once in one batch and
    // once in many small batches: RAM, registers, output, screen and both
    // counters must match exactly.
    fn run(batch: u64) -> (Vec<u8>, Vec<u8>, u64, u64, DebugState) {
        let mut machine = echo_machine();
        machine.reset().unwrap();
        machine.type_str("HI");
        let mut output = Vec::new();
        let mut remaining = 6 * FRAME_TICKS;
        while remaining > 0 {
            let step = batch.min(remaining);
            output.extend(machine.run_ticks(step).unwrap());
            remaining -= step;
        }
        (
            output,
            machine.bus().ram_slice().to_vec(),
            machine.master_ticks(),
            machine.cpu_cycles(),
            machine.cpu().debug_state(),
        )
    }

    let continuous = run(6 * FRAME_TICKS);
    for batch in [1u64, 7, 31, 64, 1000] {
        assert_eq!(run(batch), continuous, "batch size {batch} changed the run");
    }
    assert_eq!(continuous.0, b"HI");
}

/// A host input event applied at an exact absolute master tick.
#[derive(Debug, Clone, Copy)]
enum Event {
    Type(u8),
    ResetLine(bool),
}

/// Scripted timeline: power-on RESET driven through the physical line, a
/// key read and echoed, a second RESET mid-run, and a key typed at the
/// very end that never gets read.
const TIMELINE: &[(u64, Event)] = &[
    (0, Event::ResetLine(true)),
    (100, Event::ResetLine(false)),
    (2 * FRAME_TICKS, Event::Type(b'A')),
    (6 * FRAME_TICKS, Event::ResetLine(true)),
    (6 * FRAME_TICKS + 200, Event::ResetLine(false)),
    (TIMELINE_TOTAL - 5, Event::Type(b'B')),
];

const TIMELINE_TOTAL: u64 = 12 * FRAME_TICKS;

/// Everything a host can observe after running the timeline.
#[derive(Debug, PartialEq, Eq)]
struct Outcome {
    cpu_cycles: u64,
    master_ticks: u64,
    debug: DebugState,
    ram: Vec<u8>,
    output: Vec<u8>,
    screen: Vec<Vec<u8>>,
    cursor: (usize, usize),
    pending: bool,
}

/// Run [`TIMELINE`] for [`TIMELINE_TOTAL`] master ticks, drained at every
/// `batch`-tick boundary. Events always land on their exact tick: a batch
/// is cut short at an event rather than the event being deferred.
fn run_timeline(batch: u64) -> Outcome {
    let mut machine = echo_machine();
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
            machine.tick().unwrap();
        }
        executed = stop;
        output.extend(machine.drain_output());
    }

    Outcome {
        cpu_cycles: machine.cpu_cycles(),
        master_ticks: machine.master_ticks(),
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
        pending: machine.keyboard().has_pending(),
    }
}

#[test]
fn scripted_reset_and_input_timeline_is_batch_boundary_independent() {
    let single = run_timeline(1);

    // The scenario really exercises what it claims: a character echoed and
    // a key never read.
    assert_eq!(single.master_ticks, TIMELINE_TOTAL);
    assert_eq!(single.output, b"A", "'A' was read and echoed");
    assert!(single.pending, "the last key must still be unread");
    assert!(
        single.cpu_cycles < single.master_ticks,
        "refresh gates Φ2, so a frame of board time is fewer CPU cycles"
    );

    for batch in [7u64, 31, 64, TIMELINE_TOTAL] {
        assert_eq!(
            run_timeline(batch),
            single,
            "batch size {batch} changed the observable outcome"
        );
    }
}

/// Run until the machine sits on a horizontal-period boundary, so a test
/// that counts slots starts from a known phase.
fn align_to_period(machine: &mut Apple1) {
    let period = 65 * 14;
    let phase = machine.master_ticks() % period;
    if phase != 0 {
        machine.run_ticks(period - phase).unwrap();
    }
}

#[test]
fn a_master_tick_reports_a_cpu_cycle_only_on_a_real_bus_phase() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &[0x4C, 0x00, 0x00])
        .unwrap(); // JMP $0000
    machine.reset().unwrap();
    align_to_period(&mut machine);

    let mut cpu_ticks = 0u64;
    let mut refresh_ticks = 0u64;
    let mut refresh_windows = 0u64;
    let mut was_refresh = false;
    let mut frames = 0u64;
    // One horizontal period: 65 character clocks of 14 master ticks.
    for _ in 0..(65 * 14) {
        let tick = machine.tick().unwrap();
        cpu_ticks += u64::from(tick.cpu.is_some());
        refresh_ticks += u64::from(tick.refresh);
        if tick.refresh && !was_refresh {
            refresh_windows += 1;
        }
        was_refresh = tick.refresh;
        frames += u64::from(tick.frame_completed);
    }
    assert_eq!(cpu_ticks, 61, "65 slots minus 4 refresh clocks");
    assert_eq!(refresh_windows, 4, "one refresh clock per H6 && H10 slot");
    assert_eq!(
        refresh_ticks,
        4 * 14,
        "the refresh clock spans the whole character clock"
    );
    assert_eq!(frames, 0, "a horizontal period is not a frame");
}

#[test]
fn refresh_keeps_the_board_running_without_extra_cpu_accesses() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &[0x4C, 0x00, 0x00])
        .unwrap(); // JMP $0000
    machine.reset().unwrap();
    align_to_period(&mut machine);

    let period = 65 * 14;
    let mut cpu_cycles = 0u64;
    let mut refresh_windows = 0u64;
    let mut was_refresh = false;
    for _ in 0..(3 * period) {
        let tick = machine.tick().unwrap();
        if tick.cpu.is_some() {
            assert!(
                !tick.refresh,
                "a CPU bus cycle completed while Φ2 was suppressed"
            );
            cpu_cycles += 1;
        }
        if tick.refresh && !was_refresh {
            refresh_windows += 1;
        }
        was_refresh = tick.refresh;
    }

    assert_eq!(
        cpu_cycles,
        3 * 61,
        "each horizontal period carries exactly its 61 un-gated Φ2 clocks"
    );
    assert_eq!(refresh_windows, 3 * 4);

    // Board time still advanced across the whole window.
    assert_eq!(machine.master_ticks() % period, 0);
}

#[test]
fn video_frames_advance_on_the_vertical_terminal_count() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &[0x4C, 0x00, 0x00])
        .unwrap();
    machine.reset().unwrap();
    let phase = machine.master_ticks() % FRAME_TICKS;
    if phase != 0 {
        machine.run_ticks(FRAME_TICKS - phase).unwrap();
    }
    let base = machine.video_frames();

    machine.run_ticks(FRAME_TICKS - 1).unwrap();
    assert_eq!(machine.video_frames(), base, "one tick short of a frame");
    machine.run_ticks(1).unwrap();
    assert_eq!(machine.video_frames(), base + 1);
    machine.run_ticks(2 * FRAME_TICKS).unwrap();
    assert_eq!(machine.video_frames(), base + 3);
}

#[test]
fn io_pending_tracks_input_and_handshakes_but_not_the_free_running_scan() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.run_ticks(2 * FRAME_TICKS).unwrap();
    assert!(
        !machine.io_pending(),
        "an idle machine with nothing queued has no I/O in flight"
    );

    machine.type_char(b'A');
    assert!(machine.io_pending(), "queued input is pending");
    machine.run_ticks(4 * FRAME_TICKS).unwrap();
    assert!(
        !machine.io_pending(),
        "the key was read, echoed and taken; only the scan keeps running"
    );
}

#[test]
fn run_ticks_returns_output_even_when_the_budget_is_short() {
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'P');

    // Drive the machine in small slices: whatever the terminal has
    // finished by the end of the batch must come back from that batch.
    let mut collected = Vec::new();
    for _ in 0..(8 * FRAME_TICKS / 1000) {
        collected.extend(machine.run_ticks(1000).unwrap());
    }
    assert_eq!(collected, b"P");
}

#[test]
fn an_unsupported_opcode_reports_the_cycle_that_read_it() {
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine.bus_mut().load_ram(0x0000, &[0x02]).unwrap(); // undefined opcode
    // The reset sequence itself fetches the undefined opcode, so the
    // error surfaces there.
    let error = machine
        .reset()
        .expect_err("the undefined opcode must fail the run");
    assert!(matches!(
        error,
        hesper_cpu6502::CpuError::UnsupportedOpcode { .. }
    ));
    assert!(
        machine.cpu_cycles() > 0,
        "the fetch that read the opcode really happened and is counted"
    );
}

#[test]
fn cycle_records_come_only_from_real_cpu_phases() {
    // A `Cycle` is returned exactly when the CPU completed a bus phase;
    // Φ1 ticks and refresh-suppressed Φ2 ticks return `None`.
    let rom = rom_with_reset_vector(0x0000);
    let mut machine = Apple1::new(&rom).unwrap();
    machine
        .bus_mut()
        .load_ram(0x0000, &[0x4C, 0x00, 0x00])
        .unwrap();
    machine.reset().unwrap();

    let before = machine.cpu_cycles();
    let mut cycles: Vec<Cycle> = Vec::new();
    for _ in 0..(FRAME_TICKS / 4) {
        if let Some(cycle) = machine.tick().unwrap().cpu {
            cycles.push(cycle);
        }
    }
    assert!(!cycles.is_empty());
    assert_eq!(cycles.len() as u64, machine.cpu_cycles() - before);
    // Every reported cycle is a real bus phase of the running program.
    for cycle in &cycles {
        assert!(cycle.bus.address < 0x1000);
    }
}
