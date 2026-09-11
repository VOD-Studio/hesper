//! Ratatui host for the two bounded emulators.  It deliberately owns no
//! machine behavior: Apple-1 execution still goes through `Session`.

use std::{
    collections::VecDeque,
    error::Error,
    fs,
    io::{self, IsTerminal},
    path::{Path, PathBuf},
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

use crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use hesper_apple1::display::{COLUMNS, Display, ROWS};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Alignment, Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    apple1::{Session, StopReason, TraceOptions, create_machine, load_program, load_rom},
    config::{self, AppConfig, BorderStyle, ColorMode, ScreenColor},
    format_registers, run_demo_with_trace,
    terminal::{TerminalGuard, TerminalMode},
};

const IDLE_BATCH_CPU_CYCLES: u64 = 2_000;
const INPUT_LIMIT: usize = 4_096;
const MIN_WIDTH: u16 = 44;
const MIN_HEIGHT: u16 = 30;

/// Options accepted by `hesper apple1` when that command opens the TUI.
#[derive(Debug, Clone, Default)]
pub struct Apple1Launch {
    pub rom: Option<PathBuf>,
    pub program: Option<PathBuf>,
    pub max_cycles: Option<u64>,
    pub trace: bool,
    pub bus_trace: bool,
    pub trace_limit: usize,
    /// Set by the CLI for `hesper apple1`; no-argument TUI startup leaves it
    /// false so the center is always shown first.
    pub direct: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Page {
    Center,
    Apple1,
    Demo,
    Config,
    Info,
    Settings,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum MenuKind {
    Emulator,
    Session,
    Display,
    Help,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PathTarget {
    Rom,
    Program,
}

#[derive(Debug)]
enum Overlay {
    Menu {
        kind: MenuKind,
        selected: usize,
    },
    RebootConfirm,
    QuitConfirm,
    Error(String),
    Browser {
        target: PathTarget,
        directory: PathBuf,
        entries: Vec<DirEntry>,
        selected: usize,
        error: Option<String>,
    },
}

#[derive(Debug)]
struct DirEntry {
    path: PathBuf,
    name: String,
    directory: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ConfigFocus {
    Rom,
    Program,
    Validate,
    Launch,
    Cancel,
}

#[derive(Debug)]
struct ConfigForm {
    rom: String,
    program: String,
    focus: ConfigFocus,
    status: Option<String>,
}

#[derive(Debug)]
struct Resources {
    rom: [u8; 256],
    program: Option<Vec<u8>>,
    rom_path: PathBuf,
}

#[derive(Debug)]
struct DemoView {
    lines: Vec<String>,
}

struct App {
    page: Page,
    previous_page: Page,
    overlay: Option<Overlay>,
    config: AppConfig,
    config_path: Option<PathBuf>,
    config_warning: Option<String>,
    selected_machine: usize,
    form: ConfigForm,
    session: Option<Session>,
    resources: Option<Resources>,
    launch: Apple1Launch,
    user_paused: bool,
    faulted: bool,
    fatal_error: Option<String>,
    input: VecDeque<u8>,
    notification: Option<(String, Instant)>,
    demo: Option<DemoView>,
    settings_row: usize,
    terminal_size: (u16, u16),
    dirty: bool,
    exit: bool,
    stop_message: Option<String>,
}

impl App {
    fn new(launch: Option<Apple1Launch>) -> Self {
        let config_path = config::default_config_path().ok();
        let (config, config_warning) = match config_path.as_deref().map(config::load) {
            Some(Ok(value)) => (value, None),
            Some(Err(error)) => (AppConfig::default(), Some(format!("配置未载入：{error}"))),
            None => (
                AppConfig::default(),
                Some("无法定位配置目录；本次使用默认设置".into()),
            ),
        };
        let launch = launch.unwrap_or_default();
        let rom = launch.rom.as_ref().map_or_else(
            || {
                config
                    .apple1
                    .rom_path
                    .as_ref()
                    .map_or_else(String::new, |path| path.display().to_string())
            },
            |path| path.display().to_string(),
        );
        let program = launch
            .program
            .as_ref()
            .map_or_else(String::new, |path| path.display().to_string());
        let selected_machine = usize::from(config.last_selected == "demo");
        let direct = launch.direct;
        Self {
            page: if direct { Page::Config } else { Page::Center },
            previous_page: Page::Center,
            overlay: None,
            config,
            config_path,
            config_warning,
            selected_machine,
            form: ConfigForm {
                rom,
                program,
                focus: ConfigFocus::Rom,
                status: None,
            },
            session: None,
            resources: None,
            launch,
            user_paused: false,
            faulted: false,
            fatal_error: None,
            input: VecDeque::new(),
            notification: None,
            demo: None,
            settings_row: 0,
            terminal_size: (0, 0),
            dirty: true,
            exit: false,
            stop_message: None,
        }
    }

    fn has_active_session(&self) -> bool {
        self.session.is_some()
    }

    fn normal_size(&self) -> bool {
        self.terminal_size.0 >= MIN_WIDTH && self.terminal_size.1 >= MIN_HEIGHT
    }

    fn running(&self) -> bool {
        self.session.is_some()
            && self.page == Page::Apple1
            && self.overlay.is_none()
            && !self.user_paused
            && !self.faulted
            && self.normal_size()
            && !self.exit
    }

    fn notify(&mut self, text: impl Into<String>) {
        self.notification = Some((text.into(), Instant::now() + Duration::from_millis(2_500)));
        self.dirty = true;
    }

    fn transient_status(&mut self) -> Option<String> {
        match &self.notification {
            Some((text, until)) if Instant::now() < *until => Some(text.clone()),
            Some(_) => {
                self.notification = None;
                self.dirty = true;
                None
            }
            None => None,
        }
    }

    fn error(&mut self, error: impl Into<String>) {
        self.overlay = Some(Overlay::Error(error.into()));
        self.dirty = true;
    }

    fn text_path(text: &str, what: &str) -> Result<PathBuf, String> {
        if text.is_empty() {
            Err(format!("请先输入{what}路径"))
        } else {
            Ok(PathBuf::from(text))
        }
    }

    fn validate_resources(&self) -> Result<Resources, String> {
        let rom_path = Self::text_path(&self.form.rom, "ROM")?;
        let rom_text = rom_path
            .to_str()
            .ok_or("ROM 路径不是 UTF-8，不能用于此 TUI 表单")?;
        let rom = load_rom(rom_text).map_err(|error| error.to_string())?;
        let program_path = if self.form.program.is_empty() {
            None
        } else {
            Some(Self::text_path(&self.form.program, "程序")?)
        };
        let program = program_path
            .as_ref()
            .map(|path| -> Result<Vec<u8>, String> {
                let text = path
                    .to_str()
                    .ok_or_else(|| "程序路径不是 UTF-8，不能用于此 TUI 表单".to_owned())?;
                load_program(text).map_err(|error| error.to_string())
            })
            .transpose()?;
        let absolute_rom = fs::canonicalize(&rom_path)
            .map_err(|error| format!("无法规范化 ROM 路径 '{}': {error}", rom_path.display()))?;
        Ok(Resources {
            rom,
            program,
            rom_path: absolute_rom,
        })
    }

    fn save_config(&mut self) -> Result<(), String> {
        let path = self
            .config_path
            .as_deref()
            .ok_or("无法定位配置目录；设置仅在本次运行有效")?;
        config::save(path, &self.config)
    }

    fn validate_and_save(&mut self) -> Option<Resources> {
        match self.validate_resources() {
            Ok(resources) => {
                self.config.apple1.rom_path = Some(resources.rom_path.clone());
                match self.save_config() {
                    Ok(()) => self.form.status = Some("ROM 已校验，路径已保存".into()),
                    Err(error) => self.form.status = Some(format!("配置未保存：{error}")),
                }
                self.dirty = true;
                Some(resources)
            }
            Err(error) => {
                self.form.status = Some(error);
                self.dirty = true;
                None
            }
        }
    }

    fn start_configured(&mut self, save_path: bool) {
        let resources = if save_path {
            self.validate_and_save()
        } else {
            match self.validate_resources() {
                Ok(resources) => Some(resources),
                Err(error) => {
                    self.form.status = Some(error);
                    None
                }
            }
        };
        let Some(resources) = resources else {
            return;
        };
        let machine = match create_machine(&resources.rom, resources.program.as_deref()) {
            Ok(machine) => machine,
            Err(error) => {
                self.form.status = Some(error.to_string());
                return;
            }
        };
        let trace = TraceOptions::new(
            self.launch.trace,
            self.launch.bus_trace,
            self.launch.trace_limit.max(1),
        );
        let mut session = Session::new(machine, self.launch.max_cycles, trace);
        match session.boot() {
            Ok(stop) => {
                let _ = session.machine_mut().drain_output();
                self.session = Some(session);
                self.resources = Some(resources);
                self.page = Page::Apple1;
                self.previous_page = Page::Center;
                self.user_paused = false;
                self.faulted = false;
                self.fatal_error = None;
                self.input.clear();
                self.notify("[NEW MACHINE] 已启动");
                if let Some(stop) = stop {
                    self.stop(stop);
                }
            }
            Err(error) => {
                let message = format!("Apple-1 启动失败：{error}");
                self.fatal_error = Some(message.clone());
                self.error(message);
            }
        }
    }

    fn stop(&mut self, stop: StopReason) {
        self.stop_message = Some(stop.to_string());
        self.exit = true;
        self.dirty = true;
    }

    fn reset(&mut self) {
        let result = self.session.as_mut().map(Session::reset);
        match result {
            Some(Ok(Some(stop))) => self.stop(stop),
            Some(Ok(None)) => {
                self.faulted = false;
                self.fatal_error = None;
                self.notify("[RESET] 已完成 · RAM / 屏幕保留");
            }
            Some(Err(error)) => {
                self.faulted = true;
                let message = format!("RESET 失败：{error}");
                self.fatal_error = Some(message.clone());
                self.error(message);
            }
            None => {}
        }
    }

    fn clear_screen(&mut self) {
        if let Some(session) = &mut self.session {
            session.machine_mut().clear_screen();
            self.notify("[CLEAR] 已清屏");
        }
    }

    fn reboot(&mut self) {
        let (Some(resources), Some(session)) = (&self.resources, &mut self.session) else {
            return;
        };
        let machine = match create_machine(&resources.rom, resources.program.as_deref()) {
            Ok(machine) => machine,
            Err(error) => {
                self.error(format!("无法重新上电：{error}"));
                return;
            }
        };
        session.recreate(machine);
        self.input.clear();
        match session.boot() {
            Ok(Some(stop)) => self.stop(stop),
            Ok(None) => {
                self.faulted = false;
                self.fatal_error = None;
                self.notify("[NEW MACHINE] 已重新上电");
            }
            Err(error) => {
                self.faulted = true;
                let message = format!("重新上电失败：{error}");
                self.fatal_error = Some(message.clone());
                self.error(message);
            }
        }
    }

    fn return_to_center(&mut self) {
        self.page = Page::Center;
        self.overlay = None;
        self.dirty = true;
    }

    fn open_help(&mut self) {
        self.previous_page = self.page;
        self.page = Page::Help;
        self.dirty = true;
    }

    fn run_demo(&mut self) {
        self.page = Page::Demo;
        self.demo = Some(
            match run_demo_with_trace(crate::DEFAULT_MAX_STEPS, |_, _| {}) {
                Ok(result) => DemoView {
                    lines: vec![
                        "6502 内置演示已完成".into(),
                        "$0200..$0209: 0 1 2 3 4 5 6 7 8 9".into(),
                        format_registers(result.registers),
                        format!(
                            "{} instructions, {} total cycles",
                            result.steps,
                            result.instruction_cycles + result.reset_cycles
                        ),
                    ],
                },
                Err(error) => DemoView {
                    lines: vec![format!("演示失败：{error}")],
                },
            },
        );
        self.dirty = true;
    }

    fn open_browser(&mut self, target: PathTarget) {
        let text = match target {
            PathTarget::Rom => &self.form.rom,
            PathTarget::Program => &self.form.program,
        };
        let directory = Path::new(text)
            .parent()
            .filter(|path| path.is_dir())
            .map(Path::to_path_buf)
            .or_else(|| std::env::current_dir().ok())
            .unwrap_or_else(|| PathBuf::from("."));
        self.overlay = Some(browser(target, directory));
        self.dirty = true;
    }

    fn feed_input(&mut self) {
        let Some(session) = &mut self.session else {
            return;
        };
        if !session.machine().keyboard().has_pending()
            && let Some(byte) = self.input.pop_front()
        {
            session.machine_mut().type_char(byte);
        }
    }

    fn queue_machine_bytes(&mut self, bytes: Vec<u8>, skipped: usize) {
        if !self.normal_size() {
            self.notify("窗口过小，无法输入机器键盘");
            return;
        }
        if self.input.len() + bytes.len() > INPUT_LIMIT {
            self.notify(format!(
                "输入队列已满（上限 {INPUT_LIMIT}），本次输入未接收"
            ));
            return;
        }
        self.input.extend(bytes);
        if skipped > 0 {
            self.notify(format!("已跳过 {skipped} 个不支持的粘贴字符"));
        }
        self.dirty = true;
    }

    fn handle_paste(&mut self, text: String) {
        if self.page != Page::Apple1 || self.overlay.is_some() {
            return;
        }
        let mut bytes = Vec::new();
        let mut skipped = 0;
        let mut chars = text.chars().peekable();
        while let Some(ch) = chars.next() {
            match ch {
                '\r' => {
                    if chars.peek() == Some(&'\n') {
                        chars.next();
                    }
                    bytes.push(b'\r');
                }
                '\n' => bytes.push(b'\r'),
                '\t' | '\x1b' => skipped += 1,
                c if c.is_ascii_graphic() || c == ' ' => bytes.push(c as u8),
                _ => skipped += 1,
            }
        }
        self.queue_machine_bytes(bytes, skipped);
    }

    fn advance(&mut self) {
        let _ = self.transient_status();
        if !self.running() {
            return;
        }
        self.feed_input();
        let result = self.session.as_mut().map(|session| {
            let result = session.advance(IDLE_BATCH_CPU_CYCLES);
            let _ = session.machine_mut().drain_output();
            result
        });
        match result {
            Some(Ok(Some(stop))) => self.stop(stop),
            Some(Ok(None)) => self.dirty = true,
            Some(Err(error)) => {
                self.faulted = true;
                let message = format!("CPU 错误：{error}");
                self.fatal_error = Some(message.clone());
                self.error(message);
            }
            None => {}
        }
    }

    fn handle_event(&mut self, event: Event) {
        match event {
            Event::Resize(width, height) => {
                self.terminal_size = (width, height);
                self.dirty = true;
            }
            Event::Paste(text) => self.handle_paste(text),
            Event::Key(key) => self.handle_key(key),
            Event::FocusGained | Event::FocusLost | Event::Mouse(_) => {}
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        if self.overlay.is_some() {
            self.handle_overlay_key(key);
            return;
        }
        if !self.normal_size() {
            if is_quit(key) {
                if self.has_active_session() {
                    self.overlay = Some(Overlay::QuitConfirm);
                } else {
                    self.exit = true;
                }
            }
            return;
        }
        if is_quit(key) {
            if self.has_active_session() {
                self.overlay = Some(Overlay::QuitConfirm);
            } else {
                self.exit = true;
            }
            self.dirty = true;
            return;
        }
        if key.code == KeyCode::F(10) {
            self.overlay = Some(Overlay::Menu {
                kind: MenuKind::Emulator,
                selected: 0,
            });
            self.dirty = true;
            return;
        }
        match self.page {
            Page::Center => self.center_key(key),
            Page::Apple1 => self.apple_key(key),
            Page::Config => self.config_key(key),
            Page::Demo => match key.code {
                KeyCode::Char('r') | KeyCode::Char('R') => self.run_demo(),
                KeyCode::Esc => self.return_to_center(),
                _ => {}
            },
            Page::Info | Page::Help => {
                if key.code == KeyCode::Esc {
                    self.page = self.previous_page;
                    self.dirty = true;
                }
            }
            Page::Settings => self.settings_key(key),
        }
    }

    fn center_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Up => {
                self.selected_machine = self.selected_machine.saturating_sub(1);
            }
            KeyCode::Down => {
                self.selected_machine = (self.selected_machine + 1).min(1);
            }
            KeyCode::Enter => {
                if self.selected_machine == 0 {
                    if self.session.is_some() {
                        self.page = Page::Apple1;
                    } else if self.form.rom.is_empty() {
                        self.page = Page::Config;
                    } else {
                        self.start_configured(true);
                    }
                } else {
                    self.run_demo();
                }
            }
            KeyCode::Char('c') | KeyCode::Char('C') => self.page = Page::Config,
            KeyCode::Char('i') | KeyCode::Char('I') => self.page = Page::Info,
            KeyCode::F(1) => self.open_help(),
            _ => {}
        }
        self.dirty = true;
    }

    fn apple_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::F(1) {
            self.open_help();
            return;
        }
        if key.code == KeyCode::F(2) {
            self.overlay = Some(Overlay::Menu {
                kind: MenuKind::Session,
                selected: 0,
            });
            return;
        }
        if key.code == KeyCode::F(3) {
            self.config.ui.sidebar = !self.config.ui.sidebar;
            self.notify(if self.config.ui.sidebar {
                "侧栏已显示"
            } else {
                "侧栏已隐藏"
            });
            return;
        }
        if key.modifiers.contains(KeyModifiers::CONTROL) {
            match key.code {
                KeyCode::Char('p') | KeyCode::Char('P') => {
                    self.user_paused = !self.user_paused;
                    self.notify(if self.user_paused {
                        "[PAUSED] 已暂停"
                    } else {
                        "[RESUMED] 已继续"
                    });
                }
                KeyCode::Char('r') | KeyCode::Char('R') => self.reset(),
                KeyCode::Char('l') | KeyCode::Char('L') => self.clear_screen(),
                KeyCode::Char('n') | KeyCode::Char('N') => {
                    self.overlay = Some(Overlay::RebootConfirm)
                }
                _ => {}
            }
            return;
        }
        let byte = match key.code {
            KeyCode::Enter => Some(b'\r'),
            KeyCode::Backspace => Some(b'_'),
            KeyCode::Esc => Some(0x1b),
            KeyCode::Char(c) if c.is_ascii_graphic() || c == ' ' => Some(c as u8),
            _ => None,
        };
        if let Some(byte) = byte {
            self.queue_machine_bytes(vec![byte], 0);
        }
    }

    fn config_key(&mut self, key: KeyEvent) {
        if key.code == KeyCode::Esc {
            self.return_to_center();
            return;
        }
        if key.code == KeyCode::F(4) {
            self.open_browser(if self.form.focus == ConfigFocus::Program {
                PathTarget::Program
            } else {
                PathTarget::Rom
            });
            return;
        }
        if key.code == KeyCode::Tab {
            self.form.focus = match self.form.focus {
                ConfigFocus::Rom => ConfigFocus::Program,
                ConfigFocus::Program => ConfigFocus::Validate,
                ConfigFocus::Validate => ConfigFocus::Launch,
                ConfigFocus::Launch => ConfigFocus::Cancel,
                ConfigFocus::Cancel => ConfigFocus::Rom,
            };
            self.dirty = true;
            return;
        }
        match self.form.focus {
            ConfigFocus::Rom | ConfigFocus::Program => {
                let text = if self.form.focus == ConfigFocus::Rom {
                    &mut self.form.rom
                } else {
                    &mut self.form.program
                };
                match key.code {
                    KeyCode::Backspace => {
                        text.pop();
                    }
                    KeyCode::Enter => {
                        self.form.focus = if self.form.focus == ConfigFocus::Rom {
                            ConfigFocus::Program
                        } else {
                            ConfigFocus::Validate
                        };
                    }
                    KeyCode::Char(ch) => text.push(ch),
                    _ => {}
                }
            }
            ConfigFocus::Validate if key.code == KeyCode::Enter => {
                let _ = self.validate_and_save();
            }
            ConfigFocus::Launch if key.code == KeyCode::Enter => self.start_configured(true),
            ConfigFocus::Cancel if key.code == KeyCode::Enter => self.return_to_center(),
            _ => {}
        }
        self.dirty = true;
    }

    fn settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.page = self.previous_page;
            }
            KeyCode::Up => self.settings_row = self.settings_row.saturating_sub(1),
            KeyCode::Down => self.settings_row = (self.settings_row + 1).min(4),
            KeyCode::Enter => match self.settings_row {
                0 => {
                    self.config.ui.screen_color = match self.config.ui.screen_color {
                        ScreenColor::Green => ScreenColor::Amber,
                        ScreenColor::Amber => ScreenColor::White,
                        ScreenColor::White => ScreenColor::Green,
                    }
                }
                1 => self.config.ui.sidebar = !self.config.ui.sidebar,
                2 => {
                    self.config.ui.border = match self.config.ui.border {
                        BorderStyle::Rounded => BorderStyle::Ascii,
                        BorderStyle::Ascii => BorderStyle::Rounded,
                    }
                }
                3 => {
                    self.config.ui.color_mode = match self.config.ui.color_mode {
                        ColorMode::Auto => ColorMode::Truecolor,
                        ColorMode::Truecolor => ColorMode::Ansi256,
                        ColorMode::Ansi256 => ColorMode::Mono,
                        ColorMode::Mono => ColorMode::Auto,
                    }
                }
                4 => match self.save_config() {
                    Ok(()) => self.notify("设置已保存"),
                    Err(error) => self.error(format!("配置未保存：{error}")),
                },
                _ => {}
            },
            _ => {}
        }
        self.dirty = true;
    }

    fn handle_overlay_key(&mut self, key: KeyEvent) {
        let Some(overlay) = self.overlay.take() else {
            return;
        };
        match overlay {
            Overlay::Error(_) => {
                if key.code == KeyCode::Esc || key.code == KeyCode::Enter {
                    self.dirty = true;
                } else {
                    self.overlay = Some(overlay);
                }
            }
            Overlay::QuitConfirm => match key.code {
                KeyCode::Enter => self.exit = true,
                KeyCode::Esc => {}
                _ => self.overlay = Some(Overlay::QuitConfirm),
            },
            Overlay::RebootConfirm => match key.code {
                KeyCode::Enter => self.reboot(),
                KeyCode::Esc => {}
                _ => self.overlay = Some(Overlay::RebootConfirm),
            },
            Overlay::Menu {
                mut kind,
                mut selected,
            } => {
                let count = menu_entries(kind).len();
                match key.code {
                    KeyCode::Esc => {}
                    KeyCode::Left => {
                        kind = previous_menu(kind);
                        selected = 0;
                        self.overlay = Some(Overlay::Menu { kind, selected });
                    }
                    KeyCode::Right => {
                        kind = next_menu(kind);
                        selected = 0;
                        self.overlay = Some(Overlay::Menu { kind, selected });
                    }
                    KeyCode::Up => {
                        selected = selected.saturating_sub(1);
                        self.overlay = Some(Overlay::Menu { kind, selected });
                    }
                    KeyCode::Down => {
                        selected = (selected + 1).min(count.saturating_sub(1));
                        self.overlay = Some(Overlay::Menu { kind, selected });
                    }
                    KeyCode::Enter => self.execute_menu(kind, selected),
                    _ => self.overlay = Some(Overlay::Menu { kind, selected }),
                }
            }
            Overlay::Browser {
                target,
                directory,
                entries,
                mut selected,
                error,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Up => {
                    selected = selected.saturating_sub(1);
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        selected,
                        error,
                    });
                }
                KeyCode::Down => {
                    selected = (selected + 1).min(entries.len().saturating_sub(1));
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        selected,
                        error,
                    });
                }
                KeyCode::Backspace => {
                    let parent = directory.parent().unwrap_or(&directory).to_path_buf();
                    self.overlay = Some(browser(target, parent));
                }
                KeyCode::Enter => {
                    if let Some(entry) = entries.get(selected) {
                        if entry.directory {
                            self.overlay = Some(browser(target, entry.path.clone()));
                        } else if let Some(text) = entry.path.to_str() {
                            if target == PathTarget::Rom {
                                self.form.rom = text.to_owned();
                            } else {
                                self.form.program = text.to_owned();
                            }
                        } else {
                            self.error("选中的文件路径不是 UTF-8，不能写入配置");
                        }
                    } else {
                        self.overlay = Some(Overlay::Browser {
                            target,
                            directory,
                            entries,
                            selected,
                            error,
                        });
                    }
                }
                _ => {
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        selected,
                        error,
                    })
                }
            },
        }
        self.dirty = true;
    }

    fn execute_menu(&mut self, kind: MenuKind, selected: usize) {
        match (kind, selected) {
            (MenuKind::Emulator, 0) => self.return_to_center(),
            (MenuKind::Emulator, 1) => {
                self.page = Page::Config;
            }
            (MenuKind::Emulator, 2) => self.run_demo(),
            (MenuKind::Session, 0) => {
                self.page = Page::Apple1;
            }
            (MenuKind::Session, 1) => {
                self.user_paused = !self.user_paused;
                self.notify(if self.user_paused {
                    "[PAUSED] 已暂停"
                } else {
                    "[RESUMED] 已继续"
                });
            }
            (MenuKind::Session, 2) => self.reset(),
            (MenuKind::Session, 3) => self.clear_screen(),
            (MenuKind::Session, 4) => self.overlay = Some(Overlay::RebootConfirm),
            (MenuKind::Session, 5) => self.return_to_center(),
            (MenuKind::Session, 6) => self.overlay = Some(Overlay::QuitConfirm),
            (MenuKind::Display, 0) => {
                self.previous_page = self.page;
                self.page = Page::Settings;
            }
            (MenuKind::Help, 0) => self.open_help(),
            _ => {}
        }
        self.dirty = true;
    }

    fn draw(&mut self, frame: &mut Frame<'_>) {
        self.terminal_size = (frame.area().width, frame.area().height);
        if !self.normal_size() {
            draw_too_small(frame, self);
            return;
        }
        let theme = Theme::from_config(&self.config);
        let area = frame.area();
        frame.render_widget(
            Block::default().style(Style::default().bg(theme.background)),
            area,
        );
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Length(26),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("HESPER", theme.title()),
                Span::styled("  经典计算机模拟器", theme.muted()),
                Span::styled("                                      ", theme.base()),
            ]))
            .style(theme.base()),
            rows[0],
        );
        draw_menu_bar(frame, rows[1], &theme);
        match self.page {
            Page::Center => draw_center(frame, rows[2], self, &theme),
            Page::Apple1 => draw_apple1(frame, rows[2], self, &theme),
            Page::Config => draw_config(frame, rows[2], self, &theme),
            Page::Info => draw_info(frame, rows[2], &theme),
            Page::Demo => draw_demo(frame, rows[2], self, &theme),
            Page::Settings => draw_settings(frame, rows[2], self, &theme),
            Page::Help => draw_help(frame, rows[2], &theme),
        }
        let continuous = if self.faulted {
            "故障"
        } else if self.user_paused || !self.running() && self.page == Page::Apple1 {
            "暂停"
        } else if self.page == Page::Apple1 {
            "运行中"
        } else {
            "就绪"
        };
        let status = self.transient_status().unwrap_or_else(|| continuous.into());
        frame.render_widget(Paragraph::new(status).style(theme.status()), rows[3]);
        frame.render_widget(
            Paragraph::new(footer(self.page))
                .style(theme.muted())
                .alignment(Alignment::Center),
            rows[4],
        );
        if let Some(overlay) = &self.overlay {
            draw_overlay(frame, area, overlay, &theme);
        }
        self.dirty = false;
    }
}

