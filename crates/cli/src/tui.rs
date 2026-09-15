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

use crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use hesper_apple1::display::{COLUMNS, Display, ROWS};
use ratatui::{
    Frame, Terminal,
    backend::CrosstermBackend,
    buffer::Buffer,
    layout::{Constraint, Direction, Layout, Margin, Position, Rect},
    style::{Color, Modifier, Style},
    symbols,
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Padding, Paragraph, Widget, Wrap},
};
use unicode_width::UnicodeWidthStr;

use crate::{
    apple1::{
        ProgramImage, ProgramSource, Session, StopReason, TraceOptions, create_machine, load_rom,
        parse_program_address,
    },
    config::{self, AppConfig, BorderStyle, ColorMode, ScreenColor},
    format_registers,
    presets::{APPLE1_PRESETS, Category, ProgramPreset},
    run_demo_with_trace,
    terminal::{TerminalGuard, TerminalMode},
};

const IDLE_BATCH_CPU_CYCLES: u64 = 2_000;
const INPUT_LIMIT: usize = 4_096;
const MIN_WIDTH: u16 = 44;
const MIN_HEIGHT: u16 = 30;

/// Options accepted by `hesper apple1` when that command opens the TUI.
#[derive(Debug, Clone)]
pub struct Apple1Launch {
    pub rom: Option<PathBuf>,
    pub program: Option<PathBuf>,
    pub preset: Option<&'static ProgramPreset>,
    pub program_address: u16,
    pub max_cycles: Option<u64>,
    pub trace: bool,
    pub bus_trace: bool,
    pub trace_limit: usize,
    /// Set by the CLI for `hesper apple1`; no-argument TUI startup leaves it
    /// false so the center is always shown first.
    pub direct: bool,
}

impl Default for Apple1Launch {
    fn default() -> Self {
        Self {
            rom: None,
            program: None,
            preset: None,
            program_address: 0,
            max_cycles: None,
            trace: false,
            bus_trace: false,
            trace_limit: 64,
            direct: false,
        }
    }
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
enum CenterAction {
    Configure,
    Launch,
    Resume,
    Demo,
}

impl CenterAction {
    fn label(self) -> &'static str {
        match self {
            Self::Configure => " Enter 配置 Apple-1 ",
            Self::Launch => " Enter 启动 Apple-1 ",
            Self::Resume => " Enter 返回保留会话 ",
            Self::Demo => " Enter 运行演示 ",
        }
    }
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
    Programs {
        state: ListState,
    },
    Menu {
        kind: MenuKind,
        selected: usize,
    },
    RebootConfirm {
        confirmed: bool,
    },
    ReplaceConfirm {
        resources: Box<Resources>,
        save_path: bool,
        confirmed: bool,
    },
    QuitConfirm,
    Error(String),
    Browser {
        target: PathTarget,
        directory: PathBuf,
        entries: Vec<DirEntry>,
        state: ListState,
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
    ProgramAddress,
    Validate,
    Launch,
    Cancel,
}

impl ConfigFocus {
    fn adjacent(self, backwards: bool) -> Self {
        let fields = [
            Self::Rom,
            Self::Program,
            Self::ProgramAddress,
            Self::Validate,
            Self::Launch,
            Self::Cancel,
        ];
        let index = fields.iter().position(|field| *field == self).unwrap();
        fields[(index + if backwards { fields.len() - 1 } else { 1 }) % fields.len()]
    }
}

#[derive(Debug)]
struct ConfigEdit {
    text: String,
    /// UTF-8 byte boundary; terminal placement uses display-cell width.
    cursor: usize,
}

impl ConfigEdit {
    fn new(text: String) -> Self {
        let cursor = text.len();
        Self { text, cursor }
    }

    fn insert(&mut self, text: &str) {
        self.text.insert_str(self.cursor, text);
        self.cursor += text.len();
    }

    fn previous(&self) -> usize {
        self.text[..self.cursor]
            .char_indices()
            .next_back()
            .map_or(0, |(index, _)| index)
    }

    fn next(&self) -> usize {
        self.cursor
            + self.text[self.cursor..]
                .chars()
                .next()
                .map_or(0, char::len_utf8)
    }
}

#[derive(Debug)]
struct ConfigForm {
    rom: String,
    program: String,
    program_address: String,
    preset: Option<&'static ProgramPreset>,
    focus: ConfigFocus,
    edit: Option<ConfigEdit>,
    status: Option<String>,
}

impl ConfigForm {
    fn focused_text(&mut self) -> Option<&mut String> {
        match self.focus {
            ConfigFocus::Rom => Some(&mut self.rom),
            ConfigFocus::Program => Some(&mut self.program),
            ConfigFocus::ProgramAddress => Some(&mut self.program_address),
            _ => None,
        }
    }
}

#[derive(Debug)]
struct Resources {
    rom: [u8; 256],
    program: Option<ProgramImage>,
    rom_path: PathBuf,
    preset: Option<&'static ProgramPreset>,
}

impl Resources {
    /// The address the launch form reports: the program block, not a tape
    /// header or the BASIC image a BASIC program is shipped with.
    fn program_address(&self) -> u16 {
        self.program.as_ref().map_or(0, ProgramImage::load_address)
    }

    fn program_size(&self) -> usize {
        self.program.as_ref().map_or(0, ProgramImage::size)
    }
}

#[derive(Debug)]
struct DemoView {
    lines: Vec<String>,
}

struct App {
    page: Page,
    page_history: Vec<Page>,
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
            page_history: Vec::new(),
            overlay: None,
            config,
            config_path,
            config_warning,
            selected_machine,
            form: ConfigForm {
                rom,
                program,
                program_address: format!("0x{:04X}", launch.program_address),
                preset: launch.preset,
                focus: ConfigFocus::Program,
                edit: None,
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
        // A preset carries its own addresses; without one the field must parse
        // even before a file is chosen, so a half-typed address never sits in
        // the form unnoticed.
        let source = if let Some(preset) = self.form.preset {
            Some(ProgramSource::Preset(preset))
        } else {
            let address = parse_program_address(&self.form.program_address).map_err(|_| {
                "程序加载地址无效：请输入 0–65535 的十进制数，或 0xE000 / $E000 形式的十六进制地址"
                    .to_owned()
            })?;
            if self.form.program.is_empty() {
                None
            } else {
                Some(ProgramSource::File {
                    path: &self.form.program,
                    address,
                })
            }
        };
        let program = source
            .map(|source| source.load().map_err(|error| error.to_string()))
            .transpose()?;
        let absolute_rom = fs::canonicalize(&rom_path)
            .map_err(|error| format!("无法规范化 ROM 路径 '{}': {error}", rom_path.display()))?;
        Ok(Resources {
            rom,
            program,
            rom_path: absolute_rom,
            preset: self.form.preset,
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
                self.save_rom_path(&resources);
                Some(resources)
            }
            Err(error) => {
                self.form.status = Some(error);
                self.dirty = true;
                None
            }
        }
    }

    fn save_rom_path(&mut self, resources: &Resources) {
        self.config.apple1.rom_path = Some(resources.rom_path.clone());
        self.form.status = Some(match self.save_config() {
            Ok(()) => "ROM 已校验，路径已保存".into(),
            Err(error) => format!("配置未保存：{error}"),
        });
        self.dirty = true;
    }

    fn start_configured(&mut self, save_path: bool) {
        match self.validate_resources() {
            Ok(resources) => self.request_start(resources, save_path),
            Err(error) => {
                self.form.status = Some(error);
                self.dirty = true;
            }
        }
    }

    fn request_start(&mut self, resources: Resources, save_path: bool) {
        if self.has_active_session() {
            self.overlay = Some(Overlay::ReplaceConfirm {
                resources: Box::new(resources),
                save_path,
                confirmed: false,
            });
            self.dirty = true;
        } else {
            self.start_resources(resources, save_path);
        }
    }

    fn start_resources(&mut self, resources: Resources, save_path: bool) {
        let trace = match TraceOptions::new(
            self.launch.trace,
            self.launch.bus_trace,
            self.launch.trace_limit,
        ) {
            Ok(trace) => trace,
            Err(error) => {
                self.form.status = Some(error.into());
                self.dirty = true;
                return;
            }
        };
        let machine = match create_machine(&resources.rom, resources.program.as_ref()) {
            Ok(machine) => machine,
            Err(error) => {
                self.form.status = Some(error.to_string());
                self.dirty = true;
                return;
            }
        };
        if save_path {
            self.save_rom_path(&resources);
        }
        if let Some(session) = &mut self.session {
            // Replacing resources is still part of this process's bounded
            // session: never replenish its budget or discard its trace.
            session.recreate(machine);
        } else {
            self.session = Some(Session::new(machine, self.launch.max_cycles, trace));
        }
        self.resources = Some(resources);
        self.open_page(Page::Apple1);
        self.input.clear();
        let session = self.session.as_mut().expect("session just installed");
        let result = session.boot();
        let _ = session.machine_mut().drain_output();
        match result {
            Ok(stop) => {
                self.faulted = false;
                self.fatal_error = None;
                if let Some(stop) = stop {
                    self.stop(stop);
                } else {
                    let message = self
                        .resources
                        .as_ref()
                        .and_then(|resources| resources.preset)
                        .map_or_else(
                            || "[NEW MACHINE] 已启动".into(),
                            |preset| format!("{} 已加载 · {}", preset.name, preset.startup),
                        );
                    self.notify(message);
                }
            }
            Err(error) => {
                self.faulted = true;
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
        let machine = match create_machine(&resources.rom, resources.program.as_ref()) {
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
        self.open_page(Page::Center);
        self.overlay = None;
    }

    fn open_page(&mut self, page: Page) {
        if page == self.page {
            return;
        }
        if matches!(page, Page::Info | Page::Help | Page::Settings) {
            if let Some(index) = self
                .page_history
                .iter()
                .position(|previous| *previous == page)
            {
                // Revisit the existing level instead of creating a cycle.
                self.page_history.truncate(index);
            } else {
                self.page_history.push(self.page);
            }
        } else {
            self.page_history.clear();
            self.form.edit = None;
        }
        // At most the origin and two other auxiliary pages are retained.
        self.page = page;
        self.dirty = true;
    }

    fn return_to_previous_page(&mut self) {
        self.page = self.page_history.pop().unwrap_or(Page::Center);
        self.dirty = true;
    }

    fn open_help(&mut self) {
        self.open_page(Page::Help);
    }

    fn run_demo(&mut self) {
        self.open_page(Page::Demo);
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
        let text = self.form.edit.as_ref().map_or_else(
            || match target {
                PathTarget::Rom => &self.form.rom,
                PathTarget::Program => &self.form.program,
            },
            |edit| &edit.text,
        );
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
        if self.overlay.is_some() {
            return;
        }
        if self.page == Page::Config {
            let Some(edit) = &mut self.form.edit else {
                return;
            };
            let label = if self.form.focus == ConfigFocus::ProgramAddress {
                "地址"
            } else {
                "路径"
            };
            if text.chars().any(char::is_control) {
                self.form.status = Some(format!(
                    "{label}粘贴未接收：请使用不含换行或控制字符的单行文本"
                ));
            } else {
                // Paths are literal text, including spaces and shell syntax.
                edit.insert(&text);
                self.form.status = None;
            }
            self.dirty = true;
            return;
        }
        if self.page != Page::Apple1 {
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
            Event::Mouse(mouse) => self.handle_mouse(mouse),
            Event::FocusGained | Event::FocusLost => {}
        }
    }

    fn handle_mouse(&mut self, mouse: MouseEvent) {
        if !self.config.ui.mouse || !self.normal_size() {
            return;
        }
        let clicked = mouse.kind == MouseEventKind::Down(MouseButton::Left);
        if !clicked && mouse.kind != MouseEventKind::Moved {
            return;
        }
        let menu = match self.overlay {
            Some(Overlay::Menu { kind, selected }) => Some((kind, selected)),
            None => None,
            // Confirmations, errors and file browsers retain exclusive focus.
            Some(_) => return,
        };
        let area = Rect::new(0, 0, self.terminal_size.0, self.terminal_size.1);
        let position = Position::new(mouse.column, mouse.row);
        if let Some((kind, _, _)) = menu_tabs(area)
            .into_iter()
            .find(|(_, _, tab)| tab.contains(position))
        {
            if clicked || menu.is_some_and(|(active, _)| active != kind) {
                self.overlay = if clicked && menu.is_some_and(|(active, _)| active == kind) {
                    None
                } else {
                    Some(Overlay::Menu { kind, selected: 0 })
                };
                self.dirty = true;
            }
            return;
        }
        if let Some((kind, selected)) = menu {
            let popup = menu_popup(area, kind);
            // Match the dropdown's border and horizontal content padding.
            let items = popup.inner(Margin::new(2, 1));
            if items.contains(position) {
                let index = usize::from(mouse.row - items.y);
                if clicked {
                    self.overlay = None;
                    self.execute_menu(kind, index);
                    self.dirty = true;
                } else if index != selected {
                    self.overlay = Some(Overlay::Menu {
                        kind,
                        selected: index,
                    });
                    self.dirty = true;
                }
            } else if clicked && !popup.contains(position) {
                self.overlay = None;
                self.dirty = true;
            }
        }
    }

    fn handle_key(&mut self, key: KeyEvent) {
        if key.kind != KeyEventKind::Press {
            return;
        }
        // A command can stop execution before advance() has a chance to
        // invalidate the frame (menus and confirmations in particular).
        self.dirty = true;
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
                    self.return_to_previous_page();
                }
            }
            Page::Settings => self.settings_key(key),
        }
    }

