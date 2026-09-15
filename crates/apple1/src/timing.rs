//! Apple I board-level clock model.
//!
//! Derives the machine's character clock, horizontal/video timing, and
//! refresh-cycle Φ2 gating from the original 14.31818 MHz crystal — a
//! digital edge model, not an analog electrical simulation.
//!
//! ## Crystal and division
//!
//! The Apple I board uses a 14.31818 MHz crystal (four times the NTSC
//! colour-burst frequency). D11 divides by 14 to produce the ~1.023 MHz
//! character-rate clock Φ0 that feeds the CPU and the video terminal.
//! One *master tick* is one crystal period; 14 master ticks = one
//! character clock.
//!
//! ## Horizontal timing (D6 + D7)
//!
//! D6 is a 74160 decade counter; D7 is a 74161 binary counter.
//! They share the character clock, with D6 terminal count (9) enabling
//! D7. The horizontal sequence runs from count 95 (D6=5, D7=9) to
//! 159 (D6=9, D7=15), giving 65 character-clock slots.
//!
//! H6 = D7 Q2 (bit 2 of D7); H10 = D6 terminal count.
//! `H6 && H10` gates the refresh window, which falls at counts where
//! D6=9 *and* D7 bit 2 is set: 129, 139, 149, 159 — slots 34, 44, 54, 64
//! in the zero-based slot sequence (slot 0 = count 95).
//!
//! ## Refresh Φ2 suppression
//!
//! During a refresh clock the CPU's Φ2 output is gated off the board
//! (Apple-1 Operation Manual, Section III / REFRESH). The CPU holds Φ1;
//! Φ0 continues un-gated for DRAM use. The PIA sees no E edge during
//! refresh. This module models Φ2 suppression as a digital gate: the CPU
//! does not advance its Φ2 half-cycle, and no bus access occurs.
//!
//! ## Vertical timing (D8 + D9, D15)
//!
//! D8/D9 form an eight-bit synchronous counter. LAST H increments it
//! while D15 Q1 enables counting; D15's preset-A sequence inserts six
//! held scan lines into a normal 256-count frame. During VBL, /WC1
//! instead loads BF or 00 according to D6 Q3. Loading takes priority
//! over both LAST H and D15 inhibition, so feedback changes frame length.
//!
//! ## Integration
//!
//! Each master tick, `Timing::tick(write_control)` returns a [`TimingEvent`].
//! The machine drives the CPU on `phi1_edge` and non-refresh `phi2_edge`.
//! Character edges remain at phase 13; MEMΦ rises at phase 3, four
//! ticks after the preceding character edge. Refresh-only ticks advance
//! board time and video/B3 timing without CPU cycles.

/// Crystal frequency in Hz.
pub const CRYSTAL_HZ: u64 = 14_318_180;

/// Master ticks per character clock (divide-by-14).
pub const MASTER_TICKS_PER_CHAR: u8 = 14;

/// Slots per horizontal period: D6 (0–9) × D7 (0–15) from count 95 to 159.
pub const HORIZ_SLOTS: u8 = 65;

/// Master ticks per horizontal period: HORIZ_SLOTS × MASTER_TICKS_PER_CHAR.
pub const HORIZ_MASTER_TICKS: u64 = HORIZ_SLOTS as u64 * MASTER_TICKS_PER_CHAR as u64;

/// The four refresh slots (zero-based) where `H6 && H10` is true.
pub const REFRESH_SLOTS: [u8; 4] = [34, 44, 54, 64];

/// Character clocks per horizontal line (the 65-slot D6/D7 sequence).
pub const LINE_SLOTS: u8 = HORIZ_SLOTS;

/// Character clocks in the visible burst: one per displayed column.  The
/// carousel (video shift-register memory) is only clocked during this
/// burst, not on every horizontal slot — most of the line is blanking.
pub const BURST_SLOTS: u8 = 40;

/// Scan lines per character row.  The 2519 line buffer replays one row of
/// 40 characters eight times: seven for the 5x7 glyph and one blank.
pub const SCAN_LINES_PER_ROW: u8 = 8;

/// Visible character rows.
pub const ROWS: u8 = 24;

/// Scan lines per normal frame: 256 counter values plus six inhibited lines.
pub const SCAN_LINES_PER_FRAME: u16 = 262;

