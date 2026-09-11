//! Apple I CLI host: loads a machine, then drives it either through a real
//! terminal in raw mode (one key at a time, host commands reserved) or
//! through a plain line-buffered stdin loop when stdin is not a terminal
//! (scripts, pipes, CI smoke checks) — the integration tests in
//! `crates/cli/tests/apple1.rs` exercise the latter path.
//!
//! Every master tick this host runs goes through [`Session::tick`], so
//! `--max-cycles` is an exact ceiling on the whole session: boot, RESET,
//! recreate, and every input batch included. One master tick is one
//! 14.31818 MHz crystal period and 14 master ticks make one ~1.023 MHz CPU
//! cycle; `--max-cycles` counts real CPU cycles, while a bus record's `M=`
//! marker reports the session's master ticks.

use std::{
    collections::VecDeque,
    error::Error,
    fmt::{self, Write as _},
    fs,
    io::{self, BufRead, IsTerminal, Write},
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, queue,
    style::Print,
    terminal,
};
use hesper_apple1::{
    Apple1, Apple1Bus,
    display::{COLUMNS, Display, ROWS},
    machine::{RESET_COMPLETION_BUDGET, RESET_HOLD_CYCLES, Tick},
};
use hesper_cpu6502::{CpuError, StepKind};
use sha2::{Digest, Sha256};
use signal_hook::SigId;

use crate::{format_bus_trace, format_instruction_trace};

/// Real CPU cycles to run per idle tick while nothing new has arrived (a
/// keystroke in the interactive loop, or after a line in the batch loop)
/// and no budget check has fired. Large relative to a real 1 MHz Apple I
/// because this crate does not throttle to wall-clock time (see
/// `crates/apple1/src/lib.rs`); it only bounds how much host-side work
/// happens between input checks.
const IDLE_BATCH_CPU_CYCLES: u64 = 2_000;

/// Real CPU cycles to run immediately after RESET to reach the monitor's
/// prompt (or whatever the loaded program prints first) before accepting
/// input.
const BOOT_BATCH_CPU_CYCLES: u64 = 50_000;

/// The exact 256-byte Woz Monitor image this CLI accepts, by SHA-256.
///
/// The Apple I firmware is not shipped, embedded, or downloaded here (see
/// `crates/apple1/tests/data/README.md`); the host supplies it with
/// `--rom`. Pinning its identity keeps a wrong-but-same-length file from
/// booting into unexplained garbage: the machine library still accepts any
/// valid 256-byte ROM for original firmware, only this CLI is fixed.
const WOZMON_SHA256: [u8; 32] = [
    0xe5, 0xaf, 0x0d, 0x1c, 0x40, 0x57, 0xbd, 0x8e, 0x0e, 0xf5, 0xcb, 0x06, 0x9c, 0x20, 0x8f, 0xf7,
    0xcc, 0x09, 0x84, 0xa7, 0xdf, 0xf5, 0x3b, 0x12, 0xc5, 0xcf, 0x11, 0x9d, 0xe8, 0xcb, 0x5c, 0x25,
];

/// Inclusive bounds on `--trace-limit`, matching the demo host's.
const TRACE_LIMIT_RANGE: std::ops::RangeInclusive<usize> = 1..=4096;

/// Width of the status field drawn below the machine's 24 rows, wide enough
/// for the longest marker (`[NEW MACHINE]`) so a shorter one overwrites it.
const STATUS_WIDTH: usize = 16;

/// Why the run loop stopped. Distinguishes a normal, expected yield from a
/// user-requested stop, a budget limit, and a real error — `run_apple1`
/// reports each with its own message and (for `Error`) a non-zero exit via
/// `main`'s existing `Err` handling; the other three are graceful (exit 0).
#[derive(Debug)]
enum StopReason {
    /// The user quit interactively (Ctrl-C/Ctrl-D) or stdin hit EOF.
    UserOrEof,
    /// `--max-cycles` was reached.
    BudgetExceeded(u64),
    /// An external signal (SIGTERM/SIGINT/SIGHUP/SIGQUIT) asked the
    /// process to stop — e.g. a supervisor, `kill`, or a closed terminal
    /// window. Raw mode disables the terminal's own Ctrl-C/Ctrl-\ signal
    /// generation (see `run_interactive`'s doc comment), but an externally
    /// delivered signal is unrelated to that and would otherwise bypass
    /// `RawMode`'s `Drop` entirely, leaving the real terminal stuck in
    /// raw mode after this process exits.
    Signaled,
}

impl fmt::Display for StopReason {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::UserOrEof => write!(f, "[stopped]"),
            Self::BudgetExceeded(cycles) => write!(f, "[max cycles reached: {cycles}]"),
            Self::Signaled => write!(f, "[stopped by signal]"),
        }
    }
}

/// Which CPU observations a session records, and how many it keeps.
///
/// Both kinds share one bounded queue: `--trace-limit` caps total records,
/// so diagnostics stay bounded no matter how long the machine runs.
struct TraceOptions {
    instructions: bool,
    bus: bool,
    limit: usize,
}

impl TraceOptions {
    fn enabled(&self) -> bool {
        self.instructions || self.bus
    }
}

/// One CLI session over one machine at a time.
///
/// `total_cpu_cycles` is this session's accumulated real-CPU-cycle count,
/// deliberately separate from the machine's own `cpu_cycles()`: recreating
/// the machine (Ctrl-N) gives a fresh machine counter, but the session
/// ceiling and its trace keep going. `total_master_ticks` (session board
/// time, the trace's `M=` marker) and `total_frames` (completed video
/// frames, the batch drain's quiet measure) carry over the same way.
struct Session {
    machine: Apple1,
    total_cpu_cycles: u64,
    total_master_ticks: u64,
    total_frames: u64,
    max_cycles: Option<u64>,
    trace: TraceOptions,
    recent: VecDeque<String>,
}

