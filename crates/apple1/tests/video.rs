//! Production CPU -> PIA -> carousel -> line buffer -> serial video tests.
//! Expected pixels are explicit wiring/font observations, never screen().

use hesper_apple1::{Apple1, VideoSample};

const LINE: u64 = 910;
const FRAME: u64 = 262 * LINE;

fn writer(text: &[u8]) -> Apple1 {
    assert!(text.len() < 256);
    let mut rom = [0; 256];
    rom[0xfc] = 0;
    rom[0xfd] = 2;
    let mut machine = Apple1::new(&rom).unwrap();
    // Configure the real handshake, then emit a NUL-terminated RAM string.
    let program = [
        0xa9, 0x7f, 0x8d, 0x12, 0xd0, // DDRB
        0xa9, 0x27, 0x8d, 0x13, 0xd0, // CRB
        0xa2, 0x00, // LDX #0
        0xbd, 0x00, 0x03, // NEXT: LDA $0300,X ($020c)
        0xf0, 0x0b, // BEQ DONE ($021c)
        0x8d, 0x12, 0xd0, // STA $D012
        0x2c, 0x12, 0xd0, 0x30, 0xfb, // WAIT: BIT $D012 / BMI WAIT
        0xe8, 0xd0, 0xf0, // INX / BNE NEXT
        0x4c, 0x1c, 0x02, // DONE: JMP DONE
    ];
    machine.bus_mut().load_ram(0x0200, &program).unwrap();
    machine.bus_mut().load_ram(0x0300, text).unwrap();
    machine.reset().unwrap();
    machine
}

fn finish_writing(machine: &mut Apple1, expected: &[u8]) {
    let mut output = Vec::new();
    let mut idle_frames = 0;
    for _ in 0..FRAME * (expected.len() as u64 + 5) {
        let tick = machine.tick().unwrap();
        output.extend(machine.drain_output());
        if tick.frame_completed {
            if output == expected && !machine.io_pending() {
                idle_frames += 1;
                if idle_frames == 2 {
                    return;
                }
            } else {
                idle_frames = 0;
            }
        }
    }
    panic!("writer did not settle: {output:?}");
}

fn frame(machine: &mut Apple1) -> Vec<VideoSample> {
    let samples: Vec<_> = (0..FRAME).map(|_| machine.tick().unwrap().video).collect();
    assert_eq!(machine.master_ticks() % FRAME, 0);
    samples
}

fn glyph(samples: &[VideoSample], row: usize, column: usize) -> [[bool; 7]; 8] {
    // One row of line-memory latency; D1 loads at H120+column, phase 11.
    std::array::from_fn(|scan| {
        std::array::from_fn(|dot| {
            let tick = ((row + 1) * 8 + scan) * LINE as usize + (25 + column) * 14 + 11 + dot * 2;
            samples[tick].luminance
        })
    })
}

#[test]
fn cpu_write_reaches_video_on_replay_not_on_host_screen_update() {
    let mut machine = writer(b"F");
    let mut first_pixel = None;
    let mut accepted_at = None;
    while machine.master_ticks() < FRAME {
        let at = machine.master_ticks();
        let sample = machine.tick().unwrap().video;
        if !machine.drain_output().is_empty() {
            accepted_at = Some(at);
        }
        if sample.luminance && first_pixel.is_none() {
            first_pixel = Some(at);
        }
    }
    assert_eq!(accepted_at, Some(7 * LINE + 25 * 14 + 3));
    assert_eq!(first_pixel, Some(9 * LINE + 25 * 14 + 11));
    let samples = frame(&mut machine);
    let f = glyph(&samples, 0, 0);
    assert_eq!(f[0], [false; 7]);
    assert_eq!(f[1], [true, true, true, true, true, false, false]);
    assert_eq!(f[2], [true, false, false, false, false, false, false]);
    assert_eq!(f[4], [true, true, true, true, false, false, false]);
    assert_eq!(f[7], f[2]);
    // A blinked @ cursor is real sampled pixels at the next cell.
    assert_eq!(machine.display().screen()[0][1], b' ');
    assert_eq!(
        glyph(&samples, 0, 1)[1],
        [false, true, true, true, false, false, false]
    );
}

