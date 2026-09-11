//! Small, explicit TUI preferences.  The command-line hosts never read this
//! file, so a broken personal configuration cannot affect scripts.

use std::{
    fs, io,
    path::{Path, PathBuf},
};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};

pub const SCHEMA_VERSION: u32 = 1;

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ScreenColor {
    #[default]
    Green,
    Amber,
    White,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum BorderStyle {
    #[default]
    Rounded,
    Ascii,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub(crate) enum ColorMode {
    #[default]
    Auto,
    Truecolor,
    Ansi256,
    Mono,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct UiConfig {
    pub screen_color: ScreenColor,
    pub sidebar: bool,
    pub border: BorderStyle,
    pub color_mode: ColorMode,
    pub mouse: bool,
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            screen_color: ScreenColor::Green,
            sidebar: true,
            border: BorderStyle::Rounded,
            color_mode: ColorMode::Auto,
            mouse: false,
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct Apple1Config {
    pub rom_path: Option<PathBuf>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(default, deny_unknown_fields)]
pub(crate) struct AppConfig {
    pub schema_version: u32,
    pub last_selected: String,
    pub ui: UiConfig,
    pub apple1: Apple1Config,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            schema_version: SCHEMA_VERSION,
            last_selected: "apple1".to_owned(),
            ui: UiConfig::default(),
            apple1: Apple1Config::default(),
        }
    }
}

pub(crate) fn default_config_path() -> Result<PathBuf, String> {
    ProjectDirs::from("", "", "hesper")
        .map(|dirs| dirs.config_dir().join("config.toml"))
        .ok_or_else(|| "cannot determine Hesper configuration directory".to_owned())
}

pub(crate) fn load(path: &Path) -> Result<AppConfig, String> {
    if !path.exists() {
        return Ok(AppConfig::default());
    }
    let text = fs::read_to_string(path)
        .map_err(|err| format!("cannot read configuration '{}': {err}", path.display()))?;
    let config: AppConfig = toml::from_str(&text)
        .map_err(|err| format!("invalid configuration '{}': {err}", path.display()))?;
    if config.schema_version != SCHEMA_VERSION {
        return Err(format!(
            "unsupported configuration schema_version {} (expected {SCHEMA_VERSION})",
            config.schema_version
        ));
    }
    Ok(config)
}

/// Replace a configuration atomically enough for one local process: the old
/// file remains intact until a complete sibling temporary file is ready.
pub(crate) fn save(path: &Path, config: &AppConfig) -> Result<(), String> {
    if path.to_str().is_none() {
        return Err(
            "configuration path is not valid UTF-8 and cannot be represented in TOML".into(),
        );
    }
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).map_err(|err| {
            format!(
                "cannot create configuration directory '{}': {err}",
                parent.display()
            )
        })?;
    }
    let text = toml::to_string_pretty(config)
        .map_err(|err| format!("cannot serialize configuration: {err}"))?;
    let temp = path.with_extension("toml.tmp");
    fs::write(&temp, text).map_err(|err| {
        format!(
            "cannot write temporary configuration '{}': {err}",
            temp.display()
        )
    })?;
    replace(&temp, path)
        .map_err(|err| format!("cannot replace configuration '{}': {err}", path.display()))
}

#[cfg(unix)]
fn replace(temp: &Path, path: &Path) -> io::Result<()> {
    fs::rename(temp, path)
}

#[cfg(windows)]
fn replace(temp: &Path, path: &Path) -> io::Result<()> {
    if path.exists() {
        fs::remove_file(path)?;
    }
    fs::rename(temp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trip_keeps_unicode_and_space_paths() {
        let dir = std::env::temp_dir().join(format!("hesper-config-{}", std::process::id()));
        let path = dir.join("config.toml");
        let mut config = AppConfig::default();
        config.apple1.rom_path = Some(PathBuf::from("/tmp/含 空格/wozmon.bin"));
        save(&path, &config).unwrap();
        assert_eq!(load(&path).unwrap(), config);
        let _ = fs::remove_dir_all(dir);
    }
}
