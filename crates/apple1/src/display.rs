//! Apple I video terminal: 2504 carousel memory, 2519 line buffer, and the
//! C7 acknowledge/capture logic.
//!
//! # Carousel memory
//!
//! The terminal has no random-access video RAM. Its screen storage is six
//! 2504 1024-bit shift registers wired in parallel (six bits per
//! character) that recirculate continuously: 1024 installed character
//! slots for a screen of 24 rows x 40 columns = 960 visible cells, leaving
//! 64 slots that are never displayed in a normal frame.
//!
//! MEMΦ advances the carousel one slot at a time. It is *not* a free
//! running clock: the terminal only bursts it while characters must be
//! made visible — forty pulses during the first scan line of each
//! character row, and one pulse per blanking line while the 64 spare slots
//! are stepped through and erased. One frame therefore advances the
//! carousel exactly 24 x 40 + 64 = 1024 slots, i.e. one lap.
//!
//! Because the carousel only moves during those bursts, a character is
//! accepted at the moment the cursor's slot is exposed — which is what
//! makes display writes slow and position dependent.
//!
//! # Line buffer
//!
//! A row of characters has to be shown eight times (seven for the 5x7
//! glyph, one blank). The carousel cannot jump back forty slots, so each
//! row is copied out of the carousel into the 2519 40-character
//! recirculating line buffer ([`Display::line`]) on LINEΦ and replayed
//! from there by the character generator for the remaining scan lines.
//!
//! # Character acceptance (C7)
//!
//! `CURS` is high while the slot under the cursor is exposed. The terminal
//! registers the board's `DA` line (PIA CB2 through an inverter) in the C7
//! flip-flops: an assertion is captured as a *request* and held until the
//! cursor's slot comes round, where the write logic is
//! `ack_n = !(CURS && DA)`, `control = !(ack_n || RD6 || RD7)`,
//! `write_n = ack_n || control`. In words:
//!
//! - The terminal takes exactly one character per request, however long
//!   the CPU leaves DA asserted, and a write that lands between MEMΦ
//!   bursts is held rather than missed.
//! - `RD6`/`RD7` clear means the seven data lines carry a control code:
//!   the handshake still completes, but nothing is written to the
//!   carousel and the cursor does not move.
//! - Otherwise the six data tracks (bits 0-4 and the inverted bit 6; bit 5
//!   is dropped) are shifted into the carousel and the cursor advances one
//!   slot, which is also what makes `CURS` fall away before the next
//!   character can be taken.
//!
//! Every accept asserts RDA, which the board turns into the B3 one-shot
//! pulse on the PIA's CB1 — see `crate::machine`. That is what releases
//! CB2, which is also how software knows the terminal is ready again.
//!
//! A carriage return ($0D, decoded separately from `RD1`/`RD3`/`RD4` high
//! and `RD2`/`RD5`/`RD6`/`RD7` low) is accepted like any other control
//! code, but additionally clears the carousel to the end of the line one
//! slot at a time so the cursor lands on column 0 of the next row. The
//! carousel can only fill the line *it is currently passing*; there is no
//! addressable addressing, so the blanking is a sequence, not an
//! assignment. While that sequence runs it owns the carousel's write port,
//! so a character offered during the fill waits and is taken at the
//! cursor's next slot — the start of the next line.
//!
//! # Vertical blanking and scrolling
//!
//! The 64 slots outside the visible window are erased during vertical
//! blanking as the carousel steps through them. Without that erase, the
//! slots left over from the previous lap would scroll in from the bottom
//! of the screen.
//!
//! Scrolling itself is a vertical reload: when the cursor advances past
//! the last visible slot, the display origin moves forward one row at the
//! vertical boundary, so the visible window shifts up and the freshly
//! erased spare slots become the new bottom row. The screen never moves
//! its contents directly.
//!
//! # Host projection
//!
//! [`Display::screen`], [`Display::cursor`], and [`Display::drain_output`]
//! are a host text projection. The carousel keeps only the six bits the
//! hardware stores ([`Display::memory`]); the projection keeps the
//! normalised ASCII the host renders in a parallel array that no control
//! logic reads, so the projection can never feed back into the handshake.
//!
//! # CLEAR SCREEN
//!
//! The Apple I keyboard has two pushbuttons: RESET and CLEAR SCREEN
//! (Apple-1 Operation Manual, Section I / Keyboard). CLEAR SCREEN is a
//! video-board input, entirely separate from the system reset line:
//! [`Display::clear_screen`] blanks the carousel and homes the cursor
//! without touching the CPU, the PIA, the keyboard, or this model's
//! in-flight handshake. It is modeled as one functional action, not as a
//! button pulse of a particular width.

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