    fn center_action(&self) -> CenterAction {
        if self.selected_machine == 1 {
            CenterAction::Demo
        } else if self.has_active_session() {
            CenterAction::Resume
        } else if self.validate_resources().is_ok() {
            CenterAction::Launch
        } else {
            CenterAction::Configure
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
            KeyCode::Enter => match self.center_action() {
                CenterAction::Resume => self.open_page(Page::Apple1),
                CenterAction::Configure => {
                    self.form.status = if self.form.rom.is_empty() {
                        None
                    } else {
                        self.validate_resources().err()
                    };
                    self.open_page(Page::Config);
                }
                CenterAction::Launch => {
                    // A resource can disappear between preview and launch.
                    // Keep any resulting validation error on a visible form.
                    self.open_page(Page::Config);
                    self.start_configured(true);
                }
                CenterAction::Demo => self.run_demo(),
            },
            KeyCode::Char('c' | 'C') if self.selected_machine == 0 => self.open_page(Page::Config),
            KeyCode::Char('i' | 'I') if self.selected_machine == 0 => self.open_page(Page::Info),
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
                    self.overlay = Some(Overlay::RebootConfirm { confirmed: false })
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
        if key.code == KeyCode::F(3) && self.form.edit.is_none() {
            self.open_programs();
            return;
        }
        if key.code == KeyCode::F(4) {
            match self.form.focus {
                ConfigFocus::Rom => self.open_browser(PathTarget::Rom),
                ConfigFocus::Program => self.open_browser(PathTarget::Program),
                _ => {}
            }
            return;
        }
        if let Some(edit) = &mut self.form.edit {
            match key.code {
                KeyCode::Esc => {
                    self.form.edit = None;
                    self.form.status = None;
                }
                KeyCode::Enter => {
                    let edit = self.form.edit.take().unwrap();
                    if let Some(field) = self.form.focused_text() {
                        *field = edit.text;
                    }
                    self.form.status = None;
                }
                KeyCode::Left => edit.cursor = edit.previous(),
                KeyCode::Right => edit.cursor = edit.next(),
                KeyCode::Home => edit.cursor = 0,
                KeyCode::End => edit.cursor = edit.text.len(),
                KeyCode::Backspace => {
                    let start = edit.previous();
                    edit.text.drain(start..edit.cursor);
                    edit.cursor = start;
                    self.form.status = None;
                }
                KeyCode::Delete => {
                    edit.text.drain(edit.cursor..edit.next());
                    self.form.status = None;
                }
                KeyCode::Char('u' | 'U') if key.modifiers.contains(KeyModifiers::CONTROL) => {
                    edit.text.clear();
                    edit.cursor = 0;
                    self.form.status = None;
                }
                KeyCode::Char(ch)
                    if !ch.is_control()
                        && !key
                            .modifiers
                            .intersects(KeyModifiers::CONTROL | KeyModifiers::ALT) =>
                {
                    edit.insert(&ch.to_string());
                    self.form.status = None;
                }
                _ => {}
            }
            return;
        }
        match key.code {
            KeyCode::Esc => self.return_to_center(),
            KeyCode::Up | KeyCode::BackTab => self.form.focus = self.form.focus.adjacent(true),
            KeyCode::Down | KeyCode::Tab => {
                self.form.focus = self
                    .form
                    .focus
                    .adjacent(key.modifiers.contains(KeyModifiers::SHIFT));
            }
            KeyCode::Left | KeyCode::Right
                if matches!(
                    self.form.focus,
                    ConfigFocus::Validate | ConfigFocus::Launch | ConfigFocus::Cancel
                ) =>
            {
                self.form.focus = match (self.form.focus, key.code) {
                    (ConfigFocus::Validate, KeyCode::Left) => ConfigFocus::Cancel,
                    (ConfigFocus::Cancel, KeyCode::Right) => ConfigFocus::Validate,
                    (_, KeyCode::Left) => self.form.focus.adjacent(true),
                    _ => self.form.focus.adjacent(false),
                };
            }
            KeyCode::Enter => match self.form.focus {
                ConfigFocus::Program => self.open_programs(),
                ConfigFocus::ProgramAddress if self.form.preset.is_some() => {
                    self.form.status =
                        Some("预置程序使用固定加载地址；按 F3 可切换为本地文件".into());
                }
                ConfigFocus::Validate => {
                    let _ = self.validate_and_save();
                }
                ConfigFocus::Launch => self.start_configured(true),
                ConfigFocus::Cancel => self.return_to_center(),
                _ => {
                    if let Some(text) = self.form.focused_text().cloned() {
                        self.form.edit = Some(ConfigEdit::new(text));
                        self.form.status = None;
                    }
                }
            },
            _ => {}
        }
    }

    fn open_programs(&mut self) {
        let index = self.form.preset.map_or_else(
            || usize::from(!self.form.program.is_empty()),
            |preset| {
                APPLE1_PRESETS
                    .iter()
                    .position(|item| item.id == preset.id)
                    .map_or(PICKER_FIXED_ROWS, |position| position + PICKER_FIXED_ROWS)
            },
        );
        let mut state = ListState::default();
        state.select(Some(picker_screen_row(index)));
        self.overlay = Some(Overlay::Programs { state });
    }

    fn settings_key(&mut self, key: KeyEvent) {
        match key.code {
            KeyCode::Esc => {
                self.return_to_previous_page();
            }
            KeyCode::Up => self.settings_row = self.settings_row.saturating_sub(1),
            KeyCode::Down => self.settings_row = (self.settings_row + 1).min(5),
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
                4 => self.config.ui.mouse = !self.config.ui.mouse,
                5 => match self.save_config() {
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
            Overlay::Programs { mut state } => {
                let row = state.selected().unwrap_or(0);
                match key.code {
                    KeyCode::Esc => return,
                    KeyCode::Up => state.select(Some(picker_step(row, false))),
                    KeyCode::Down => state.select(Some(picker_step(row, true))),
                    KeyCode::Left => state.select(Some(picker_category_step(row, false))),
                    KeyCode::Right => state.select(Some(picker_category_step(row, true))),
                    KeyCode::Home => state.select(Some(0)),
                    KeyCode::End => {
                        state.select(Some(picker_screen_row(
                            PICKER_FIXED_ROWS + APPLE1_PRESETS.len() - 1,
                        )));
                    }
                    KeyCode::Enter => {
                        let Some(index) = picker_index_at_row(row) else {
                            // A category header is not a choice: keep the
                            // picker open instead of silently choosing.
                            self.overlay = Some(Overlay::Programs { state });
                            return;
                        };
                        self.form.preset = index
                            .checked_sub(PICKER_FIXED_ROWS)
                            .map(|position| &APPLE1_PRESETS[position]);
                        self.form.focus = ConfigFocus::Program;
                        self.form.status = None;
                        if index == 0 {
                            self.form.program.clear();
                            self.form.program_address = "0x0000".into();
                        } else if index == 1 {
                            self.form.edit = Some(ConfigEdit::new(self.form.program.clone()));
                        }
                        return;
                    }
                    _ => {}
                }
                self.overlay = Some(Overlay::Programs { state });
            }
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
            Overlay::RebootConfirm { mut confirmed } => match key.code {
                KeyCode::Enter => {
                    if confirmed {
                        self.reboot();
                    }
                }
                KeyCode::Esc => {}
                _ => {
                    match key.code {
                        KeyCode::Left => confirmed = false,
                        KeyCode::Right => confirmed = true,
                        KeyCode::Tab | KeyCode::BackTab => confirmed = !confirmed,
                        _ => {}
                    }
                    self.overlay = Some(Overlay::RebootConfirm { confirmed });
                }
            },
            Overlay::ReplaceConfirm {
                resources,
                save_path,
                mut confirmed,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Enter => {
                    if confirmed {
                        self.start_resources(*resources, save_path);
                    }
                }
                _ => {
                    match key.code {
                        KeyCode::Left => confirmed = false,
                        KeyCode::Right => confirmed = true,
                        KeyCode::Tab | KeyCode::BackTab => confirmed = !confirmed,
                        _ => {}
                    }
                    self.overlay = Some(Overlay::ReplaceConfirm {
                        resources,
                        save_path,
                        confirmed,
                    });
                }
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
                mut state,
                error,
            } => match key.code {
                KeyCode::Esc => {}
                KeyCode::Up => {
                    state.select(Some(state.selected().unwrap_or(0).saturating_sub(1)));
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        state,
                        error,
                    });
                }
                KeyCode::Down => {
                    state.select(Some(
                        (state.selected().unwrap_or(0) + 1).min(entries.len().saturating_sub(1)),
                    ));
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        state,
                        error,
                    });
                }
                KeyCode::Backspace => {
                    let parent = directory.parent().unwrap_or(&directory).to_path_buf();
                    self.overlay = Some(browser(target, parent));
                }
                KeyCode::Enter => {
                    if let Some(entry) = state.selected().and_then(|index| entries.get(index)) {
                        if entry.directory {
                            self.overlay = Some(browser(target, entry.path.clone()));
                        } else if let Some(text) = entry.path.to_str() {
                            if let Some(edit) = &mut self.form.edit {
                                *edit = ConfigEdit::new(text.to_owned());
                            } else if target == PathTarget::Rom {
                                self.form.rom = text.to_owned();
                            } else {
                                self.form.program = text.to_owned();
                                self.form.preset = None;
                            }
                            self.form.status = None;
                        } else {
                            self.error("选中的文件路径不是 UTF-8，不能写入配置");
                        }
                    } else {
                        self.overlay = Some(Overlay::Browser {
                            target,
                            directory,
                            entries,
                            state,
                            error,
                        });
                    }
                }
                _ => {
                    self.overlay = Some(Overlay::Browser {
                        target,
                        directory,
                        entries,
                        state,
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
                self.open_page(Page::Config);
            }
            (MenuKind::Emulator, 2) => self.run_demo(),
            (MenuKind::Session, 0) => {
                self.open_page(Page::Apple1);
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
            (MenuKind::Session, 4) => {
                self.overlay = Some(Overlay::RebootConfirm { confirmed: false })
            }
            (MenuKind::Session, 5) => self.return_to_center(),
            (MenuKind::Session, 6) => self.overlay = Some(Overlay::QuitConfirm),
            (MenuKind::Display, 0) => {
                self.open_page(Page::Settings);
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
        frame.render_widget(Block::default().style(theme.base()), area);
        let rows = Layout::default()
            .direction(Direction::Vertical)
            .constraints([
                Constraint::Length(1),
                Constraint::Length(1),
                Constraint::Min(26),
                Constraint::Length(1),
                Constraint::Length(1),
            ])
            .split(area);
        frame.render_widget(
            Paragraph::new(Line::from(vec![
                Span::styled("  HESPER", theme.title()),
                Span::styled("  经典计算机模拟器", theme.muted()),
            ]))
            .style(theme.text()),
            rows[0],
        );
        let active_menu = match self.overlay {
            Some(Overlay::Menu { kind, .. }) => Some(kind),
            _ => None,
        };
        draw_menu_bar(frame, area, active_menu, &theme);
        match self.page {
            Page::Center => draw_center(frame, rows[2], self, &theme),
            Page::Apple1 => draw_apple1(frame, rows[2], self, &theme),
            Page::Config => draw_config(frame, rows[2], self, &theme),
            Page::Info => draw_info(frame, rows[2], &theme),
            Page::Demo => draw_demo(frame, rows[2], self, &theme),
            Page::Settings => draw_settings(frame, rows[2], self, &theme),
            Page::Help => draw_help(frame, rows[2], &theme),
        }
        let (continuous, status_style) = if self.faulted {
            (
                "故障",
                theme.text().fg(theme.fault).add_modifier(Modifier::BOLD),
            )
        } else if self.running() {
            ("运行中", theme.text().fg(theme.success))
        } else if self.has_active_session() {
            ("暂停", theme.status())
        } else {
            ("就绪", theme.muted())
        };
        let status =
            Layout::horizontal([Constraint::Length(12), Constraint::Min(0)]).split(rows[3]);
        frame.render_widget(
            Paragraph::new(format!("  [{continuous}]")).style(status_style),
            status[0],
        );
        frame.render_widget(
            Paragraph::new(self.transient_status().unwrap_or_default()).style(theme.status()),
            status[1],
        );
        frame.render_widget(
            Paragraph::new(footer(self, rows[4].width, &theme)).style(theme.panel()),
            rows[4],
        );
        if let Some(overlay) = &mut self.overlay {
            draw_overlay(frame, rows[2], overlay, &theme);
        }
        self.dirty = false;
    }
}

pub fn run(launch: Option<Apple1Launch>) -> Result<(), Box<dyn Error>> {
    let launch = launch.unwrap_or_default();
    // Reject invalid library arguments before acquiring terminal modes or
    // installing a session, just like the text-mode public entry point.
    TraceOptions::new(launch.trace, launch.bus_trace, launch.trace_limit)?;
    if !io::stdin().is_terminal()
        || !io::stdout().is_terminal()
        || std::env::var("TERM").ok().as_deref() == Some("dumb")
    {
        return Err("TUI requires a non-dumb terminal on both stdin and stdout".into());
    }
    let terminated = Arc::new(AtomicBool::new(false));
    let mut guard = TerminalGuard::enter(TerminalMode::Tui, &terminated)?;
    let backend = CrosstermBackend::new(io::stdout());
    let mut terminal = Terminal::new(backend)?;
    let mut app = App::new(Some(launch));
    guard.set_mouse(app.config.ui.mouse)?;
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
        guard.set_mouse(app.config.ui.mouse)?;
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
            state: ListState::default().with_selected(Some(0)),
            error: None,
        },
        Err(error) => Overlay::Browser {
            target,
            directory,
            entries: Vec::new(),
            state: ListState::default(),
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

fn footer(app: &App, width: u16, theme: &Theme) -> Line<'static> {
    let vertical = if theme.ascii { "Up/Down" } else { "↑↓" };
    let hints = match &app.overlay {
        Some(Overlay::Programs { .. }) => {
            vec![("Enter", "选择"), ("Esc", "返回"), (vertical, "移动")]
        }
        Some(Overlay::Menu { .. }) => vec![
            ("Enter", "执行"),
            ("Esc", "关闭"),
            (vertical, "选择"),
            (theme.horizontal_keys(), "切换"),
        ],
        Some(Overlay::Browser { .. }) => vec![
            ("Enter", "选择"),
            ("Esc", "返回"),
            (vertical, "移动"),
            ("Backspace", "上一级"),
        ],
        Some(Overlay::RebootConfirm { .. } | Overlay::ReplaceConfirm { .. }) => {
            vec![("Enter", "执行"), ("Esc", "取消"), ("Tab", "选择")]
        }
        Some(Overlay::QuitConfirm) => vec![("Enter", "确认退出"), ("Esc", "取消")],
        Some(Overlay::Error(_)) => vec![("Enter / Esc", "关闭")],
        None => match app.page {
            Page::Center => {
                let mut hints = vec![
                    (vertical, "选择"),
                    ("Enter", "执行"),
                    ("F10", "菜单"),
                    ("F1", "帮助"),
                ];
                if app.selected_machine == 0 {
                    hints.extend([("C", "配置"), ("I", "信息")]);
                }
                hints.push(("Ctrl+C", "退出"));
                hints
            }
            Page::Apple1 => vec![
                ("F1", "帮助"),
                ("F2", "会话"),
                ("Ctrl+R", "复位"),
                ("F10", "菜单"),
                ("Ctrl+P", if app.user_paused { "继续" } else { "暂停" }),
                ("F3", "侧栏"),
                ("Ctrl+C", "退出"),
            ],
            Page::Config => {
                let field = matches!(
                    app.form.focus,
                    ConfigFocus::Rom | ConfigFocus::Program | ConfigFocus::ProgramAddress
                );
                let mut hints = if app.form.edit.is_some() {
                    vec![
                        ("Enter", "确认"),
                        ("Esc", "取消编辑"),
                        ("Ctrl+U", "清空"),
                        (theme.horizontal_keys(), "光标"),
                        ("Home/End", "首尾"),
                    ]
                } else {
                    vec![
                        (
                            "Enter",
                            if app.form.focus == ConfigFocus::Program {
                                "选择"
                            } else if app.form.focus == ConfigFocus::ProgramAddress
                                && app.form.preset.is_some()
                            {
                                "查看"
                            } else if field {
                                "编辑"
                            } else {
                                "执行"
                            },
                        ),
                        ("F3", "预置"),
                        ("Esc", "返回"),
                        (vertical, "选择"),
                        ("Tab/S-Tab", "切换"),
                    ]
                };
                if matches!(app.form.focus, ConfigFocus::Rom | ConfigFocus::Program) {
                    hints.push(("F4", "浏览"));
                }
                hints
            }
            Page::Demo => vec![("R", "重跑"), ("Esc", "返回"), ("F10", "菜单")],
            Page::Settings => vec![
                (
                    "Enter",
                    if app.settings_row == 5 {
                        "保存"
                    } else {
                        "切换"
                    },
                ),
                ("Esc", "返回"),
                (vertical, "选择"),
                ("F10", "菜单"),
            ],
            _ => vec![("Esc", "返回"), ("F10", "菜单")],
        },
    };
    let mut spans = vec![Span::raw("  ")];
    let mut used = 2;
    for (key, label) in hints {
        let gap = if spans.len() == 1 { 0 } else { 2 };
        let length = gap + key.width() + 1 + label.width();
        // Omit lower-priority hints as whole units, never half a shortcut.
        if used + length + 2 > usize::from(width) {
            continue;
        }
        if gap > 0 {
            spans.push(Span::raw("  "));
        }
        spans.push(Span::styled(key, theme.title()));
        spans.push(Span::styled(format!(" {label}"), theme.muted()));
        used += length;
    }
    Line::from(spans)
}

struct Theme {
    background: Color,
    panel: Color,
    border: Color,
    foreground: Color,
    muted_color: Color,
    accent: Color,
    selection_bg: Color,
    screen_bg: Color,
    screen_fg: Color,
    notice: Color,
    success: Color,
    fault: Color,
    mono: bool,
    ascii: bool,
}

impl Theme {
    fn from_config(config: &AppConfig) -> Self {
        Self::with_capabilities(
            config,
            crossterm::style::available_color_count(),
            std::env::var_os("NO_COLOR").is_some(),
        )
    }

    fn with_capabilities(config: &AppConfig, colors: u16, no_color: bool) -> Self {
        let mode = match &config.ui.color_mode {
            ColorMode::Auto if no_color || colors < 256 => ColorMode::Mono,
            ColorMode::Auto if colors == u16::MAX => ColorMode::Truecolor,
            ColorMode::Auto => ColorMode::Ansi256,
            explicit => explicit.clone(),
        };
        let color = |r, g, b, index| match mode {
            ColorMode::Mono => Color::Reset,
            ColorMode::Ansi256 => Color::Indexed(index),
            _ => Color::Rgb(r, g, b),
        };
        let screen_fg = match config.ui.screen_color {
            ScreenColor::Green => color(155, 223, 164, 151),
            ScreenColor::Amber => color(240, 203, 138, 222),
            ScreenColor::White => color(216, 227, 216, 188),
        };
        Self {
            background: color(17, 21, 29, 233),
            panel: color(25, 31, 42, 234),
            border: color(55, 66, 82, 239),
            foreground: color(229, 234, 242, 255),
            muted_color: color(162, 175, 191, 249),
            accent: color(140, 175, 255, 111),
            selection_bg: color(35, 49, 72, 235),
            screen_bg: color(12, 21, 17, 232),
            screen_fg,
            notice: color(241, 196, 126, 222),
            success: color(155, 223, 164, 151),
            fault: color(242, 141, 149, 210),
            mono: mode == ColorMode::Mono,
            ascii: config.ui.border == BorderStyle::Ascii,
        }
    }
    // Leaf text inherits its containing surface's background.
    fn text(&self) -> Style {
        if self.mono {
            Style::default()
        } else {
            Style::default().fg(self.foreground)
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
            self.text().fg(self.muted_color)
        }
    }
    fn title(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.text().fg(self.accent).add_modifier(Modifier::BOLD)
        }
    }
    fn selected(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::REVERSED)
        } else {
            self.text()
                .fg(self.accent)
                .bg(self.selection_bg)
                .add_modifier(Modifier::BOLD)
        }
    }
    fn primary(&self) -> Style {
        if self.mono {
            self.selected().add_modifier(Modifier::BOLD)
        } else {
            Style::default()
                .fg(self.background)
                .bg(self.accent)
                .add_modifier(Modifier::BOLD)
        }
    }
    fn status(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::BOLD)
        } else {
            self.text().fg(self.notice)
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
        Block::default()
            .borders(Borders::ALL)
            .title(title)
            .title_style(self.muted())
            .style(self.panel())
            .border_style(if self.mono {
                Style::default()
            } else {
                Style::default().fg(self.border)
            })
            .border_set(if self.ascii {
                symbols::border::Set {
                    top_left: "+",
                    top_right: "+",
                    bottom_left: "+",
                    bottom_right: "+",
                    vertical_left: "|",
                    vertical_right: "|",
                    horizontal_top: "-",
                    horizontal_bottom: "-",
                }
            } else {
                symbols::border::ROUNDED
            })
    }