impl Session {
    fn new(machine: Apple1, max_cycles: Option<u64>, trace: TraceOptions) -> Self {
        Self {
            machine,
            total_cpu_cycles: 0,
            total_master_ticks: 0,
            total_frames: 0,
            max_cycles,
            trace,
            recent: VecDeque::new(),
        }
    }

    /// Real CPU cycles left in the session budget, or `None` when
    /// unlimited.
    fn remaining(&self) -> Option<u64> {
        self.max_cycles
            .map(|max| max.saturating_sub(self.total_cpu_cycles))
    }

    /// The only place this host advances the emulated board. Returns
    /// `Ok(None)` when the budget is exhausted, without touching the
    /// machine (budget 0 must not advance any machine time).
    ///
    /// A tick with no real CPU cycle (Φ1 phase, or a Φ2 suppressed by
    /// refresh) still advances board time and video timing; it just does
    /// not count against `--max-cycles`, and therefore records no trace
    /// line. A tick that ends in a CPU error propagates the error without
    /// counting: the bus access that failed produced no `Cycle` to report,
    /// so no bus line is invented for it.
    fn tick(&mut self) -> Result<Option<Tick>, CpuError> {
        if self.remaining() == Some(0) {
            return Ok(None);
        }
        let tick = self.machine.tick()?;
        self.total_master_ticks += 1;
        if tick.cpu.is_some() {
            self.total_cpu_cycles += 1;
        }
        if tick.frame_completed {
            self.total_frames += 1;
        }

        // With both kinds off nothing is snapshotted, formatted, or queued.
        if self.trace.bus
            && let Some(cycle) = tick.cpu
        {
            let state = self.machine.cpu().debug_state();
            self.record(format_bus_trace(
                &cycle,
                &state,
                self.total_cpu_cycles,
                Some(self.total_master_ticks),
            ));
        }
        if self.trace.instructions
            && let Some(step) = tick.cpu.and_then(|cycle| cycle.completed)
        {
            self.record(format_instruction_trace(&step, self.total_cpu_cycles));
        }
        Ok(Some(tick))
    }

    /// Keep the newest record, dropping the oldest past the limit.
    fn record(&mut self, line: String) {
        if self.recent.len() == self.trace.limit {
            self.recent.pop_front();
        }
        self.recent.push_back(line);
    }

    /// Write the retained records. Called only after the terminal has been
    /// restored, so diagnostics never land inside the machine's screen.
    fn write_trace(&self, out: &mut impl Write) -> io::Result<()> {
        for line in &self.recent {
            writeln!(out, "{line}")?;
        }
        Ok(())
    }

    /// Run up to `requested_cpu_cycles` real CPU cycles. Ticks that advance
    /// no CPU cycle (Φ1, refresh-suppressed Φ2) only move board time and do
    /// not count toward the request. Stops the moment the budget is
    /// exhausted — including exactly at the end of the batch, so the host
    /// never accepts one more input or runs "one last batch" past the
    /// ceiling.
    fn advance(&mut self, requested_cpu_cycles: u64) -> Result<Option<StopReason>, CpuError> {
        let mut completed = 0u64;
        while completed < requested_cpu_cycles {
            match self.tick()? {
                None => return Ok(Some(StopReason::BudgetExceeded(self.total_cpu_cycles))),
                Some(tick) => {
                    if tick.cpu.is_some() {
                        completed += 1;
                    }
                }
            }
        }
        if self.remaining() == Some(0) {
            return Ok(Some(StopReason::BudgetExceeded(self.total_cpu_cycles)));
        }
        Ok(None)
    }

    /// Physical RESET driven cycle by cycle under the session budget: hold
    /// the line for [`RESET_HOLD_CYCLES`] real CPU cycles, release it, then
    /// run until the CPU reports its reset sequence complete within
    /// [`RESET_COMPLETION_BUDGET`] real CPU cycles.
    ///
    /// Running out of budget during the hold, the release, or the vector
    /// read is an ordinary budget stop: the reset line keeps whatever state
    /// it has and the sequence resumes if the host is given more budget.
    /// `CycleBudgetExceeded` is reserved for a CPU that never completes a
    /// fixed-length reset with budget to spare.
    fn reset(&mut self) -> Result<Option<StopReason>, CpuError> {
        self.machine.set_reset_line(true);
        let mut held = 0u64;
        while held < RESET_HOLD_CYCLES {
            match self.tick()? {
                None => return Ok(Some(StopReason::BudgetExceeded(self.total_cpu_cycles))),
                Some(tick) => {
                    if tick.cpu.is_some() {
                        held += 1;
                    }
                }
            }
        }
        self.machine.set_reset_line(false);
        let mut elapsed = 0u64;
        while elapsed < RESET_COMPLETION_BUDGET {
            match self.tick()? {
                None => return Ok(Some(StopReason::BudgetExceeded(self.total_cpu_cycles))),
                Some(tick) => {
                    if tick.cpu.is_some() {
                        elapsed += 1;
                    }
                    if let Some(step) = tick.cpu.and_then(|cycle| cycle.completed)
                        && step.kind == StepKind::Reset
                    {
                        return Ok(None);
                    }
                }
            }
        }
        Err(CpuError::CycleBudgetExceeded {
            address: self.machine.cpu().registers().pc,
            budget: RESET_COMPLETION_BUDGET,
        })
    }

    /// RESET plus the boot batch that carries the machine to its first
    /// prompt. Used for the initial start and for Ctrl-N.
    fn boot(&mut self) -> Result<Option<StopReason>, CpuError> {
        match self.reset()? {
            Some(stop) => Ok(Some(stop)),
            None => self.advance(BOOT_BATCH_CPU_CYCLES),
        }
    }
}