pub fn run(launch: Option<Apple1Launch>) -> Result<(), Box<dyn Error>> {
    if !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || std::env::var("TERM").ok().as_deref() == Some("dumb")
    {
        return Err("TUI requires a non-dumb terminal on both stdin and stdout".into());
    }
    let terminated = Arc::new(AtomicBool::new(false));
    let guard = TerminalGuard::enter(TerminalMode::Tui, &terminated)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(launch);
    if app.launch.direct && !app.form.rom.is_empty() {
        app.start_configured(false);
    }
    while !app.exit {
        if terminated.load(Ordering::Relaxed) {
            app.stop_message = Some("[stopped by signal]".into());
            break;
        }
        let timeout = if app.running() {
            Duration::from_millis(10)
        } else {
            Duration::from_millis(50)
        };
        if event::poll(timeout)? {
            app.handle_event(event::read()?);
            for _ in 0..31 {
                if !event::poll(Duration::ZERO)? {
                    break;
                }
                app.handle_event(event::read()?);
            }
        }
        app.advance();
        if app.dirty {
            terminal.draw(|frame| app.draw(frame))?;
        }
    }
    drop(terminal);
    drop(guard);
    if let Some(session) = &app.session {
        session.write_trace(&mut io::stderr())?;
    }
    if let Some(message) = app.stop_message {
        eprintln!("\n{message}");
    }
    if let Some(error) = app.fatal_error {
        return Err(error.into());
    }
    Ok(())
}