    fn horizontal_keys(&self) -> &'static str {
        if self.ascii { "Left/Right" } else { "←/→" }
    }
}

/// Drawing and mouse hit testing share terminal-cell geometry, including
/// the width of Chinese labels and the gaps between tabs.
fn menu_tabs(area: Rect) -> [(MenuKind, &'static str, Rect); 4] {
    let mut x = area.x.saturating_add(2);
    [
        (MenuKind::Emulator, " 模拟器 "),
        (MenuKind::Session, " 会话 "),
        (MenuKind::Display, " 显示 "),
        (MenuKind::Help, " 帮助 "),
    ]
    .map(|(kind, label)| {
        let width = label.width() as u16;
        let tab = Rect::new(x, area.y.saturating_add(1), width, 1).intersection(area);
        x = x.saturating_add(width + 1);
        (kind, label, tab)
    })
}

fn menu_popup(area: Rect, kind: MenuKind) -> Rect {
    let (_, _, tab) = menu_tabs(area)
        .into_iter()
        .find(|(candidate, _, _)| *candidate == kind)
        .expect("every menu has a tab");
    let entries = menu_entries(kind);
    let width = (entries.iter().map(|entry| entry.width()).max().unwrap_or(0) as u16 + 4)
        .max(14)
        .min(area.width);
    Rect::new(
        tab.x.min(area.right().saturating_sub(width)),
        tab.bottom(),
        width,
        (entries.len() as u16 + 2).min(area.bottom().saturating_sub(tab.bottom())),
    )
}

fn draw_menu_bar(frame: &mut Frame<'_>, area: Rect, active: Option<MenuKind>, theme: &Theme) {
    for (kind, label, tab) in menu_tabs(area) {
        frame.render_widget(
            Paragraph::new(label).style(if active == Some(kind) {
                theme.selected()
            } else {
                theme.muted()
            }),
            tab,
        );
    }
}

fn draw_center(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let width = area.width.saturating_sub(4).min(104);
    let wide = width >= 76;
    let height = area.height.min(if wide { 20 } else { 24 });
    let content = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 3,
        width,
        height,
    };
    let regions = if wide {
        Layout::horizontal([
            Constraint::Length(26),
            Constraint::Length(3),
            Constraint::Min(0),
        ])
    } else {
        Layout::vertical([
            Constraint::Length(7),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
    }
    .split(content);
    let columns = [regions[0], regions[2]];
    let machines = [
        ("Apple-1", "40×24 字符终端"),
        ("6502 内置演示", "计数程序与执行结果"),
    ];
    let items = machines.iter().enumerate().map(|(index, (name, caption))| {
        let selected = index == app.selected_machine;
        let marker = if selected { "> " } else { "  " };
        ListItem::new(vec![
            Line::from(format!("{marker}{name}")),
            Line::from(Span::styled(format!("  {caption}"), theme.muted())),
            Line::default(),
        ])
        .style(if selected {
            theme.selected()
        } else {
            theme.text()
        })
    });
    let list_rows = Layout::vertical([Constraint::Length(1), Constraint::Min(0)]).split(columns[0]);
    frame.render_widget(
        Paragraph::new("选择模拟器").style(theme.muted()),
        list_rows[0],
    );
    frame.render_widget(List::new(items), list_rows[1]);

    let action = app.center_action();
    let block = theme.block("").padding(Padding::new(2, 2, 1, 1));
    let inner = block.inner(columns[1]);
    frame.render_widget(block, columns[1]);
    // Keep the primary action visible even with long resource paths, warnings,
    // or the vertically stacked layout at the minimum terminal size.
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ])
    .split(inner);
    let parameter = |label: &'static str, value: String| {
        Line::from(vec![
            Span::styled(label, theme.muted()),
            Span::raw(truncate_path(
                &value,
                usize::from(inner.width.saturating_sub(6)),
            )),
        ])
    };
    let details = if app.selected_machine == 0 {
        let status = match action {
            CenterAction::Resume if app.faulted => "会话故障",
            CenterAction::Resume => "会话已保留",
            CenterAction::Launch => "已校验",
            _ if app.form.rom.is_empty() => "未配置",
            _ => "资源待检查",
        };
        let rom_path = if let Some(resources) = &app.resources {
            resources.rom_path.display().to_string()
        } else if app.form.rom.is_empty() {
            "尚未选择".into()
        } else {
            app.form.rom.clone()
        };
        let program = if let Some(resources) = &app.resources {
            resources.program.as_ref().map_or_else(
                || "未加载".into(),
                |_| {
                    format!(
                        "已加载 {} B @ ${:04X}",
                        resources.program_size(),
                        resources.program_address()
                    )
                },
            )
        } else if let Some(preset) = app.form.preset {
            format!("{} @ ${:04X}", preset.name, preset.load)
        } else if app.form.program.is_empty() {
            "未加载".into()
        } else {
            app.form.program.clone()
        };
        vec![
            Line::from(Span::styled(
                "Apple-1",
                theme.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled("6502 计算机 · 复古字符终端", theme.muted())),
            Line::default(),
            parameter("CPU   ", "MOS 6502 / NMOS".into()),
            parameter("显示  ", "40×24 字符".into()),
            parameter("RAM   ", "8 KiB（两组 4 KiB）".into()),
            parameter("ROM   ", format!("{status} · 256 B")),
            parameter("路径  ", rom_path),
            parameter("程序  ", program),
        ]
    } else {
        vec![
            Line::from(Span::styled(
                "6502 内置演示",
                theme.text().add_modifier(Modifier::BOLD),
            )),
            Line::from(Span::styled("运行一次，看清程序的执行结果", theme.muted())),
            Line::default(),
            parameter("CPU   ", "MOS 6502 / NMOS".into()),
            parameter("程序  ", "内置计数程序".into()),
            parameter("输出  ", "内存、寄存器与执行周期".into()),
            parameter("资源  ", "已内置 · 无需 ROM 文件".into()),
            Line::default(),
            Line::from(Span::styled("Apple-1 会话会继续保留。", theme.muted())),
        ]
    };
    frame.render_widget(Paragraph::new(details), rows[0]);
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(action.label(), theme.primary()))),
        rows[2],
    );
    let hint = app
        .config_warning
        .as_deref()
        .unwrap_or(if app.selected_machine == 0 {
            "C 配置    I 模拟器信息"
        } else {
            "运行后可按 R 重跑，Esc 返回。"
        });
    frame.render_widget(Paragraph::new(hint).style(theme.muted()), rows[3]);
}

