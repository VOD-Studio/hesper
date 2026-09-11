//! Apple I CLI host: loads a machine, then drives it either through a real
//! terminal in raw mode (one key at a time, host commands reserved) or
//! through a plain line-buffered stdin loop when stdin is not a terminal
//! (scripts, pipes, CI smoke checks) — the integration tests in
//! `crates/cli/tests/apple1.rs` exercise the latter path.
//!
//! Every emulated cycle this host runs goes through [`Session::cycle`], so
//! `--max-cycles` is an exact ceiling on the whole session: boot, RESET,
//! recreate, and every input batch included.

use std::{
    error::Error,
    fmt::{self, Write as _},
    fs,
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
use hesper_apple1::{
    Apple1, Apple1Bus,
    machine::{RESET_COMPLETION_BUDGET, RESET_HOLD_CYCLES},
};
use hesper_cpu6502::{CpuError, Cycle, StepKind};
use sha2::{Digest, Sha256};

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

/// One CLI session over one machine at a time.
///
/// `total_cycles` is this session's accumulated cycle count, deliberately
/// separate from `Apple1::total_cycles`: recreating the machine (Ctrl-N)
/// gives a fresh machine counter, but the session ceiling keeps counting.
struct Session {
    machine: Apple1,
    total_cycles: u64,
    max_cycles: Option<u64>,
}

impl Session {
    fn new(machine: Apple1, max_cycles: Option<u64>) -> Self {
        Self {
            machine,
            total_cycles: 0,
            max_cycles,
        }
    }

    /// Cycles left in the session budget, or `None` when unlimited.
    fn remaining(&self) -> Option<u64> {
        self.max_cycles
            .map(|max| max.saturating_sub(self.total_cycles))
    }

    /// The only place this host runs an emulated cycle. Returns `Ok(None)`
    /// when the budget is exhausted, without touching the machine.
    ///
    /// A cycle that ends in a CPU error still counts: its bus access
    /// really happened (see `Apple1::cycle`).
    fn cycle(&mut self) -> Result<Option<Cycle>, CpuError> {
        if self.remaining() == Some(0) {
            return Ok(None);
        }
        self.total_cycles += 1;
        let cycle = self.machine.cycle()?;
        Ok(Some(cycle))
    }

    /// Run up to `requested` cycles. Stops the moment the budget is
    /// exhausted — including exactly at the end of the batch, so the host
    /// never accepts one more input or runs "one last batch" past the
    /// ceiling.
    fn advance(&mut self, requested: u64) -> Result<Option<StopReason>, CpuError> {
        for _ in 0..requested {
            if self.cycle()?.is_none() {
                return Ok(Some(StopReason::BudgetExceeded(self.total_cycles)));
            }
        }
        if self.remaining() == Some(0) {
            return Ok(Some(StopReason::BudgetExceeded(self.total_cycles)));
        }
        Ok(None)
    }

    /// Physical RESET driven cycle by cycle under the session budget: hold
    /// the line for [`RESET_HOLD_CYCLES`], release it, then run until the
    /// CPU reports its reset sequence complete.
    ///
    /// Running out of budget during the hold, the release, or the vector
    /// read is an ordinary budget stop: the reset line keeps whatever state
    /// it has and the sequence resumes if the host is given more budget.
    /// `CycleBudgetExceeded` is reserved for a CPU that never completes a
    /// fixed-length reset with budget to spare.
    fn reset(&mut self) -> Result<Option<StopReason>, CpuError> {
        self.machine.set_reset_line(true);
        for _ in 0..RESET_HOLD_CYCLES {
            if self.cycle()?.is_none() {
                return Ok(Some(StopReason::BudgetExceeded(self.total_cycles)));
            }
        }
        self.machine.set_reset_line(false);
        for _ in 0..RESET_COMPLETION_BUDGET {
            match self.cycle()? {
                None => return Ok(Some(StopReason::BudgetExceeded(self.total_cycles))),
                Some(cycle) => {
                    if let Some(step) = cycle.completed
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
            None => self.advance(BOOT_BATCH_CYCLES),
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
    trace_limit: usize,
) -> Result<(), Box<dyn Error>> {
    // Checked here too, not only in the argument parser: a direct library
    // call must not be able to install an unbounded diagnostic queue.
    if !TRACE_LIMIT_RANGE.contains(&trace_limit) {
        return Err("--trace-limit requires 1..4096".into());
    }
    if trace {
        eprintln!("[--trace not yet implemented for apple1]");
    }
    if bus_trace {
        eprintln!("[--bus-trace not yet implemented for apple1]");
    }

    // 1. Load and fully validate ROM and optional program (exact ROM
    // identity, program fits in RAM) before any machine state exists to
    // mutate; a bad path, wrong image, or oversized program fails here
    // with nothing partially applied and no cycle executed.
    let rom = load_rom(rom_path)?;
    let program = program_path.map(load_program).transpose()?;

    // 2. Build the machine and the session. Nothing has executed yet: with
    // `--max-cycles 0` the run stops here having reported zero cycles.
    let machine = create_machine(&rom, program.as_deref(), cycles_per_char)?;
    let mut session = Session::new(machine, max_cycles);

    let outcome = if io::stdin().is_terminal() {
        run_interactive(&mut session, &rom, program.as_deref(), cycles_per_char)
    } else {
        run_batch(&mut session)
    };

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

/// Construct a machine and load the optional program. Runs no cycle: RESET
/// is the session's job, so it stays inside the cycle budget. Shared by the
/// initial start and by the interactive loop's "recreate machine" command.
fn create_machine(
    rom: &[u8; 256],
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
    Ok(machine)
}

/// Non-interactive loop: read whole lines from stdin (scripts, pipes).
/// A real terminal never reaches this path (see `run_apple1`).
///
/// The idle batches below only mean "the host stops advancing for now to
/// read the next line": the emulated CPU is still running its polling
/// loop, and nothing here treats quiet output as a halted program.
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
                advance_and_print(session, Boot::No(IDLE_BATCH_CYCLES), &mut stdout)?
            {
                return Ok(stop);
            }
        }
        session.machine.type_char(b'\r');
        if let Some(stop) = advance_and_print(session, Boot::No(IDLE_BATCH_CYCLES), &mut stdout)? {
            return Ok(stop);
        }

        // Keep advancing until the machine has been quiet for a few
        // batches, then go read the next line.
        let mut quiet_batches = 0;
        while quiet_batches < 3 {
            let stop = session.advance(IDLE_BATCH_CYCLES * 5)?;
            let output = session.machine.drain_output();
            if output.is_empty() {
                quiet_batches += 1;
            } else {
                quiet_batches = 0;
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
/// Each iteration handles at most one terminal event and then runs one
/// bounded CPU batch, so a stream of keystrokes, pastes, or resize events
/// can never starve the emulated machine.
///
/// Host commands:
/// - Ctrl-C / Ctrl-D: quit.
/// - Ctrl-R: physical RESET (preserves RAM and queued input; see
///   `Apple1::set_reset_line`), run under the session budget.
/// - Ctrl-L: clear the *terminal's* visible screen (host presentation
///   only, no machine state and no cycle).
/// - Ctrl-P: pause/resume. While paused no CPU batch runs, but keystrokes
///   are still queued and delivered once resumed — nothing is dropped, it
///   is only delayed. Explicit control commands still act while paused:
///   Ctrl-R and Ctrl-N run their (budgeted) cycles and leave the session
///   paused afterwards; pause suppresses free-running, not commands the
///   user explicitly asked for.
/// - Ctrl-N: recreate the machine from the original ROM/program bytes
///   (fresh RAM, fresh devices, then RESET) and reboot — distinct from
///   RESET, which preserves RAM. The session's cycle budget and counter
///   carry over; only the machine is new.
///
/// Raw mode disables the terminal's own ISIG processing, so Ctrl-C/Ctrl-D
/// arrive here as ordinary keystrokes (handled above), never as a
/// delivered `SIGINT`. An externally delivered signal — `kill`, a
/// supervisor, or a closed terminal window sending `SIGHUP` — is
/// unrelated to that and is registered for before raw mode is enabled, so
/// even a signal during the boot batch still unwinds through `RawMode`
/// instead of leaving the real terminal stuck in raw mode.
fn run_interactive(
    session: &mut Session,
    rom: &[u8; 256],
    program: Option<&[u8]>,
    cycles_per_char: NonZeroU64,
) -> Result<StopReason, Box<dyn Error>> {
    let terminated = Arc::new(AtomicBool::new(false));
    for signal in [
        signal_hook::consts::SIGTERM,
        signal_hook::consts::SIGINT,
        signal_hook::consts::SIGHUP,
        signal_hook::consts::SIGQUIT,
    ] {
        signal_hook::flag::register(signal, Arc::clone(&terminated))?;
    }

    let _raw = RawMode::enable()?;
    let mut stdout = io::stdout();
    let mut paused = false;

    if let Some(stop) = advance_and_print(session, Boot::Yes, &mut stdout)? {
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
                        let output = session.machine.drain_output();
                        print_output(&output, &mut stdout)?;
                        write!(stdout, "\r\n[RESET]\r\n")?;
                        stdout.flush()?;
                        if let Some(stop) = stop {
                            return Ok(stop);
                        }
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
                        session.machine = create_machine(rom, program, cycles_per_char)?;
                        write!(stdout, "\r\n[NEW MACHINE]\r\n")?;
                        stdout.flush()?;
                        if let Some(stop) = advance_and_print(session, Boot::Yes, &mut stdout)? {
                            return Ok(stop);
                        }
                    }
                    Action::Key(byte) => session.machine.type_char(byte),
                    Action::None => {}
                },
                Event::Resize(_, _) | Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
                Event::Paste(text) => {
                    for ch in text.chars().filter(char::is_ascii) {
                        session.machine.type_char(ch.to_ascii_uppercase() as u8);
                    }
                }
            }
        }

        if !paused
            && let Some(stop) =
                advance_and_print(session, Boot::No(IDLE_BATCH_CYCLES), &mut stdout)?
        {
            return Ok(stop);
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

    fn spinning_session(max_cycles: Option<u64>) -> Session {
        let rom = test_rom(0x0000);
        let machine = create_machine(&rom, Some(SPIN), NonZeroU64::new(50).unwrap()).unwrap();
        Session::new(machine, max_cycles)
    }

    #[test]
    fn a_zero_budget_runs_no_cycle_at_all() {
        let mut session = spinning_session(Some(0));
        assert!(matches!(
            session.boot().unwrap(),
            Some(StopReason::BudgetExceeded(0))
        ));
        assert_eq!(session.total_cycles, 0);
        assert_eq!(session.machine.total_cycles(), 0);
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
            assert_eq!(session.total_cycles, budget);
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
        assert!(session.cycle().unwrap().is_none());
        assert!(matches!(
            session.advance(1_000).unwrap(),
            Some(StopReason::BudgetExceeded(80))
        ));
        assert_eq!(session.total_cycles, 80);
    }

    #[test]
    fn recreating_the_machine_keeps_the_session_budget() {
        let rom = test_rom(0x0000);
        // Enough for one full boot (RESET plus the 50 000-cycle boot
        // batch), nowhere near enough for two.
        let mut session = spinning_session(Some(60_000));
        assert!(session.boot().unwrap().is_none());
        let before = session.total_cycles;
        assert!(before >= BOOT_BATCH_CYCLES);

        session.machine = create_machine(&rom, Some(SPIN), NonZeroU64::new(50).unwrap()).unwrap();
        assert_eq!(session.machine.total_cycles(), 0, "the machine is new");
        assert_eq!(
            session.total_cycles, before,
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
        let after_reset = session.total_cycles;
        assert!(session.advance(1_000).unwrap().is_none());
        assert_eq!(session.total_cycles, after_reset + 1_000);
        assert_eq!(session.machine.total_cycles(), session.total_cycles);
    }
}