fn is_quit(key: KeyEvent) -> bool {
    key.modifiers.contains(KeyModifiers::CONTROL)
        && matches!(key.code, KeyCode::Char('c' | 'C' | 'd' | 'D'))
}

fn browser(target: PathTarget, directory: PathBuf) -> Overlay {
    let result = (|| -> Result<Vec<DirEntry>, String> {
        let mut entries = Vec::new();
        for entry in fs::read_dir(&directory)
            .map_err(|error| format!("无法读取目录 '{}': {error}", directory.display()))?
        {
            let entry = entry.map_err(|error| error.to_string())?;
            if entries.len() == 512 {
                return Err("目录条目超过 512；请输入更具体的路径".into());
            }
            let file_type = entry.file_type().map_err(|error| error.to_string())?;
            entries.push(DirEntry {
                path: entry.path(),
                name: entry.file_name().to_string_lossy().into_owned(),
                directory: file_type.is_dir(),
            });
        }
        entries.sort_by(|left, right| {
            right
                .directory
                .cmp(&left.directory)
                .then_with(|| left.name.cmp(&right.name))
        });
        Ok(entries)
    })();
    match result {
        Ok(entries) => Overlay::Browser {
            target,
            directory,
            entries,
            selected: 0,
            error: None,
        },
        Err(error) => Overlay::Browser {
            target,
            directory,
            entries: Vec::new(),
            selected: 0,
            error: Some(error),
        },
    }
}

