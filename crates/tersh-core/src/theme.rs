//! Colour themes: the built-in set, user theme files and per-key overrides.
//!
//! In `config.toml` a theme is either a name or a table:
//!
//! ```toml
//! theme = "tokyo-night"
//!
//! # or: start from a theme and override some colours
//! [theme]
//! name = "nord"
//! background = "#20242c"
//! palette = ["#3b4252", "#ff6b6b"]   # replaces the first entries only
//! ```
//!
//! Names are looked up in `~/.config/tersh/themes/<name>.toml` first, then in
//! the built-in set. A theme file holds the same keys as the `[theme]` table;
//! its own `name` key is the theme it extends.

use std::fmt;
use std::path::{Path, PathBuf};

use serde::de::{self, MapAccess, Visitor};
use serde::{Deserialize, Deserializer, Serialize};

use crate::config::{parse_hex, Config};

/// A fully resolved theme.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Theme {
    pub name: String,
    pub foreground: String,
    pub background: String,
    pub cursor: String,
    pub selection: String,
    /// Highlight colour for links, focus rings and card accents.
    pub accent: String,
    /// The 16 ANSI colours.
    pub palette: Vec<String>,
    /// Problems found while resolving (unknown name, bad colours).
    #[serde(skip)]
    pub warnings: Vec<String>,
}

impl Default for Theme {
    fn default() -> Self {
        builtin(DEFAULT_THEME).expect("default theme is built in")
    }
}

impl<'de> Deserialize<'de> for Theme {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        Ok(ThemeSpec::deserialize(deserializer)?.resolve(&Config::themes_dir()))
    }
}

/// What the user wrote: a theme name, or a table of overrides on top of one.
#[derive(Debug, Clone, Default, PartialEq, Deserialize)]
#[serde(default)]
pub struct ThemeSpec {
    pub name: Option<String>,
    pub foreground: Option<String>,
    pub background: Option<String>,
    pub cursor: Option<String>,
    pub selection: Option<String>,
    pub accent: Option<String>,
    pub palette: Option<Vec<String>>,
}

impl ThemeSpec {
    fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        struct SpecVisitor;
        impl<'de> Visitor<'de> for SpecVisitor {
            type Value = ThemeSpec;
            fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
                f.write_str("a theme name or a [theme] table")
            }
            fn visit_str<E: de::Error>(self, name: &str) -> Result<ThemeSpec, E> {
                Ok(ThemeSpec { name: Some(name.to_owned()), ..Default::default() })
            }
            fn visit_map<A: MapAccess<'de>>(self, map: A) -> Result<ThemeSpec, A::Error> {
                // Forward to the derived impl so field errors and unknown keys are reported normally.
                Deserialize::deserialize(de::value::MapAccessDeserializer::new(map))
            }
        }
        deserializer.deserialize_any(SpecVisitor)
    }

    /// Resolve against theme files in `themes_dir` and the built-in themes.
    pub fn resolve(&self, themes_dir: &Path) -> Theme {
        let mut warnings = Vec::new();
        let mut theme = self.base(themes_dir, &mut warnings, 0);
        self.apply(&mut theme, &mut warnings);
        theme.warnings = warnings;
        theme
    }

    fn base(&self, themes_dir: &Path, warnings: &mut Vec<String>, depth: usize) -> Theme {
        let Some(name) = self.name.as_deref().filter(|n| !n.is_empty()) else {
            return builtin(DEFAULT_THEME).unwrap();
        };
        lookup(name, themes_dir, warnings, depth).unwrap_or_else(|| {
            warnings.push(format!("unknown theme \"{name}\" (built-in themes: {})", builtin_names().join(", ")));
            builtin(DEFAULT_THEME).unwrap()
        })
    }

    fn apply(&self, theme: &mut Theme, warnings: &mut Vec<String>) {
        if let Some(palette) = self.palette.as_ref().filter(|p| p.len() > 16) {
            warnings.push(format!("theme.palette has {} colours; only the first 16 are used", palette.len()));
        }
        let mut set = |slot: &mut String, value: &Option<String>, key: &str| {
            if let Some(value) = value {
                if parse_hex(value).is_some() {
                    *slot = value.clone();
                } else {
                    warnings.push(format!("theme.{key}: \"{value}\" is not a #rrggbb colour"));
                }
            }
        };
        set(&mut theme.foreground, &self.foreground, "foreground");
        set(&mut theme.background, &self.background, "background");
        set(&mut theme.cursor, &self.cursor, "cursor");
        set(&mut theme.selection, &self.selection, "selection");
        let accent_overridden = self.accent.is_some();
        set(&mut theme.accent, &self.accent, "accent");
        if let Some(palette) = &self.palette {
            for (i, value) in palette.iter().take(16).enumerate() {
                set(&mut theme.palette[i], &Some(value.clone()), &format!("palette[{i}]"));
            }
            // Keep the accent in step with a recoloured bright blue unless it was set explicitly.
            if !accent_overridden && palette.len() > 12 {
                theme.accent = theme.palette[12].clone();
            }
        }
    }
}