/// The 2504 carousel, the 2519 line buffer, and the C7 write logic.
pub struct Display {
    /// Six data tracks, packed as one byte per slot. Bits 0-4 are data
    /// bits 0-4; bit 5 is the inverted data bit 6 (bit 5 is not stored).
    memory: [u8; SLOTS],
    /// Host-projection character per slot. Not hardware state: the
    /// projection needs the accepted ASCII, and nothing in the control
    /// path reads it.
    host: [u8; SLOTS],
    /// Carousel slot under the cursor.
    cursor: usize,
    /// Carousel slot displayed at screen cell (0, 0).
    origin: usize,
    /// 2519 line buffer: the 40 characters currently being displayed.
    line: [u8; COLUMNS],
    /// Row whose forty characters the line buffer currently holds.
    scanning_row: usize,
    /// Host projection of the visible window.
    screen: [[u8; COLUMNS]; ROWS],
    /// `DA` as seen at the previous character clock, for edge detection.
    da_prev: bool,
    /// A character has been offered and is waiting for the cursor's slot.
    /// Captured on DA's rising transition and cleared by the take.
    request: bool,
    /// Slots still to be blanked by an accepted carriage return.
    clear_to_eol: u8,
    /// The cursor left the visible window; the display origin moves at
    /// the next vertical reload.
    scroll_pending: bool,
    /// RDA was asserted by the last MEMΦ edge and has not been reported.
    rda: bool,
    /// Accepted characters waiting for `drain_output`.
    output: Vec<u8>,
}

impl Display {
    /// A carousel of blank characters with the cursor at the home slot.
    pub fn new() -> Self {
        Self {
            memory: [0; SLOTS],
            host: [b' '; SLOTS],
            cursor: 0,
            origin: 0,
            line: [0; COLUMNS],
            scanning_row: 0,
            screen: [[b' '; COLUMNS]; ROWS],
            da_prev: false,
            request: false,
            clear_to_eol: 0,
            scroll_pending: false,
            rda: false,
            output: Vec::new(),
        }
    }

    /// The six data tracks the carousel holds, one byte per slot: bits 0-4
    /// and the inverted bit 6 of each accepted character. This is the
    /// hardware store, not the host projection — hosts read the projected
    /// screen through [`Display::screen`]. Only the unit tests inspect it.
    #[cfg(test)]
    pub(crate) fn memory(&self) -> &[u8; SLOTS] {
        &self.memory
    }

    /// Advance the video board by one character clock.
    ///
    /// `data` is the seven character data lines (PIA PB0-PB6, or zero when
    /// the PIA is not driving them) and `da` is the board's DA line — PIA
    /// CB2 through the inverter.
    pub(crate) fn clock(&mut self, ev: &TimingEvent, data: u8, da: bool) {
        // C7 registers DA. A request is captured on the transition and
        // held until the cursor's slot arrives, so the terminal takes
        // exactly one character per request however long the CPU leaves
        // the line asserted — and a write that lands between bursts is
        // not missed the way a purely level-sampled latch would miss it.
        if da && !self.da_prev {
            self.request = true;
        }
        self.da_prev = da;

        if ev.line_load {
            self.load_line(ev.burst);
        }

        if ev.mem_clock {
            // A carriage-return fill drives the carousel's write port
            // until the line is clear, so a character offered while the
            // fill is still walking waits its turn instead of landing in
            // the middle of the line being erased.
            let ready = self.request && self.clear_to_eol == 0;
            if ready && (ev.burst as usize) < VISIBLE_SLOTS {
                let exposed = (self.origin + ev.burst as usize) % SLOTS;
                if exposed == self.cursor {
                    self.accept(exposed, data);
                    self.request = false;
                }
            }
            if ev.burst as usize >= VISIBLE_SLOTS {
                // Vertical blanking: whichever spare slot is passing is
                // erased, so leftovers cannot scroll in from the bottom.
                let spare = (self.origin + ev.burst as usize) % SLOTS;
                self.store(spare, 0, b' ');
            }
            self.clear_one();
            self.vertical_reload();
        }
    }

    /// Accept a character the terminal has clocked in under the cursor.
    fn accept(&mut self, slot: usize, data: u8) {
        let control = data & 0x60 == 0;
        // The one-shot fires on the acknowledge, not on the data: a
        // control code is accepted and released just like a printable one.
        self.rda = true;

        if control {
            if data == 0x0d {
                // Carriage return: the rest of the line is filled with
                // blanks, one slot per character clock, which is also what
                // walks the cursor onto the next row. The fill itself
                // appends nothing — the CR is reported once, here.
                self.output.push(b'\r');
                let column = (slot + SLOTS - self.origin) % SLOTS % COLUMNS;
                self.clear_to_eol = (COLUMNS - column) as u8;
            }
            return;
        }

        // Six tracks: data bits 0-4, then the inverted bit 6. Bit 5 is not
        // stored at all (the character generator rebuilds the case bit).
        let tracks = (data & 0x1f) | if data & 0x40 == 0 { 0x20 } else { 0 };
        let projected = (data & 0x7f).to_ascii_uppercase();
        self.store(slot, tracks, projected);
        self.output.push(projected);
        self.step_cursor();
    }