fn menu_entries(kind: MenuKind) -> &'static [&'static str] {
    match kind {
        MenuKind::Emulator => &["返回启动中心", "Apple-1 配置", "运行 6502 内置演示"],
        MenuKind::Session => &[
            "返回屏幕",
            "暂停 / 继续",
            "物理 RESET",
            "CLEAR SCREEN",
            "重新上电",
            "返回启动中心",
            "退出 Hesper",
        ],
        MenuKind::Display => &["显示设置"],
        MenuKind::Help => &["帮助"],
    }
}

fn previous_menu(kind: MenuKind) -> MenuKind {
    match kind {
        MenuKind::Emulator => MenuKind::Help,
        MenuKind::Session => MenuKind::Emulator,
        MenuKind::Display => MenuKind::Session,
        MenuKind::Help => MenuKind::Display,
    }
}
fn next_menu(kind: MenuKind) -> MenuKind {
    match kind {
        MenuKind::Emulator => MenuKind::Session,
        MenuKind::Session => MenuKind::Display,
        MenuKind::Display => MenuKind::Help,
        MenuKind::Help => MenuKind::Emulator,
    }
}

fn footer(page: Page) -> &'static str {
    match page {
        Page::Apple1 => "F1 帮助  F2 会话  F3 侧栏  F10 菜单  Ctrl+P 暂停  Ctrl+C 退出",
        Page::Config => "Tab 切换焦点  F4 浏览目录  Enter 确认  Esc 取消",
        Page::Center => "↑↓ 选择  Enter 启动  C 配置  I 信息  F1 帮助  F10 菜单",
        _ => "Esc 返回  F10 菜单",
    }
}

