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
    alternate: bool,
    wrap_disabled: bool,
    signals: Vec<SigId>,
}

impl TerminalGuard {
    pub(crate) fn enter(mode: TerminalMode, terminated: &Arc<AtomicBool>) -> io::Result<Self> {
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
            guard
                .signals
                .push(signal_hook::flag::register(signal, Arc::clone(terminated))?);
        }
        terminal::enable_raw_mode()?;
        guard.raw = true;
        if mode == TerminalMode::Tui {
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
}

impl Drop for TerminalGuard {
    fn drop(&mut self) {
        let mut stdout = io::stdout();
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
        for id in self.signals.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