fn draw_apple1(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let show_sidebar = app.config.ui.sidebar && area.width >= 80;
    let width = area.width.min(if show_sidebar { 92 } else { 44 });
    let content = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(26) / 3,
        width,
        height: area.height.min(26),
    };
    let sections = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Length(44), Constraint::Min(1)])
        .split(content);
    let screen_area = if show_sidebar { sections[0] } else { content };
    let screen_rect = Rect {
        x: screen_area.x.saturating_add(1),
        y: content.y,
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
                    .block(theme.block(" 会话状态 ").padding(Padding::new(1, 1, 1, 1)))
                    .style(theme.text())
                    .wrap(Wrap { trim: false }),
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

/// Keep the caret inside the field even for long paths and wide characters.
fn edit_view(edit: &ConfigEdit, width: u16) -> (&str, u16) {
    let mut start = edit.cursor;
    let mut cells = 0;
    for (index, ch) in edit.text[..edit.cursor].char_indices().rev() {
        let next = ch.to_string().width();
        if cells + next >= usize::from(width) {
            break;
        }
        cells += next;
        start = index;
    }
    (&edit.text[start..], cells as u16)
}

fn draw_config(frame: &mut Frame<'_>, area: Rect, app: &App, theme: &Theme) {
    let program = app
        .form
        .preset
        .map_or(app.form.program.as_str(), |preset| preset.name);
    let address = app.form.preset.map_or_else(
        || app.form.program_address.clone(),
        |preset| format!("0x{:04X}", preset.load),
    );
    let program_hint = app.form.preset.map_or_else(
        || "Enter 选择预置 · F4 浏览本地文件".into(),
        |preset| format!("预置 {} B · {}", preset.size(), preset.startup),
    );
    let card = centered(
        Rect::new(0, 0, area.width.saturating_sub(4).min(96), 26),
        area,
    );
    let block = theme.block(" 启动配置 ").padding(Padding::new(2, 2, 1, 1));
    let inner = block.inner(card);
    frame.render_widget(block, card);
    let rows = Layout::vertical([
        Constraint::Length(2),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(4),
        Constraint::Length(3),
        Constraint::Length(3),
        Constraint::Length(2),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(Line::from(vec![
            Span::styled("Apple-1", theme.title()),
            Span::styled("  /  启动资源", theme.muted()),
        ])),
        rows[0],
    );
    for (index, (focus, label, value, placeholder, hint)) in [
        (
            ConfigFocus::Rom,
            "01  ROM 镜像 · 必填",
            app.form.rom.as_str(),
            "选择 Woz Monitor 镜像",
            "Woz Monitor · 256 B · 启动时校验镜像",
        ),
        (
            ConfigFocus::Program,
            "02  程序 · F3",
            program,
            "未选择程序",
            program_hint.as_str(),
        ),
        (
            ConfigFocus::ProgramAddress,
            "03  加载地址",
            address.as_str(),
            "输入程序加载地址",
            if app.form.preset.is_some() {
                "预置程序自动设置加载地址"
            } else {
                "十进制 / 0xE000 / $E000"
            },
        ),
    ]
    .into_iter()
    .enumerate()
    {
        let field_rows =
            Layout::vertical([Constraint::Length(3), Constraint::Length(1)]).split(rows[index + 1]);
        let selected = app.form.focus == focus;
        let edit = app.form.edit.as_ref().filter(|_| selected);
        let active = selected && app.overlay.is_none();
        let mut block = theme.block(format!(" {label} "));
        if active {
            let style = if edit.is_some() {
                theme.status()
            } else {
                theme.title()
            };
            block = block.border_style(style).title_style(style).title(
                Line::from(if edit.is_some() {
                    " 编辑中 "
                } else if focus == ConfigFocus::Program {
                    " Enter 选择 "
                } else if app.form.preset.is_some() && focus == ConfigFocus::ProgramAddress {
                    " 固定地址 "
                } else {
                    " Enter 编辑 "
                })
                .right_aligned(),
            );
        }
        let input = block.inner(field_rows[0]).inner(Margin::new(1, 0));
        frame.render_widget(block, field_rows[0]);
        if let Some(edit) = edit {
            let (visible, cursor) = edit_view(edit, input.width);
            frame.render_widget(Paragraph::new(visible).style(theme.text()), input);
            if active && input.width > 0 {
                frame.set_cursor_position((input.x + cursor, input.y));
            }
        } else {
            let (text, style) = if value.is_empty() {
                (placeholder.to_owned(), theme.muted())
            } else {
                (truncate_path(value, usize::from(input.width)), theme.text())
            };
            frame.render_widget(
                Paragraph::new(text).style(if active { theme.selected() } else { style }),
                input,
            );
        }
        frame.render_widget(
            Paragraph::new(format!(" {hint}")).style(theme.muted()),
            field_rows[1],
        );
    }
    let status = app
        .form
        .status
        .as_deref()
        .unwrap_or(if app.form.edit.is_some() {
            "Enter 确认修改，Esc 放弃本次编辑。"
        } else {
            "Enter 选择程序；其他字段 Enter 编辑；F4 浏览路径。"
        });
    frame.render_widget(
        Paragraph::new(status)
            .style(if app.form.status.is_some() {
                theme.status()
            } else {
                theme.muted()
            })
            .wrap(Wrap { trim: false }),
        rows[4],
    );
    let buttons = Layout::horizontal([
        Constraint::Length(14),
        Constraint::Length(2),
        Constraint::Length(10),
        Constraint::Length(2),
        Constraint::Length(8),
    ])
    .split(rows[5]);
    for (index, (focus, label)) in [
        (ConfigFocus::Validate, "校验并保存"),
        (ConfigFocus::Launch, "启动"),
        (ConfigFocus::Cancel, "取消"),
    ]
    .into_iter()
    .enumerate()
    {
        let selected = app.form.focus == focus && app.overlay.is_none();
        let style = if selected {
            theme.primary()
        } else if focus == ConfigFocus::Launch {
            theme.title()
        } else {
            theme.muted()
        };
        frame.render_widget(
            Paragraph::new(label)
                .centered()
                .block(theme.block("").border_style(if selected {
                    theme.title()
                } else {
                    theme.muted()
                }))
                .style(style),
            buttons[index * 2],
        );
    }
    let path = app
        .config_path
        .as_ref()
        .map_or_else(|| "不可用".into(), |path| path.display().to_string());
    frame.render_widget(
        Paragraph::new(vec![
            Line::from("配置文件"),
            Line::from(truncate_path(&path, usize::from(rows[6].width))),
        ])
        .style(theme.muted()),
        rows[6],
    );
}

fn draw_info(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let text = "Apple-1 固定机器配置\n\n$0000–$0FFF：4 KiB RAM\n$E000–$EFFF：4 KiB RAM（可加载 BASIC）\n$D010–$D013 及硬件别名：MC6821 PIA（键盘与显示）\n$FF00–$FFFF：256 B Woz Monitor ROM\n\n当前模型驱动 NMOS 6502、PIA 与 40×24 字符显示。DRAM 刷新与显示忙时序按仓库已有近似建模；不宣称 Apple II、非官方 opcode 或所有板卡修订的兼容性。";
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("模拟器信息"))
            .style(theme.text())
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
            .style(theme.text()),
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
        format!(
            "鼠标菜单：{}",
            if app.config.ui.mouse { "开" } else { "关" }
        ),
        "保存设置".to_owned(),
    ];
    let items: Vec<ListItem<'_>> = values
        .iter()
        .enumerate()
        .map(|(index, text)| {
            ListItem::new(text.as_str()).style(if index == app.settings_row {
                theme.selected()
            } else {
                theme.text()
            })
        })
        .collect();
    frame.render_widget(
        List::new(items)
            .block(theme.block("显示设置"))
            .style(theme.text()),
        area,
    );
}

fn draw_help(frame: &mut Frame<'_>, area: Rect, theme: &Theme) {
    let text = "Apple-1 运行页\nF1 帮助 · F2 会话菜单 · F3 显示/隐藏侧栏 · F10 顶栏菜单\nCtrl+P 暂停/继续 · Ctrl+R 物理 RESET · Ctrl+L CLEAR SCREEN · Ctrl+N 重新上电\nCtrl+C / Ctrl+D 退出 · Enter 发送 CR · Backspace 发送 _ · Esc 发送机器取消输入\n\n鼠标：点击顶栏展开下拉菜单，移动选择，点击菜单项执行；再次点击标题或点击外部收起。显示设置可开关鼠标菜单，旧配置若已关闭，可用 F10 进入设置开启。\n\n菜单、表单和确认弹窗会阻止机器自由运行；它们获得焦点时按键不会漏给机器。配置页默认聚焦「程序」字段，直接按 Enter 即打开预置程序选择框；方向键或 Tab/Shift+Tab 选择其他字段，Enter 进入编辑，再按 Enter 确认，Esc 取消编辑。编辑时可粘贴单行文本，左右键/Home/End 移动光标，Ctrl+U 清空；路径可按 F4 浏览。F3 在任何字段都能打开预置程序选择框；BASIC (Huston) 自动加载到 $E000，启动后输入 E000R。机器屏幕页的粘贴会将 CR/LF 规范为单个 CR。";
    frame.render_widget(
        Paragraph::new(text)
            .block(theme.block("帮助"))
            .style(theme.text())
            .wrap(Wrap { trim: false }),
        area,
    );
}

/// Rows of the program picker: `0` is "load no program", `1` is "local binary
/// file", and `PICKER_FIXED_ROWS + n` is `APPLE1_PRESETS[n]`. The site's four
/// categories appear as header rows between them and are not indices, so no
/// keystroke can land on one.
const PICKER_FIXED_ROWS: usize = 2;

/// Screen row of a picker index, counting the category headers above it.
fn picker_screen_row(index: usize) -> usize {
    if index < PICKER_FIXED_ROWS {
        return index;
    }
    let position = index - PICKER_FIXED_ROWS;
    let headers = Category::ALL
        .iter()
        .filter(|category| first_preset_of(**category) <= position)
        .count();
    PICKER_FIXED_ROWS + position + headers
}

/// Picker index of a screen row; `None` on a category header.
fn picker_index_at_row(row: usize) -> Option<usize> {
    (0..PICKER_FIXED_ROWS + APPLE1_PRESETS.len()).find(|index| picker_screen_row(*index) == row)
}

/// The next selectable row in one direction: headers are skipped and the ends
/// of the list hold.
fn picker_step(row: usize, forward: bool) -> usize {
    let last = picker_screen_row(PICKER_FIXED_ROWS + APPLE1_PRESETS.len() - 1);
    let mut row = row;
    loop {
        let next = if forward {
            if row >= last {
                return last;
            }
            row + 1
        } else {
            if row == 0 {
                return 0;
            }
            row - 1
        };
        row = next;
        if picker_index_at_row(row).is_some() {
            return row;
        }
    }
}

/// First program row of a neighbouring category, so Left/Right walk the same
/// four categories the site publishes.
fn picker_category_step(row: usize, forward: bool) -> usize {
    let current = picker_index_at_row(row)
        .and_then(|index| index.checked_sub(PICKER_FIXED_ROWS))
        .map(|position| APPLE1_PRESETS[position].category);
    let target = match (current, forward) {
        (Some(category), true) => Category::ALL
            .iter()
            .copied()
            .skip_while(|item| *item != category)
            .nth(1),
        (Some(category), false) => Category::ALL
            .iter()
            .copied()
            .take_while(|item| *item != category)
            .last(),
        (None, true) => Category::ALL.first().copied(),
        (None, false) => None,
    };
    match (target, current, forward) {
        (Some(category), _, _) => picker_screen_row(PICKER_FIXED_ROWS + first_preset_of(category)),
        (None, Some(_), _) => picker_screen_row(1),
        (None, None, _) => picker_screen_row(0),
    }
}

