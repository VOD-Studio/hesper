//! Apple I CLI host: loads a machine, then drives it either through a real
//! terminal in raw mode (one key at a time, host commands reserved) or
//! through a plain line-buffered stdin loop when stdin is not a terminal
//! (scripts, pipes, CI smoke checks) — the integration tests in
//! `crates/cli/tests/apple1.rs` exercise the latter path.

use std::{
    error::Error,
    fmt, fs,
    io::{self, BufRead, IsTerminal, Write},
    num::NonZeroU64,
    sync::{Arc, atomic::AtomicBool},
    time::Duration,
};

use crossterm::{
    cursor,
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute, terminal,
};
use hesper_apple1::Apple1;

/// Cycles to run per idle tick while nothing new has arrived (a keystroke
/// in the interactive loop, or after a line in the batch loop) and no
/// budget check has fired. Large relative to a real 1 MHz Apple I because
/// this crate does not throttle to wall-clock time (see
/// `crates/apple1/src/lib.rs`); it only bounds how much host-side work
/// happens between input checks.
const IDLE_BATCH_CYCLES: u64 = 2_000;

/// Cycles to run immediately after RESET to reach the monitor's prompt
/// (or whatever the loaded program prints first) before accepting input.
const BOOT_BATCH_CYCLES: u64 = 50_000;

/// Why the run loop stopped. Distinguishes a normal, expected yield from a
/// user-requested stop, a budget limit, and a real error — `run_apple1`
/// reports each with its own message and (for `Error`) a non-zero exit via
/// `main`'s existing `Err` handling; the other three are graceful (exit 0).
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

/// Run the Apple I with the given ROM and optional program.
pub fn run_apple1(
    rom_path: &str,
    program_path: Option<&str>,
    cycles_per_char: NonZeroU64,
    max_cycles: Option<u64>,
    trace: bool,
    bus_trace: bool,
) -> Result<(), Box<dyn Error>> {
    if trace {
        eprintln!("[--trace not yet implemented for apple1]");
    }
    if bus_trace {
        eprintln!("[--bus-trace not yet implemented for apple1]");
    }

    // 1. Load ROM and optional program. Both are validated (exact size,
    // fits in RAM) before any machine state exists to mutate; a bad path
    // or oversized program fails here with nothing partially applied.
    let rom = fs::read(rom_path).map_err(|e| format!("cannot read ROM file '{rom_path}': {e}"))?;
    let program = program_path
        .map(|path| fs::read(path).map_err(|e| format!("cannot read program file '{path}': {e}")))
        .transpose()?;

    let mut machine = boot(&rom, program.as_deref(), cycles_per_char)?;
    let mut stdout = io::stdout();
    let output = machine.run_cycles(BOOT_BATCH_CYCLES)?;
    print_output(&output, &mut stdout)?;

    let stop = if io::stdin().is_terminal() {
        run_interactive(
            &mut machine,
            &rom,
            program.as_deref(),
            cycles_per_char,
            max_cycles,
        )?
    } else {
        run_batch(&mut machine, max_cycles)?
    };
    eprintln!("\n{stop}");
    Ok(())
}

/// Construct a machine, load the optional program, and drive it through a
/// real physical RESET. Shared by the initial boot and by the interactive
/// loop's "recreate machine" command.
fn boot(
    rom: &[u8],
    program: Option<&[u8]>,
    cycles_per_char: NonZeroU64,
) -> Result<Apple1, Box<dyn Error>> {
    let mut machine = Apple1::new(rom, Some(cycles_per_char))?;
    if let Some(bytes) = program {
        machine
            .bus_mut()
            .load_ram(0x0000, bytes)
            .map_err(|e| format!("cannot load program: {e}"))?;
    }
    machine.reset()?;
    Ok(machine)
}

/// Non-interactive loop: read whole lines from stdin (scripts, pipes).
/// Unchanged in spirit from the original implementation — a real terminal
/// never reaches this path (see `run_apple1`).
fn run_batch(machine: &mut Apple1, max_cycles: Option<u64>) -> Result<StopReason, Box<dyn Error>> {
    let stdin = io::stdin();
    let mut stdout = io::stdout();

    loop {
        if let Some(max) = max_cycles
            && machine.total_cycles() >= max
        {
            return Ok(StopReason::BudgetExceeded(machine.total_cycles()));
        }

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
            machine.type_char(ch as u8);
            let output = machine.run_cycles(IDLE_BATCH_CYCLES)?;
            print_output(&output, &mut stdout)?;
        }
        machine.type_char(b'\r');
        let output = machine.run_cycles(IDLE_BATCH_CYCLES)?;
        print_output(&output, &mut stdout)?;

        // Run until output stops for a while (machine is idle, polling
        // for the next line), still honoring the cycle budget.
        let mut idle_batches = 0;
        loop {
            let output = machine.run_cycles(IDLE_BATCH_CYCLES * 5)?;
            if output.is_empty() {
                idle_batches += 1;
                if idle_batches >= 3 {
                    break;
                }
            } else {
                idle_batches = 0;
                print_output(&output, &mut stdout)?;
            }
            if let Some(max) = max_cycles
                && machine.total_cycles() >= max
            {
                return Ok(StopReason::BudgetExceeded(machine.total_cycles()));
            }
        }
    }
}