struct Theme {
    background: Color,
    panel: Color,
    border: Color,
    foreground: Color,
    muted_color: Color,
    accent: Color,
    screen_bg: Color,
    screen_fg: Color,
    notice: Color,
    mono: bool,
    ascii: bool,
}

impl Theme {
    fn from_config(config: &AppConfig) -> Self {
        let mono = config.ui.color_mode == ColorMode::Mono
            || (config.ui.color_mode == ColorMode::Auto && std::env::var_os("NO_COLOR").is_some());
        let screen_fg = match config.ui.screen_color {
            ScreenColor::Green => Color::Rgb(155, 223, 164),
            ScreenColor::Amber => Color::Rgb(240, 203, 138),
            ScreenColor::White => Color::Rgb(216, 227, 216),
        };
        Self {
            background: Color::Rgb(17, 21, 29),
            panel: Color::Rgb(25, 31, 42),
            border: Color::Rgb(55, 66, 82),
            foreground: Color::Rgb(229, 234, 242),
            muted_color: Color::Rgb(162, 175, 191),
            accent: Color::Rgb(140, 175, 255),
            screen_bg: Color::Rgb(12, 21, 17),
            screen_fg,
            notice: Color::Rgb(241, 196, 126),
            mono,
            ascii: config.ui.border == BorderStyle::Ascii,
        }
    }
    fn base(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.foreground).bg(self.background)
        }
    }
    fn panel(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.foreground).bg(self.panel)
        }
    }
    fn muted(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::DIM)
        } else {
            self.base().fg(self.muted_color)
        }
    }
    fn title(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.base().fg(self.accent).add_modifier(Modifier::BOLD)
        }
    }
    fn selected(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            self.panel().fg(self.accent).add_modifier(Modifier::BOLD)
        }
    }
    fn status(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.panel().fg(self.notice)
        }
    }
    fn screen(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.screen_fg).bg(self.screen_bg)
        }
    }
    fn block(&self, title: impl Into<Line<'static>>) -> Block<'static> {
        let mut block = Block::default()
            .borders(Borders::ALL)
            .title(title)
            .style(self.panel())
            .border_style(if self.mono {
                Style::default()
            } else {
                Style::default().fg(self.border)
            });
        if self.ascii {
            block = block.border_set(symbols::border::PLAIN);
        }
        block
    }
}

