//! Cross-device timing boundaries: how the board clock, the CPU's Φ2 gate,
//! the PIA's enable, the video terminal's carousel, and the B3 one-shot
//! interleave.
//!
//! Everything here is observed through the machine's public surface —
//! `Tick`, its counters, the screen, and the drained output — so the
//! assertions are about behaviour, not about internal fields.

use hesper_apple1::Apple1;
use hesper_cpu6502::{Bus, CpuError};

/// Master ticks in one complete video frame: 262 scan lines of 65
/// character clocks, each 14 crystal periods.
const FRAME_TICKS: u64 = 262 * 65 * 14;

/// The B3 one-shot's CB1 pulse: the schematic's nominal 3.5 µs quantised up
/// to whole crystal periods.
const B3_TICKS: u64 = 51;

/// The four refresh clocks in a horizontal period (D6/D7 counts 129, 139,
/// 149, 159).
const REFRESH_SLOTS: [u64; 4] = [34, 44, 54, 64];

/// Keyboard echo: poll IRQA1, read the key, offer it, then wait for DA.
/// The final wait is what makes the terminal's own pace visible — nothing
/// is written until the previous character has been taken.
const ECHO_PROGRAM: &[u8] = &[
    0xA9, 0x7F, 0x8D, 0x12, 0xD0, // $0000 DDRB = $7F
    0xA9, 0x27, 0x8D, 0x13, 0xD0, // $0005 CRB = $27
    0xA9, 0x07, 0x8D, 0x11, 0xD0, // $000A CRA = $07
    0x2C, 0x11, 0xD0, // $000F POLL: BIT $D011
    0x10, 0xFB, // $0012 BPL POLL
    0xAD, 0x10, 0xD0, // $0014 LDA $D010
    0x29, 0x7F, // $0017 AND #$7F
    0x8D, 0x12, 0xD0, // $0019 STA $D012
    0x2C, 0x12, 0xD0, // $001C WAIT: BIT $D012
    0x30, 0xFB, // $001F BMI WAIT
    0x4C, 0x0F, 0x00, // $0021 JMP POLL
];

fn echo_machine() -> Apple1 {
    let mut rom = [0u8; 256];
    rom[0xFC] = 0x00;
    rom[0xFD] = 0x00;
    let mut machine = Apple1::new(&rom).unwrap();
    machine.bus_mut().load_ram(0x0000, ECHO_PROGRAM).unwrap();
    machine
}

/// Run master ticks until the display cursor reaches `cursor`, or panic.
fn run_until_cursor(machine: &mut Apple1, cursor: (usize, usize), budget: u64, what: &str) {
    for _ in 0..budget {
        if machine.display().cursor() == cursor {
            return;
        }
        machine.tick().unwrap();
    }
    if machine.display().cursor() != cursor {
        panic!("{what} did not happen within {budget} master ticks");
    }
}

#[test]
fn refresh_stops_phi2_but_not_the_video_board() {
    // Across a horizontal period, exactly the four refresh clocks carry no
    // CPU bus cycle — and the video terminal keeps scanning through them
    // all the same.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    // Align to a horizontal-period boundary: the board counters are a pure
    // function of the master tick count.
    let phase = machine.master_ticks() % (65 * 14);
    if phase != 0 {
        machine.run_ticks(65 * 14 - phase).unwrap();
    }
    let base = machine.master_ticks();

    let mut bus_access_at = Vec::new();
    let mut refresh_windows = Vec::new();
    let mut was_refresh = false;
    for _ in 0..(65 * 14) {
        let tick = machine.tick().unwrap();
        let slot = (machine.master_ticks() - base - 1) / 14;
        if tick.cpu.is_some() {
            bus_access_at.push(slot);
        }
        if tick.refresh && !was_refresh {
            refresh_windows.push(slot);
        }
        was_refresh = tick.refresh;
    }

    assert_eq!(refresh_windows, REFRESH_SLOTS);
    for slot in &bus_access_at {
        assert!(
            !REFRESH_SLOTS.contains(slot),
            "a CPU bus access landed inside refresh clock {slot}"
        );
    }
    assert_eq!(bus_access_at.len(), 61, "65 slots minus 4 refresh clocks");
}