/// Scan lines occupied by the visible rows.
pub const VISIBLE_SCAN_LINES: u16 = ROWS as u16 * SCAN_LINES_PER_ROW as u16;

/// Carousel slots stepped during normal vertical blanking: eight pulses
/// on each of eight scan lines, making 64 spare slots beyond the visible 960.
pub const BLANK_SLOTS: u16 = 64;

/// Carousel slots in the visible window.
pub const VISIBLE_SLOTS: u16 = ROWS as u16 * BURST_SLOTS as u16;

/// What happened on a single master tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingEvent {
    /// Rising edge of Φ1 — call `Cpu::half_cycle` once.
    pub phi1_edge: bool,
    /// Ungated Φ2 rising edge. Call `Cpu::half_cycle` only if `refresh`
    /// is false; refresh clocks produce no CPU bus access or PIA E edge.
    pub phi2_edge: bool,
    /// The current character clock is a refresh cycle (Φ2 suppressed).
    pub refresh: bool,
    /// Character-clock rising edge at phase 13, sampling the counters.
    pub char_edge: bool,
    /// MEMΦ rising edge at phase 3: 40 pulses on scan 7 of visible rows,
    /// or eight pulses on scan 7 during vertical blanking.
    pub mem_clock: bool,
    /// Raster offset exposed by this MEMΦ edge: `row * 40 + column`
    /// in the visible window, or `960 + n` in vertical blanking.
    /// This is not the continuous carousel index: a vertical reload
    /// can repeat raster offsets without rewinding the carousel.
    /// Only meaningful while `mem_clock` is set.
    pub burst: u16,
    /// The current vertical scan line is inside the vertical blanking
    /// interval.
    pub vbi: bool,
    /// Horizontal terminal-count level, H == 159.
    pub last_h: bool,
    /// Value synchronously loaded into D8/D9 on this character edge.
    /// A load of zero is not a completed frame.
    pub vertical_reload: Option<u8>,
    /// Natural FF→00 terminal increment, not a synchronous load to zero.
    pub frame_completed: bool,
}

/// Board-level timing state.
#[derive(Debug, Clone)]
pub struct Timing {
    /// Total master ticks since creation or last machine recreate.
    master_ticks: u64,
    /// 0..13 — position within the current character clock.
    char_phase: u8,
    /// 0..64 — slot index within the horizontal period.
    slot: u8,
    /// D8/D9 eight-bit vertical counter; inhibited lines repeat a value.
    vertical: u8,
    /// D15 four-bit counter, preset to A outside its count window.
    inhibit: u8,
    /// Completed video frame counter.
    frames: u64,
}

impl Timing {
    /// Initialise to slot 0 (D6=5, D7=9 → count 95), phase 0 of the
    /// character clock, first scan line of the first row — a
    /// deterministic, repeatable normal scan state.
    pub fn new() -> Self {
        Self {
            master_ticks: 0,
            char_phase: 0,
            slot: 0,
            vertical: 0,
            inhibit: 0x0a,
            frames: 0,
        }
    }

    /// Total master ticks since creation.
    pub fn master_ticks(&self) -> u64 {
        self.master_ticks
    }

    /// Completed video frames.
    pub fn frames(&self) -> u64 {
        self.frames
    }

    /// Whether the vertical counters are inside the blanking interval.
    pub fn in_vbi(&self) -> bool {
        self.vertical >= VISIBLE_SCAN_LINES as u8
    }