#[test]
fn hardware_aliases_have_identical_video_despite_different_host_text() {
    for (left, right) in [
        (b'@', b'`'),
        (b'[', b'{'),
        (b'\\', b'|'),
        (b']', b'}'),
        (b'^', b'~'),
        (b'_', 0x7f),
    ] {
        let mut a = writer(&[left]);
        let mut b = writer(&[right]);
        finish_writing(&mut a, &[left]);
        finish_writing(&mut b, &[right]);
        assert_ne!(a.display().screen()[0][0], b.display().screen()[0][0]);
        assert_eq!(frame(&mut a), frame(&mut b), "alias {left:02x}/{right:02x}");
    }
}

#[test]
fn clear_and_written_spaces_are_dark_while_at_is_visible() {
    let mut machine = writer(b" @ ");
    finish_writing(&mut machine, b" @ ");
    let samples = frame(&mut machine);
    assert_eq!(glyph(&samples, 0, 0), [[false; 7]; 8]);
    assert!(glyph(&samples, 0, 1).iter().flatten().any(|pixel| *pixel));
    assert_eq!(glyph(&samples, 0, 2), [[false; 7]; 8]);
    machine.clear_screen();
    let samples = frame(&mut machine);
    assert_eq!(glyph(&samples, 0, 1), [[false; 7]; 8]);
    assert_eq!(glyph(&samples, 0, 2), [[false; 7]; 8]);
}

#[test]
fn reset_preserves_pixels_and_sync_and_blink_is_driven_by_board_time() {
    let mut machine = writer(b"F");
    finish_writing(&mut machine, b"F");
    machine.set_reset_line(true);
    let lit = frame(&mut machine);
    assert!(glyph(&lit, 0, 1).iter().flatten().any(|p| *p));
    // Nominal D13 high period ~.3465 s, low ~.17325 s. Sample well inside low.
    while machine.master_ticks() < FRAME * 25 {
        machine.tick().unwrap();
    }
    let dark = frame(&mut machine);
    assert_eq!(glyph(&dark, 0, 1), [[false; 7]; 8]);
    assert_eq!(glyph(&lit, 0, 0), glyph(&dark, 0, 0));
    assert_eq!(
        lit.iter()
            .map(|s| (s.hsync, s.vsync, s.sync))
            .collect::<Vec<_>>(),
        dark.iter()
            .map(|s| (s.hsync, s.vsync, s.sync))
            .collect::<Vec<_>>()
    );
}

#[test]
fn scrolling_keeps_horizontal_sync_and_pixels_follow_the_rescanned_ring() {
    let text: Vec<_> = (0..25).flat_map(|row| [b'A' + row, b'\r']).collect();
    let mut machine = writer(&text);
    // RESET can finish inside an existing pulse; do not invent its edge.
    let mut hsync_previous = machine.tick().unwrap().video.hsync;
    let mut hsync_at = None;
    let mut frame_at = 0;
    let mut changed_frames = 0;
    let mut output = Vec::new();
    let mut settled = 0;
    for _ in 0..FRAME * 60 {
        let at = machine.master_ticks();
        let tick = machine.tick().unwrap();
        if tick.video.hsync && !hsync_previous {
            if let Some(previous) = hsync_at {
                assert_eq!(at - previous, LINE);
            }
            hsync_at = Some(at);
        }
        hsync_previous = tick.video.hsync;
        assert_eq!(tick.video.sync, tick.video.hsync || tick.video.vsync);
        output.extend(machine.drain_output());
        if tick.frame_completed {
            let duration = machine.master_ticks() - frame_at;
            if duration != FRAME {
                changed_frames += 1;
            }
            frame_at = machine.master_ticks();
            if output == text && !machine.io_pending() {
                settled += 1;
                if settled == 2 {
                    break;
                }
            }
        }
    }
    assert_eq!(output, text);
    assert_eq!(settled, 2);
    assert!(changed_frames > 0, "H18 must change the capture duration");
    // After two scrolls, original row 2 ('C') occupies the top text row.
    let samples: Vec<_> = (0..FRAME).map(|_| machine.tick().unwrap().video).collect();
    assert_eq!(
        glyph(&samples, 0, 0)[1],
        [false, true, true, true, false, false, false]
    );
    assert_eq!(
        glyph(&samples, 0, 0)[2],
        [true, false, false, false, true, false, false]
    );
    assert_eq!(
        glyph(&samples, 0, 0)[3],
        [true, false, false, false, false, false, false]
    );
    // Last displayed row is blank with the new video cursor at column zero.
    assert_eq!(glyph(&samples, 23, 1), [[false; 7]; 8]);
}