fn lookup(name: &str, themes_dir: &Path, warnings: &mut Vec<String>, depth: usize) -> Option<Theme> {
    // A user file may share a built-in's name to restyle it; `name = "<itself>"` then means the built-in.
    if depth < 8 {
        let path = themes_dir.join(format!("{name}.toml"));
        if let Some(spec) = read_theme_file(&path, warnings) {
            let mut theme = match spec.name.as_deref() {
                Some(base) if base == name => builtin(name).unwrap_or_default(),
                Some(_) => spec.base(themes_dir, warnings, depth + 1),
                None => builtin(DEFAULT_THEME).unwrap(),
            };
            spec.apply(&mut theme, warnings);
            theme.name = name.to_owned();
            return Some(theme);
        }
    } else {
        warnings.push(format!("theme \"{name}\": too many nested themes"));
    }
    builtin(name)
}

fn read_theme_file(path: &Path, warnings: &mut Vec<String>) -> Option<ThemeSpec> {
    let text = std::fs::read_to_string(path).ok()?;
    match toml::from_str::<ThemeSpec>(&text) {
        Ok(spec) => Some(spec),
        Err(e) => {
            warnings.push(format!("{}: {}", path.display(), e.message()));
            None
        },
    }
}

/// Every theme name available: built-ins and files in `themes_dir`, sorted.
pub fn available(themes_dir: &Path) -> Vec<String> {
    let mut names: Vec<String> = builtin_names().into_iter().map(String::from).collect();
    if let Ok(entries) = std::fs::read_dir(themes_dir) {
        for entry in entries.flatten() {
            let path: PathBuf = entry.path();
            if path.extension().is_some_and(|e| e == "toml") {
                if let Some(stem) = path.file_stem().and_then(|s| s.to_str()) {
                    names.push(stem.to_owned());
                }
            }
        }
    }
    names.sort();
    names.dedup();
    names
}

pub const DEFAULT_THEME: &str = "tersh";