/// Catalogue position of a category's first program.
fn first_preset_of(category: Category) -> usize {
    APPLE1_PRESETS
        .iter()
        .position(|preset| preset.category == category)
        .unwrap_or(0)
}

/// The picker list: the two fixed rows, then every category with its programs.
fn picker_items(theme: &Theme) -> Vec<ListItem<'static>> {
    let mut items =
        Vec::with_capacity(PICKER_FIXED_ROWS + APPLE1_PRESETS.len() + Category::ALL.len());
    items.push(ListItem::new("不加载程序").style(theme.text()));
    items.push(ListItem::new("本地二进制文件…").style(theme.text()));
    for category in Category::ALL {
        items.push(ListItem::new(category.label()).style(theme.title()));
        for preset in ProgramPreset::in_category(category) {
            items.push(ListItem::new(format!("  {}", preset.name)).style(theme.text()));
        }
    }
    items
}

fn draw_overlay(frame: &mut Frame<'_>, area: Rect, overlay: &mut Overlay, theme: &Theme) {
    let popup = if let Overlay::Menu { kind, .. } = overlay {
        menu_popup(frame.area(), *kind)
    } else {
        let width = area.width.saturating_sub(8).clamp(20, 84);
        let height = match overlay {
            Overlay::Programs { .. } => {
                (PICKER_FIXED_ROWS + APPLE1_PRESETS.len() + Category::ALL.len() + 8) as u16
            }
            Overlay::Browser { .. } => area.height.saturating_sub(6).min(20),
            Overlay::RebootConfirm { .. } | Overlay::ReplaceConfirm { .. } => area.height.min(12),
            _ => 5,
        }
        .min(area.height);
        centered(
            Rect {
                x: area.x,
                y: area.y,
                width,
                height,
            },
            area,
        )
    };
    // Clear alone can leave half a Chinese character crossing a popup edge.
    // Blank both halves, or the terminal diff may skip the new border cell.
    let buffer = frame.buffer_mut();
    for y in popup.top()..popup.bottom() {
        for x in [popup.left(), popup.right()] {
            if x > buffer.area.left()
                && x < buffer.area.right()
                && buffer[(x - 1, y)].symbol().width() > 1
            {
                buffer[(x - 1, y)].set_char(' ');
                buffer[(x, y)].set_char(' ');
            }
        }
    }
    frame.render_widget(Clear, popup);
    match overlay {
        Overlay::Programs { state } => {
            let block = theme.block(" 选择程序 ").padding(Padding::horizontal(1));
            let inner = block.inner(popup);
            frame.render_widget(block.style(theme.panel()), popup);
            // The detail pane is what tells the user how to start the machine
            // after the pick, so it gets its own fixed rows: identity, load
            // range, start command, source.
            let rows = Layout::vertical([Constraint::Min(3), Constraint::Length(4)]).split(inner);
            frame.render_stateful_widget(
                List::new(picker_items(theme))
                    .style(theme.panel())
                    .highlight_style(theme.selected()),
                rows[0],
                state,
            );
            let selected = state
                .selected()
                .and_then(picker_index_at_row)
                .and_then(|index| index.checked_sub(PICKER_FIXED_ROWS))
                .map(|position| &APPLE1_PRESETS[position]);
            let detail = selected.map_or_else(
                || {
                    vec![
                        Line::from(Span::styled("未选择预置程序", theme.text())),
                        Line::from(Span::styled(
                            "Enter 确认 · 左右键切换分类 · 上下键移动",
                            theme.muted(),
                        )),
                    ]
                },
                |preset| {
                    vec![
                        Line::from(Span::styled(
                            format!("{} · {} {}", preset.name, preset.author, preset.year),
                            theme.text(),
                        )),
                        Line::from(Span::styled(
                            if preset.needs_expansion {
                                format!(
                                    "{} B · {} · 需要 $1000–$1FFF 扩展内存（本机未建模）",
                                    preset.size(),
                                    preset.ranges()
                                )
                            } else {
                                format!("{} B · {}", preset.size(), preset.ranges())
                            },
                            theme.muted(),
                        )),
                        Line::from(Span::styled(preset.startup, theme.status())),
                        Line::from(Span::styled(
                            format!("{} · {}", preset.source, preset.license_label()),
                            theme.muted(),
                        )),
                    ]
                },
            );
            frame.render_widget(Paragraph::new(detail).style(theme.panel()), rows[1]);
        }
        Overlay::Menu { kind, selected } => {
            let lines = menu_entries(*kind)
                .iter()
                .enumerate()
                .map(|(index, text)| {
                    ListItem::new(*text).style(if index == *selected {
                        theme.selected()
                    } else {
                        theme.text()
                    })
                })
                .collect::<Vec<_>>();
            frame.render_widget(
                List::new(lines)
                    .block(theme.block("").padding(Padding::horizontal(1)))
                    .style(theme.panel()),
                popup,
            );
        }
        Overlay::RebootConfirm { confirmed } => {
            draw_confirmation(
                frame,
                popup,
                theme,
                "重新上电",
                "重新上电会丢弃当前 RAM 和设备状态；会话周期预算与 trace 保留。".into(),
                *confirmed,
            );
        }
        Overlay::ReplaceConfirm {
            resources,
            confirmed,
            ..
        } => {
            draw_confirmation(
                frame,
                popup,
                theme,
                "替换并启动",
                format!(
                    "替换会丢弃当前 RAM、设备状态和输入队列；会话周期预算与 trace 保留。\nROM：{}",
                    resources.rom_path.display()
                ),
                *confirmed,
            );
        }
        Overlay::QuitConfirm => frame.render_widget(
            Paragraph::new("退出会结束本次进程内会话。\nEnter 确认  Esc 取消")
                .block(theme.block("确认退出"))
                .style(theme.panel()),
            popup,
        ),
        Overlay::Error(error) => frame.render_widget(
            Paragraph::new(format!("{error}\n\nEnter / Esc 关闭"))
                .block(theme.block("错误"))
                .style(theme.panel())
                .wrap(Wrap { trim: false }),
            popup,
        ),
        Overlay::Browser {
            target,
            directory,
            entries,
            state,
            error,
        } => {
            let header = format!(
                "浏览 {}：{}",
                if *target == PathTarget::Rom {
                    "ROM"
                } else {
                    "程序"
                },
                truncate_path(
                    &directory.to_string_lossy(),
                    usize::from(popup.width.saturating_sub(6))
                )
            );
            let block = theme.block(header);
            let inner = block.inner(popup);
            frame.render_widget(block, popup);
            let rows = Layout::vertical([
                Constraint::Length(1),
                Constraint::Length(u16::from(error.is_some())),
                Constraint::Min(0),
            ])
            .split(inner);
            frame.render_widget(
                Paragraph::new("Backspace 上一级；Enter 选择；Esc 返回").style(theme.panel()),
                rows[0],
            );
            if let Some(error) = error {
                frame.render_widget(
                    Paragraph::new(error.as_str()).style(theme.status()),
                    rows[1],
                );
            }
            let items = entries.iter().map(|entry| {
                let label = if entry.directory {
                    format!("{}/", entry.name)
                } else {
                    entry.name.clone()
                };
                ListItem::new(truncate_path(&label, usize::from(rows[2].width)))
            });
            frame.render_stateful_widget(
                List::new(items)
                    .style(theme.panel())
                    .highlight_style(theme.selected()),
                rows[2],
                state,
            );
        }
    }
}