    /// Blank one slot of a pending carriage-return fill and walk the
    /// cursor along with it.
    fn clear_one(&mut self) {
        if self.clear_to_eol == 0 {
            return;
        }
        self.clear_to_eol -= 1;
        let slot = self.cursor;
        self.store(slot, 0, b' ');
        self.step_cursor();
    }

    /// Move the cursor one carousel slot, scrolling when it leaves the
    /// visible window.
    fn step_cursor(&mut self) {
        self.cursor = (self.cursor + 1) % SLOTS;
        if (self.cursor + SLOTS - self.origin) % SLOTS >= VISIBLE_SLOTS {
            self.scroll_pending = true;
        }
    }

    /// Write one carousel slot and update the host projection.
    fn store(&mut self, slot: usize, tracks: u8, projected: u8) {
        self.memory[slot] = tracks;
        self.host[slot] = projected;
        let offset = (slot + SLOTS - self.origin) % SLOTS;
        if offset < VISIBLE_SLOTS {
            let (row, column) = (offset / COLUMNS, offset % COLUMNS);
            self.screen[row][column] = projected;
            // The character generator is fed from the line buffer, so a
            // slot written while its own row is being scanned shows up
            // immediately.
            if row == self.scanning_row {
                self.line[column] = tracks;
            }
        }
    }

    /// Copy the row about to be scanned out of the carousel into the 2519
    /// line buffer.
    fn load_line(&mut self, burst: u16) {
        let row = burst as usize / COLUMNS;
        if row >= ROWS {
            return;
        }
        for column in 0..COLUMNS {
            let slot = (self.origin + row * COLUMNS + column) % SLOTS;
            self.line[column] = self.memory[slot];
            self.screen[row][column] = self.host[slot];
        }
        self.scanning_row = row;
    }

    /// Apply a scroll requested while the cursor walked off the bottom.
    /// The visible window moves forward one row; the new bottom row is the
    /// first row of spare slots, which vertical blanking erased on the
    /// previous lap.
    fn vertical_reload(&mut self) {
        if !self.scroll_pending {
            return;
        }
        self.scroll_pending = false;
        self.origin = (self.origin + COLUMNS) % SLOTS;
        for row in 0..ROWS {
            for column in 0..COLUMNS {
                let slot = (self.origin + row * COLUMNS + column) % SLOTS;
                self.screen[row][column] = self.host[slot];
            }
        }
    }

    /// Carousel offset of the cursor within the visible window. Only the
    /// unit tests need the raw offset; hosts use [`Display::cursor`].
    #[cfg(test)]
    fn cursor_offset(&self) -> usize {
        (self.cursor + SLOTS - self.origin) % SLOTS
    }

    /// Take the "RDA is asserted" event for the board's B3 one-shot.
    pub(crate) fn take_rda(&mut self) -> bool {
        std::mem::take(&mut self.rda)
    }

    /// Drain all completed output characters since the last drain.
    /// These are seven-bit characters with ASCII letters uppercased.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Read-only snapshot of the 40x24 character screen. Row 0 is the top
    /// line currently visible; scrolling shifts row content down in index,
    /// matching what a host terminal would show, not a raw carousel
    /// offset.
    pub fn screen(&self) -> &[[u8; COLUMNS]; ROWS] {
        &self.screen
    }

    /// Current cursor position as `(row, column)`, both 0-based.
    pub fn cursor(&self) -> (usize, usize) {
        let offset = (self.cursor + SLOTS - self.origin) % SLOTS;
        (offset / COLUMNS, offset % COLUMNS)
    }

    /// Whether the terminal has work in flight: a carriage-return fill
    /// still walking the line, or a scroll waiting for the vertical
    /// reload. A character merely waiting under the cursor is the PIA's
    /// handshake, not the terminal's.
    pub fn io_pending(&self) -> bool {
        self.clear_to_eol > 0 || self.scroll_pending
    }

