//! Apple I display shift-register model.
//!
//! Models the video output Port B protocol:
//! - CPU writes a character to `$D012` (Port B data).
//! - The shift register becomes busy (PB7 = 1).
//! - After `cycles_per_char` cycles, the character is fully sent,
//!   PB7 returns to 0, and a rising edge on CB1 sets IRQB1.
//!
//! # Screen model
//!
//! The real Apple I terminal is a 40-column by 24-line character display
//! backed by shift-register memory (not random-access RAM); a written
//! character advances a hardware cursor, and hitting the last column or
//! sending CR on the last line scrolls the screen up one line in
//! hardware. This module tracks that same 40x24 grid and cursor position
//! as the authoritative machine-side screen state ([`Display::screen`]);
//! `drain_output` separately hands the host a raw byte stream for
//! presentation (a terminal in canonical mode already wraps/scrolls for
//! display, independent of this machine-side model).
//!
//! Column/row geometry and hardware scroll-on-CR/scroll-on-fill behavior
//! are drawn from secondary technical sources (see `docs/references.md#apple-i`),
//! not a page-by-page primary manual/schematic citation.
//!
//! # CLEAR SCREEN
//!
//! The Apple I keyboard has two pushbuttons: RESET and CLEAR SCREEN
//! (Apple-1 Operation Manual, Section I / Keyboard). CLEAR SCREEN is a
//! video-board input, entirely separate from the system reset line:
//! [`Display::clear_screen`] blanks the grid and homes the cursor without
//! touching the CPU, the PIA, the keyboard, or this model's in-flight
//! character timing. It is modeled as one functional action, not as a
//! button pulse of a particular width.

use std::num::NonZeroU64;

use crate::pia::Pia6821;

/// Default cycles per character (~1 ms at 1 MHz).
pub const DEFAULT_CYCLES_PER_CHAR: NonZeroU64 = NonZeroU64::new(1000).unwrap();

/// Screen width in characters.
pub const COLUMNS: usize = 40;

/// Screen height in lines.
pub const ROWS: usize = 24;

/// Display shift-register model.
pub struct Display {
    /// Cycles a character takes to shift out. Non-zero by type: a
    /// zero-cycle character would never expire the busy timer.
    cycles_per_char: NonZeroU64,
    cycles_remaining: u64,
    /// Characters that have been fully sent, awaiting `drain_output`.
    output: Vec<u8>,
    /// Last character latched into the display.
    latch: u8,
    /// Whether the display is currently busy sending.
    busy: bool,
    /// 40x24 character grid; space-filled cells are blank.
    screen: [[u8; COLUMNS]; ROWS],
    cursor_row: usize,
    cursor_col: usize,
}

impl Display {
    pub fn new(cycles_per_char: NonZeroU64) -> Self {
        Self {
            cycles_per_char,
            cycles_remaining: 0,
            output: Vec::new(),
            latch: 0,
            busy: false,
            screen: [[b' '; COLUMNS]; ROWS],
            cursor_row: 0,
            cursor_col: 0,
        }
    }

    /// Advance the display timer by one cycle.  When the timer expires the
    /// character is collected and the display becomes ready (CB1 rising edge).
    pub fn tick(&mut self, pia: &mut Pia6821) {
        if !self.busy {
            return;
        }
        self.cycles_remaining -= 1;
        if self.cycles_remaining == 0 {
            self.busy = false;
            self.commit_char(self.latch & 0x7F);
            // Rising edge on CB1 → sets IRQB1 if enabled.
            pia.set_cb1(true);
            pia.set_cb1(false);
        }
    }

    /// Apply a fully-sent character to the screen model and the host
    /// delivery queue. CR moves to column 0 of the next line (scrolling if
    /// on the last line); every other byte is written as a printable cell
    /// and advances the cursor, wrapping (and scrolling) past the last
    /// column — the Apple I terminal has no hardware erase/backspace, so
    /// non-CR control bytes are not special-cased.
    fn commit_char(&mut self, ch: u8) {
        self.output.push(ch);
        if ch == b'\r' {
            self.cursor_col = 0;
            self.advance_line();
            return;
        }
        self.screen[self.cursor_row][self.cursor_col] = ch;
        self.cursor_col += 1;
        if self.cursor_col == COLUMNS {
            self.cursor_col = 0;
            self.advance_line();
        }
    }

    /// Move to the next line, scrolling the screen up one line when
    /// already on the last one.
    fn advance_line(&mut self) {
        if self.cursor_row + 1 == ROWS {
            self.screen.rotate_left(1);
            self.screen[ROWS - 1] = [b' '; COLUMNS];
        } else {
            self.cursor_row += 1;
        }
    }

    /// Update Port B input pins before the PIA is read by the CPU.
    /// PB7 = busy flag; PB6–PB0 = last character written for read-back.
    pub fn update_pia(&self, pia: &mut Pia6821) {
        let value = if self.busy {
            0x80 | (self.latch & 0x7F)
        } else {
            self.latch & 0x7F
        };
        pia.set_port_b_inputs(value);
    }

    /// Notify the display that the CPU wrote a character to Port B's output
    /// register (`$D012`). Starts the shift-register busy timer.
    ///
    /// A write that arrives while a previous character is still busy
    /// replaces it and restarts the timer: this crate does not model an
    /// independently-clocked video shift register that could reject or
    /// queue the write, so the interrupted character is dropped and never
    /// reaches `screen`/`drain_output`. Real hardware expects software to
    /// poll PB7 (or wait for the CB1 acknowledge edge) before writing
    /// again, as the Woz Monitor's own display routine does.
    pub fn on_write(&mut self, ch: u8) {
        self.busy = true;
        self.latch = ch;
        self.cycles_remaining = self.cycles_per_char.get();
    }

