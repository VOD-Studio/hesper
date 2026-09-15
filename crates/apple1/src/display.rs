//! Apple I text terminal: six 2504 data tracks, a circulating cursor, and
//! the C7 write/CR control path.
//!
//! The 1024-slot memory advances only on MEMΦ, independently of the
//! raster's vertical counter. In a normal frame there are 24 bursts of
//! forty slots on scan line 7 of each character row, then eight bursts
//! of eight slots during vertical blanking. The latter erase the 64
//! spare slots.
//!
//! A request is accepted when the memory head reaches the cursor.
//! Printable characters write the six data tracks and move the cursor.
//! CR owns the write port until LAST H, erasing each passing slot. Other
//! control characters are acknowledged without moving the cursor.
//! The host observes RDA through the board's B3/PIA handshake.
//!
//! # Scrolling
//!
//! C7's write control is held between MEMΦ edges and fed back to the
//! vertical counters. A write or end-of-line CR can therefore reload
//! D8/D9 during VBL. The counter's raster position changes; the memory
//! head does not jump. Repeating scan line 191 adds forty real shifts.
//! The projected screen origin is derived from these two positions, not
//! advanced by an independent scroll request or a fixed frame delay.
//!
//! # Host observations
//!
//! `screen`, `cursor`, and `drain_output` are text projections. Memory
//! retains only bits 0–4 and inverted bit 6; a parallel ASCII array
//! preserves the existing host character convention without feeding
//! it into hardware control. The 2513/pixel output path is not modeled.
//!
//! CLEAR SCREEN remains an atomic host operation, not a timed button
//! pulse: it blanks memory, homes the cursor, and aligns the blank ring
//! to the current raster position. It cancels CR/write control without
//! resetting board time, CPU, PIA, or an offered character.

use crate::timing::TimingEvent;

/// Screen width in characters.
pub const COLUMNS: usize = 40;

/// Screen height in lines.
pub const ROWS: usize = 24;

/// Character slots installed in the carousel (six 2504 shift registers).
pub const SLOTS: usize = 1024;

/// Carousel slots in the visible window.
pub const VISIBLE_SLOTS: usize = COLUMNS * ROWS;

/// Scan lines each character row occupies.
pub const SCAN_LINES: usize = 8;

/// The circulating video memory and its host text projection.
pub struct Display {
    /// Packed six-bit hardware data, independent of the host character set.
    memory: [u8; SLOTS],
    /// Host-only character for each physical memory slot.
    host: [u8; SLOTS],
    /// Physical slot under the circulating cursor.
    cursor: usize,
    /// Physical slot corresponding to projected screen cell (0, 0).
    origin: usize,
    /// Next physical slot exposed by MEMΦ. Never jumps on vertical reload.
    head: usize,
    /// Next slot in a normal raster pass, independent of `head`.
    raster_slot: usize,
    screen: [[u8; COLUMNS]; ROWS],
    da_prev: bool,
    request: bool,
    /// An accepted CR owns the write port through LAST H.
    clearing: bool,
    /// Asserted /WC1, held until the next MEMΦ edge.
    write_control: bool,
    rda: bool,
    output: Vec<u8>,
}