    /// CLEAR SCREEN: blank the whole carousel and home the cursor.
    ///
    /// This is the Apple I keyboard's second pushbutton (see the module
    /// docs), a video-board input independent of the system reset line. It
    /// runs no machine time and touches nothing else: the CPU, the PIA,
    /// the keyboard queue, an in-flight B3 pulse, and already-accepted
    /// output stay exactly as they are. A character that is still being
    /// offered on the port lines is still offered, so the terminal
    /// accepts it at the homed cursor's next opportunity.
    pub fn clear_screen(&mut self) {
        self.memory = [0; SLOTS];
        self.host = [b' '; SLOTS];
        self.line = [0; COLUMNS];
        self.screen = [[b' '; COLUMNS]; ROWS];
        self.cursor = self.origin;
        self.clear_to_eol = 0;
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
    use crate::timing::BLANK_SLOTS;

    /// One MEMΦ/character-clock step at carousel offset `burst`.
    fn step(display: &mut Display, burst: u16, data: u8, da: bool) {
        let ev = TimingEvent {
            phi1_edge: false,
            phi2_edge: false,
            refresh: false,
            char_edge: true,
            mem_clock: true,
            burst,
            line_load: burst.is_multiple_of(COLUMNS as u16),
            vbi: burst >= VISIBLE_SLOTS as u16,
            frame_completed: false,
        };
        display.clock(&ev, data, da);
    }

    /// Present `ch` on the port lines so that the terminal takes it at the
    /// cursor's next opportunity, then release DA a few clocks later the
    /// way the B3/CB1 acknowledge does and run the frame out.
    fn present(display: &mut Display, ch: u8) {
        let offset = display.cursor_offset();
        // DA has to be registered one clock before the cursor's slot, so
        // assertion starts one clock early (or in the previous frame when
        // the cursor sits on the first slot).
        let first = if offset == 0 { SLOTS - 1 } else { offset - 1 };
        step(display, first as u16, ch, true);
        // The pulse releases CB2 a few character clocks later, which is
        // also what lets the terminal look at the next character.
        let release = offset + 4;
        for burst in 0..SLOTS {
            if burst == first {
                continue;
            }
            step(display, burst as u16, ch, burst <= release);
        }
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
        assert!(display.memory()[0] & 0x1f != 0, "the six tracks are stored");
    }

    #[test]
    fn a_character_is_taken_only_when_its_own_slot_passes() {
        let mut display = Display::new();
        for ch in b"ABCDE" {
            present(&mut display, *ch);
        }
        assert_eq!(display.cursor(), (0, 5));

        // Offer 'Z' from the start of a frame. DA reaches the C7 output on
        // the first clock, but the accept can only happen at the cursor's
        // own slot, five clocks later.
        step(&mut display, 0, b'Z', true);
        for burst in 1..5u16 {
            step(&mut display, burst, b'Z', true);
            assert_eq!(
                display.screen()[0][5],
                b' ',
                "the cursor slot has not been exposed at burst {burst}"
            );
        }
        assert_eq!(display.cursor(), (0, 5));
        step(&mut display, 5, b'Z', true);
        assert_eq!(display.screen()[0][5], b'Z');
        assert_eq!(display.cursor(), (0, 6));
    }

    #[test]
    fn the_data_lines_are_sampled_when_the_character_is_taken() {
        let mut display = Display::new();
        // 'A' is offered, but the port lines change to 'B' before the
        // cursor slot arrives: the terminal stores what is on the lines at
        // the moment it takes the character.
        step(&mut display, SLOTS as u16 - 1, b'A', true);
        step(&mut display, 0, b'B', true);
        assert_eq!(display.screen()[0][0], b'B');
        assert_eq!(display.drain_output(), b"B");

        // Once taken, changing the lines again cannot rewrite the cell.
        step(&mut display, 1, b'C', true);
        assert_eq!(display.screen()[0][0], b'B');
    }

    #[test]
    fn a_character_offered_after_its_slot_waits_for_the_next_lap() {
        let mut display = Display::new();
        // Offer 'Q' one clock too late for slot 0 of this lap.
        step(&mut display, 1, b'Q', true);
        for burst in 2..SLOTS {
            step(&mut display, burst as u16, b'Q', false);
        }
        assert_eq!(
            display.screen()[0][0],
            b' ',
            "slot 0 has already gone past in this lap"
        );
        assert_eq!(display.cursor(), (0, 0));

        // The next lap exposes slot 0 again and takes it.
        step(&mut display, SLOTS as u16 - 1, b'Q', true);
        step(&mut display, 0, b'Q', true);
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

        // A blanking pass erases it, which is what keeps stale characters
        // from scrolling in at the bottom.
        for offset in 0..BLANK_SLOTS {
            step(&mut display, VISIBLE_SLOTS as u16 + offset, 0x7f, false);
        }
        assert_eq!(display.memory()[spare], 0);
        assert_eq!(display.host[spare], b' ');
    }
}
