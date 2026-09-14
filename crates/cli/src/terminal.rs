//! The single owner of terminal capabilities for each interactive host.

use std::{
    io::{self, IsTerminal},
    sync::{Arc, atomic::AtomicBool},
};

use crossterm::{cursor, event, execute, terminal};
use signal_hook::SigId;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TerminalMode {
    Tui,
    RawStream,
}

pub(crate) struct TerminalGuard {
    raw: bool,
    paste: bool,
    mouse: bool,
    alternate: bool,
    wrap_disabled: bool,
    color_disabled: Option<bool>,
    signals: Vec<SigId>,
}

impl TerminalGuard {
    pub(crate) fn enter(mode: TerminalMode, terminated: &Arc<AtomicBool>) -> io::Result<Self> {
        let mut guard = Self {
            raw: false,
            paste: false,
            mouse: false,
            alternate: false,
            wrap_disabled: false,
            color_disabled: None,
            signals: Vec::new(),
        };
        for signal in [
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGHUP,
            signal_hook::consts::SIGQUIT,
        ] {
            guard
                .signals
                .push(signal_hook::flag::register(signal, Arc::clone(terminated))?);
        }
        terminal::enable_raw_mode()?;
        guard.raw = true;
        if mode == TerminalMode::Tui {
            // The TUI resolves NO_COLOR together with explicit preferences.
            // Let its palette reach the backend, including Reset when the
            // user switches back to mono. Restore the library policy on exit.
            guard.color_disabled = Some(crossterm::style::Colored::ansi_color_disabled_memoized());
            crossterm::style::force_color_output(true);
            let mut stdout = io::stdout();
            debug_assert!(stdout.is_terminal());
            execute!(stdout, event::EnableBracketedPaste)?;
            guard.paste = true;
            execute!(stdout, terminal::EnterAlternateScreen)?;
            guard.alternate = true;
            execute!(
                stdout,
                terminal::DisableLineWrap,
                cursor::Hide,
                terminal::Clear(terminal::ClearType::All),
                cursor::MoveTo(0, 0)
            )?;
            guard.wrap_disabled = true;
        }
        Ok(guard)
    }

    pub(crate) fn set_mouse(&mut self, enabled: bool) -> io::Result<()> {
        if self.mouse != enabled {
            // Track acquisition before writing so Drop also cleans up a
            // partially written enable sequence if the terminal fails.
            if enabled {
                self.mouse = true;
                execute!(io::stdout(), event::EnableMouseCapture)?;
            } else {
                execute!(io::stdout(), event::DisableMouseCapture)?;
                self.mouse = false;
            }
        }
        Ok(())
    }
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
        if self.mouse {
            let _ = execute!(stdout, event::DisableMouseCapture);
        }
        if self.alternate {
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
        if let Some(disabled) = self.color_disabled {
            crossterm::style::Colored::set_ansi_color_disabled(disabled);
        }
        for id in self.signals.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
