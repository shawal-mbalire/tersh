//! tersh's file format: decoding `config.toml` and theme files into domain
//! types. TOML errors become [`AppError`]s here, at the boundary.

use tersh_domain::error::{AppError, Code, Result};
use tersh_domain::theme::ThemeSpec;
use tersh_domain::{Config, ThemeSource};

/// Decode `config.toml`; unknown keys become warnings, malformed TOML is an error.
pub fn config(text: &str, themes: &dyn ThemeSource) -> Result<Config> {
    let mut unknown = Vec::new();
    let deserializer = toml::Deserializer::parse(text).map_err(syntax)?;
    let config: Config =
        serde_ignored::deserialize(deserializer, |path| unknown.push(path.to_string())).map_err(syntax)?;
    Ok(config.resolve(unknown, themes))
}

/// Decode a theme file (`themes/NAME.toml`).
pub fn theme(text: &str) -> Result<ThemeSpec> {
    toml::from_str(text).map_err(|e: toml::de::Error| AppError::new(Code::ThemeSyntax, "invalid theme file").because(e))
}

fn syntax(error: toml::de::Error) -> AppError {
    AppError::new(Code::ConfigSyntax, "invalid config.toml").because(error)
}

#[cfg(test)]
mod tests {
    use tersh_domain::config::{Binding, DEFAULT_FONT, TEMPLATE};
    use tersh_domain::theme::BuiltinOnly;
    use tersh_domain::Theme;

    use super::*;

    fn parse(text: &str) -> Result<Config> {
        config(text, &BuiltinOnly)
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
        let config = parse("shell = 'fish'\n[font]\nsize = 16\n[window]\ndecorations = true\n").unwrap();
        assert_eq!(config.shell(), Some("fish"));
        assert_eq!(config.font.size, 16.0);
        assert_eq!(config.font.line_height, 1.2);
        assert!(config.window.decorations);
        assert!(!Config::default().window.decorations);
        assert_eq!(config.theme.palette.len(), 16);
        assert!(config.rich.enabled);
    }

    #[test]
    fn template_is_valid_and_matches_defaults() {
        let config = config(TEMPLATE, &BuiltinOnly).unwrap();
        assert!(config.warnings.is_empty(), "{:?}", config.warnings);
        assert_eq!(config.theme, Theme::default());
    }

    #[test]
    fn collects_warnings() {
        let config = parse("scrolback = 5\ntheme = 'nope'\n[font]\nsize = 12\nfamly = 'x'\n").unwrap();
        assert_eq!(config.font.size, 12.0);
        assert_eq!(config.warnings.len(), 3, "{:?}", config.warnings);
        assert!(config.warnings.iter().any(|w| w.contains("`scrolback`")), "{:?}", config.warnings);
        assert_eq!(parse("[font\n").unwrap_err().code, Code::ConfigSyntax);
    }

    #[test]
    fn keybindings() {
        let config = parse("[keybindings]\n'ctrl+shift+k' = 'clear_cards'\n'alt+b' = { send = \"\\u001bb\" }\n").unwrap();
        assert_eq!(config.keybindings["ctrl+shift+k"], Binding::Action("clear_cards".into()));
        assert_eq!(config.keybindings["alt+b"], Binding::Send { send: "\u{1b}b".into() });
    }
}

#[cfg(test)]
mod theme_tests {
    use super::*;

    #[test]
    fn theme_files() {
        assert_eq!(theme("name = 'nord'\nbackground = '#000000'").unwrap().name.as_deref(), Some("nord"));
        assert_eq!(theme("name = [").unwrap_err().code, Code::ThemeSyntax);
    }
}