fn draw_menu_bar(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let line = Line::from(vec![
        Span::styled("  模拟器  ", theme.title()),
        Span::styled("会话  ", theme.base()),
        Span::styled("显示  ", theme.base()),
        Span::styled("帮助", theme.base()),
    ]);
    frame.render_widget(Paragraph::new(line).style(theme.panel()), area);
}

fn draw_center(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let columns = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(38), Constraint::Percentage(62)])
        .split(area);
    let machines = ["Apple-1", "6502 内置演示"];
    let items: Vec<ListItem<'_>> = machines
        .iter()
        .enumerate()
        .map(|(index, name)| {
            ListItem::new(Line::from(Span::styled(
                *name,
                if index == app.selected_machine {
                    theme.selected()
                } else {
                    theme.base()
                },
            )))
        })
        .collect();
    frame.render_widget(List::new(items).block(theme.block("模拟器")), columns[0]);
    let apple = app.selected_machine == 0;
    let status = if app.session.is_some() {
        "本次进程内保留的会话可继续"
    } else if app.form.rom.is_empty() {
        "ROM 状态：未配置"
    } else {
        match app.validate_resources() {
            Ok(_) => "ROM 状态：已校验",
            Err(_) => "ROM 状态：不可用",
        }
    };
    let details = if apple {
        vec![
            "Apple-1",
            "固定实现：MOS 6502 / NMOS、40×24 字符屏幕、4 KiB RAM、256 B Woz Monitor ROM。",
            status,
            "Enter 启动/继续会话；C 配置 ROM；I 查看实现范围。",
        ]
    } else {
        vec![
            "6502 内置演示",
            "运行受限的原创计数程序并显示真实寄存器和内存结果。",
            "它不会替换已保留的 Apple-1 会话。",
            "Enter 运行。",
        ]
    };
    let text = details.into_iter().map(Line::from).collect::<Vec<_>>();
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("详情"))
            .style(theme.base())
            .wrap(Wrap { trim: false }),
        columns[1],
    );
    if let Some(warning) = &app.config_warning {
        frame.render_widget(
            Paragraph::new(warning.as_str()).style(theme.status()),
            Rect {
                x: columns[1].x + 1,
                y: columns[1].bottom().saturating_sub(2),
                width: columns[1].width.saturating_sub(2),
                height: 1,
            },
        );
    }
}