impl Display {
    /// Initialise a blank ring aligned to the beginning of a normal frame.
    pub fn new() -> Self {
        Self {
            memory: [0; SLOTS],
            host: [b' '; SLOTS],
            cursor: 0,
            origin: 0,
            head: 0,
            raster_slot: 0,
            screen: [[b' '; COLUMNS]; ROWS],
            da_prev: false,
            request: false,
            clearing: false,
            write_control: false,
            rda: false,
            output: Vec::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn memory(&self) -> &[u8; SLOTS] {
        &self.memory
    }

    /// Feed /WC1 back to the counters; `true` means asserted (low).
    pub(crate) fn write_control(&self) -> bool {
        self.write_control
    }

    /// Observe character/MEMΦ edges and counter reloads from the board.
    pub(crate) fn clock(&mut self, ev: &TimingEvent, data: u8, da: bool) {
        if !da {
            self.request = false;
        } else if !self.da_prev {
            self.request = true;
        }
        self.da_prev = da;

        if let Some(vertical) = ev.vertical_reload {
            // Both hardware presets are in the visible range: 00 starts
            // a new raster pass; BF repeats the last row's scan line 7.
            self.raster_slot = vertical as usize / SCAN_LINES * COLUMNS;
            self.align_projection();
        } else if ev.frame_completed {
            self.raster_slot = 0;
            self.align_projection();
        }

        if !ev.mem_clock {
            return;
        }

        let slot = self.head;
        self.write_control = false;
        if self.request && !self.clearing && slot == self.cursor {
            self.write_control = self.accept(slot, data);
            self.request = false;
        }

        if self.clearing {
            self.store(slot, 0, b' ');
            self.cursor = (slot + 1) % SLOTS;
            if ev.last_h {
                self.clearing = false;
                self.write_control = true;
            }
        }

        if ev.vbi {
            self.store(slot, 0, b' ');
        }
        self.head = (slot + 1) % SLOTS;
        self.raster_slot = (ev.burst as usize + 1) % SLOTS;
    }

    /// Accept live port data and return whether WRITE is asserted.
    fn accept(&mut self, slot: usize, data: u8) -> bool {
        self.rda = true;
        if data & 0x60 == 0 {
            if data == 0x0d {
                self.output.push(b'\r');
                self.clearing = true;
            }
            return false;
        }

        let tracks = (data & 0x1f) | if data & 0x40 == 0 { 0x20 } else { 0 };
        let projected = (data & 0x7f).to_ascii_uppercase();
        self.store(slot, tracks, projected);
        self.output.push(projected);
        self.cursor = (slot + 1) % SLOTS;
        true
    }

    fn store(&mut self, slot: usize, tracks: u8, projected: u8) {
        self.memory[slot] = tracks;
        self.host[slot] = projected;
        let offset = (slot + SLOTS - self.origin) % SLOTS;
        if offset < VISIBLE_SLOTS {
            self.screen[offset / COLUMNS][offset % COLUMNS] = projected;
        }
    }

    /// A raster reload changes the interpretation of the passing ring,
    /// never the ring's physical position or its contents.
    fn align_projection(&mut self) {
        let origin = (self.head + SLOTS - self.raster_slot) % SLOTS;
        if origin == self.origin {
            return;
        }
        self.origin = origin;
        for (row, cells) in self.screen.iter_mut().enumerate() {
            for (column, cell) in cells.iter_mut().enumerate() {
                *cell = self.host[(origin + row * COLUMNS + column) % SLOTS];
            }
        }
    }

    /// Take the acknowledgement event that triggers B3.
    pub(crate) fn take_rda(&mut self) -> bool {
        std::mem::take(&mut self.rda)
    }

    /// Drain accepted printable characters and carriage returns.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Read-only host projection of the visible window.
    pub fn screen(&self) -> &[[u8; COLUMNS]; ROWS] {
        &self.screen
    }

    /// Project the cursor as `(row, column)`, both zero-based.
    ///
    /// A cursor in the spare slots is offscreen until the vertical reload;
    /// hosts must hide it when the row is outside the visible window.
    pub fn cursor(&self) -> (usize, usize) {
        let offset = (self.cursor + SLOTS - self.origin) % SLOTS;
        (offset / COLUMNS, offset % COLUMNS)
    }

    /// Whether CR or a write-control pulse is still in flight.
    pub fn io_pending(&self) -> bool {
        self.clearing || self.write_control
    }

    /// Atomic host CLEAR SCREEN, preserving board clocks and the handshake.
    pub fn clear_screen(&mut self) {
        self.memory = [0; SLOTS];
        self.host = [b' '; SLOTS];
        self.screen = [[b' '; COLUMNS]; ROWS];
        self.head = (self.origin + self.raster_slot) % SLOTS;
        self.cursor = self.origin;
        self.clearing = false;
        self.write_control = false;
    }
}

impl Default for Display {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::machine::B3_PULSE_TICKS;
    use crate::timing::{HORIZ_MASTER_TICKS, SCAN_LINES_PER_FRAME, Timing};

    const FRAME_TICKS: u64 = HORIZ_MASTER_TICKS * SCAN_LINES_PER_FRAME as u64;

    fn tick(timing: &mut Timing, display: &mut Display, data: u8, da: bool) -> TimingEvent {
        let ev = timing.tick(display.write_control());
        if ev.char_edge || ev.mem_clock {
            display.clock(&ev, data, da);
        }
        ev
    }

    fn next_mem(timing: &mut Timing, display: &mut Display, data: u8, da: bool) {
        for _ in 0..2 * FRAME_TICKS {
            if tick(timing, display, data, da).mem_clock {
                return;
            }
        }
        panic!("the circulating memory did not advance");
    }

    /// Every call starts and finishes at a frame boundary. The memory
    /// phase comes from the real counters, including any CR reload.
    fn present(display: &mut Display, ch: u8) {
        let mut timing = Timing::new();
        display.take_rda();
        let mut accepted_at = None;
        for _ in 0..2 * FRAME_TICKS {
            let da = accepted_at.is_none_or(|at| timing.master_ticks() < at + B3_PULSE_TICKS);
            let ev = tick(&mut timing, display, ch, da);
            if display.rda && accepted_at.is_none() {
                accepted_at = Some(timing.master_ticks());
            }
            if ev.frame_completed
                && accepted_at.is_some_and(|at| timing.master_ticks() >= at + B3_PULSE_TICKS)
            {
                return;
            }
        }
        panic!("the terminal did not accept {ch:02X} and finish its frame");
    }

    #[test]
    fn blank_terminal_has_an_empty_screen_and_a_home_cursor() {
        let display = Display::new();
        assert_eq!(display.screen(), &[[b' '; COLUMNS]; ROWS]);
        assert_eq!(display.cursor(), (0, 0));
        assert!(!display.io_pending());
        assert_eq!(display.memory(), &[0; SLOTS]);
    }

    #[test]
    fn accepted_character_lands_on_the_screen_and_advances_the_cursor() {
        let mut display = Display::new();
        present(&mut display, b'A');
        assert_eq!(display.screen()[0][0], b'A');
        assert_eq!(display.cursor(), (0, 1));
        assert_eq!(display.drain_output(), b"A");
    }

    #[test]
    fn a_character_is_taken_only_when_its_own_slot_passes() {
        let mut display = Display::new();
        for ch in b"ABCDE" {
            present(&mut display, *ch);
        }
        assert_eq!(display.cursor(), (0, 5));

        let mut timing = Timing::new();
        next_mem(&mut timing, &mut display, b'Z', true);
        for burst in 1..5u16 {
            next_mem(&mut timing, &mut display, b'Z', true);
            assert_eq!(
                display.screen()[0][5],
                b' ',
                "the cursor slot has not been exposed at burst {burst}"
            );
        }
        assert_eq!(display.cursor(), (0, 5));
        next_mem(&mut timing, &mut display, b'Z', true);
        assert_eq!(display.screen()[0][5], b'Z');
        assert_eq!(display.cursor(), (0, 6));
    }

    #[test]
    fn the_data_lines_are_sampled_when_the_character_is_taken() {
        let mut display = Display::new();
        // 'A' is offered, but the port lines change to 'B' before the
        // cursor slot arrives: the terminal stores what is on the lines at
        // the moment it takes the character.
        let mut timing = Timing::new();
        tick(&mut timing, &mut display, b'A', true);
        next_mem(&mut timing, &mut display, b'B', true);
        assert_eq!(display.screen()[0][0], b'B');
        assert_eq!(display.drain_output(), b"B");

        // Once taken, changing the lines again cannot rewrite the cell.
        next_mem(&mut timing, &mut display, b'C', true);
        assert_eq!(display.screen()[0][0], b'B');
    }

    #[test]
    fn a_character_offered_after_its_slot_waits_for_the_next_lap() {
        let mut display = Display::new();
        let mut timing = Timing::new();
        next_mem(&mut timing, &mut display, 0, false);
        // Offer 'Q' one memory clock too late for slot 0 of this lap.
        next_mem(&mut timing, &mut display, b'Q', true);
        for _ in 2..SLOTS {
            next_mem(&mut timing, &mut display, b'Q', false);
        }
        assert_eq!(
            display.screen()[0][0],
            b' ',
            "slot 0 has already gone past in this lap"
        );
        assert_eq!(display.cursor(), (0, 0));

        // The next lap exposes slot 0 again and takes it.
        next_mem(&mut timing, &mut display, b'Q', true);
        assert_eq!(display.screen()[0][0], b'Q');
        assert_eq!(display.cursor(), (0, 1));
    }

    #[test]
    fn control_codes_are_acknowledged_but_never_stored() {
        let mut display = Display::new();
        present(&mut display, 0x02); // ctrl-B
        assert!(
            display.take_rda(),
            "a control code still completes the handshake"
        );
        assert_eq!(display.screen()[0][0], b' ');
        assert_eq!(
            display.cursor(),
            (0, 0),
            "a control code does not move the cursor"
        );
        assert!(display.drain_output().is_empty());
    }

    #[test]
    fn output_normalizes_case_and_keeps_punctuation() {
        let mut display = Display::new();
        for ch in b"0aZ@[\x60{~_" {
            present(&mut display, *ch);
        }
        assert_eq!(display.drain_output(), b"0AZ@[\x60{~_");
    }

    #[test]
    fn carriage_return_blanks_to_the_end_of_the_line_and_wraps() {
        let mut display = Display::new();
        for ch in b"HI" {
            present(&mut display, *ch);
        }
        present(&mut display, b'\r');
        assert_eq!(display.drain_output(), b"HI\r", "CR is reported once");
        assert_eq!(display.screen()[0][..2], *b"HI");
        assert_eq!(
            display.screen()[0][2..],
            [b' '; COLUMNS - 2],
            "the fill blanks the rest of the line"
        );
        assert_eq!(display.cursor(), (1, 0));
        assert!(display.drain_output().is_empty(), "the fill adds no output");
    }

    #[test]
    fn carriage_return_on_the_last_column_still_wraps() {
        let mut display = Display::new();
        for column in 0..COLUMNS - 1 {
            present(&mut display, b'0' + (column % 10) as u8);
        }
        assert_eq!(display.cursor(), (0, COLUMNS - 1));
        present(&mut display, b'\r');
        assert_eq!(display.cursor(), (1, 0));
        assert_eq!(display.screen()[0][COLUMNS - 1], b' ');
    }

    #[test]
    fn filling_the_line_wraps_to_the_next_row() {
        let mut display = Display::new();
        for column in 0..COLUMNS {
            present(&mut display, b'0' + (column % 10) as u8);
        }
        assert_eq!(display.cursor(), (1, 0));
        assert_eq!(
            display.screen()[0],
            *b"0123456789012345678901234567890123456789"
        );
        assert_eq!(display.screen()[1], [b' '; COLUMNS]);
    }

    #[test]
    fn the_bottom_row_scrolls_through_the_vertical_reload() {
        let mut display = Display::new();
        // Fill every row with a distinct marker and a carriage return.
        for row in 0..ROWS {
            present(&mut display, b'A' + (row % 26) as u8);
            present(&mut display, b'\r');
        }
        // The last carriage return walked the cursor off the bottom of the
        // window, which moved the display origin up one row.
        assert_eq!(display.cursor(), (ROWS - 1, 0));
        assert_eq!(display.screen()[0][0], b'B', "row 1 is now the top row");
        assert_eq!(
            display.screen()[ROWS - 2][0],
            b'A' + ((ROWS - 1) % 26) as u8,
            "the last written row moved up one"
        );
        assert_eq!(
            display.screen()[ROWS - 1],
            [b' '; COLUMNS],
            "the erased spare slots became the new bottom row"
        );

        // The next character lands on that new bottom row.
        present(&mut display, b'Z');
        assert_eq!(display.cursor(), (ROWS - 1, 1));
        assert_eq!(display.screen()[ROWS - 1][0], b'Z');
    }

    #[test]
    fn clearing_the_screen_blanks_everything_and_homes_the_cursor() {
        let mut display = Display::new();
        present(&mut display, b'A');
        let _ = display.drain_output();
        display.clear_screen();
        assert_eq!(display.screen(), &[[b' '; COLUMNS]; ROWS]);
        assert_eq!(display.cursor(), (0, 0));
        assert_eq!(display.memory(), &[0; SLOTS]);
        assert!(display.drain_output().is_empty());
    }

    #[test]
    fn rda_is_asserted_once_per_taken_character() {
        let mut display = Display::new();
        assert!(!display.take_rda());
        present(&mut display, b'A');
        assert!(display.take_rda(), "the accept asserted RDA");
        assert!(
            !display.take_rda(),
            "RDA is a single pulse, not a level held until read"
        );
        assert_eq!(display.cursor(), (0, 1));
    }

    #[test]
    fn blanking_steps_erase_the_spare_slots() {
        let mut display = Display::new();
        // Put a marker in a slot that is outside the visible window.
        let spare = (display.origin + VISIBLE_SLOTS) % SLOTS;
        display.memory[spare] = 0x1f;
        display.host[spare] = b'X';

        let mut timing = Timing::new();
        for _ in 0..SLOTS {
            next_mem(&mut timing, &mut display, 0x7f, false);
        }
        assert_eq!(display.memory()[spare], 0);
        assert_eq!(display.host[spare], b' ');
    }
}