/// Run the Apple I with the given ROM and optional program.
pub fn run_apple1(
    rom_path: &str,
    program_path: Option<&str>,
    max_cycles: Option<u64>,
    trace: bool,
    bus_trace: bool,
    trace_limit: usize,
) -> Result<(), Box<dyn Error>> {
    // Checked here too, not only in the argument parser: a direct library
    // call must not be able to install an unbounded diagnostic queue.
    if !TRACE_LIMIT_RANGE.contains(&trace_limit) {
        return Err("--trace-limit requires 1..4096".into());
    }
    let trace_options = TraceOptions {
        instructions: trace,
        bus: bus_trace,
        limit: trace_limit,
    };

    // 1. Load and fully validate ROM and optional program (exact ROM
    // identity, program fits in RAM) before any machine state exists to
    // mutate; a bad path, wrong image, or oversized program fails here
    // with nothing partially applied and no cycle executed.
    let rom = load_rom(rom_path)?;
    let program = program_path.map(load_program).transpose()?;

    // 2. Build the machine and the session. Nothing has executed yet: with
    // `--max-cycles 0` the run stops here having reported zero cycles.
    let machine = create_machine(&rom, program.as_deref())?;
    let mut session = Session::new(machine, max_cycles, trace_options);

    let outcome = if io::stdin().is_terminal() {
        run_interactive(&mut session, &rom, program.as_deref())
    } else {
        run_batch(&mut session)
    };

    // 3. The terminal is restored by now (the interactive guard is dropped
    // on the way out), so the retained records go to stderr while stdout
    // keeps only machine output. A budget stop, a user quit, a signal, and
    // a CPU error all reach this point.
    if session.trace.enabled() {
        let _ = session.write_trace(&mut io::stderr());
    }

    let stop = outcome?;
    eprintln!("\n{stop}");
    Ok(())
}

/// Read the Woz Monitor ROM: exactly 256 bytes and exactly the pinned
/// image (see [`WOZMON_SHA256`]). No download, no way to skip the check.
fn load_rom(path: &str) -> Result<[u8; 256], Box<dyn Error>> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read ROM file '{path}': {e}"))?;
    if bytes.len() != Apple1Bus::ROM_SIZE {
        return Err(format!(
            "ROM file '{path}' is {} bytes; the Woz Monitor image is exactly {} bytes",
            bytes.len(),
            Apple1Bus::ROM_SIZE
        )
        .into());
    }
    let digest = Sha256::digest(&bytes);
    if digest.as_slice() != WOZMON_SHA256 {
        return Err(format!(
            "ROM SHA-256 mismatch for '{path}': got {}, expected {}",
            hex(&digest),
            hex(&WOZMON_SHA256)
        )
        .into());
    }
    let mut rom = [0u8; Apple1Bus::ROM_SIZE];
    rom.copy_from_slice(&bytes);
    Ok(rom)
}

/// Read an optional raw program image loaded at `$0000`. Any length that
/// fits the Apple I's 4 KiB RAM is accepted, including empty.
fn load_program(path: &str) -> Result<Vec<u8>, Box<dyn Error>> {
    let bytes = fs::read(path).map_err(|e| format!("cannot read program file '{path}': {e}"))?;
    if bytes.len() > Apple1Bus::RAM_SIZE {
        return Err(format!(
            "program '{path}' is {} bytes: program exceeds 4 KiB Apple I RAM",
            bytes.len()
        )
        .into());
    }
    Ok(bytes)
}

/// Lowercase hex, only built for a mismatch message.
fn hex(bytes: &[u8]) -> String {
    let mut text = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        let _ = write!(text, "{byte:02x}");
    }
    text
}

/// Construct a machine and load the optional program. Runs no master tick:
/// RESET is the session's job, so it stays inside the cycle budget. Shared
/// by the initial start and by the interactive loop's "recreate machine"
/// command.
fn create_machine(rom: &[u8; 256], program: Option<&[u8]>) -> Result<Apple1, Box<dyn Error>> {
    let mut machine = Apple1::new(rom)?;
    if let Some(bytes) = program {
        machine
            .bus_mut()
            .load_ram(0x0000, bytes)
            .map_err(|e| format!("cannot load program: {e}"))?;
    }
    Ok(machine)
}

/// Non-interactive loop: read whole lines from stdin (scripts, pipes).
/// A real terminal never reaches this path (see `run_apple1`).
///
/// The idle batches below only mean "the host stops advancing for now to
/// read the next line": the emulated CPU is still running its polling loop,
/// and nothing here treats quiet output as a halted program. The drain
/// after each line is a generic host EOF-quiet policy: it waits for three
/// complete video frames with no new output and no I/O in flight, then goes
/// back to reading stdin. It reads no Woz Monitor private state, and it
/// cannot decide whether an arbitrary program will ever print again — it
/// only decides when this host stops waiting before the next line.
fn run_batch(session: &mut Session) -> Result<StopReason, Box<dyn Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    if let Some(stop) = advance_and_print(session, Boot::Yes, &mut stdout)? {
        return Ok(stop);
    }

    loop {
        let mut line = String::new();
        match stdin.lock().read_line(&mut line) {
            Ok(0) => return Ok(StopReason::UserOrEof),
            Err(e) => return Err(format!("input error: {e}").into()),
            Ok(_) => {}
        }

        // Canonical-mode terminals translate the user's physical Enter key
        // (CR, 0x0D) to LF (0x0A) before `read_line` sees it (POSIX
        // ICRNL). Strip whatever line ending arrived and replay it as a
        // single CR — the byte the Woz Monitor ROM actually expects to
        // terminate a line.
        let content = line.strip_suffix('\n').unwrap_or(line.as_str());
        let content = content.strip_suffix('\r').unwrap_or(content);
        for ch in content.chars() {
            session.machine.type_char(ch as u8);
            if let Some(stop) =
                advance_and_print(session, Boot::No(IDLE_BATCH_CPU_CYCLES), &mut stdout)?
            {
                return Ok(stop);
            }
        }
        session.machine.type_char(b'\r');
        if let Some(stop) =
            advance_and_print(session, Boot::No(IDLE_BATCH_CPU_CYCLES), &mut stdout)?
        {
            return Ok(stop);
        }

        // Keep advancing until the machine has been quiet for three whole
        // video frames, then go read the next line. A batch that produced
        // output or still has I/O in flight (unread keyboard input, a
        // display handshake mid-shift) resets the counter: only real,
        // complete quiet frames count.
        let mut quiet_frames = 0u64;
        while quiet_frames < 3 {
            let frames_before = session.total_frames;
            let stop = session.advance(IDLE_BATCH_CPU_CYCLES)?;
            let output = session.machine.drain_output();
            if output.is_empty() && !session.machine.io_pending() {
                quiet_frames += session.total_frames - frames_before;
            } else {
                quiet_frames = 0;
                print_output(&output, &mut stdout)?;
            }
            if let Some(stop) = stop {
                return Ok(stop);
            }
        }
    }
}

