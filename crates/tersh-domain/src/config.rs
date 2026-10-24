//! User configuration: the model, defaults and parsing rules. Where the text
//! comes from (normally `~/.config/tersh/config.toml`) is up to a
//! `ConfigStore` adapter.
//!
//! Every key is optional, and problems are collected as warnings rather than
//! failing: a typo shouldn't stop the terminal from starting.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

pub use crate::theme::Theme;
use crate::theme::{ThemeSource, ThemeSpec};

/// The commented config written on first run.
pub const TEMPLATE: &str = include_str!("../config.template.toml");

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Shell to run. `None` lets the PTY adapter pick the system default (`$SHELL`).
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// Extra environment variables for the shell.
    pub env: BTreeMap<String, String>,
    pub scrollback: usize,
    pub font: Font,
    pub window: Window,
    pub cursor: Cursor,
    /// The resolved theme (what the UI gets).
    #[serde(skip_deserializing)]
    pub theme: Theme,
    /// The theme as written: a name or a table. [`Config::parse`] resolves it into `theme`.
    #[serde(rename = "theme", skip_serializing, deserialize_with = "ThemeSpec::deserialize_name_or_table")]
    pub theme_spec: ThemeSpec,
    /// Key combo (`"ctrl+shift+t"`) to action name or `{ send = "text" }`.
    /// Merged over the defaults; bind a combo to `"none"` to free it.
    pub keybindings: BTreeMap<String, Binding>,
    pub rich: Rich,
    pub plugins: Plugins,
    pub update: Update,
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
            theme_spec: ThemeSpec::default(),
            keybindings: BTreeMap::new(),
            rich: Rich::default(),
            plugins: Plugins::default(),
            update: Update::default(),
            warnings: Vec::new(),
        }
    }
}

/// Sono ships with the app. Nerd Font icons come from the bundled symbols font,
/// which the UI adds to any family list, so no Nerd Font needs to be installed.
pub const DEFAULT_FONT: &str = "'Sono Variable', 'Sono', monospace";

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
            family: DEFAULT_FONT.into(),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Update {
    /// When a new version is installed (`tersh update`), restart into it as
    /// soon as the shell is idle (or only a tmux/zellij client is running).
    pub auto_restart: bool,
}

impl Default for Update {
    fn default() -> Self {
        Self { auto_restart: true }
    }
}

impl Config {
    /// Parse config text. Unknown keys and theme problems become [`Config::warnings`];
    /// only malformed TOML is an error. Theme names are resolved against `themes`.
    pub fn parse(text: &str, themes: &dyn ThemeSource) -> Result<Self, String> {
        let mut unknown = Vec::new();
        let deserializer = toml::Deserializer::parse(text).map_err(|e| e.to_string())?;
        let mut config: Self = serde_ignored::deserialize(deserializer, |path| unknown.push(path.to_string()))
            .map_err(|e: toml::de::Error| e.to_string())?;
        config.warnings.extend(unknown.into_iter().map(|key| format!("unknown config key `{key}`")));
        config.theme = config.theme_spec.resolve(themes);
        config.warnings.append(&mut config.theme.warnings);
        Ok(config)
    }

    /// Defaults, with the theme resolved (so user theme files apply even without a config file).
    pub fn defaults(themes: &dyn ThemeSource) -> Self {
        Self::parse("", themes).unwrap_or_default()
    }

    /// The configured shell, if any.
    pub fn shell(&self) -> Option<&str> {
        self.shell.as_deref().filter(|s| !s.is_empty())
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
    use crate::theme::BuiltinOnly;

    fn parse(text: &str) -> Result<Config, String> {
        Config::parse(text, &BuiltinOnly)
    }

    #[test]
    fn serializes_the_resolved_theme() {
        let config = parse("theme = 'rosepine'").unwrap();
        let json = serde_json::to_value(&config).unwrap();
        assert_eq!(json["theme"]["name"], "rose-pine");
        assert_eq!(json["theme"]["background"], "#191724");
        assert!(json.get("theme_spec").is_none());
        assert_eq!(Config::default().font.family, DEFAULT_FONT);
    }

    #[test]
    fn partial_config_keeps_defaults() {
        let config = parse("shell = 'fish'\n[font]\nsize = 16\n").unwrap();
        assert_eq!(config.shell(), Some("fish"));
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.font.line_height, 1.2);
        assert_eq!(config.theme.palette.len(), 16);
        assert!(config.rich.enabled);
    }

    #[test]
    fn template_is_valid_and_matches_defaults() {
        let config = Config::parse(TEMPLATE, &BuiltinOnly).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.theme, Theme::default());
    }

    #[test]
    fn collects_warnings() {
        let config = parse("scrolback = 5\ntheme = 'nope'\n[font]\nsize = 12\nfamly = 'x'\n").unwrap();
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.warnings.len(), 3, "{:?}", config.warnings);
        assert!(config.warnings.iter().any(|w| w.contains("`scrolback`")), "{:?}", config.warnings);
        assert!(parse("[font\n").is_err());
    }

    #[test]
    fn keybindings() {
        let config = parse("[keybindings]\n'ctrl+shift+k' = 'clear_cards'\n'alt+b' = { send = \"\\u001bb\" }\n").unwrap();
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