    /// Advance exactly one master tick and return the events it produced.
    ///
    /// Events are determined from the state *before* the tick (the "old
    /// state" rule).  State is committed after computing events.
    /// `write_control` means the display's active-low /WC1 is asserted.
    /// It is sampled on character edges and loads D8/D9 only during VBL.
    pub fn tick(&mut self, write_control: bool) -> TimingEvent {
        let old_phase = self.char_phase;
        let old_slot = self.slot;
        let vertical = self.vertical;

        let phi1_edge = old_phase == 0;
        let phi2_edge = old_phase == MASTER_TICKS_PER_CHAR / 2;
        let char_edge = old_phase == MASTER_TICKS_PER_CHAR - 1;

        let count = old_slot + 95;
        let d6 = count % 10;
        let d7 = count / 10;
        let h6 = d7 & 0x04 != 0;
        let refresh = h6 && d6 == 9;
        let last_h = count == 159;
        let vbi = self.in_vbi();

        // C10 pin 10 selects BF rather than 00 on the shared preset bus.
        let preset_high = vbi && d6 & 0x08 == 0;
        let mem_clock = old_phase == 3 && h6 && vertical & 7 == 7 && !preset_high;
        let burst = if !mem_clock {
            0
        } else if vbi {
            VISIBLE_SLOTS
                + ((vertical as u16 - VISIBLE_SCAN_LINES) / SCAN_LINES_PER_ROW as u16) * 8
                + (d7 as u16 - 12) * 2
                + (d6 as u16 - 8)
        } else {
            (vertical / SCAN_LINES_PER_ROW) as u16 * BURST_SLOTS as u16 + (count - 120) as u16
        };

        self.master_ticks += 1;
        self.char_phase = if char_edge { 0 } else { old_phase + 1 };

        let mut frame_completed = false;
        let mut vertical_reload = None;
        if char_edge {
            // Both 74161s sample /LOAD before their count enables.
            if write_control && vbi {
                let preset = if preset_high { 0xbf } else { 0 };
                self.vertical = preset;
                vertical_reload = Some(preset);
            } else if last_h && self.inhibit & 0x02 != 0 {
                self.vertical = vertical.wrapping_add(1);
                if vertical == u8::MAX {
                    self.frames += 1;
                    frame_completed = true;
                }
            }

            // D15 samples the same old vertical state as D8/D9.
            let inhibit_load_n = vbi && vertical & 0x20 != 0 && vertical & 0x18 == 0;
            if !inhibit_load_n {
                self.inhibit = 0x0a;
            } else if last_h {
                self.inhibit = self.inhibit.wrapping_add(1) & 0x0f;
            }
            self.slot = if last_h { 0 } else { old_slot + 1 };
        }

        TimingEvent {
            phi1_edge,
            phi2_edge,
            refresh,
            char_edge,
            mem_clock,
            burst,
            vbi,
            last_h,
            vertical_reload,
            frame_completed,
        }
    }

    /// Reset board timing for a machine recreate (Ctrl-N). Restores the
    /// deterministic preset state, not physical-RESET board behaviour.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for Timing {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizontal_period_is_910_ticks_with_61_cpu_and_4_refresh() {
        let mut t = Timing::new();
        let mut cpu_cycles = 0u64;
        let mut refresh_cycles = 0u64;
        let mut total_ticks = 0u64;

        for _ in 0..HORIZ_MASTER_TICKS {
            let ev = t.tick(false);
            total_ticks += 1;
            if ev.phi2_edge && !ev.refresh {
                cpu_cycles += 1;
            }
            if ev.phi2_edge && ev.refresh {
                refresh_cycles += 1;
            }
        }