/// What to run in one host step.
enum Boot {
    /// RESET plus the boot batch.
    Yes,
    /// A plain batch of this many cycles.
    No(u64),
}

/// Advance the session, then write whatever the display finished — even
/// when the run stopped, so the last characters are never swallowed.
fn advance_and_print(
    session: &mut Session,
    what: Boot,
    stdout: &mut impl Write,
) -> Result<Option<StopReason>, Box<dyn Error>> {
    let stop = match what {
        Boot::Yes => session.boot()?,
        Boot::No(cycles) => session.advance(cycles)?,
    };
    let output = session.machine.drain_output();
    print_output(&output, stdout)?;
    Ok(stop)
}

/// How the host presents the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum View {
    /// A fixed 40x24 grid drawn from the machine's own screen model.
    /// Requires a real terminal on both stdin and stdout.
    Grid,
    /// The raw stream of characters the display finished, CR expanded to
    /// CRLF. Used whenever stdout is not a terminal, so no cursor or
    /// screen-control sequence is ever written into a file or a pipe.
    Stream,
}

/// RAII guard for everything this host changes about the real terminal,
/// plus the signal handlers it installs.
///
/// Signals are registered *before* raw mode and before any emulated cycle
/// runs: a signal delivered during the boot batch must still unwind through
/// this guard instead of leaving the terminal in raw mode. Each field
/// records only what actually succeeded, so a partial failure rolls back
/// exactly what was applied; `Drop` undoes it in reverse order.
struct TerminalGuard {
    raw: bool,
    paste: bool,
    alternate: bool,
    wrap_disabled: bool,
    signals: Vec<SigId>,
}

impl TerminalGuard {
    fn enter(view: View, terminated: &Arc<AtomicBool>) -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            paste: false,
            alternate: false,
            wrap_disabled: false,
            signals: Vec::new(),
        };
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
            signal_hook::consts::SIGQUIT,
        ] {
            // On error `guard` is dropped here, unregistering whatever was
            // already installed.
            guard
                .signals
                .push(signal_hook::flag::register(signal, Arc::clone(terminated))?);
        }

        terminal::enable_raw_mode()?;
        guard.raw = true;

        if view == View::Grid {
            let mut stdout = io::stdout();
            // Every sequence below goes to stdout, so it is only ever sent
            // when stdout really is the terminal: a redirected stdout must
            // stay a clean character stream. Bracketed paste makes a paste
            // arrive as one event instead of a burst of keystrokes.
            execute!(stdout, event::EnableBracketedPaste)?;
            guard.paste = true;
            execute!(stdout, terminal::EnterAlternateScreen)?;
            guard.alternate = true;
            // The machine wraps at its own 40th column; the host terminal
            // must not wrap on top of that.
            execute!(stdout, terminal::DisableLineWrap)?;
            guard.wrap_disabled = true;
            execute!(
                stdout,
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            )?;
        }
        Ok(guard)
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        // Exact reverse of `enter`: wrap, alternate screen, bracketed
        // paste, raw mode, then the signal handlers.
        let mut stdout = io::stdout();
        if self.alternate {
            // The renderer hides the cursor whenever the machine's cursor
            // falls outside a small window.
            let _ = execute!(stdout, cursor::Show);
        }
        if self.wrap_disabled {
            let _ = execute!(stdout, terminal::EnableLineWrap);
        }
        if self.alternate {
            let _ = execute!(stdout, terminal::LeaveAlternateScreen);
        }
        if self.paste {
            let _ = execute!(stdout, event::DisableBracketedPaste);
        }
        if self.raw {
            let _ = terminal::disable_raw_mode();
        }
        for id in self.signals.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}

/// Project one machine screen byte for a host terminal: printable ASCII as
/// itself, everything else as a blank cell.
///
/// The machine's grid holds whatever bytes software wrote, including ESC
/// and other control codes. Handing those to the host terminal would let
/// emulated software drive the real terminal; they still occupy their cell.
/// This is a safe projection, not an original character-ROM emulation.
fn projected(byte: u8) -> char {
    if (0x20..=0x7E).contains(&byte) {
        char::from(byte)
    } else {
        ' '
    }
}