fn draw_apple1(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let show_sidebar = app.config.ui.sidebar && area.width >= 80;
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(area);
    let screen_area = if show_sidebar { sections[0] } else { area };
    let screen_rect = Rect {
        x: screen_area.x.saturating_add(1),
        y: screen_area.y,
        width: 42.min(screen_area.width.saturating_sub(2)),
        height: 26.min(screen_area.height),
    };
    if let Some(session) = &app.session {
        frame.render_widget(
            ScreenWidget {
                display: session.machine().display(),
                theme,
            },
            screen_rect,
        );
        if app.overlay.is_none() {
            let (row, column) = session.machine().display().cursor();
            frame.set_cursor_position((
                screen_rect.x + 1 + column as u16,
                screen_rect.y + 1 + row as u16,
            ));
        }
        if show_sidebar {
            let registers = session.machine().cpu().registers();
            let details = vec![
                "机器状态".to_owned(),
                format!("PC  ${:04X}", registers.pc),
                format!("A   ${:02X}", registers.a),
                format!("X   ${:02X}", registers.x),
                format!("Y   ${:02X}", registers.y),
                format!("SP  ${:02X}", registers.sp),
                format!("P   ${:02X}", registers.status.bits()),
                String::new(),
                format!("会话累计周期  {}", session.total_cpu_cycles()),
                format!("机器周期      {}", session.machine().cpu_cycles()),
                format!(
                    "输入待处理    {}",
                    if session.machine().keyboard().has_pending() || !app.input.is_empty() {
                        "有"
                    } else {
                        "无"
                    }
                ),
                "字符时序      主板固定".into(),
            ];
            frame.render_widget(
                Paragraph::new(details.join("\n"))
                    .block(theme.block("状态"))
                    .style(theme.base()),
                sections[1],
            );
        }
    } else {
        frame.render_widget(
            Paragraph::new("尚未创建 Apple-1 会话。返回启动中心配置 ROM。")
                .block(theme.block("Apple-1")),
            area,
        );
    }
}

struct ScreenWidget<'a> {
    display: &'a Display,
    theme: &'a Theme,
}

impl Widget for ScreenWidget<'_> {
    fn render(self, area: Rect, buffer: &mut Buffer) {
        let block = self
            .theme
            .block("APPLE-1 DISPLAY · 40×24")
            .style(self.theme.screen());
        let inner = block.inner(area);
        block.render(area, buffer);
        for (row, line) in self.display.screen().iter().enumerate().take(ROWS) {
            for (column, byte) in line.iter().enumerate().take(COLUMNS) {
                let x = inner.x.saturating_add(column as u16);
                let y = inner.y.saturating_add(row as u16);
                if x >= inner.right() || y >= inner.bottom() {
                    continue;
                }
                let style = self.theme.screen();
                if let Some(cell) = buffer.cell_mut((x, y)) {
                    cell.set_char(projected(*byte)).set_style(style);
                }
            }
        }
    }
}

fn projected(byte: u8) -> char {
    if (0x20..=0x7e).contains(&byte) {
        byte as char
    } else {
        ' '
    }
}

fn draw_config(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let focused = |field| {
        if app.form.focus == field {
            theme.selected()
        } else {
            theme.base()
        }
    };
    let title = "Apple-1 启动配置";
    let path = app.config_path.as_ref().map_or_else(
        || "配置文件：不可用".into(),
        |path| format!("配置文件：{}", path.display()),
    );
    let lines = vec![
        Line::from("ROM 必须是已固定 SHA-256 的 256 B Woz Monitor 镜像。"),
        Line::from(vec![
            Span::styled("ROM 路径: ", theme.muted()),
            Span::styled(&app.form.rom, focused(ConfigFocus::Rom)),
        ]),
        Line::from(vec![
            Span::styled("程序路径（可选）: ", theme.muted()),
            Span::styled(&app.form.program, focused(ConfigFocus::Program)),
        ]),
        Line::from("F4 浏览当前字段目录；目录最多显示 512 项，不递归扫描。"),
        Line::from(Span::styled("[校验并保存]", focused(ConfigFocus::Validate))),
        Line::from(Span::styled("[启动]", focused(ConfigFocus::Launch))),
        Line::from(Span::styled("[取消]", focused(ConfigFocus::Cancel))),
        Line::from(path),
        Line::from(app.form.status.as_deref().unwrap_or("")),
    ];
    frame.render_widget(
        Paragraph::new(lines)
            .block(theme.block(title))
            .style(theme.base())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_info(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let text = "Apple-1 固定机器配置\n\n$0000–$0FFF：4 KiB RAM\n$D010–$D013：MC6821 PIA（键盘与显示）\n$FF00–$FFFF：256 B Woz Monitor ROM\n\n当前模型驱动 NMOS 6502、PIA 与 40×24 字符显示。DRAM 刷新与显示忙时序按仓库已有近似建模；不宣称 Apple II、非官方 opcode 或所有板卡修订的兼容性。";
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("模拟器信息"))
            .style(theme.base())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_demo(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let text = app
        .demo
        .as_ref()
        .map(|demo| demo.lines.join("\n"))
        .unwrap_or_else(|| "正在运行演示…".into());
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("6502 内置演示结果"))
            .style(theme.base()),
        area,
    );
}

fn draw_settings(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let values = [
        format!("屏幕颜色：{:?}", app.config.ui.screen_color),
        format!(
            "状态侧栏：{}",
            if app.config.ui.sidebar { "开" } else { "关" }
        ),
        format!("边框：{:?}", app.config.ui.border),
        format!("颜色模式：{:?}", app.config.ui.color_mode),
        "保存设置".to_owned(),
    ];
    let items: Vec<ListItem<'_>> = values
        .iter()
        .enumerate()
        .map(|(index, text)| {
            ListItem::new(Line::from(Span::styled(
                text,
                if index == app.settings_row {
                    theme.selected()
                } else {
                    theme.base()
                },
            )))
        })
        .collect();
    frame.render_widget(
        List::new(items)
            .block(theme.block("显示设置"))
            .style(theme.base()),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let text = "Apple-1 运行页\nF1 帮助 · F2 会话菜单 · F3 显示/隐藏侧栏 · F10 顶栏菜单\nCtrl+P 暂停/继续 · Ctrl+R 物理 RESET · Ctrl+L CLEAR SCREEN · Ctrl+N 重新上电\nCtrl+C / Ctrl+D 退出 · Enter 发送 CR · Backspace 发送 _ · Esc 发送机器取消输入\n\n菜单、表单和确认弹窗会阻止机器自由运行；它们获得焦点时按键不会漏给机器。粘贴仅在机器屏幕页生效，CR/LF 会规范为单个 CR。";
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("帮助"))
            .style(theme.base())
            .wrap(Wrap { trim: false }),
        area,
    );
}