/// RAII guard: restores the terminal's cooked mode on every exit path
/// (normal return, `?` early return, or panic), matching the
/// `enable_raw_mode`/`disable_raw_mode` pairing crossterm expects.
struct RawMode;

impl RawMode {
    fn enable() -> io::Result<Self> {
        terminal::enable_raw_mode()?;
        Ok(Self)
    }
}

impl Drop for RawMode {
    fn drop(&mut self) {
        let _ = terminal::disable_raw_mode();
    }
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
/// The Apple I keyboard is uppercase-only 7-bit ASCII with no
/// backspace/erase key of its own: the Woz Monitor recognizes underscore
/// (`_`, `$5F`) as its line-edit backspace and ESC (`$1B`) to cancel a
/// line, and only ever compares against uppercase letters. A modern
/// keyboard's Backspace/Esc/lowercase-letter keys are translated to match
/// what a real Apple I keyboard would have sent, so software written
/// against the real hardware behaves the same from a modern terminal.
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
        KeyCode::Char(c) if c.is_ascii() => Action::Key(c.to_ascii_uppercase() as u8),
        _ => Action::None,
    }
}

/// Interactive loop over a real terminal: one keystroke at a time in raw
/// mode, with host commands reserved (see [`classify_key`]) and everything
/// else forwarded to the emulated keyboard.
///
/// Host commands:
/// - Ctrl-C / Ctrl-D: quit.
/// - Ctrl-R: physical RESET (preserves RAM and queued input; see
///   `Apple1::reset`).
/// - Ctrl-L: clear the *terminal's* visible screen. The real Apple I has
///   no clear-screen hardware input at all (see
///   `crates/apple1/src/lib.rs`); this is host presentation only and does
///   not touch machine state.
/// - Ctrl-P: pause/resume. While paused the machine does not advance (no
///   cycles run), but keystrokes are still queued and delivered once
///   resumed — nothing is dropped, it is only delayed.
/// - Ctrl-N: recreate the machine from the original ROM/program bytes
///   (fresh RAM, fresh devices, then RESET) and reboot — distinct from
///   RESET, which preserves RAM.
///
/// Raw mode disables the terminal's own ISIG processing, so Ctrl-C/Ctrl-D
/// arrive here as ordinary keystrokes (handled above), never as a
/// delivered `SIGINT`. An externally delivered signal — `kill`, a
/// supervisor, or a closed terminal window sending `SIGHUP` — is
/// unrelated to that and is registered for separately below so `RawMode`
/// still runs on the way out instead of leaving the real terminal stuck
/// in raw mode.
fn run_interactive(
    machine: &mut Apple1,
    rom: &[u8],
    program: Option<&[u8]>,
    cycles_per_char: NonZeroU64,
    max_cycles: Option<u64>,
) -> Result<StopReason, Box<dyn Error>> {
    let _raw = RawMode::enable()?;
    let mut stdout = io::stdout();
    let mut paused = false;

    let terminated = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGQUIT,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&terminated))?;
    }

    loop {
        if terminated.load(std::sync::atomic::Ordering::Relaxed) {
            return Ok(StopReason::Signaled);
        }
        if let Some(max) = max_cycles
            && machine.total_cycles() >= max
        {
            return Ok(StopReason::BudgetExceeded(machine.total_cycles()));
        }

        if event::poll(Duration::from_millis(15))? {
            match event::read()? {
                Event::Key(key) => match classify_key(key) {
                    Action::Quit => return Ok(StopReason::UserOrEof),
                    Action::Reset => {
                        machine.reset()?;
                        write!(stdout, "\r\n[RESET]\r\n")?;
                        stdout.flush()?;
                    }
                    Action::ClearScreen => {
                        execute!(
                            stdout,
                            terminal::Clear(terminal::ClearType::All),
                            cursor::MoveTo(0, 0)
                        )?;
                    }
                    Action::TogglePause => {
                        paused = !paused;
                        let label = if paused { "PAUSED" } else { "RESUMED" };
                        write!(stdout, "\r\n[{label}]\r\n")?;
                        stdout.flush()?;
                    }
                    Action::Recreate => {
                        *machine = boot(rom, program, cycles_per_char)?;
                        write!(stdout, "\r\n[NEW MACHINE]\r\n")?;
                        stdout.flush()?;
                        let output = machine.run_cycles(BOOT_BATCH_CYCLES)?;
                        print_output(&output, &mut stdout)?;
                    }
                    Action::Key(byte) => machine.type_char(byte),
                    Action::None => {}
                },
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
                Event::Paste(text) => {
                    for ch in text.chars().filter(char::is_ascii) {
                        machine.type_char(ch.to_ascii_uppercase() as u8);
                    }
                }
            }
        } else if !paused {
            let output = machine.run_cycles(IDLE_BATCH_CYCLES)?;
            print_output(&output, &mut stdout)?;
        }
    }
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
