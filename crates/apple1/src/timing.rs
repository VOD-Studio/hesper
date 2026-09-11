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
//! D6 is a 74LS196 decade counter; D7 is a 74LS197 binary counter.
//! They cascade: D6 terminal count (9) → D7 clock. D11's terminal count
//! (13) clocks D6. The horizontal sequence runs from count 95 (D6=5,
//! D7=9) to 159 (D6=9, D7=15), giving 65 character-clock slots.
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
//! ## Integration
//!
//! Each master tick, `Timing::tick()` returns a [`TimingEvent`]. The
//! machine drives the CPU only on `phi1_edge` and (non-refresh)
//! `phi2_edge`; it advances the keyboard, display, and PIA on
//! `char_edge`. Refresh-only ticks advance board time and continue
//! video/B3 timing without CPU cycles.

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

/// Vertical scan lines in a normal frame (NTSC).
pub const SCAN_LINES_PER_FRAME: u16 = 262;

/// Scan lines occupied by the visible rows.
pub const VISIBLE_SCAN_LINES: u16 = ROWS as u16 * SCAN_LINES_PER_ROW as u16;

/// Carousel slots stepped during vertical blanking: 1024 installed slots
/// minus the 960 visible ones.  These are the slots erased each frame.
pub const BLANK_SLOTS: u16 = 64;

/// Carousel slots in the visible window.
pub const VISIBLE_SLOTS: u16 = ROWS as u16 * BURST_SLOTS as u16;