struct Builtin {
    name: &'static str,
    foreground: &'static str,
    background: &'static str,
    cursor: &'static str,
    selection: &'static str,
    /// `None` uses bright blue (palette 12).
    accent: Option<&'static str>,
    palette: [&'static str; 16],
}

#[rustfmt::skip]
const BUILTINS: &[Builtin] = &[
    Builtin {
        name: "tersh", foreground: "#d4d4d4", background: "#1e1e1e", cursor: "#aeafad", selection: "#264f78", accent: None,
        palette: ["#000000", "#cd3131", "#0dbc79", "#e5e510", "#2472c8", "#bc3fbc", "#11a8cd", "#e5e5e5",
                  "#666666", "#f14c4c", "#23d18b", "#f5f543", "#3b8eea", "#d670d6", "#29b8db", "#ffffff"],
    },
    Builtin {
        name: "tersh-light", foreground: "#333333", background: "#ffffff", cursor: "#333333", selection: "#add6ff", accent: Some("#0451a5"),
        palette: ["#000000", "#cd3131", "#00bc00", "#949800", "#0451a5", "#bc05bc", "#0598bc", "#555555",
                  "#666666", "#cd3131", "#14ce14", "#b5ba00", "#0451a5", "#bc05bc", "#0598bc", "#a5a5a5"],
    },
    Builtin {
        name: "catppuccin-mocha", foreground: "#cdd6f4", background: "#1e1e2e", cursor: "#f5e0dc", selection: "#585b70", accent: Some("#cba6f7"),
        palette: ["#45475a", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#bac2de",
                  "#585b70", "#f38ba8", "#a6e3a1", "#f9e2af", "#89b4fa", "#f5c2e7", "#94e2d5", "#a6adc8"],
    },
    Builtin {
        name: "catppuccin-latte", foreground: "#4c4f69", background: "#eff1f5", cursor: "#dc8a78", selection: "#acb0be", accent: Some("#8839ef"),
        palette: ["#5c5f77", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#acb0be",
                  "#6c6f85", "#d20f39", "#40a02b", "#df8e1d", "#1e66f5", "#ea76cb", "#179299", "#bcc0cc"],
    },
    Builtin {
        name: "tokyo-night", foreground: "#c0caf5", background: "#1a1b26", cursor: "#c0caf5", selection: "#33467c", accent: None,
        palette: ["#15161e", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#a9b1d6",
                  "#414868", "#f7768e", "#9ece6a", "#e0af68", "#7aa2f7", "#bb9af7", "#7dcfff", "#c0caf5"],
    },
    Builtin {
        name: "gruvbox-dark", foreground: "#ebdbb2", background: "#282828", cursor: "#ebdbb2", selection: "#504945", accent: Some("#fabd2f"),
        palette: ["#282828", "#cc241d", "#98971a", "#d79921", "#458588", "#b16286", "#689d6a", "#a89984",
                  "#928374", "#fb4934", "#b8bb26", "#fabd2f", "#83a598", "#d3869b", "#8ec07c", "#ebdbb2"],
    },
    Builtin {
        name: "nord", foreground: "#d8dee9", background: "#2e3440", cursor: "#d8dee9", selection: "#434c5e", accent: Some("#88c0d0"),
        palette: ["#3b4252", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#88c0d0", "#e5e9f0",
                  "#4c566a", "#bf616a", "#a3be8c", "#ebcb8b", "#81a1c1", "#b48ead", "#8fbcbb", "#eceff4"],
    },
    Builtin {
        name: "dracula", foreground: "#f8f8f2", background: "#282a36", cursor: "#f8f8f2", selection: "#44475a", accent: Some("#bd93f9"),
        palette: ["#21222c", "#ff5555", "#50fa7b", "#f1fa8c", "#bd93f9", "#ff79c6", "#8be9fd", "#f8f8f2",
                  "#6272a4", "#ff6e6e", "#69ff94", "#ffffa5", "#d6acff", "#ff92df", "#a4ffff", "#ffffff"],
    },
    Builtin {
        name: "one-dark", foreground: "#abb2bf", background: "#282c34", cursor: "#528bff", selection: "#3e4451", accent: None,
        palette: ["#282c34", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#abb2bf",
                  "#5c6370", "#e06c75", "#98c379", "#e5c07b", "#61afef", "#c678dd", "#56b6c2", "#ffffff"],
    },
    Builtin {
        name: "solarized-dark", foreground: "#839496", background: "#002b36", cursor: "#93a1a1", selection: "#073642", accent: Some("#268bd2"),
        palette: ["#073642", "#dc322f", "#859900", "#b58900", "#268bd2", "#d33682", "#2aa198", "#eee8d5",
                  "#002b36", "#cb4b16", "#586e75", "#657b83", "#839496", "#6c71c4", "#93a1a1", "#fdf6e3"],
    },
    Builtin {
        name: "rose-pine", foreground: "#e0def4", background: "#191724", cursor: "#524f67", selection: "#403d52", accent: Some("#c4a7e7"),
        palette: ["#26233a", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4",
                  "#6e6a86", "#eb6f92", "#31748f", "#f6c177", "#9ccfd8", "#c4a7e7", "#ebbcba", "#e0def4"],
    },
];

pub fn builtin_names() -> Vec<&'static str> {
    BUILTINS.iter().map(|b| b.name).collect()
}