fn draw_overlay(frame: &mut Frame<'_>, area: Rect, overlay: &Overlay, theme: &Theme) {
    let width = area.width.saturating_sub(8).clamp(20, 70);
    let height = match overlay {
        Overlay::Menu { kind, .. } => (menu_entries(*kind).len() as u16 + 3).min(area.height),
        Overlay::Browser { .. } => area.height.saturating_sub(6).min(20),
        _ => 5,
    };
    let popup = centered(
        Rect {
            x: area.x,
            y: area.y,
            width,
            height,
        },
        area,
    );
    frame.render_widget(Clear, popup);
    match overlay {
        Overlay::Menu { kind, selected } => {
            let heading = match kind { MenuKind::Emulator => "模拟器", MenuKind::Session => "会话", MenuKind::Display => "显示", MenuKind::Help => "帮助" };
            let lines = menu_entries(*kind).iter().enumerate().map(|(index, text)| Line::from(Span::styled(*text, if index == *selected { theme.selected() } else { theme.base() }))).collect::<Vec<_>>();
            frame.render_widget(Paragraph::new(lines).block(theme.block(format!("{heading}  ←/→ 切换主菜单"))).style(theme.panel()), popup);
        }
        Overlay::RebootConfirm => frame.render_widget(Paragraph::new("重新上电会丢弃当前 RAM 和设备状态；会话周期预算与 trace 保留。\nEnter 确认  Esc 取消").block(theme.block("确认重新上电")).style(theme.panel()).wrap(Wrap { trim: false }), popup),
        Overlay::QuitConfirm => frame.render_widget(Paragraph::new("退出会结束本次进程内会话。\nEnter 确认  Esc 取消").block(theme.block("确认退出")).style(theme.panel()), popup),
        Overlay::Error(error) => frame.render_widget(Paragraph::new(format!("{error}\n\nEnter / Esc 关闭")).block(theme.block("错误")).style(theme.panel()).wrap(Wrap { trim: false }), popup),
        Overlay::Browser { target, directory, entries, selected, error } => {
            let header = format!("浏览 {}：{}", if *target == PathTarget::Rom { "ROM" } else { "程序" }, truncate_path(&directory.to_string_lossy(), usize::from(popup.width.saturating_sub(6))));
            let mut lines = vec![Line::from("Backspace 上一级；Enter 进入目录或选择文件；Esc 返回")];
            if let Some(error) = error { lines.push(Line::from(Span::styled(error, theme.status()))); }
            for (index, entry) in entries.iter().enumerate().take(15) {
                let label = if entry.directory { format!("{}/", entry.name) } else { entry.name.clone() };
                lines.push(Line::from(Span::styled(truncate_path(&label, usize::from(popup.width.saturating_sub(6))), if index == *selected { theme.selected() } else { theme.base() })));
            }
            frame.render_widget(Paragraph::new(lines).block(theme.block(header)).style(theme.panel()), popup);
        }
    }
}

fn draw_too_small(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let text = format!(
        "窗口过小：需要至少 {MIN_WIDTH}×{MIN_HEIGHT}，当前 {}×{}。\n机器已暂停。Ctrl+C/Ctrl+D 退出；有会话时 Enter 确认退出，Esc 取消。",
        area.width, area.height
    );
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
    if let Some(Overlay::QuitConfirm) = &app.overlay {
        frame.render_widget(
            Paragraph::new("确认退出？ Enter 确认 / Esc 取消")
                .block(Block::default().borders(Borders::ALL)),
            centered(
                Rect {
                    x: 0,
                    y: 0,
                    width: area.width.min(36),
                    height: area.height.min(4),
                },
                area,
            ),
        );
    }
}

fn centered(popup: Rect, area: Rect) -> Rect {
    Rect {
        x: area.x + area.width.saturating_sub(popup.width) / 2,
        y: area.y + area.height.saturating_sub(popup.height) / 2,
        width: popup.width.min(area.width),
        height: popup.height.min(area.height),
    }
}

fn truncate_path(value: &str, width: usize) -> String {
    if value.width() <= width {
        return value.to_owned();
    }
    let mut tail = String::new();
    let mut used = 1;
    for ch in value.chars().rev() {
        let next = ch.to_string();
        let char_width = next.width();
        if used + char_width > width {
            break;
        }
        tail.insert(0, ch);
        used += char_width;
    }
    format!("…{tail}")
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::{Terminal, backend::TestBackend};

    #[test]
    fn narrow_terminal_is_safe() {
        let backend = TestBackend::new(1, 1);
        let mut terminal = Terminal::new(backend).unwrap();
        let mut app = App::new(None);
        terminal.draw(|frame| app.draw(frame)).unwrap();
    }

    #[test]
    fn truncation_respects_terminal_cell_width() {
        let value = truncate_path("目录/很长的文件名.bin", 10);
        assert!(value.width() <= 10);
    }

    #[test]
    fn paste_normalizes_newlines_and_never_becomes_a_host_command() {
        let mut app = App::new(None);
        app.page = Page::Apple1;
        app.terminal_size = (120, 40);
        app.handle_paste("a\r\nb\nc\t\u{1b}".into());
        assert_eq!(app.input.into_iter().collect::<Vec<_>>(), b"a\rb\rc");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn temporary_overlay_and_user_pause_are_independent() {
        let mut app = App::new(None);
        app.page = Page::Apple1;
        app.terminal_size = (120, 40);
        app.session = Some(Session::new(
            create_machine(&test_rom(), Some(&[0x4c, 0x00, 0x00])).unwrap(),
            None,
            TraceOptions::new(false, false, 64),
        ));
        assert!(app.running());
        app.apple_key(KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL));
        assert!(!app.running());
        app.overlay = Some(Overlay::Menu {
            kind: MenuKind::Session,
            selected: 0,
        });
        app.overlay = None;
        assert!(!app.running(), "closing a menu must not clear user pause");
    }

    fn test_rom() -> [u8; 256] {
        let mut rom = [0; 256];
        rom[0xfc] = 0;
        rom[0xfd] = 0;
        rom
    }
}