/// Draw the machine's screen at the terminal's top-left corner.
///
/// Reads only `screen()`/`cursor()` — never the bus — and writes one
/// buffered frame with a single flush. The grid is fixed at 40x24: a wider
/// terminal does not reflow it. A window smaller than that shows the
/// visible rectangle only and hides the machine cursor when it falls
/// outside; machine state is never changed to fit the window. `status` is
/// drawn on the 25th row when the window has one.
fn draw_screen(
    display: &Display,
    status: &str,
    size: (u16, u16),
    out: &mut impl Write,
) -> io::Result<()> {
    let (width, height) = size;
    if width == 0 || height == 0 {
        return Ok(());
    }
    let columns = usize::from(width).min(COLUMNS);
    let rows = usize::from(height).min(ROWS);

    let mut frame = Vec::with_capacity(rows * (columns + 8) + 64);
    for (row, line) in display.screen().iter().take(rows).enumerate() {
        let text: String = line[..columns].iter().copied().map(projected).collect();
        queue!(frame, cursor::MoveTo(0, row as u16), Print(text))?;
    }
    if usize::from(height) > ROWS {
        // Padded to a fixed field instead of clearing the line, so one
        // write both shows the new marker and erases the previous one.
        let room = usize::from(width).min(STATUS_WIDTH);
        let marker: String = status
            .chars()
            .chain(std::iter::repeat(' '))
            .take(room)
            .collect();
        queue!(frame, cursor::MoveTo(0, ROWS as u16), Print(marker))?;
    }

    let (cursor_row, cursor_column) = display.cursor();
    if cursor_row < rows && cursor_column < columns {
        queue!(
            frame,
            cursor::MoveTo(cursor_column as u16, cursor_row as u16),
            cursor::Show
        )?;
    } else {
        queue!(frame, cursor::Hide)?;
    }

    out.write_all(&frame)?;
    out.flush()
}

/// A single keypress translated into what the CLI does with it. Host
/// commands (quit, RESET, clear screen, pause, recreate) are reserved
/// modifier combinations that a real Apple I keyboard cannot produce
/// (it has no Ctrl key at all); every other key maps to a byte forwarded
/// to the emulated keyboard.
enum Action {
    Quit,
    Reset,
    ClearScreen,
    TogglePause,
    Recreate,
    Key(u8),
    None,
}

/// Map a real keyboard's keypress to an [`Action`].
///
/// Translate Enter to CR, Backspace to the Woz Monitor's underscore
/// erase key, and Esc to its cancel byte. ASCII letter case is handled
/// by the emulated Keyboard, not the host adapter.
fn classify_key(key: KeyEvent) -> Action {
    if key.kind != KeyEventKind::Press {
        return Action::None;
    }
    if key.modifiers.contains(KeyModifiers::CONTROL) {
        return match key.code {
            KeyCode::Char('c') | KeyCode::Char('C') => Action::Quit,
            KeyCode::Char('d') | KeyCode::Char('D') => Action::Quit,
            KeyCode::Char('r') | KeyCode::Char('R') => Action::Reset,
            KeyCode::Char('l') | KeyCode::Char('L') => Action::ClearScreen,
            KeyCode::Char('p') | KeyCode::Char('P') => Action::TogglePause,
            KeyCode::Char('n') | KeyCode::Char('N') => Action::Recreate,
            _ => Action::None,
        };
    }
    match key.code {
        KeyCode::Enter => Action::Key(b'\r'),
        KeyCode::Backspace => Action::Key(b'_'),
        KeyCode::Esc => Action::Key(0x1B),
        KeyCode::Char(c) if c.is_ascii() => Action::Key(c as u8),
        _ => Action::None,
    }
}