    /// Drain all completed output characters since the last drain.
    pub fn drain_output(&mut self) -> Vec<u8> {
        std::mem::take(&mut self.output)
    }

    /// Read-only snapshot of the 40x24 character screen. Row 0 is the top
    /// line currently visible; scrolling shifts row content down in index,
    /// matching what a host terminal would show, not a raw shift-register
    /// offset.
    pub fn screen(&self) -> &[[u8; COLUMNS]; ROWS] {
        &self.screen
    }

    /// Current cursor position as `(row, column)`, both 0-based.
    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_col)
    }

    /// CLEAR SCREEN: blank the whole grid and home the cursor.
    ///
    /// This is the Apple I keyboard's second pushbutton (see the module
    /// docs), a video-board input independent of the system reset line. It
    /// touches nothing else: a character still shifting out keeps its
    /// timer and lands at the cleared screen's new cursor position when it
    /// completes, and already-completed bytes still awaiting
    /// `drain_output` are still delivered.
    pub fn clear_screen(&mut self) {
        self.screen = [[b' '; COLUMNS]; ROWS];
        self.cursor_row = 0;
        self.cursor_col = 0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cycles(n: u64) -> NonZeroU64 {
        NonZeroU64::new(n).unwrap()
    }

    /// Drive one character through the busy timer to completion.
    fn send(display: &mut Display, ch: u8) {
        let mut pia = Pia6821::new();
        display.on_write(ch);
        while display.busy {
            display.tick(&mut pia);
        }
    }

    #[test]
    fn single_character_writes_screen_cell_and_advances_cursor() {
        let mut display = Display::new(cycles(1));
        send(&mut display, b'A');
        assert_eq!(display.screen()[0][0], b'A');
        assert_eq!(display.cursor(), (0, 1));
        assert_eq!(display.drain_output(), b"A");
    }

    #[test]
    fn carriage_return_wraps_to_column_zero_of_next_line() {
        let mut display = Display::new(cycles(1));
        send(&mut display, b'A');
        send(&mut display, b'\r');
        assert_eq!(display.cursor(), (1, 0));
        // CR itself is not a printable cell.
        assert_eq!(display.screen()[0][1], b' ');
    }

    #[test]
    fn filling_a_line_wraps_without_explicit_cr() {
        let mut display = Display::new(cycles(1));
        for _ in 0..COLUMNS {
            send(&mut display, b'x');
        }
        assert_eq!(
            display.cursor(),
            (1, 0),
            "the 41st cell must start the next line"
        );
        assert_eq!(display.screen()[0], [b'x'; COLUMNS]);
        assert_eq!(display.screen()[1], [b' '; COLUMNS]);
    }

    #[test]
    fn scrolling_on_last_line_shifts_rows_up_and_clears_bottom() {
        let mut display = Display::new(cycles(1));
        for row in 0..ROWS {
            send(&mut display, b'0' + (row % 10) as u8);
            send(&mut display, b'\r');
        }
        // After ROWS lines, the last CR scrolled once: row 0's original
        // '0' is gone, and every line's marker shifted up by one.
        assert_eq!(display.screen()[0][0], b'1');
        assert_eq!(
            display.screen()[ROWS - 2][0],
            b'0' + ((ROWS - 1) % 10) as u8
        );
        assert_eq!(
            display.screen()[ROWS - 1],
            [b' '; COLUMNS],
            "scrolled-in row must be blank"
        );
        assert_eq!(display.cursor(), (ROWS - 1, 0));
    }

    #[test]
    fn write_while_busy_drops_pending_character() {
        let mut display = Display::new(cycles(10));
        let mut pia = Pia6821::new();
        display.on_write(b'A');
        for _ in 0..3 {
            display.tick(&mut pia);
        }
        // 'A' is still busy (needs 10 ticks); overwrite it.
        display.on_write(b'B');
        for _ in 0..10 {
            display.tick(&mut pia);
        }
        assert_eq!(display.drain_output(), b"B", "'A' must never be delivered");
        assert_eq!(display.screen()[0][0], b'B');
    }

    #[test]
    fn clear_screen_blanks_grid_and_homes_cursor() {
        let mut display = Display::new(cycles(1));
        send(&mut display, b'A');
        send(&mut display, b'B');
        assert_eq!(display.cursor(), (0, 2));

        display.clear_screen();

        assert_eq!(display.screen(), &[[b' '; COLUMNS]; ROWS]);
        assert_eq!(display.cursor(), (0, 0));
    }

    #[test]
    fn clear_screen_leaves_an_in_flight_character_to_land_on_the_new_screen() {
        let mut display = Display::new(cycles(10));
        let mut pia = Pia6821::new();
        send(&mut display, b'A');
        assert_eq!(display.drain_output(), b"A");

        display.on_write(b'B'); // in flight, not yet committed
        for _ in 0..3 {
            display.tick(&mut pia);
        }
        display.clear_screen();
        assert_eq!(display.screen()[0][0], b' ', "'A' is cleared");

        // The in-flight character keeps its timer and lands at the homed
        // cursor when it finishes — CLEAR SCREEN is not a video reset.
        for _ in 0..7 {
            display.tick(&mut pia);
        }
        assert_eq!(display.drain_output(), b"B");
        assert_eq!(display.screen()[0][0], b'B');
        assert_eq!(display.cursor(), (0, 1));
    }
}
