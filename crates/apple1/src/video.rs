//! Digital Apple I video: C3/2519, D2/2513, D1/74166 and C13 sync.
//!
//! This consumes the existing board counters, including H18 reloads. It
//! never reads the host text projection. See `docs/apple1/video.md` for
//! wiring, clock phases, font provenance and the electrical boundary.

mod font;

use crate::timing::{CRYSTAL_HZ, TimingEvent};

/// D13's nominal astable intervals: .693*(R10+R11)*C7 and .693*R11*C7,
/// with 10 kΩ, 10 kΩ, 25 µF. Startup phase and RC tolerances are conventions.
const BLINK_HIGH_TICKS: u64 = CRYSTAL_HZ * 693 / 2_000;
const BLINK_LOW_TICKS: u64 = CRYSTAL_HZ * 693 / 4_000;

/// Settled digital outputs for one master-clock interval.
///
/// Booleans use asserted semantics. Nominal output levels are a host
/// convention, not a simulation of Q5, the potentiometer, or a 75 Ω load.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct VideoSample {
    /// D1 QH, the serial white-pixel signal.
    pub luminance: bool,
    /// Composite sync after C13: horizontal sync OR D15's vertical gate.
    pub sync: bool,
    /// Horizontal sync asserted (C9's H4 OR H6 output is low).
    pub hsync: bool,
    /// D15 Q3 low, independent of the much longer memory VBL interval.
    pub vsync: bool,
    /// A rising dot clock occurred; seven such edges per character.
    pub dot_edge: bool,
}

impl VideoSample {
    /// Nominal composite levels: sync 0 mV, black 300 mV, white 1000 mV.
    /// Sync takes priority over the pixel signal.
    pub fn nominal_millivolts(self) -> u16 {
        if self.sync {
            0
        } else if self.luminance {
            1000
        } else {
            300
        }
    }
}

/// Fixed-size device state; the host owns all capture storage.
pub(crate) struct Video {
    /// Six packed 40-stage tracks. `head` is the oldest/output stage.
    line: [u8; 40],
    head: usize,
    /// D1 A..H, with H in bit 7. A/B/C and serial input are grounded.
    shift: u8,
    blink_ticks: u64,
}

impl Video {
    pub(crate) fn new() -> Self {
        Self {
            line: [0x20; 40],
            head: 0,
            shift: 0,
            blink_ticks: 0,
        }
    }

    /// Extend the existing atomic host CLEAR convention to the video state.
    /// Physical RESET does not call this; the blink oscillator keeps running.
    pub(crate) fn clear_screen(&mut self) {
        self.line = [0x20; 40];
        self.shift = 0;
    }

    pub(crate) fn tick(&mut self, ev: &TimingEvent, tracks: u8, cursor: bool) -> VideoSample {
        let h6 = ev.horizontal >= 120;
        let hsync = (ev.horizontal / 10) & 1 == 0 && !h6;
        let dot_edge = ev.phase & 1 != 0;

        // D11's Q3 rising edge is phase 13. Its stable count sequence at
        // phases 0,2,...,12 is A,B,C,D,E,F,0. D1 samples the old TC=1
        // at phase 11 (F->0); D10 permits loading only while H6 is high.
        if dot_edge {
            if ev.phase == 11 && h6 {
                // Q5->H ... Q1->D. Load before C3 changes on the derived
                // LINEΦ edge; combinational ROM lookup adds no register.
                self.shift =
                    font::GLYPHS[self.line[self.head] as usize][(ev.vertical & 7) as usize] << 3;
            } else {
                self.shift <<= 1;
            }
        }

        // LINEΦ = NAND(H6, D11.Q2), rising at F->0. RC is /LINE7:
        // load on row-address 7, recirculate otherwise. The input was
        // selected on MEMΦ (phase 3), not from the newly advanced head.
        if ev.phase == 11 && h6 {
            if ev.vertical & 7 == 7 {
                let blink = self.blink_ticks < BLINK_HIGH_TICKS;
                // VBL enables C4/C14's clear, even between MEMΦ pulses.
                self.line[self.head] = if ev.vbi {
                    0x20
                } else {
                    character_address(tracks, cursor && blink)
                };
            }
            self.head = (self.head + 1) % self.line.len();
        }

        self.blink_ticks += 1;
        if self.blink_ticks == BLINK_HIGH_TICKS + BLINK_LOW_TICKS {
            self.blink_ticks = 0;
        }

        // C15 feeds C13.D1. C13 samples the old counter decode on the
        // master clock, hence counter changes at phase 13 appear next tick.
        VideoSample {
            luminance: self.shift & 0x80 != 0,
            sync: hsync || ev.vsync,
            hsync,
            vsync: ev.vsync,
            dot_edge,
        }
    }
}