/// Interactive loop over a real terminal: one keystroke at a time in raw
/// mode, with host commands reserved (see [`classify_key`]) and everything
/// else forwarded to the emulated keyboard.
///
/// Each iteration handles at most one terminal event and then runs one
/// bounded CPU batch, so a stream of keystrokes, pastes, or resize events
/// can never starve the emulated machine.
///
/// With a real terminal on stdout the machine is presented as its own fixed
/// 40x24 screen ([`draw_screen`]); with stdout redirected the same session
/// writes the plain character stream instead, so nothing puts screen
/// control sequences into a file or a pipe.
///
/// Host commands:
/// - Ctrl-C / Ctrl-D: quit.
/// - Ctrl-R: physical RESET (preserves RAM, the screen, and queued input;
///   see `Apple1::set_reset_line`), run under the session budget.
/// - Ctrl-L: CLEAR SCREEN — the Apple I keyboard's second pushbutton. It
///   blanks the machine's own screen and runs no cycle (see
///   `Apple1::clear_screen`).
/// - Ctrl-P: pause/resume. While paused no CPU batch runs, but keystrokes
///   are still queued and delivered once resumed — nothing is dropped, it
///   is only delayed. Explicit control commands still act while paused:
///   Ctrl-R and Ctrl-N run their (budgeted) cycles and leave the session
///   paused afterwards; pause suppresses free-running, not commands the
///   user explicitly asked for.
/// - Ctrl-N: recreate the machine from the original ROM/program bytes
///   (fresh RAM, fresh devices, blank screen, then RESET) and reboot —
///   distinct from RESET, which preserves RAM and the screen. The session's
///   cycle budget and counter carry over; only the machine is new.
///
/// Raw mode disables the terminal's own ISIG processing, so Ctrl-C/Ctrl-D
/// arrive here as ordinary keystrokes (handled above), never as a
/// delivered `SIGINT`. An externally delivered signal — `kill`, a
/// supervisor, or a closed terminal window sending `SIGHUP` — is unrelated
/// to that and is registered for before raw mode is enabled, so even a
/// signal during the boot batch still unwinds through [`TerminalGuard`].
fn run_interactive(
    session: &mut Session,
    rom: &[u8; 256],
    program: Option<&[u8]>,
) -> Result<StopReason, Box<dyn Error>> {
    let view = if io::stdout().is_terminal() {
        View::Grid
    } else {
        View::Stream
    };
    let terminated = Arc::new(AtomicBool::new(false));
    let _guard = TerminalGuard::enter(view, &terminated)?;
    let mut stdout = io::stdout();
    let mut paused = false;
    let mut status = String::new();
    // The first frame always draws: the boot screen is new information.
    let mut redraw = true;

    let stop = session.boot()?;
    present(view, session, &mut redraw, &mut stdout)?;
    if let Some(stop) = stop {
        draw_if_needed(view, session, &status, &mut redraw, &mut stdout)?;
        return Ok(stop);
    }

    loop {
        if terminated.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(StopReason::Signaled);
        }

        if event::poll(Duration::from_millis(15))? {
            match event::read()? {
                Event::Key(key) => match classify_key(key) {
                    Action::Quit => return Ok(StopReason::UserOrEof),
                    Action::Reset => {
                        let stop = session.reset()?;
                        present(view, session, &mut redraw, &mut stdout)?;
                        announce(view, "[RESET]", &mut status, &mut redraw, &mut stdout)?;
                        if let Some(stop) = stop {
                            draw_if_needed(view, session, &status, &mut redraw, &mut stdout)?;
                            return Ok(stop);
                        }
                    }
                    Action::ClearScreen => {
                        session.machine.clear_screen();
                        redraw = true;
                    }
                    Action::TogglePause => {
                        paused = !paused;
                        let label = if paused { "[PAUSED]" } else { "[RESUMED]" };
                        announce(view, label, &mut status, &mut redraw, &mut stdout)?;
                    }
                    Action::Recreate => {
                        session.machine = create_machine(rom, program)?;
                        announce(view, "[NEW MACHINE]", &mut status, &mut redraw, &mut stdout)?;
                        let stop = session.boot()?;
                        present(view, session, &mut redraw, &mut stdout)?;
                        redraw = true;
                        if let Some(stop) = stop {
                            draw_if_needed(view, session, &status, &mut redraw, &mut stdout)?;
                            return Ok(stop);
                        }
                    }
                    Action::Key(byte) => session.machine.type_char(byte),
                    Action::None => {}
                },
                Event::Resize(_, _) => {
                    if view == View::Grid {
                        // Drop whatever the old layout left behind, then
                        // redraw the machine's unchanged screen.
                        execute!(stdout, terminal::Clear(terminal::ClearType::All))?;
                        redraw = true;
                    }
                }
                Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
                Event::Paste(text) => {
                    for ch in text.chars().filter(char::is_ascii) {
                        session.machine.type_char(ch as u8);
                    }
                }
            }
        }

        let mut stop = None;
        if !paused {
            stop = session.advance(IDLE_BATCH_CPU_CYCLES)?;
            present(view, session, &mut redraw, &mut stdout)?;
        }
        draw_if_needed(view, session, &status, &mut redraw, &mut stdout)?;
        if let Some(stop) = stop {
            return Ok(stop);
        }
    }
}

/// Take this batch's finished characters. The stream view writes them; the
/// grid view only notes that the machine's screen changed, since the screen
/// itself is the authoritative content.
fn present(
    view: View,
    session: &mut Session,
    redraw: &mut bool,
    stdout: &mut impl Write,
) -> io::Result<()> {
    let output = session.machine.drain_output();
    match view {
        View::Grid => *redraw |= !output.is_empty(),
        View::Stream => print_output(&output, stdout)?,
    }
    Ok(())
}

/// Report a host command: the grid view puts it on the status row, the
/// stream view prints it inline.
fn announce(
    view: View,
    label: &str,
    status: &mut String,
    redraw: &mut bool,
    stdout: &mut impl Write,
) -> io::Result<()> {
    match view {
        View::Grid => {
            status.clear();
            status.push_str(label);
            *redraw = true;
            Ok(())
        }
        View::Stream => {
            write!(stdout, "\r\n{label}\r\n")?;
            stdout.flush()
        }
    }
}

/// Redraw the grid only when something actually changed; an idle batch with
/// no finished characters and no status change draws nothing.
fn draw_if_needed(
    view: View,
    session: &Session,
    status: &str,
    redraw: &mut bool,
    stdout: &mut impl Write,
) -> io::Result<()> {
    if view != View::Grid || !*redraw {
        return Ok(());
    }
    draw_screen(session.machine.display(), status, terminal::size()?, stdout)?;
    *redraw = false;
    Ok(())
}

