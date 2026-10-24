//! User configuration, read from `~/.config/tersh/config.toml`.
//!
//! Every key is optional. The file is created from a commented template on
//! first run, and the app reloads it (and the theme files) when they change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

pub use crate::theme::Theme;

/// The commented config written on first run.
pub const TEMPLATE: &str = include_str!("../config.template.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Shell to run. Defaults to `$SHELL`, then the platform default.
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// Extra environment variables for the shell.
    pub env: BTreeMap<String, String>,
    pub scrollback: usize,
    pub font: Font,
    pub window: Window,
    pub cursor: Cursor,
    pub theme: Theme,
    /// Key combo (`"ctrl+shift+t"`) to action name or `{ send = "text" }`.
    /// Merged over the defaults; bind a combo to `"none"` to free it.
    pub keybindings: BTreeMap<String, Binding>,
    pub rich: Rich,
    pub plugins: Plugins,
    /// Problems found while loading (syntax errors, unknown keys, bad colours).
    #[serde(skip_deserializing)]
    pub warnings: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            shell: None,
            args: Vec::new(),
            env: Default::default(),
            scrollback: 10_000,
            font: Font::default(),
            window: Window::default(),
            cursor: Cursor::default(),
            theme: Theme::default(),
            keybindings: BTreeMap::new(),
            rich: Rich::default(),
            plugins: Plugins::default(),
            warnings: Vec::new(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Font {
    pub family: String,
    pub size: f32,
    pub line_height: f32,
    /// Draw bold text in the bright variant of ANSI colours 0-7.
    pub bold_is_bright: bool,
}

impl Default for Font {
    fn default() -> Self {
        Self {
            family: "'JetBrainsMono Nerd Font', 'JetBrains Mono', 'Fira Code', Menlo, Consolas, monospace".into(),
            size: 14.0,
            line_height: 1.2,
            bold_is_bright: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Window {
    /// Padding around the grid in CSS pixels.
    pub padding: f32,
    /// Background opacity, 0.0 - 1.0.
    pub opacity: f32,
    /// Ask the OS for a blurred (acrylic / vibrancy) backdrop where supported.
    pub blur: bool,
    pub width: f64,
    pub height: f64,
}

impl Default for Window {
    fn default() -> Self {
        Self { padding: 10.0, opacity: 0.92, blur: true, width: 960.0, height: 600.0 }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum CursorShape {
    #[default]
    Block,
    Beam,
    Underline,
}

/// The cursor programs get unless they ask for another (e.g. Neovim's insert-mode beam).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Cursor {
    pub shape: CursorShape,
    pub blink: bool,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Binding {
    /// A named action: `copy`, `paste`, `zoom_in`, `reload_config`, ... or one a plugin registered.
    Action(String),
    /// Type text into the shell, e.g. `{ send = "\u001bb" }`.
    Send { send: String },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Rich {
    /// Accept rich-media messages at all.
    pub enabled: bool,
    /// Accept them from the PTY stream (OSC 7701). When false only the local
    /// socket is honoured, so output from e.g. `cat`ing an untrusted file or a
    /// remote host can't create cards.
    pub osc: bool,
    /// Let HTML cards reach the network (scripts/images from CDNs).
    pub html_network: bool,
}

impl Default for Rich {
    fn default() -> Self {
        Self { enabled: true, osc: true, html_network: false }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Plugins {
    /// Plugin directory names to skip.
    pub disabled: Vec<String>,
}

impl Config {
    /// `~/.config/tersh` (or the platform equivalent).
    pub fn dir() -> PathBuf {
        dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("tersh")
    }

    pub fn path() -> PathBuf {
        Self::dir().join("config.toml")
    }

    pub fn plugin_dir() -> PathBuf {
        Self::dir().join("plugins")
    }

    pub fn themes_dir() -> PathBuf {
        Self::dir().join("themes")
    }

    /// User stylesheet applied over the built-in one.
    pub fn style_path() -> PathBuf {
        Self::dir().join("style.css")
    }

    /// Load the config file. Problems are collected in [`Config::warnings`]
    /// rather than failing: a typo shouldn't stop the terminal from starting.
    pub fn load() -> Self {
        Self::load_from(&Self::path())
    }

    pub fn load_from(path: &Path) -> Self {
        match std::fs::read_to_string(path) {
            Ok(text) => Self::parse(&text).unwrap_or_else(|e| {
                let mut config = Self::default();
                config.warnings.push(format!("{}: {e}; using defaults", path.display()));
                config
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Self::default(),
            Err(e) => {
                let mut config = Self::default();
                config.warnings.push(format!("{}: {e}", path.display()));
                config
            },
        }
    }

    /// Parse config text, noting unknown keys and theme problems as warnings.
    pub fn parse(text: &str) -> Result<Self, String> {
        let mut unknown = Vec::new();
        let deserializer = toml::Deserializer::parse(text).map_err(|e| e.to_string())?;
        let mut config: Self = serde_ignored::deserialize(deserializer, |path| unknown.push(path.to_string()))
            .map_err(|e: toml::de::Error| e.to_string())?;
        config.warnings.extend(unknown.into_iter().map(|key| format!("unknown config key `{key}`")));
        config.warnings.append(&mut config.theme.warnings);
        Ok(config)
    }

    /// Write the commented template if there is no config file yet.
    pub fn ensure_file() -> std::io::Result<()> {
        let path = Self::path();
        if path.exists() {
            return Ok(());
        }
        std::fs::create_dir_all(Self::themes_dir())?;
        std::fs::create_dir_all(Self::plugin_dir())?;
        std::fs::write(path, TEMPLATE)
    }

    pub fn shell(&self) -> String {
        if let Some(shell) = self.shell.as_ref().filter(|s| !s.is_empty()) {
            return shell.clone();
        }
        if cfg!(windows) {
            std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
        } else {
            std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
        }
    }
}

/// Parse `#rrggbb` (or `#rgb`) into its components.
pub fn parse_hex(color: &str) -> Option<(u8, u8, u8)> {
    let hex = color.strip_prefix('#')?;
    let channel = |s: &str| u8::from_str_radix(s, 16).ok();
    match hex.len() {
        6 => Some((channel(&hex[0..2])?, channel(&hex[2..4])?, channel(&hex[4..6])?)),
        3 => {
            let expand = |i: usize| channel(&hex[i..i + 1]).map(|v| v * 17);
            Some((expand(0)?, expand(1)?, expand(2)?))
        },
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn partial_config_keeps_defaults() {
        let config = Config::parse("shell = 'fish'\n[font]\nsize = 16\n").unwrap();
        assert_eq!(config.shell(), "fish");
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.font.line_height, 1.2);
        assert_eq!(config.theme.palette.len(), 16);
        assert!(config.rich.enabled);
    }

    #[test]
    fn template_is_valid_and_matches_defaults() {
        let config = Config::parse(TEMPLATE).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.theme, Theme::default());
    }

    #[test]
    fn collects_warnings() {
        let config = Config::parse("scrolback = 5\ntheme = 'nope'\n[font]\nsize = 12\nfamly = 'x'\n").unwrap();
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.warnings.len(), 3, "{:?}", config.warnings);
        assert!(config.warnings.iter().any(|w| w.contains("`scrolback`")), "{:?}", config.warnings);
        assert!(Config::parse("[font\n").is_err());
    }

    #[test]
    fn keybindings() {
        let config = Config::parse("[keybindings]\n'ctrl+shift+k' = 'clear_cards'\n'alt+b' = { send = \"\\u001bb\" }\n").unwrap();
        assert_eq!(config.keybindings["ctrl+shift+k"], Binding::Action("clear_cards".into()));
        assert_eq!(config.keybindings["alt+b"], Binding::Send { send: "\u{1b}b".into() });
    }

    #[test]
    fn hex_colors() {
        assert_eq!(parse_hex("#1e1e1e"), Some((0x1e, 0x1e, 0x1e)));
        assert_eq!(parse_hex("#fff"), Some((255, 255, 255)));
        assert_eq!(parse_hex("red"), None);
    }
}