pub fn builtin(name: &str) -> Option<Theme> {
    BUILTINS.iter().find(|b| b.name == name).map(|b| Theme {
        name: b.name.into(),
        foreground: b.foreground.into(),
        background: b.background.into(),
        cursor: b.cursor.into(),
        selection: b.selection.into(),
        accent: b.accent.unwrap_or(b.palette[12]).into(),
        palette: b.palette.map(String::from).to_vec(),
        warnings: Vec::new(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn spec(toml_text: &str) -> ThemeSpec {
        #[derive(Deserialize)]
        struct Wrapper {
            #[serde(deserialize_with = "ThemeSpec::deserialize")]
            theme: ThemeSpec,
        }
        toml::from_str::<Wrapper>(toml_text).unwrap().theme
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("tersh-theme-{}-{tag}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn builtins_are_complete_and_valid() {
        for name in builtin_names() {
            let theme = builtin(name).unwrap();
            assert_eq!(theme.palette.len(), 16, "{name}");
            for color in [&theme.foreground, &theme.background, &theme.cursor, &theme.selection, &theme.accent]
                .into_iter()
                .chain(&theme.palette)
            {
                assert!(parse_hex(color).is_some(), "{name}: {color}");
            }
        }
    }

    #[test]
    fn name_or_table() {
        let none = Path::new("/nonexistent");
        assert_eq!(spec("theme = 'nord'").resolve(none).background, "#2e3440");
        let theme = spec("[theme]\nname = 'nord'\nbackground = '#000000'\npalette = ['#111111']").resolve(none);
        assert_eq!(theme.background, "#000000");
        assert_eq!(theme.palette[0], "#111111");
        assert_eq!(theme.palette[1], "#bf616a");
        assert!(theme.warnings.is_empty());
    }

    #[test]
    fn bad_values_warn_and_fall_back() {
        let theme = spec("[theme]\nname = 'nope'\nforeground = 'red'").resolve(Path::new("/nonexistent"));
        assert_eq!(theme.name, DEFAULT_THEME);
        assert_eq!(theme.foreground, Theme::default().foreground);
        assert_eq!(theme.warnings.len(), 2, "{:?}", theme.warnings);
    }

    #[test]
    fn theme_files_extend_others() {
        let dir = scratch_dir("files");
        std::fs::write(dir.join("mine.toml"), "name = 'dracula'\nbackground = '#101010'\n").unwrap();
        std::fs::write(dir.join("nord.toml"), "name = 'nord'\nforeground = '#ffffff'\n").unwrap();
        std::fs::write(dir.join("loop.toml"), "name = 'loop2'\n").unwrap();
        std::fs::write(dir.join("loop2.toml"), "name = 'loop'\n").unwrap();

        let mine = spec("theme = 'mine'").resolve(&dir);
        assert_eq!((mine.name.as_str(), mine.background.as_str(), mine.foreground.as_str()), ("mine", "#101010", "#f8f8f2"));

        // Restyling a built-in under its own name.
        let nord = spec("theme = 'nord'").resolve(&dir);
        assert_eq!((nord.foreground.as_str(), nord.background.as_str()), ("#ffffff", "#2e3440"));

        let looped = spec("theme = 'loop'").resolve(&dir);
        assert!(looped.warnings.iter().any(|w| w.contains("too many")), "{:?}", looped.warnings);

        let names = available(&dir);
        assert!(names.contains(&"mine".to_string()) && names.contains(&"tersh".to_string()));
    }
}