fn print_output(output: &[u8], stdout: &mut impl Write) -> io::Result<()> {
    for &byte in output {
        if byte == b'\r' {
            stdout.write_all(b"\r\n")?;
        } else {
            stdout.write_all(&[byte])?;
        }
    }
    stdout.flush()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Minimal original ROM whose reset vector points at RAM $0000; the
    /// pinned Woz Monitor image is never needed to test the host's budget.
    fn test_rom(program_start: u16) -> [u8; 256] {
        let mut rom = [0u8; 256];
        rom[0xFC] = program_start as u8;
        rom[0xFD] = (program_start >> 8) as u8;
        rom
    }

    /// Spin forever at $0000 (JMP $0000), so the machine always has real
    /// work to do and never stops on its own.
    const SPIN: &[u8] = &[0x4C, 0x00, 0x00];

    fn session_with(max_cycles: Option<u64>, trace: TraceOptions) -> Session {
        let rom = test_rom(0x0000);
        let machine = create_machine(&rom, Some(SPIN)).unwrap();
        Session::new(machine, max_cycles, trace)
    }

    fn spinning_session(max_cycles: Option<u64>) -> Session {
        session_with(
            max_cycles,
            TraceOptions {
                instructions: false,
                bus: false,
                limit: 64,
            },
        )
    }

    #[test]
    fn a_zero_budget_runs_no_cycle_at_all() {
        let mut session = spinning_session(Some(0));
        assert!(matches!(
            session.boot().unwrap(),
            Some(StopReason::BudgetExceeded(0))
        ));
        assert_eq!(session.total_cpu_cycles, 0);
        assert_eq!(session.machine.cpu_cycles(), 0);
        assert_eq!(session.machine.master_ticks(), 0);
    }

    #[test]
    fn a_budget_stops_exactly_on_its_own_cycle_mid_reset() {
        for budget in [1u64, 2, 5] {
            let mut session = spinning_session(Some(budget));
            let stop = session.boot().unwrap();
            assert!(
                matches!(stop, Some(StopReason::BudgetExceeded(total)) if total == budget),
                "budget {budget} must stop at exactly {budget} cycles"
            );
            assert_eq!(session.total_cpu_cycles, budget);
        }
    }

    #[test]
    fn a_budget_exhausted_at_a_batch_boundary_stops_instead_of_running_on() {
        let mut session = spinning_session(Some(80));
        // RESET completes well inside 80 cycles, so boot's batch is what
        // runs out of budget.
        let stop = session.boot().unwrap();
        assert!(matches!(stop, Some(StopReason::BudgetExceeded(80))));

        // Asking for more must not execute anything else.
        assert!(session.tick().unwrap().is_none());
        assert!(matches!(
            session.advance(1_000).unwrap(),
            Some(StopReason::BudgetExceeded(80))
        ));
        assert_eq!(session.total_cpu_cycles, 80);
    }

    #[test]
    fn recreating_the_machine_keeps_the_session_budget() {
        let rom = test_rom(0x0000);
        // Enough for one full boot (RESET plus the 50 000-cycle boot
        // batch), nowhere near enough for two.
        let mut session = spinning_session(Some(60_000));
        assert!(session.boot().unwrap().is_none());
        let before = session.total_cpu_cycles;
        assert!(before >= BOOT_BATCH_CPU_CYCLES);

        session.machine = create_machine(&rom, Some(SPIN)).unwrap();
        assert_eq!(session.machine.cpu_cycles(), 0, "the machine is new");
        assert_eq!(session.machine.master_ticks(), 0, "the machine is new");
        assert_eq!(
            session.total_cpu_cycles, before,
            "the session's own count must not reset with the machine"
        );

        let stop = session.boot().unwrap();
        assert!(
            matches!(stop, Some(StopReason::BudgetExceeded(60_000))),
            "the session ceiling still applies after a recreate, got {stop:?}"
        );
    }

    #[test]
    fn an_unlimited_session_runs_every_requested_cycle() {
        let mut session = spinning_session(None);
        assert!(session.reset().unwrap().is_none());
        let after_reset = session.total_cpu_cycles;
        assert!(session.advance(1_000).unwrap().is_none());
        assert_eq!(session.total_cpu_cycles, after_reset + 1_000);
        assert_eq!(session.machine.cpu_cycles(), session.total_cpu_cycles);
    }

    #[test]
    fn both_trace_kinds_share_one_bounded_queue() {
        let mut session = session_with(
            None,
            TraceOptions {
                instructions: true,
                bus: true,
                limit: 4,
            },
        );
        assert!(session.reset().unwrap().is_none());
        assert!(session.advance(500).unwrap().is_none());

        assert_eq!(
            session.recent.len(),
            4,
            "the limit caps both kinds together, not each separately"
        );
        // The newest record belongs to the most recent CPU cycle, and a
        // bus record precedes the completion it belongs to.
        let index = session.total_cpu_cycles;
        let last = session.recent.back().unwrap();
        assert!(
            last.contains(&format!("C{index:06}")) || last.contains(&format!("total={index}")),
            "unexpected newest record: {last}"
        );
    }

    #[test]
    fn a_disabled_trace_records_nothing() {
        let mut session = spinning_session(None);
        assert!(session.reset().unwrap().is_none());
        assert!(session.advance(500).unwrap().is_none());
        assert!(session.recent.is_empty());
        assert!(!session.trace.enabled());
    }

    /// A machine whose screen already holds `text`, produced by a tiny RAM
    /// program that writes each byte to Port B and waits for the display's
    /// DA (PB7) handshake before the next one, exactly as the monitor's
    /// ECHO routine does, then paces itself so a CR's clear-to-EOL fill can
    /// finish before the next write. A zero byte ends the list.
    ///
    /// Reaching the screen through the machine keeps these host rendering
    /// tests independent of how the board implements the shift register,
    /// but it also means waiting for real video timing: the terminal takes
    /// a character only while the scanner clocks the cursor's slot, so each
    /// character costs about one frame (~238 000 master ticks, ~60
    /// characters/second, as on the real machine). The budget below is
    /// five frames, enough for the short strings used here. The session
    /// drives the reset and the advance, so the program starts the way a
    /// real run does.
    fn machine_showing(text: &[u8]) -> Apple1 {
        let mut program = vec![
            0xA2, 0x00, // LDX #$00
            0xA9, 0x7F, // LDA #$7F
            0x8D, 0x12, 0xD0, // STA $D012  (DDRB: PB0-PB6 out, PB7 = DA in)
            0xA9, 0xA7, // LDA #$A7
            0x8D, 0x13, 0xD0, // STA $D013  (CRB: CB2 write-strobe handshake)
            0xBD, 0x24, 0x00, // loop: LDA $0024,X
            0xF0, 0x10, // BEQ done ($0021)
            0x2C, 0x12, 0xD0, // wait: BIT $D012  (PB7 = DA; BIT preserves A)
            0x30, 0xFB, // BMI wait  (the monitor's ECHO spin)
            0x8D, 0x12, 0xD0, // STA $D012
            0xA0, 0xC8, // LDY #$C8
            0x88, // dly: DEY
            0xD0, 0xFD, // BNE dly  (~1 000 CPU cycles: outlast a CR fill)
            0xE8, // INX
            0xD0, 0xEB, // BNE loop ($000C)
            0x4C, 0x21, 0x00, // done: JMP done
        ];
        program.extend_from_slice(text);
        program.push(0x00);

        let mut session = Session::new(
            create_machine(&test_rom(0x0000), Some(&program)).unwrap(),
            None,
            TraceOptions {
                instructions: false,
                bus: false,
                limit: 64,
            },
        );
        assert!(session.reset().unwrap().is_none());
        assert!(session.advance(80_000).unwrap().is_none());
        session.machine
    }

    /// The text a frame wrote at each absolute position, as
    /// `(row, column, text)`; cursor show/hide sequences are not cells.
    fn frame_cells(frame: &[u8]) -> Vec<(u16, u16, String)> {
        let text = String::from_utf8(frame.to_vec()).expect("utf-8 frame");
        let mut cells = Vec::new();
        for chunk in text.split('\u{1b}').skip(1) {
            let Some(body) = chunk.strip_prefix('[') else {
                continue;
            };
            let Some((coordinates, rest)) = body.split_once('H') else {
                continue;
            };
            let Some((row, column)) = coordinates.split_once(';') else {
                continue;
            };
            if let (Ok(row), Ok(column)) = (row.parse::<u16>(), column.parse::<u16>()) {
                cells.push((row - 1, column - 1, rest.to_owned()));
            }
        }
        cells
    }

    const SHOW_CURSOR: &str = "\u{1b}[?25h";
    const HIDE_CURSOR: &str = "\u{1b}[?25l";

    #[test]
    fn a_wide_terminal_does_not_reflow_the_forty_column_grid() {
        let machine = machine_showing(b"AB\rC");
        let mut frame = Vec::new();
        draw_screen(machine.display(), "[PAUSED]", (80, 30), &mut frame).unwrap();
        let cells = frame_cells(&frame);

        let rows: Vec<u16> = cells.iter().map(|&(row, _, _)| row).collect();
        assert_eq!(
            rows,
            (0..=24).chain(std::iter::once(1)).collect::<Vec<u16>>(),
            "24 screen rows, the status row, then the cursor move"
        );
        for &(row, column, ref text) in &cells[..ROWS] {
            assert_eq!(column, 0, "row {row} must start at the left edge");
            assert_eq!(
                text.chars().count(),
                COLUMNS,
                "row {row} must be exactly 40 cells wide, not the host width"
            );
        }
        assert!(cells[0].2.starts_with("AB "));
        assert!(cells[1].2.starts_with("C "));
        assert_eq!(cells[ROWS].2.trim_end(), "[PAUSED]");
        // The machine cursor sits after 'C' on the second line.
        assert_eq!((cells[ROWS + 1].0, cells[ROWS + 1].1), (1, 1));
        let text = String::from_utf8(frame).unwrap();
        assert!(text.ends_with(SHOW_CURSOR));
    }

    #[test]
    fn a_twenty_four_row_window_draws_no_status_row() {
        let machine = machine_showing(b"A");
        let mut frame = Vec::new();
        draw_screen(machine.display(), "[RESET]", (40, 24), &mut frame).unwrap();
        let cells = frame_cells(&frame);
        assert!(
            cells.iter().all(|&(row, _, _)| usize::from(row) < ROWS),
            "nothing may be drawn outside the machine's own 24 rows"
        );
        assert!(
            !String::from_utf8(frame).unwrap().contains("[RESET]"),
            "a 24-row window has no room for a status marker"
        );
    }

    #[test]
    fn a_small_window_clips_the_grid_and_hides_an_offscreen_cursor() {
        // Cursor ends at row 1, column 1 — outside a 1x1 window.
        let machine = machine_showing(b"AB\rC");
        let mut frame = Vec::new();
        draw_screen(machine.display(), "[PAUSED]", (20, 10), &mut frame).unwrap();
        let cells = frame_cells(&frame);
        assert_eq!(cells.len(), 10 + 1, "10 visible rows plus the cursor move");
        for (_, _, text) in &cells[..10] {
            assert_eq!(text.chars().count(), 20, "rows are clipped, not reflowed");
        }

        let mut tiny = Vec::new();
        draw_screen(machine.display(), "", (1, 1), &mut tiny).unwrap();
        let tiny = String::from_utf8(tiny).unwrap();
        assert!(
            tiny.ends_with(HIDE_CURSOR),
            "a cursor outside the window must be hidden, got: {tiny:?}"
        );
        assert_eq!(
            frame_cells(tiny.as_bytes()).len(),
            1,
            "one visible cell row"
        );
    }

    #[test]
    fn a_zero_sized_window_emits_nothing() {
        let machine = machine_showing(b"A");
        for size in [(0, 24), (40, 0), (0, 0)] {
            let mut frame = Vec::new();
            draw_screen(machine.display(), "[RESET]", size, &mut frame).unwrap();
            assert!(
                frame.is_empty(),
                "size {size:?} must not emit any coordinate command"
            );
        }
    }

    #[test]
    fn a_non_printable_screen_code_is_drawn_as_a_blank() {
        // 0x7F is a screen cell the host terminal cannot render; the
        // carousel refuses only codes below $20, so $7F is the one stored
        // value outside printable ASCII the machine can hold. It still
        // takes its cell, and no non-printable byte may reach the terminal.
        let machine = machine_showing(&[b'A', 0x7F, b'B']);
        let mut frame = Vec::new();
        draw_screen(machine.display(), "", (40, 24), &mut frame).unwrap();
        let cells = frame_cells(&frame);
        assert_eq!(&cells[0].2[..3], "A B", "the cell is kept but blanked");
        assert!(
            !cells.iter().any(|(_, _, text)| text.contains('\u{1b}')
                || text.contains('\u{7}')
                || text.contains('\u{7f}')),
            "no non-printable machine byte may reach the terminal"
        );
    }
}