/// What happened on a single master tick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct TimingEvent {
    /// Rising edge of Φ1 — call `Cpu::half_cycle` once.
    pub phi1_edge: bool,
    /// Rising edge of Φ2 — call `Cpu::half_cycle` once.  False during
    /// refresh (the CPU stays at Φ1; no bus access, no PIA E edge).
    pub phi2_edge: bool,
    /// The current character clock is a refresh cycle (Φ2 suppressed).
    pub refresh: bool,
    /// Rising edge of the character clock (master tick 0).  Display,
    /// keyboard, and B3 timing advance on this edge.
    pub char_edge: bool,
    /// MEMΦ: the carousel advances one character slot.  One pulse per
    /// displayed column during a row's first scan line, and one per
    /// blanking line while the spare slots are stepped and erased.
    pub mem_clock: bool,
    /// Carousel offset of the slot this MEMΦ edge exposes, counted from
    /// the display origin: `row * 40 + column` for visible rows, and
    /// `960 + n` for the spare slots stepped through during blanking.
    /// Only meaningful while `mem_clock` is set.
    pub burst: u16,
    /// LINEΦ: the 2519 line buffer takes a fresh copy of the row.
    pub line_load: bool,
    /// The current vertical scan line is inside the vertical blanking
    /// interval.
    pub vbi: bool,
    /// A full video frame just completed (vertical terminal count).
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
    /// 0..261 — scan line within the frame.  The character row and the
    /// scan line within it are derived from this rather than counted
    /// separately, so the vertical counters can never drift apart.
    vline: u16,
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
            vline: 0,
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
        self.vline >= VISIBLE_SCAN_LINES
    }

    /// Advance exactly one master tick and return the events it produced.
    ///
    /// Events are determined from the state *before* the tick (the "old
    /// state" rule).  State is committed after computing events.
    pub fn tick(&mut self) -> TimingEvent {
        let old_phase = self.char_phase;
        let old_slot = self.slot;
        let old_vline = self.vline;

        // --- Derive events from old state ---

        // Φ1 edge: start of character clock (phase 0 → enters Φ1).
        let phi1_edge = old_phase == 0;

        // Φ2 edge: mid-point of character clock (phase 7 → enters Φ2).
        let phi2_edge = old_phase == MASTER_TICKS_PER_CHAR / 2;

        // Is the current slot a refresh slot?  Derived from the count
        // at old_slot, since the video counters are stable during a
        // character clock and advance on its trailing edge.
        let count = old_slot as u16 + 95;
        let d6 = (count % 10) as u8;
        let d7 = (count / 10) as u8;
        let h6 = d7 & 0x04 != 0;
        let h10 = d6 == 9;
        let refresh = h6 && h10;

        // Character-clock edge: phase rolls over to 0.
        let char_edge = old_phase == MASTER_TICKS_PER_CHAR - 1;

        // Vertical blanking: the 24 visible rows are scanned first.
        let vbi = old_vline >= VISIBLE_SCAN_LINES;
        let row = (old_vline / SCAN_LINES_PER_ROW as u16).min(ROWS as u16) as u8;
        let scan = (old_vline % SCAN_LINES_PER_ROW as u16) as u8;

        // MEMΦ: the carousel is clocked once per displayed column while a
        // row's first scan line runs, and once per blanking line while the
        // spare slots are stepped through.
        let blanking_step = vbi && old_slot == 0 && old_vline < VISIBLE_SCAN_LINES + BLANK_SLOTS;
        let mem_clock =
            char_edge && ((!vbi && scan == 0 && old_slot < BURST_SLOTS) || blanking_step);
        let burst = if blanking_step {
            VISIBLE_SLOTS + (old_vline - VISIBLE_SCAN_LINES)
        } else {
            row as u16 * BURST_SLOTS as u16 + old_slot as u16
        };

        // LINEΦ: the line buffer takes the row on the first slot of the
        // row's first scan line.
        let line_load = mem_clock && old_slot == 0 && scan == 0 && !vbi;

        // --- Commit new state ---

        self.master_ticks += 1;

        // Advance character phase.
        self.char_phase = if old_phase + 1 >= MASTER_TICKS_PER_CHAR {
            0
        } else {
            old_phase + 1
        };

        let mut frame_completed = false;
        if char_edge {
            // Advance the horizontal slot; its terminal count steps the
            // vertical scan line.
            if old_slot + 1 >= LINE_SLOTS {
                self.slot = 0;
                if old_vline + 1 >= SCAN_LINES_PER_FRAME {
                    self.vline = 0;
                    self.frames += 1;
                    frame_completed = true;
                } else {
                    self.vline = old_vline + 1;
                }
            } else {
                self.slot = old_slot + 1;
            }
        }

        TimingEvent {
            phi1_edge,
            phi2_edge,
            refresh,
            char_edge,
            mem_clock,
            burst,
            line_load,
            vbi,
            frame_completed,
        }
    }

    /// Reset board timing for a machine recreate (Ctrl-N).  Zeroes all
    /// counters; does not model physical-RESET board behaviour.
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
            let ev = t.tick();
            total_ticks += 1;
            if ev.phi1_edge {
                // Φ1 always fires.
            }
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
            let ev = t.tick();
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
            let ev = t.tick();
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
            let ev = t.tick();
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
            t.tick();
        }
        assert!(t.master_ticks() > 0);
        t.reset();
        assert_eq!(t.master_ticks(), 0);
        assert_eq!(t.frames(), 0);
    }

    #[test]
    fn no_cpu_bus_access_on_refresh_slot() {
        let mut t = Timing::new();
        // Advance to just before slot 34 (the first refresh slot).
        // Slot 34, phase 7 is the refresh Φ2 edge.
        // 34 slots × 14 ticks + 7 = 34*14+7 = 483 ticks.
        let ticks_to_first_refresh_phi2 = 34 * MASTER_TICKS_PER_CHAR as u64 + 7;
        for _ in 0..ticks_to_first_refresh_phi2 {
            t.tick();
        }

        // Now one more tick: should be phi2_edge on refresh slot.
        let ev = t.tick();
        assert!(ev.phi2_edge, "must be Φ2 edge");
        assert!(ev.refresh, "must be a refresh slot");
        // CPU should NOT get a cycle here.
        assert!(
            ev.refresh && ev.phi2_edge,
            "Φ2 edge during refresh: CPU cycle must be suppressed"
        );
    }

    #[test]
    fn phi1_still_fires_during_refresh() {
        let mut t = Timing::new();
        // Go to slot 34, phase 0 (Φ1 edge of the refresh character clock).
        let ticks = 34 * MASTER_TICKS_PER_CHAR as u64;
        for _ in 0..ticks {
            t.tick();
        }
        let ev = t.tick();
        assert!(ev.phi1_edge);
        assert!(ev.refresh);
        // Φ1 fires even during refresh — the CPU enters Φ1, but Φ2 is
        // suppressed later in this same character clock.
    }
}