#[test]
fn the_terminal_takes_a_character_during_a_refresh_clock() {
    // The video board is not gated by RF. Fill columns 0..34 so the next
    // cursor slot is column 34, which is a refresh clock, and check that
    // the character is still taken there and that the B3 one-shot that
    // answers it runs its full nominal length in board time.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_str("ABCDEFGHIJKLMNOPQRSTUVWXYZABCDEFGH");
    run_until_cursor(
        &mut machine,
        (0, 34),
        40 * FRAME_TICKS,
        "the terminal filling its line",
    );

    machine.type_char(b'Z');
    let mut taken = None;
    let mut done = None;
    for _ in 0..(3 * FRAME_TICKS) {
        let tick = machine.tick().unwrap();
        let master = machine.master_ticks();
        if taken.is_none() && machine.display().screen()[0][34] == b'Z' {
            taken = Some((master, tick.refresh));
        }
        if taken.is_some() && done.is_none() && !machine.io_pending() {
            done = Some(master);
        }
    }

    let (taken_at, during_refresh) = taken.expect("the terminal must take the character");
    assert!(
        during_refresh,
        "the cursor's slot is refresh clock 34, so the take happens with Φ2 suppressed"
    );
    assert_eq!(
        done.expect("the handshake must finish") - taken_at,
        B3_TICKS,
        "the B3 pulse is board time; refresh inside it must not stretch it"
    );
}

#[test]
fn a_character_is_taken_once_per_carousel_lap() {
    // Two keys typed together: the second cannot be taken until the
    // carousel comes round to the next cursor slot, a lap later.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'O');
    machine.type_char(b'K');

    let mut first = None;
    let mut second = None;
    for _ in 0..(4 * FRAME_TICKS) {
        machine.tick().unwrap();
        let master = machine.master_ticks();
        if first.is_none() && machine.display().screen()[0][0] != b' ' {
            first = Some(master);
        } else if first.is_some() && second.is_none() && machine.display().screen()[0][1] != b' ' {
            second = Some(master);
        }
    }

    let first = first.expect("the first key must be taken");
    let second = second.expect("the second key must be taken");
    // The second character's cursor slot is one column further along the
    // same row, so it waits one carousel lap (a frame) plus one character
    // clock — exactly.
    let expected = FRAME_TICKS + 14;
    assert_eq!(
        second - first,
        expected,
        "the second character waits one carousel lap plus one column"
    );
    assert_eq!(machine.drain_output(), b"OK");
}

#[test]
fn a_write_lands_where_the_cursor_is_when_the_slot_arrives() {
    // The acceptance instant is the video timeline, not the instant of the
    // CPU's write: offering a character before its slot comes round is
    // normal, and the character is stored when the slot arrives.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'A');

    let mut offered_at = None;
    for _ in 0..2_000 {
        machine.tick().unwrap();
        if machine.bus_mut().read(0xD012) & 0x80 != 0 {
            offered_at = Some(machine.master_ticks());
            break;
        }
    }
    let offered_at = offered_at.expect("the CPU must offer the character");
    assert_eq!(
        machine.display().screen()[0][0],
        b' ',
        "offering is not accepting"
    );

    let mut accepted_at = None;
    for _ in 0..(2 * FRAME_TICKS) {
        machine.tick().unwrap();
        if machine.display().screen()[0][0] == b'A' {
            accepted_at = Some(machine.master_ticks());
            break;
        }
    }
    let accepted_at = accepted_at.expect("the character must be accepted");
    assert!(
        accepted_at - offered_at > FRAME_TICKS / 2,
        "the terminal waits for its own slot, not for the write"
    );
    assert_eq!(machine.display().cursor(), (0, 1));
}