        assert_eq!(total_ticks, 910);
        assert_eq!(cpu_cycles, 61, "65 slots minus 4 refresh = 61 CPU cycles");
        assert_eq!(refresh_cycles, 4, "exactly 4 refresh slots");
    }

    #[test]
    fn refresh_slots_are_exactly_34_44_54_64() {
        let mut t = Timing::new();
        let mut refresh_at = Vec::new();

        for _ in 0..HORIZ_MASTER_TICKS {
            let ev = t.tick(false);
            if ev.phi2_edge && ev.refresh {
                // The slot index before the tick that produced this edge.
                // At phi2_edge (phase 7), the slot hasn't advanced yet.
                refresh_at.push(t.slot);
            }
        }

        assert_eq!(refresh_at, &[34, 44, 54, 64]);
    }

    #[test]
    fn phi1_and_phi2_alternate_correctly() {
        let mut t = Timing::new();
        let mut edges = Vec::new();

        for _ in 0..(MASTER_TICKS_PER_CHAR as u64 * 3) {
            let ev = t.tick(false);
            if ev.phi1_edge {
                edges.push('1');
            }
            if ev.phi2_edge {
                edges.push('2');
            }
        }

        // Should be 1 2 1 2 1 2 — three character clocks, alternating.
        assert_eq!(edges.iter().collect::<String>(), "121212");
    }

    #[test]
    fn char_edge_at_phase_13() {
        let mut t = Timing::new();
        let mut char_edges = Vec::new();

        for _ in 0..(MASTER_TICKS_PER_CHAR as u64 * 2) {
            let ev = t.tick(false);
            if ev.char_edge {
                char_edges.push(t.master_ticks());
            }
        }

        // char_edge fires at the last tick of each character clock:
        // tick 14, 28 (1-based master_ticks after increment).
        assert_eq!(char_edges, &[14, 28]);
    }

    #[test]
    fn reset_zeroes_all_counters() {
        let mut t = Timing::new();
        for _ in 0..500 {
            t.tick(false);
        }
        assert!(t.master_ticks() > 0);
        t.reset();
        assert_eq!(t.master_ticks(), 0);
        assert_eq!(t.frames(), 0);
    }

    #[test]
    fn phi1_still_fires_during_refresh() {
        let mut t = Timing::new();
        // Go to slot 34, phase 0 (Φ1 edge of the refresh character clock).
        let ticks = 34 * MASTER_TICKS_PER_CHAR as u64;
        for _ in 0..ticks {
            t.tick(false);
        }
        let ev = t.tick(false);
        assert!(ev.phi1_edge);
        assert!(ev.refresh);
        // Φ1 fires even during refresh — the CPU enters Φ1, but Φ2 is
        // suppressed later in this same character clock.
    }

    #[test]
    fn vertical_load_is_conditional_and_overrides_count_inhibition() {
        for (vertical, slot, inhibit, write_control, expected, reload) in [
            (191, 0, 0x0a, true, 191, None),
            (192, 0, 0x0a, false, 192, None),
            (192, 0, 0x0a, true, 0xbf, Some(0xbf)),
            (192, 33, 0x0a, true, 0, Some(0)),
            (226, 0, 0x0c, true, 0xbf, Some(0xbf)),
            (226, 64, 0x0c, true, 0, Some(0)),
            (255, 64, 0x0a, true, 0, Some(0)),
        ] {
            let mut t = Timing {
                vertical,
                slot,
                inhibit,
                char_phase: 12,
                ..Timing::new()
            };
            let before_edge = t.tick(write_control);
            assert_eq!(before_edge.vertical_reload, None);
            assert_eq!(t.vertical, vertical);
            let edge = t.tick(write_control);
            assert_eq!(edge.vertical_reload, reload);
            assert_eq!(t.vertical, expected);
            assert!(!edge.frame_completed, "a load is not a terminal increment");
            assert_eq!(t.frames(), 0);
        }
    }

    fn check_frame_bursts(inject_reload: bool, expected_lines: u64, expected_mem: u16) {
        let mut t = Timing::new();
        let mut lines = 0;
        let mut mem = 0;
        let mut bursts = [0u8; 1024];
        let mut line_pulses = 0;
        let mut pulse_distribution = [0u16; 41];
        let mut reloaded = false;
        for _ in 0..expected_lines * HORIZ_MASTER_TICKS {
            let write_control = inject_reload
                && !reloaded
                && t.vertical == 192
                && t.slot == 0
                && t.char_phase == 13;
            let phase = t.char_phase;
            let ev = t.tick(write_control);
            if let Some(value) = ev.vertical_reload {
                assert_eq!(value, 0xbf);
                reloaded = true;
            }
            if ev.mem_clock {
                assert_eq!(phase, 3);
                bursts[ev.burst as usize] += 1;
                mem += 1;
                line_pulses += 1;
            }
            if ev.char_edge && ev.last_h {
                lines += 1;
                pulse_distribution[line_pulses] += 1;
                line_pulses = 0;
            }
            assert_eq!(
                ev.frame_completed,
                t.master_ticks() == expected_lines * HORIZ_MASTER_TICKS
            );
        }
        assert_eq!(t.frames(), 1);
        assert_eq!(lines, expected_lines);
        assert_eq!(mem, expected_mem);
        assert_eq!(reloaded, inject_reload);
        assert_eq!(pulse_distribution[0], 230);
        assert_eq!(pulse_distribution[8], 8);
        assert_eq!(pulse_distribution[40], if inject_reload { 25 } else { 24 });
        for (slot, count) in bursts.into_iter().enumerate() {
            let expected = if inject_reload && (920..960).contains(&slot) {
                2
            } else {
                1
            };
            assert_eq!(count, expected, "raster slot {slot}");
        }
    }

    #[test]
    fn normal_frame_has_262_lines_and_1024_distributed_mem_pulses() {
        check_frame_bursts(false, 262, 1024);
    }

    #[test]
    fn one_bf_reload_repeats_bottom_scan_for_263_lines_and_1064_pulses() {
        check_frame_bursts(true, 263, 1064);
    }
}