fn draw_too_small(frame: &mut Frame<'_>, app: &App) {
    let area = frame.area();
    let theme = Theme::from_config(&app.config);
    let text = format!(
        "窗口过小：需要至少 {MIN_WIDTH}×{MIN_HEIGHT}，当前 {}×{}。\n机器已暂停。Ctrl+C/Ctrl+D 退出；有会话时 Enter 确认退出，Esc 取消。",
        area.width, area.height
    );
    frame.render_widget(Paragraph::new(text).wrap(Wrap { trim: true }), area);
    if let Some(Overlay::QuitConfirm) = &app.overlay {
        frame.render_widget(
            Paragraph::new("确认退出？ Enter 确认 / Esc 取消").block(theme.block("")),
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

fn draw_confirmation(
    frame: &mut Frame<'_>,
    popup: Rect,
    theme: &Theme,
    action: &str,
    text: String,
    confirmed: bool,
) {
    let block = theme.block(format!("确认{action}"));
    let inner = block.inner(popup);
    frame.render_widget(block, popup);
    // Reserve the action row so long paths or wrapped Chinese text cannot
    // push the selected button and its keyboard instructions out of view.
    let rows = Layout::vertical([
        Constraint::Min(0),
        Constraint::Length(2),
        Constraint::Length(1),
    ])
    .split(inner);
    frame.render_widget(
        Paragraph::new(text)
            .style(theme.panel())
            .wrap(Wrap { trim: false }),
        rows[0],
    );
    frame.render_widget(
        Paragraph::new(format!(
            "{} / Tab 选择\nEnter 执行；Esc 取消",
            theme.horizontal_keys()
        ))
        .style(theme.muted()),
        rows[1],
    );
    frame.render_widget(
        Paragraph::new(confirmation_buttons(confirmed, action, theme)).style(theme.panel()),
        rows[2],
    );
}

fn confirmation_buttons(confirmed: bool, action: &str, theme: &Theme) -> Line<'static> {
    Line::from(vec![
        Span::styled(
            "[取消]",
            if confirmed {
                theme.text()
            } else {
                theme.selected()
            },
        ),
        Span::raw("  "),
        Span::styled(
            format!("[{action}]"),
            if confirmed {
                theme.selected()
            } else {
                theme.text()
            },
        ),
    ])
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
    fn public_tui_entry_rejects_invalid_trace_limits_before_terminal_setup() {
        for trace_limit in [0, 4097, usize::MAX] {
            for enabled in [false, true] {
                let launch = Apple1Launch {
                    trace_limit,
                    trace: enabled,
                    bus_trace: enabled,
                    ..Apple1Launch::default()
                };
                assert_eq!(
                    run(Some(launch)).unwrap_err().to_string(),
                    "--trace-limit requires 1..4096"
                );
            }
        }
        assert_eq!(Apple1Launch::default().trace_limit, 64);
        for limit in [1, 64, 4096] {
            assert!(TraceOptions::new(true, true, limit).is_ok());
        }
    }

    fn resources(path: &str) -> Resources {
        Resources {
            rom: test_rom(),
            program: Some(ProgramImage::single(0, vec![0x4c, 0x00, 0x00]).unwrap()),
            rom_path: PathBuf::from(path),
            preset: None,
        }
    }

    #[test]
    fn preset_picker_supports_cancel_file_and_empty_selection_at_all_sizes() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.program = "/original/program.bin".into();
        app.form.program_address = "0x0200".into();
        let key = |app: &mut App, code| app.handle_key(KeyEvent::new(code, KeyModifiers::NONE));
        key(&mut app, KeyCode::F(3));
        key(&mut app, KeyCode::Down);
        app.handle_paste("ignored".into());
        key(&mut app, KeyCode::Esc);
        assert!(app.form.preset.is_none());
        assert_eq!(app.form.program, "/original/program.bin");
        key(&mut app, KeyCode::F(3));
        key(&mut app, KeyCode::Down);
        key(&mut app, KeyCode::Enter);
        // Down crosses the two fixed rows and the Games category header, so it
        // lands on the first Games program, never on the header itself.
        assert_eq!(app.form.preset, Some(&APPLE1_PRESETS[0]));
        assert_eq!(APPLE1_PRESETS[0].id, "15-puzzle");
        assert!(app.input.is_empty());

        for (width, height) in [(44, 30), (80, 30), (120, 40)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text = buffer_text(terminal.backend().buffer());
            assert!(
                text.contains("15 Puzzle") && text.contains("0x0300"),
                "{text}"
            );
            assert!(
                text.replace(' ', "").contains("02程序·F3"),
                "field titles must not overlap: {text}"
            );
            key(&mut app, KeyCode::Enter);
            terminal.draw(|frame| app.draw(frame)).unwrap();
            let text = buffer_text(terminal.backend().buffer());
            assert!(
                text.contains("15 Puzzle") && text.contains("0300R"),
                "the picker shows how to start the program: {text}"
            );
            key(&mut app, KeyCode::Esc);
        }
        key(&mut app, KeyCode::Tab);
        key(&mut app, KeyCode::Enter);
        assert!(app.form.edit.is_none(), "a preset's address is fixed");
        assert_eq!(
            app.form.program_address, "0x0200",
            "keep the external file's address"
        );

        key(&mut app, KeyCode::F(3));
        key(&mut app, KeyCode::Up);
        key(&mut app, KeyCode::Enter);
        assert!(app.form.preset.is_none());
        assert_eq!(
            app.form.edit.as_ref().unwrap().text,
            "/original/program.bin"
        );
        key(&mut app, KeyCode::Esc);
        key(&mut app, KeyCode::F(3));
        key(&mut app, KeyCode::Up);
        key(&mut app, KeyCode::Enter);
        assert!(app.form.preset.is_none() && app.form.program.is_empty());
        assert_eq!(app.form.program_address, "0x0000");
    }

    /// The picker mirrors the site's four categories, and a header is a label
    /// rather than a choice: Up/Down, Left/Right and the drawn row count all
    /// agree with that.
    #[test]
    fn program_picker_walks_the_published_categories() {
        let last = picker_screen_row(PICKER_FIXED_ROWS + APPLE1_PRESETS.len() - 1);
        let mut row = 0;
        while row < last {
            let next = picker_step(row, true);
            assert!(next > row, "Down at row {row} must advance, got {next}");
            assert!(
                picker_index_at_row(next).is_some(),
                "row {next} is a category header"
            );
            row = next;
        }
        assert_eq!(row, last, "Down reaches the last program");
        while row > 0 {
            let previous = picker_step(row, false);
            assert!(previous < row, "Up at row {row} must retreat");
            assert!(picker_index_at_row(previous).is_some());
            row = previous;
        }
        assert_eq!(row, 0, "Up returns to the first fixed row");

        for category in Category::ALL {
            row = picker_category_step(row, true);
            assert_eq!(
                picker_index_at_row(row).map(|index| index - PICKER_FIXED_ROWS),
                Some(first_preset_of(category)),
                "Right reaches {}",
                category.label()
            );
        }
        assert_eq!(
            last + 1,
            picker_items(&Theme::from_config(&AppConfig::default())).len(),
            "the drawn list has one row per picker row"
        );
    }

    #[test]
    fn bundled_image_survives_reset_and_is_restored_on_reboot() {
        use hesper_cpu6502::Bus;

        let preset = ProgramPreset::find("basic-huston").expect("basic-huston preset");
        let mut app = App::new(Some(Apple1Launch {
            preset: Some(preset),
            max_cycles: Some(1_000_000),
            ..Apple1Launch::default()
        }));
        app.config_path = None;
        assert_eq!(app.form.preset, Some(preset));
        let image = ProgramSource::Preset(preset).load().unwrap();
        let mut resources = resources("/unused/rom.bin");
        resources.rom[..3].copy_from_slice(&[0x4C, 0x00, 0xFF]); // JMP $FF00
        resources.rom[0xFC..0xFE].copy_from_slice(&0xFF00u16.to_le_bytes());
        resources.program = Some(image);
        resources.preset = Some(preset);
        app.start_resources(resources, false);
        assert!(!app.faulted);
        let bus = app.session.as_mut().unwrap().machine_mut().bus_mut();
        for block in preset.blocks {
            for (index, byte) in block.bytes.iter().enumerate() {
                assert_eq!(bus.read(block.address + index as u16), *byte);
            }
        }
        bus.write(0xE000, 0xEA);
        app.reset();
        assert_eq!(
            app.session
                .as_mut()
                .unwrap()
                .machine_mut()
                .bus_mut()
                .read(0xE000),
            0xEA
        );
        let before = app.session.as_ref().unwrap().total_cpu_cycles();
        app.form.preset = None;
        app.reboot();
        assert!(!app.faulted);
        assert_eq!(
            app.session
                .as_mut()
                .unwrap()
                .machine_mut()
                .bus_mut()
                .read(0xE000),
            0x4C
        );
        assert!(app.session.as_ref().unwrap().total_cpu_cycles() > before);
        assert_eq!(app.resources.as_ref().unwrap().preset, Some(preset));
    }

    #[test]
    fn high_program_address_survives_reset_and_reboot() {
        use hesper_cpu6502::Bus;

        let mut app = App::new(Some(Apple1Launch {
            program_address: 0xE000,
            max_cycles: Some(1_000_000),
            ..Apple1Launch::default()
        }));
        app.config_path = None;
        let mut resources = resources("/unused/rom.bin");
        resources.rom[0xFC..0xFE].copy_from_slice(&0xE000u16.to_le_bytes());
        resources.program =
            Some(ProgramImage::single(app.launch.program_address, vec![0x4C, 0x00, 0xE0]).unwrap());
        app.start_resources(resources, false);
        assert!(!app.faulted);
        let bus = app.session.as_mut().unwrap().machine_mut().bus_mut();
        assert_eq!(bus.read(0xE000), 0x4C);
        assert_eq!(bus.read(0x0000), 0);
        bus.write(0x0300, 0x12);
        bus.write(0xE100, 0x34);
        app.reset();
        assert!(!app.faulted);
        let bus = app.session.as_mut().unwrap().machine_mut().bus_mut();
        assert_eq!(bus.read(0x0300), 0x12);
        assert_eq!(bus.read(0xE100), 0x34);
        // Reboot uses the active resource address, even if launch settings differ.
        app.launch.program_address = 0;
        app.reboot();
        assert!(!app.faulted);
        let bus = app.session.as_mut().unwrap().machine_mut().bus_mut();
        assert_eq!(bus.read(0xE000), 0x4C);
        assert_eq!(bus.read(0xE002), 0xE0);
        assert_eq!(bus.read(0xE100), 0);
        assert_eq!(bus.read(0x0300), 0);
        assert_eq!(app.resources.as_ref().unwrap().program_address(), 0xE000);
    }

    #[test]
    fn config_fields_require_enter_and_escape_cancels_only_the_edit() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "/rom.bin".into();
        assert_eq!(
            app.form.focus,
            ConfigFocus::Program,
            "the program row is the default focus"
        );
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.handle_event(Event::Paste("ignored".into()));
        assert_eq!(app.form.rom, "/rom.bin", "selection must not edit a field");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(
            matches!(app.overlay, Some(Overlay::Programs { .. })),
            "Enter on the program row opens the picker directly"
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Config, "Escape returns to the form");
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Config, "Escape cancels editing first");
        assert_eq!(app.form.rom, "/rom.bin");
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        assert_eq!(app.form.focus, ConfigFocus::Program);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        // The picker's second row is the local-file path editor.
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_event(Event::Paste("/program.bin".into()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.program, "/program.bin");
        assert_eq!(app.form.focus, ConfigFocus::Program);
        app.handle_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(app.form.focus, ConfigFocus::Rom);
    }

    #[test]
    fn config_edit_moves_and_deletes_at_unicode_boundaries() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "原始.bin".into();
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        app.handle_event(Event::Paste("甲乙.bin".into()));
        for key in [KeyCode::Home, KeyCode::Right, KeyCode::Delete] {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
        }
        app.handle_event(Event::Paste("丙".into()));
        assert_eq!(app.form.edit.as_ref().unwrap().text, "甲丙.bin");
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        assert_eq!(app.form.edit.as_ref().unwrap().text, "甲.bin");
        // Navigation and modified host shortcuts cannot mutate or commit a draft.
        for key in [KeyCode::Tab, KeyCode::BackTab, KeyCode::Up, KeyCode::Down] {
            app.handle_key(KeyEvent::new(key, KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert_eq!(app.form.focus, ConfigFocus::Rom);
        assert_eq!(app.form.rom, "原始.bin");
        app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
        app.handle_event(Event::Paste(".bak".into()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.rom, "甲.bin.bak");
        assert!(app.input.is_empty());
    }

    #[test]
    fn config_layout_keeps_actions_and_edit_cursor_visible_after_resize() {
        for mode in [ColorMode::Truecolor, ColorMode::Mono] {
            let mut app = App::new(None);
            app.page = Page::Config;
            app.config.ui.color_mode = mode;
            app.form.rom = format!("/{}wozmon.bin", "很长的目录/".repeat(30));
            let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(!terminal.backend().cursor_visible());
            app.form.focus = ConfigFocus::Rom;
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            for (width, height) in [(180, 50), (80, 30), (44, 30), (120, 40)] {
                terminal.backend_mut().resize(width, height);
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let text = buffer_text(terminal.backend().buffer()).replace(' ', "");
                for label in [
                    "校验并保存",
                    "启动",
                    "取消",
                    "配置文件",
                    "编辑中",
                    "wozmon.bin",
                ] {
                    assert!(text.contains(label), "{width}x{height}: {label}");
                }
                assert!(terminal.backend().cursor_visible());
                let caret = terminal.backend().cursor_position();
                assert!(caret.x < width - 3 && caret.y < height - 2);
                let theme = Theme::from_config(&app.config);
                let hints = footer(&app, width, &theme).to_string();
                assert!(hints.contains("Enter 确认") && hints.contains("Esc 取消编辑"));
                app.handle_key(KeyEvent::new(KeyCode::Home, KeyModifiers::NONE));
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let home = terminal.backend().cursor_position();
                assert_eq!(home.y, caret.y);
                assert!(home.x < caret.x);
                app.handle_key(KeyEvent::new(KeyCode::End, KeyModifiers::NONE));
            }
            app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(!terminal.backend().cursor_visible());
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(terminal.backend().cursor_visible());
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(!terminal.backend().cursor_visible());
            assert_eq!(app.page, Page::Config);
        }
    }

    #[test]
    fn config_browser_returns_to_the_draft_and_escape_restores_the_original() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "/original.bin".into();
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE));
        assert!(matches!(
            app.overlay,
            Some(Overlay::Browser {
                target: PathTarget::Rom,
                ..
            })
        ));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert!(app.form.edit.is_some());
        app.overlay = Some(Overlay::Browser {
            target: PathTarget::Rom,
            directory: PathBuf::from("/files"),
            entries: vec![DirEntry {
                path: PathBuf::from("/files/selected.bin"),
                name: "selected.bin".into(),
                directory: false,
            }],
            state: ListState::default().with_selected(Some(0)),
            error: None,
        });
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.overlay.is_none());
        assert_eq!(app.form.edit.as_ref().unwrap().text, "/files/selected.bin");
        assert_eq!(app.form.rom, "/original.bin");
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.form.rom, "/original.bin");
        assert_eq!(app.page, Page::Config);
    }

    #[test]
    fn program_address_field_supports_navigation_editing_and_single_line_paste() {
        let mut app = App::new(Some(Apple1Launch {
            program_address: 0xE000,
            ..Apple1Launch::default()
        }));
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "/rom.bin".into();
        app.form.program = "/program.bin".into();
        assert_eq!(app.form.program_address, "0xE000");
        app.form.focus = ConfigFocus::Rom;
        for expected in [ConfigFocus::Program, ConfigFocus::ProgramAddress] {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(app.form.focus, expected);
        }
        app.handle_key(KeyEvent::new(KeyCode::F(4), KeyModifiers::NONE));
        assert!(app.overlay.is_none(), "an address is not a file path");
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert!(app.form.edit.as_ref().unwrap().text.is_empty());
        for ch in "0xE001".chars() {
            app.handle_key(KeyEvent::new(KeyCode::Char(ch), KeyModifiers::NONE));
        }
        app.handle_key(KeyEvent::new(KeyCode::Backspace, KeyModifiers::NONE));
        app.handle_event(Event::Paste("0".into()));
        assert_eq!(app.form.edit.as_ref().unwrap().text, "0xE000");
        assert_eq!(
            parse_program_address(&app.form.edit.as_ref().unwrap().text).unwrap(),
            0xE000
        );
        for paste in ["\n300", "\r300", "\0", "\u{1b}"] {
            app.handle_event(Event::Paste(paste.into()));
            assert_eq!(app.form.edit.as_ref().unwrap().text, "0xE000");
            assert!(
                app.form
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("地址粘贴未接收")
            );
        }
        assert_eq!(app.form.rom, "/rom.bin");
        assert_eq!(app.form.program, "/program.bin");
        assert!(app.input.is_empty());
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.form.edit.is_none());
        assert_eq!(app.form.program_address, "0xE000");
        assert_eq!(app.form.focus, ConfigFocus::ProgramAddress);
        for expected in [
            ConfigFocus::Validate,
            ConfigFocus::Launch,
            ConfigFocus::Cancel,
            ConfigFocus::Rom,
        ] {
            app.handle_key(KeyEvent::new(KeyCode::Tab, KeyModifiers::NONE));
            assert_eq!(app.form.focus, expected);
        }
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.focus, ConfigFocus::Rom);
        assert!(app.form.edit.is_none());
    }

    #[test]
    fn invalid_edited_address_cannot_replace_a_running_machine() {
        let mut app = app_with_session(100_000);
        app.page = Page::Config;
        let before_registers = app.session.as_ref().unwrap().machine().cpu().registers();
        let before_config = app.config.clone();
        for address in ["", "65536", "0x10000", "E000", "-1"] {
            app.form.program_address = address.into();
            app.start_configured(true);
            assert!(
                app.form
                    .status
                    .as_deref()
                    .unwrap()
                    .contains("程序加载地址无效")
            );
            assert!(app.overlay.is_none());
            assert_eq!(app.config, before_config);
            assert_eq!(
                app.session.as_ref().unwrap().machine().cpu().registers(),
                before_registers
            );
            assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
            assert_eq!(app.resources.as_ref().unwrap().program_address(), 0);
            assert_eq!(app.input, VecDeque::from(*b"A"));
        }
    }

    fn buffer_text(buffer: &Buffer) -> String {
        buffer.content.iter().map(|cell| cell.symbol()).collect()
    }

    fn mouse(app: &mut App, kind: MouseEventKind, column: u16, row: u16) {
        app.handle_event(Event::Mouse(MouseEvent {
            kind,
            column,
            row,
            modifiers: KeyModifiers::NONE,
        }));
    }

    #[test]
    fn dropdowns_render_below_the_clicked_chinese_tab_at_each_terminal_size() {
        for (width, height) in [(44, 30), (80, 30), (120, 40), (180, 50)] {
            for border in [BorderStyle::Rounded, BorderStyle::Ascii] {
                let mut app = app_with_session(100_000);
                app.config.ui.border = border.clone();
                let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                for (kind, x) in [
                    (MenuKind::Emulator, 2),
                    (MenuKind::Session, 11),
                    (MenuKind::Display, 18),
                    (MenuKind::Help, 25),
                ] {
                    app.return_to_center();
                    terminal.draw(|frame| app.draw(frame)).unwrap();
                    mouse(&mut app, MouseEventKind::Down(MouseButton::Left), x + 2, 1);
                    assert!(
                        matches!(app.overlay, Some(Overlay::Menu { kind: active, selected: 0 }) if active == kind)
                    );
                    let frame = terminal.draw(|frame| app.draw(frame)).unwrap();
                    assert_eq!(
                        frame.buffer[(x, 2)].symbol(),
                        if border == BorderStyle::Ascii {
                            "+"
                        } else {
                            "╭"
                        }
                    );
                    for (index, entry) in menu_entries(kind).iter().enumerate() {
                        let row = (x + 2..width)
                            .map(|column| frame.buffer[(column, index as u16 + 3)].symbol())
                            .collect::<String>();
                        assert!(row.replace(' ', "").starts_with(&entry.replace(' ', "")));
                    }
                    // The emitted diff must draw the border too, even when
                    // the underlying page has a wide character across it.
                    assert_eq!(
                        terminal.backend().buffer()[(x, 2)].symbol(),
                        if border == BorderStyle::Ascii {
                            "+"
                        } else {
                            "╭"
                        }
                    );
                    app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                }
            }
        }
    }

    #[test]
    fn mouse_and_keyboard_share_menu_actions_without_advancing_the_machine() {
        let click = MouseEventKind::Down(MouseButton::Left);
        for paused in [false, true] {
            let mut app = app_with_session(100_000);
            app.user_paused = paused;
            mouse(&mut app, MouseEventKind::Moved, 12, 1);
            assert!(app.overlay.is_none(), "hover alone must not open a menu");
            mouse(&mut app, click, 12, 1);
            assert!(!app.running());
            app.advance();
            assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
            assert_eq!(app.input, [b'A']);
            mouse(&mut app, MouseEventKind::Moved, 13, 4);
            assert!(matches!(
                app.overlay,
                Some(Overlay::Menu { selected: 1, .. })
            ));
            mouse(&mut app, MouseEventKind::Moved, 19, 1);
            assert!(matches!(
                app.overlay,
                Some(Overlay::Menu {
                    kind: MenuKind::Display,
                    selected: 0
                })
            ));
            mouse(&mut app, click, 20, 3);
            assert!(app.overlay.is_none());
            assert_eq!(app.page, Page::Settings);
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(app.page, Page::Apple1);
            assert_eq!(app.user_paused, paused);
            // F2 opens the same dropdown; a click runs the action once.
            app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
            mouse(&mut app, click, 13, 4);
            mouse(&mut app, MouseEventKind::Up(MouseButton::Left), 13, 4);
            assert!(app.overlay.is_none());
            assert_eq!(app.user_paused, !paused);
            mouse(&mut app, click, 12, 1);
            mouse(&mut app, click, 12, 1);
            assert!(app.overlay.is_none(), "clicking the active tab closes it");
            mouse(&mut app, click, 12, 1);
            mouse(&mut app, click, 100, 20);
            assert!(app.overlay.is_none(), "outside click closes the menu");
            assert_eq!(app.user_paused, !paused);
            assert_eq!(app.input, [b'A']);
            assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
        }
    }

    #[test]
    fn menu_mouse_input_respects_confirmation_size_and_preferences() {
        let click = MouseEventKind::Down(MouseButton::Left);
        let mut app = app_with_session(100_000);
        mouse(&mut app, click, 12, 1);
        mouse(&mut app, click, 11, 2);
        mouse(&mut app, MouseEventKind::Down(MouseButton::Right), 100, 20);
        assert!(
            matches!(app.overlay, Some(Overlay::Menu { .. })),
            "borders and right clicks do not execute or dismiss"
        );
        mouse(&mut app, click, 13, 7);
        assert!(matches!(
            app.overlay,
            Some(Overlay::RebootConfirm { confirmed: false })
        ));
        mouse(&mut app, click, 3, 1);
        mouse(&mut app, click, 13, 7);
        assert!(matches!(
            app.overlay,
            Some(Overlay::RebootConfirm { confirmed: false })
        ));
        assert_eq!(
            app.session.as_ref().unwrap().machine().bus().ram_slice()[0x0300],
            0xab
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_event(Event::Resize(30, 10));
        mouse(&mut app, click, 3, 1);
        assert!(app.overlay.is_none());
        app.handle_event(Event::Resize(44, 30));
        app.execute_menu(MenuKind::Display, 0);
        app.settings_row = 4;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(!app.config.ui.mouse);
        mouse(&mut app, click, 3, 1);
        assert!(app.overlay.is_none());
        app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
        assert!(
            matches!(app.overlay, Some(Overlay::Menu { .. })),
            "keyboard menus remain available with mouse off"
        );
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        mouse(&mut app, click, 3, 1);
        assert!(matches!(app.overlay, Some(Overlay::Menu { .. })));
    }

    #[test]
    fn session_shortcuts_draw_immediately_without_cpu_progress_or_notifications() {
        for paused in [false, true] {
            for (key, title) in [
                (KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE), "会话"),
                (
                    KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL),
                    "确认重新上电",
                ),
            ] {
                let mut app = app_with_session(100_000);
                app.user_paused = paused;
                let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                assert!(!app.dirty);
                assert!(app.notification.is_none());
                app.handle_event(Event::Key(key));
                app.advance();
                assert!(app.dirty, "opening {title} must schedule a draw");
                assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let text = buffer_text(terminal.backend().buffer()).replace(' ', "");
                assert!(text.contains(title));
                app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                assert!(app.overlay.is_none());
                assert_eq!(app.user_paused, paused);
            }
        }
    }

    #[test]
    fn reboot_requires_selecting_the_destructive_action() {
        let mut app = app_with_session(25);
        let open = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        app.handle_key(open);
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.session.as_ref().unwrap().machine().bus().ram_slice()[0x0300],
            0xab
        );
        app.handle_key(open);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(
            app.session.as_ref().unwrap().machine().bus().ram_slice()[0x0300],
            0
        );
        assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 25);
    }

    fn app_with_session(budget: u64) -> App {
        let mut app = App::new(None);
        app.config = AppConfig::default();
        app.config_path = None;
        app.terminal_size = (120, 40);
        app.page = Page::Apple1;
        let resources = resources("/original/rom.bin");
        app.config.apple1.rom_path = Some(resources.rom_path.clone());
        let mut session = Session::new(
            create_machine(&resources.rom, resources.program.as_ref()).unwrap(),
            Some(budget),
            TraceOptions::new(false, true, 64).unwrap(),
        );
        session.advance(20).unwrap();
        session
            .machine_mut()
            .bus_mut()
            .load_ram(0x0300, &[0xab])
            .unwrap();
        app.session = Some(session);
        app.resources = Some(resources);
        app.user_paused = true;
        app.input.push_back(b'A');
        app
    }

    #[test]
    fn replacement_cancel_preserves_machine_resources_config_and_input() {
        for cancel in [KeyCode::Esc, KeyCode::Enter] {
            let mut app = app_with_session(100_000);
            app.page = Page::Config;
            let before_registers = app.session.as_ref().unwrap().machine().cpu().registers();
            app.request_start(resources("/replacement/rom.bin"), true);
            assert!(matches!(
                app.overlay,
                Some(Overlay::ReplaceConfirm {
                    confirmed: false,
                    ..
                })
            ));
            app.handle_key(KeyEvent::new(cancel, KeyModifiers::NONE));
            let session = app.session.as_ref().unwrap();
            assert_eq!(session.total_cpu_cycles(), 20);
            assert_eq!(session.machine().cpu().registers(), before_registers);
            assert_eq!(session.machine().bus().ram_slice()[0x0300], 0xab);
            assert_eq!(
                app.resources.as_ref().unwrap().rom_path,
                Path::new("/original/rom.bin")
            );
            assert_eq!(
                app.config.apple1.rom_path.as_deref(),
                Some(Path::new("/original/rom.bin"))
            );
            assert!(
                app.form.status.is_none(),
                "cancellation must not attempt a config write"
            );
            assert_eq!(app.input, VecDeque::from(*b"A"));
            assert!(app.user_paused);
            assert_eq!(app.page, Page::Config);
            assert!(app.overlay.is_none());
        }
    }

    #[test]
    fn confirmed_replacement_keeps_the_session_budget_and_existing_trace() {
        let mut app = app_with_session(25);
        let mut before_trace = Vec::new();
        app.session
            .as_ref()
            .unwrap()
            .write_trace(&mut before_trace)
            .unwrap();
        assert!(!before_trace.is_empty());
        app.request_start(resources("/replacement/rom.bin"), false);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let session = app.session.as_ref().unwrap();
        assert_eq!(session.total_cpu_cycles(), 25);
        assert_eq!(session.remaining(), Some(0));
        assert_eq!(session.machine().cpu_cycles(), 5);
        assert_eq!(session.machine().bus().ram_slice()[0x0300], 0);
        let mut trace = Vec::new();
        session.write_trace(&mut trace).unwrap();
        assert!(trace.starts_with(&before_trace));
        assert!(app.input.is_empty());
        assert!(app.user_paused);
        assert!(app.exit);
        assert_eq!(
            app.stop_message.as_deref(),
            Some("[max cycles reached: 25]")
        );
        assert!(
            app.notification.is_none(),
            "an incomplete boot must not announce success"
        );
    }

    #[test]
    fn replacement_boot_error_retains_a_faulted_machine_for_recovery() {
        let mut app = app_with_session(100_000);
        let mut replacement = resources("/replacement/rom.bin");
        replacement.program = Some(ProgramImage::single(0, vec![0x02]).unwrap());
        app.request_start(replacement, false);
        app.handle_key(KeyEvent::new(KeyCode::Right, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert!(app.faulted);
        assert!(!app.running());
        assert!(app.session.as_ref().unwrap().total_cpu_cycles() >= 20);
        assert!(
            app.fatal_error
                .as_deref()
                .unwrap()
                .contains("unsupported opcode")
        );
        assert_eq!(
            app.resources.as_ref().unwrap().rom_path,
            Path::new("/replacement/rom.bin")
        );
    }

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
    fn path_paste_targets_only_the_focused_field_and_preserves_literal_text() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "/rom/".into();
        app.form.program.clear();
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        let path = "中文 目录/$HOME/$(literal)`name`.bin";
        app.handle_event(Event::Paste(path.into()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.rom, format!("/rom/{path}"));
        assert!(app.form.program.is_empty());
        app.form.focus = ConfigFocus::Program;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.handle_event(Event::Paste("程序 file.bin".into()));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.program, "程序 file.bin");
        assert!(app.input.is_empty());
        for focus in [
            ConfigFocus::Validate,
            ConfigFocus::Launch,
            ConfigFocus::Cancel,
        ] {
            app.form.focus = focus;
            app.handle_event(Event::Paste("ignored".into()));
        }
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        app.overlay = Some(Overlay::QuitConfirm);
        app.handle_event(Event::Paste("ignored".into()));
        assert_eq!(app.form.rom, format!("/rom/{path}"));
        assert_eq!(app.form.program, "程序 file.bin");
        assert!(app.session.is_none());
    }

    #[test]
    fn multiline_and_control_path_pastes_are_rejected_without_partial_edits() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.terminal_size = (120, 40);
        app.form.rom = "/original/rom.bin".into();
        app.form.focus = ConfigFocus::Rom;
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        for paste in ["part\nnext", "part\rnext", "part\0next", "part\u{1b}[31m"] {
            app.handle_event(Event::Paste(paste.into()));
            assert_eq!(app.form.rom, "/original/rom.bin");
            assert_eq!(app.form.edit.as_ref().unwrap().text, "/original/rom.bin");
            assert!(app.form.status.as_deref().unwrap().contains("未接收"));
            assert!(!app.exit);
            assert!(app.overlay.is_none());
        }
    }

    #[test]
    fn file_browser_scrolls_with_selection_and_selects_the_visible_file() {
        let mut app = App::new(None);
        app.page = Page::Config;
        app.overlay = Some(Overlay::Browser {
            target: PathTarget::Rom,
            directory: PathBuf::from("/files"),
            entries: (0..512)
                .map(|index| {
                    let name = format!("entry-{index:03}.bin");
                    DirEntry {
                        path: PathBuf::from("/files").join(&name),
                        name,
                        directory: false,
                    }
                })
                .collect(),
            state: ListState::default().with_selected(Some(0)),
            error: None,
        });
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        for _ in 0..20 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
            terminal.draw(|frame| app.draw(frame)).unwrap();
        }
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("entry-020.bin"));
        assert!(!text.contains("entry-000.bin"));
        // Navigation is bounded at both ends, and resizing preserves a
        // visible selection even when the viewport becomes narrower.
        for _ in 0..512 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        terminal.backend_mut().resize(44, 30);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(buffer_text(terminal.backend().buffer()).contains("entry-511.bin"));
        for _ in 0..512 {
            app.handle_key(KeyEvent::new(KeyCode::Up, KeyModifiers::NONE));
        }
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(buffer_text(terminal.backend().buffer()).contains("entry-000.bin"));
        for _ in 0..17 {
            app.handle_key(KeyEvent::new(KeyCode::Down, KeyModifiers::NONE));
        }
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(buffer_text(terminal.backend().buffer()).contains("entry-017.bin"));
        app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
        assert_eq!(app.form.rom, "/files/entry-017.bin");
        assert!(app.overlay.is_none());
    }

    #[test]
    fn color_capabilities_and_explicit_preferences_resolve_conservatively() {
        for (mode, colors, no_color, background) in [
            (ColorMode::Auto, u16::MAX, false, Color::Rgb(17, 21, 29)),
            (ColorMode::Auto, 256, false, Color::Indexed(233)),
            (ColorMode::Auto, 8, false, Color::Reset),
            (ColorMode::Auto, u16::MAX, true, Color::Reset),
            (ColorMode::Truecolor, 8, true, Color::Rgb(17, 21, 29)),
            (ColorMode::Ansi256, u16::MAX, true, Color::Indexed(233)),
            (ColorMode::Mono, u16::MAX, false, Color::Reset),
        ] {
            let mut config = AppConfig::default();
            config.ui.color_mode = mode;
            assert_eq!(
                Theme::with_capabilities(&config, colors, no_color).background,
                background
            );
        }
    }

    #[test]
    fn switching_color_modes_updates_every_cell_including_the_root_background() {
        let mut app = app_with_session(100_000);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        for screen in [ScreenColor::Green, ScreenColor::Amber, ScreenColor::White] {
            app.config.ui.screen_color = screen;
            for mode in [ColorMode::Truecolor, ColorMode::Ansi256, ColorMode::Mono] {
                app.config.ui.color_mode = mode.clone();
                terminal.draw(|frame| app.draw(frame)).unwrap();
                let colors = terminal
                    .backend()
                    .buffer()
                    .content
                    .iter()
                    .flat_map(|cell| [cell.fg, cell.bg])
                    .collect::<Vec<_>>();
                match mode {
                    ColorMode::Truecolor => {
                        assert!(colors.iter().any(|color| matches!(color, Color::Rgb(..))))
                    }
                    ColorMode::Ansi256 => {
                        assert!(
                            colors
                                .iter()
                                .any(|color| matches!(color, Color::Indexed(..)))
                        );
                        assert!(!colors.iter().any(|color| matches!(color, Color::Rgb(..))));
                    }
                    ColorMode::Mono => assert!(colors.iter().all(|color| *color == Color::Reset)),
                    ColorMode::Auto => unreachable!(),
                }
            }
        }
    }

    fn status_text(terminal: &Terminal<TestBackend>) -> String {
        let buffer = terminal.backend().buffer();
        (0..buffer.area.width)
            .map(|x| buffer[(x, buffer.area.height - 2)].symbol())
            .collect::<String>()
            .replace(' ', "")
    }

    #[test]
    fn footer_and_menu_focus_follow_the_current_interaction() {
        for width in [44, 80, 120] {
            for border in [BorderStyle::Rounded, BorderStyle::Ascii] {
                let mut app = app_with_session(100_000);
                app.config.ui.color_mode = ColorMode::Truecolor;
                app.config.ui.border = border;
                let theme = Theme::from_config(&app.config);
                let mut terminal = Terminal::new(TestBackend::new(width, 30)).unwrap();
                let rendered = terminal.draw(|frame| app.draw(frame)).unwrap();
                assert!((0..width).all(|x| rendered.buffer[(x, 1)].bg == theme.background));
                assert!(footer(&app, width, &theme).width() <= usize::from(width - 2));
                if width >= 80 {
                    assert!(
                        footer(&app, width, &theme)
                            .to_string()
                            .contains("Ctrl+P 继续")
                    );
                    app.user_paused = false;
                    assert!(
                        footer(&app, width, &theme)
                            .to_string()
                            .contains("Ctrl+P 暂停")
                    );
                }
                app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
                let rendered = terminal.draw(|frame| app.draw(frame)).unwrap();
                assert!((0..width).any(|x| rendered.buffer[(x, 1)].bg == theme.selection_bg));
                let hints = footer(&app, width, &theme);
                assert!(hints.width() <= usize::from(width - 2));
                assert!(hints.to_string().contains("Esc 关闭"));
                assert!(!hints.to_string().contains("Ctrl+R"));
                app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
                app.return_to_center();
                app.selected_machine = 1;
                let hints = footer(&app, width, &theme).to_string();
                assert!(!hints.contains("C 配置"));
                assert!(hints.contains("Enter 执行"));
            }
        }
    }

    #[test]
    fn center_actions_stay_visible_and_match_the_enter_key() {
        for (width, height) in [(44, 30), (79, 30), (80, 30), (120, 40), (180, 50)] {
            let mut app = app_with_session(100_000);
            app.return_to_center();
            app.config_warning = Some("配置文件的路径与错误信息".repeat(20));
            app.form.rom = format!("/missing/{}/rom.bin", "中文目录/".repeat(40));
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            for action in [
                CenterAction::Resume,
                CenterAction::Configure,
                CenterAction::Demo,
            ] {
                app.page = Page::Center;
                assert_eq!(app.center_action(), action);
                let rendered = terminal.draw(|frame| app.draw(frame)).unwrap();
                // TestBackend retains old symbols in continuation cells covered
                // by newly drawn wide characters; inspect the actual frame.
                let text = buffer_text(rendered.buffer).replace(' ', "");
                assert!(
                    text.contains(&action.label().replace(' ', "")),
                    "{width}×{height}: {text}"
                );
                app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                match action {
                    CenterAction::Resume => {
                        assert_eq!(app.page, Page::Apple1);
                        assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
                        assert!(app.user_paused);
                        app.session = None;
                        app.resources = None;
                    }
                    CenterAction::Configure => {
                        assert_eq!(app.page, Page::Config);
                        assert!(
                            app.form.status.is_some(),
                            "invalid resources must show an error"
                        );
                        app.selected_machine = 1;
                    }
                    CenterAction::Demo => {
                        assert_eq!(app.page, Page::Demo);
                        assert!(app.demo.is_some());
                    }
                    CenterAction::Launch => unreachable!(),
                }
            }
            app.page = Page::Center;
            app.selected_machine = 0;
            app.form.rom.clear();
            app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
            assert_eq!(app.page, Page::Config);
            assert!(
                app.form.status.is_none(),
                "empty initial configuration is not an error"
            );
        }
    }

    #[test]
    fn resizing_anchors_the_footer_and_preserves_the_machine_screen() {
        let mut app = app_with_session(100_000);
        app.config.ui.color_mode = ColorMode::Truecolor;
        let theme = Theme::from_config(&app.config);
        let screen_bg = theme.screen_bg;
        let before = *app.session.as_ref().unwrap().machine().display().screen();
        for (width, height) in [(44, 30), (80, 30), (120, 40), (180, 50)] {
            let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
            terminal.draw(|frame| app.draw(frame)).unwrap();
            assert!(status_text(&terminal).contains("[暂停]"));
            let buffer = terminal.backend().buffer();
            let footer = (0..width)
                .map(|x| buffer[(x, height - 1)].symbol())
                .collect::<String>();
            assert!(footer.replace(' ', "").contains("F1帮助"));
            let screen_rows = buffer
                .content
                .chunks(usize::from(width))
                .filter(|row| row.iter().filter(|cell| cell.bg == screen_bg).count() >= COLUMNS)
                .count();
            // The 24 character rows and the display's two border rows.
            assert_eq!(screen_rows, ROWS + 2);
            if width >= 80 {
                let sidebar_rows = buffer
                    .content
                    .chunks(usize::from(width))
                    .take(usize::from(height - 2))
                    .filter(|row| row.iter().any(|cell| cell.bg == theme.panel))
                    .count();
                assert_eq!(
                    sidebar_rows, screen_rows,
                    "sidebar must align with the screen"
                );
            }
            assert_eq!(
                app.session.as_ref().unwrap().machine().display().screen(),
                &before
            );
            assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), 20);
        }
    }

    #[test]
    fn reset_notification_and_expiry_leave_the_paused_status_visible() {
        let mut app = app_with_session(100_000);
        app.reset();
        let cycles = app.session.as_ref().unwrap().total_cpu_cycles();
        let mut terminal = Terminal::new(TestBackend::new(44, 30)).unwrap();
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let status = status_text(&terminal);
        assert!(status.contains("[暂停]"));
        assert!(status.contains("[RESET]"));
        app.notification.as_mut().unwrap().1 = Instant::now() - Duration::from_secs(1);
        app.advance();
        assert!(
            app.dirty,
            "expiry must redraw without a keypress or CPU cycle"
        );
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let status = status_text(&terminal);
        assert!(status.contains("[暂停]"));
        assert!(!status.contains("[RESET]"));
        assert_eq!(app.session.as_ref().unwrap().total_cpu_cycles(), cycles);
    }

    #[test]
    fn status_stays_visible_with_long_notifications_and_overlays() {
        let mut app = app_with_session(100_000);
        let mut terminal = Terminal::new(TestBackend::new(120, 40)).unwrap();
        app.user_paused = false;
        app.notify("[CLEAR] ".repeat(100));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(status_text(&terminal).contains("[运行中]"));
        app.handle_key(KeyEvent::new(KeyCode::F(2), KeyModifiers::NONE));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(status_text(&terminal).contains("[暂停]"));
        app.faulted = true;
        app.error("CPU 错误");
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(status_text(&terminal).contains("[故障]"));
        assert!(status_text(&terminal).contains("[CLEAR]"));
    }

    #[test]
    fn reopening_auxiliary_pages_preserves_the_original_return_destination() {
        for (kind, page) in [
            (MenuKind::Help, Page::Help),
            (MenuKind::Display, Page::Settings),
        ] {
            let mut app = app_with_session(100_000);
            for _ in 0..20 {
                app.handle_key(KeyEvent::new(KeyCode::F(10), KeyModifiers::NONE));
                app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
                if kind == MenuKind::Display {
                    app.handle_key(KeyEvent::new(KeyCode::Left, KeyModifiers::NONE));
                }
                app.handle_key(KeyEvent::new(KeyCode::Enter, KeyModifiers::NONE));
                assert_eq!(app.page, page);
                assert_eq!(app.page_history, [Page::Apple1]);
            }
            app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
            assert_eq!(app.page, Page::Apple1);
            assert!(app.page_history.is_empty());
            assert!(app.user_paused);
        }
    }

    #[test]
    fn nested_help_and_settings_return_without_cycles_or_stale_history() {
        let mut app = app_with_session(100_000);
        app.execute_menu(MenuKind::Display, 0);
        app.execute_menu(MenuKind::Help, 0);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Settings);
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Apple1);
        for _ in 0..20 {
            app.open_help();
            app.execute_menu(MenuKind::Display, 0);
            app.open_help();
            assert_eq!(app.page_history, [Page::Apple1]);
        }
        app.return_to_center();
        app.center_key(KeyEvent::new(KeyCode::Char('i'), KeyModifiers::NONE));
        app.handle_key(KeyEvent::new(KeyCode::Esc, KeyModifiers::NONE));
        assert_eq!(app.page, Page::Center);
        assert!(app.page_history.is_empty());
    }

    #[test]
    fn borders_and_navigation_hints_follow_the_ascii_preference() {
        let mut config = AppConfig::default();
        let mut buffer = Buffer::empty(Rect::new(0, 0, 6, 3));
        Theme::from_config(&config)
            .block("")
            .render(buffer.area, &mut buffer);
        assert_eq!(buffer[(0, 0)].symbol(), "╭");
        config.ui.border = BorderStyle::Ascii;
        let theme = Theme::from_config(&config);
        theme.block("").render(buffer.area, &mut buffer);
        for (position, expected) in [((0, 0), "+"), ((5, 2), "+"), ((1, 0), "-"), ((0, 1), "|")] {
            assert_eq!(buffer[position].symbol(), expected);
        }
        let mut app = app_with_session(100_000);
        app.config = config;
        let mut terminal = Terminal::new(TestBackend::new(44, 30)).unwrap();
        app.handle_key(KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL));
        terminal.draw(|frame| app.draw(frame)).unwrap();
        let text = buffer_text(terminal.backend().buffer());
        assert!(text.contains("Left/Right"), "{text}");
        assert!(text.replace(' ', "").contains("[取消]"));
        assert!(text.replace(' ', "").contains("[重新上电]"));
        assert!(
            !text
                .chars()
                .any(|ch| ('\u{2500}'..='\u{257f}').contains(&ch))
        );
        app.overlay = Some(Overlay::QuitConfirm);
        terminal.backend_mut().resize(30, 10);
        terminal.draw(|frame| app.draw(frame)).unwrap();
        assert!(
            !buffer_text(terminal.backend().buffer())
                .chars()
                .any(|ch| ('\u{2500}'..='\u{257f}').contains(&ch))
        );
    }

    #[test]
    fn temporary_overlay_and_user_pause_are_independent() {
        let mut app = App::new(None);
        app.page = Page::Apple1;
        app.terminal_size = (120, 40);
        app.session = Some(Session::new(
            create_machine(
                &test_rom(),
                Some(&ProgramImage::single(0, vec![0x4c, 0x00, 0x00]).unwrap()),
            )
            .unwrap(),
            None,
            TraceOptions::new(false, false, 64).unwrap(),
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