#[test]
fn clear_screen_between_offer_and_accept_homes_the_cursor() {
    // CLEAR SCREEN is a video-board input: it blanks the carousel and
    // homes the cursor without touching the handshake. The character still
    // being offered is taken at the new, homed cursor.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'M');
    for _ in 0..2_000 {
        machine.tick().unwrap();
        if machine.bus_mut().read(0xD012) & 0x80 != 0 {
            break;
        }
    }

    machine.clear_screen();
    assert_eq!(machine.display().screen(), &[[b' '; 40]; 24]);
    assert_eq!(machine.display().cursor(), (0, 0));

    machine.run_ticks(2 * FRAME_TICKS).unwrap();
    assert_eq!(
        machine.display().screen()[0][0],
        b'M',
        "the request survived CLEAR SCREEN and was taken at the homed cursor"
    );
    assert_eq!(machine.display().cursor(), (0, 1));
}

#[test]
fn an_error_keeps_output_that_finished_before_it() {
    // A CPU error surfaces from `tick`, and a character the terminal had
    // already finished is still drainable afterwards: the host must not
    // lose what the terminal completed.
    let mut machine = echo_machine();
    machine.reset().unwrap();
    machine.type_char(b'A');
    for _ in 0..(4 * FRAME_TICKS) {
        if machine.display().screen()[0][0] == b'A' {
            break;
        }
        machine.tick().unwrap();
    }
    assert_eq!(machine.display().screen()[0][0], b'A');

    // Plant an undefined opcode where the program will fetch next.
    machine.bus_mut().load_ram(0x000F, &[0x02]).unwrap();
    let mut error = None;
    for _ in 0..(4 * FRAME_TICKS) {
        if let Err(e) = machine.tick() {
            error = Some(e);
            break;
        }
    }
    let error = error.expect("the undefined opcode must fail the run");
    assert!(matches!(error, CpuError::UnsupportedOpcode { .. }));
    assert_eq!(
        machine.drain_output(),
        b"A",
        "the completed character is still there to take"
    );
}

#[test]
fn reset_and_clear_screen_during_a_carriage_return_fill() {
    // A carriage-return fill is a video sequence that walks the cursor to
    // the end of the line, up to forty character clocks. RESET must leave
    // the screen and both clocks intact, and CLEAR SCREEN must cancel the
    // fill rather than letting it land on the cleared screen.
    for clear in [false, true] {
        let mut machine = echo_machine();
        machine.reset().unwrap();
        machine.type_str("AB\r");

        // Wait for the carriage return to be taken and its fill to be
        // under way: the screen holds "AB" and the cursor is walking the
        // rest of the line.
        let mut caught = false;
        for _ in 0..(8 * FRAME_TICKS) {
            machine.tick().unwrap();
            if machine.display().screen()[0][..2] == *b"AB" && machine.display().cursor().1 > 3 {
                caught = true;
                break;
            }
        }
        assert!(caught, "the fill must be observable (clear = {clear})");

        if clear {
            machine.clear_screen();
            assert_eq!(machine.display().screen(), &[[b' '; 40]; 24]);
            assert_eq!(machine.display().cursor(), (0, 0));
            machine.run_ticks(2 * FRAME_TICKS).unwrap();
            assert_eq!(
                machine.display().cursor(),
                (0, 0),
                "a cancelled fill must not walk the cursor any further"
            );
        } else {
            machine.set_reset_line(true);
            machine.run_ticks(200).unwrap();
            machine.set_reset_line(false);
            machine.run_ticks(2 * FRAME_TICKS).unwrap();
            assert_eq!(
                machine.display().screen()[0][..2],
                *b"AB",
                "RESET preserves what was already on the screen"
            );
        }

        // Either way the machine keeps running and still takes a new
        // character at the cursor's next slot.
        machine.type_char(b'Z');
        machine.run_ticks(4 * FRAME_TICKS).unwrap();
        assert!(
            machine
                .display()
                .screen()
                .iter()
                .flatten()
                .any(|&cell| cell == b'Z'),
            "the terminal must still accept characters (clear = {clear})"
        );
    }
}