/// C10 NORs raw B6 with C12's cursor/blink term; other tracks pass through.
fn character_address(tracks: u8, cursor_lit: bool) -> u8 {
    (tracks & 0x1f)
        | if tracks & 0x20 == 0 && !cursor_lit {
            0x20
        } else {
            0
        }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::timing::{HORIZ_MASTER_TICKS, Timing};

    #[test]
    fn raw_tracks_decode_blank_at_and_cursor_without_host_ascii() {
        assert_eq!(character_address(0, false), 0x20);
        assert_eq!(character_address(0x20, false), 0);
        assert_eq!(character_address(0, true), 0);
        assert_eq!(character_address(0x21, false), 1);
        assert_eq!(character_address(0x1b, false), 0x3b);
    }

    #[test]
    fn font_has_blank_row_zero_and_asymmetric_q5_to_q1_rows() {
        assert!(font::GLYPHS.iter().all(|glyph| glyph[0] == 0));
        assert!(font::GLYPHS.iter().flatten().all(|row| row & !0x1f == 0));
        assert_eq!(font::GLYPHS[0x20], [0; 8]);
        // Independent readable rows: F's top bar, left stem and middle bar.
        assert_eq!(font::GLYPHS[6], [0, 31, 16, 16, 30, 16, 16, 16]);
        // CM2141 chart: slash occupies rows 2..6, with blank rows 1 and 7.
        assert_eq!(font::GLYPHS[0x2f], [0, 0, 1, 2, 4, 8, 16, 0]);
    }

    #[test]
    fn line_load_replays_old_output_then_exposes_all_forty_in_order() {
        let mut timing = Timing::new();
        let mut video = Video::new();
        for _ in 0..8 * HORIZ_MASTER_TICKS {
            let ev = timing.tick(false);
            let column = ev.horizontal.saturating_sub(120);
            // Raw B6=1 gives character addresses 0..31; B6=0 gives 32..39.
            let tracks = (column & 0x1f) | if column < 32 { 0x20 } else { 0 };
            video.tick(&ev, tracks, false);
        }
        assert_eq!(video.line, std::array::from_fn(|i| i as u8));
        assert_eq!(video.head, 0);
        for scan in 0..7 {
            let mut seen = Vec::new();
            for _ in 0..HORIZ_MASTER_TICKS {
                let ev = timing.tick(false);
                if ev.phase == 11 && ev.horizontal >= 120 {
                    seen.push(video.line[video.head]);
                }
                video.tick(&ev, 0, false);
            }
            assert_eq!(seen, (0..40).collect::<Vec<_>>(), "scan {scan}");
        }
    }

    #[test]
    fn serial_f_has_five_pixels_two_spaces_and_survives_line_clock() {
        let mut timing = Timing::new();
        let mut video = Video::new();
        video.line.fill(6);
        let mut bits = Vec::new();
        // Counter scan 2: F's left stem. Collect from the first D1 load.
        let start = 2 * HORIZ_MASTER_TICKS + 25 * 14 + 11;
        for tick in 0..start + 14 {
            let ev = timing.tick(false);
            let sample = video.tick(&ev, 0, false);
            if tick >= start && sample.dot_edge {
                bits.push(sample.luminance);
            }
        }
        assert_eq!(bits, [true, false, false, false, false, false, false]);
    }

    #[test]
    fn vbl_is_not_vsync_and_sync_has_explicit_nominal_levels() {
        let mut timing = Timing::new();
        let mut video = Video::new();
        let mut vbl_ticks = 0;
        let mut vsync_ticks = 0;
        let mut hsync_ticks = 0;
        loop {
            let ev = timing.tick(false);
            let sample = video.tick(&ev, 0, false);
            vbl_ticks += u64::from(ev.vbi);
            vsync_ticks += u64::from(sample.vsync);
            hsync_ticks += u64::from(sample.hsync);
            assert_eq!(sample.sync, sample.hsync || sample.vsync);
            if sample.sync {
                assert_eq!(sample.nominal_millivolts(), 0);
            }
            if ev.frame_completed {
                break;
            }
        }
        assert_eq!(vbl_ticks, 70 * HORIZ_MASTER_TICKS);
        assert_eq!(vsync_ticks, 8 * HORIZ_MASTER_TICKS);
        assert_eq!(hsync_ticks, 262 * 10 * 14);
    }
}
