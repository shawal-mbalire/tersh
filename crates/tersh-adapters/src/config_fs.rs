//! Config adapter: the user's settings directory (`~/.config/tersh`).
//!
//! ```text
//! config.toml         settings (created from the commented template)
//! style.css           CSS applied over the built-in styles
//! themes/NAME.toml    user themes
//! plugins/            NAME.js, or NAME/index.js with an optional NAME/style.css
//! ```

use std::fs;
use std::path::{Path, PathBuf};

use tersh_app::ports::{ConfigStore, Fingerprint, Plugin};
use tersh_domain::config::TEMPLATE;
use tersh_domain::{theme, Config, ThemeSource};

#[derive(Debug, Clone)]
pub struct FsConfigStore {
    dir: PathBuf,
}

impl FsConfigStore {
    /// The platform's per-user config directory (`~/.config/tersh` on Linux).
    pub fn user() -> Self {
        Self::new(dirs::config_dir().unwrap_or_else(|| PathBuf::from(".")).join("tersh"))
    }

    pub fn new(dir: impl Into<PathBuf>) -> Self {
        Self { dir: dir.into() }
    }

    pub fn dir(&self) -> &Path {
        &self.dir
    }

    fn themes_dir(&self) -> PathBuf {
        self.dir.join("themes")
    }

    fn plugin_dir(&self) -> PathBuf {
        self.dir.join("plugins")
    }

    fn style_path(&self) -> PathBuf {
        self.dir.join("style.css")
    }
}

/// User theme files in `themes/`.
struct ThemeFiles<'a>(&'a Path);

impl ThemeSource for ThemeFiles<'_> {
    fn read(&self, name: &str) -> Option<Result<String, String>> {
        let path = self.0.join(format!("{name}.toml"));
        match fs::read_to_string(&path) {
            Ok(text) => Some(Ok(text)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => Some(Err(format!("{}: {e}", path.display()))),
        }
    }

    fn names(&self) -> Vec<String> {
        let Ok(entries) = fs::read_dir(self.0) else { return Vec::new() };
        entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.extension().is_some_and(|e| e == "toml"))
            .filter_map(|p| p.file_stem().and_then(|s| s.to_str()).map(String::from))
            .collect()
    }
}

impl ConfigStore for FsConfigStore {
    fn load(&self) -> Config {
        let path = self.config_path();
        let themes = ThemeFiles(&self.themes_dir());
        match fs::read_to_string(&path) {
            Ok(text) => Config::parse(&text, &themes).unwrap_or_else(|e| {
                let mut config = Config::defaults(&themes);
                config.warnings.push(format!("{}: {e}; using defaults", path.display()));
                config
            }),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Config::defaults(&themes),
            Err(e) => {
                let mut config = Config::defaults(&themes);
                config.warnings.push(format!("{}: {e}", path.display()));
                config
            },
        }
    }

    fn ensure(&self) -> std::io::Result<()> {
        let path = self.config_path();
        if path.exists() {
            return Ok(());
        }
        fs::create_dir_all(self.themes_dir())?;
        fs::create_dir_all(self.plugin_dir())?;
        fs::write(path, TEMPLATE)
    }

    fn style(&self) -> Option<String> {
        fs::read_to_string(self.style_path()).ok()
    }

    fn themes(&self) -> Vec<String> {
        theme::available(&ThemeFiles(&self.themes_dir()))
    }

    fn plugins(&self, disabled: &[String]) -> Vec<Plugin> {
        discover_plugins(&self.plugin_dir(), disabled)
    }

    fn config_path(&self) -> PathBuf {
        self.dir.join("config.toml")
    }

    fn settings_files(&self) -> Fingerprint {
        let mut paths = vec![self.config_path(), self.style_path()];
        paths.extend(files_in(&self.themes_dir(), 0).into_iter().filter(|p| p.extension().is_some_and(|e| e == "toml")));
        modified_times(paths)
    }

    fn plugin_files(&self) -> Fingerprint {
        // The directory itself too, so adding or removing a plugin counts.
        let mut paths = vec![self.plugin_dir()];
        paths.extend(files_in(&self.plugin_dir(), 1));
        modified_times(paths)
    }
}

/// Files (and directories) under `dir`, descending `depth` more levels.
pub(crate) fn files_in(dir: &Path, depth: usize) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut out = Vec::new();
    for path in entries.flatten().map(|e| e.path()) {
        if depth > 0 && path.is_dir() {
            out.extend(files_in(&path, depth - 1));
        }
        out.push(path);
    }
    out
}

/// Each path with its modification time, sorted so the result is comparable.
pub(crate) fn modified_times(mut paths: Vec<PathBuf>) -> Fingerprint {
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let modified = fs::metadata(&path).and_then(|m| m.modified()).ok();
            (path, modified)
        })
        .collect()
}

fn discover_plugins(dir: &Path, disabled: &[String]) -> Vec<Plugin> {
    let Ok(entries) = fs::read_dir(dir) else { return Vec::new() };
    let mut plugins: Vec<Plugin> = entries
        .filter_map(Result::ok)
        .filter_map(|entry| {
            let path = entry.path();
            let file_name = entry.file_name().to_string_lossy().into_owned();
            if file_name.starts_with('.') {
                return None;
            }
            let (name, source, style) = if path.is_dir() {
                (file_name, path.join("index.js"), Some(path.join("style.css")))
            } else {
                (file_name.strip_suffix(".js")?.to_string(), path, None)
            };
            if disabled.contains(&name) {
                return None;
            }
            let source = fs::read_to_string(&source)
                .map_err(|e| log::warn!("plugin {name}: {}: {e}", source.display()))
                .ok()?;
            let style = style.and_then(|p| fs::read_to_string(p).ok());
            Some(Plugin { name, source, style })
        })
        .collect();
    plugins.sort_by(|a, b| a.name.cmp(&b.name));
    plugins
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch(tag: &str) -> FsConfigStore {
        let dir = std::env::temp_dir().join(format!("tersh-fs-{}-{tag}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        FsConfigStore::new(dir)
    }

    #[test]
    fn creates_template_and_loads_it() {
        let store = scratch("template");
        assert!(store.load().warnings.is_empty(), "missing file is fine");
        store.ensure().unwrap();
        assert_eq!(fs::read_to_string(store.config_path()).unwrap(), TEMPLATE);
        assert!(store.load().warnings.is_empty());
    }

    #[test]
    fn user_themes_style_and_plugins() {
        let store = scratch("files");
        store.ensure().unwrap();
        fs::write(store.dir().join("themes/midnight.toml"), "name = 'tokyonight'\nbackground = '#000000'\n").unwrap();
        fs::write(store.config_path(), "theme = 'midnight'\n").unwrap();
        fs::write(store.dir().join("style.css"), ".card {}").unwrap();
        fs::write(store.dir().join("plugins/b.js"), "export function activate() {}").unwrap();
        fs::create_dir_all(store.dir().join("plugins/a")).unwrap();
        fs::write(store.dir().join("plugins/a/index.js"), "export default () => {}").unwrap();
        fs::write(store.dir().join("plugins/a/style.css"), "x {}").unwrap();
        fs::write(store.dir().join("plugins/notes.txt"), "").unwrap();

        let config = store.load();
        assert_eq!((config.theme.name.as_str(), config.theme.background.as_str()), ("midnight", "#000000"));
        assert_eq!(config.theme.foreground, "#c0caf5", "inherits tokyo-night");
        assert!(store.themes().contains(&"midnight".to_string()));
        assert_eq!(store.style().as_deref(), Some(".card {}"));

        let plugins = store.plugins(&[]);
        let names: Vec<_> = plugins.iter().map(|p| p.name.as_str()).collect();
        assert_eq!(names, ["a", "b"]);
        assert_eq!(plugins[0].style.as_deref(), Some("x {}"));
        assert_eq!(store.plugins(&["a".into()]).len(), 1);
    }

    #[test]
    fn broken_config_falls_back_with_a_warning() {
        let store = scratch("broken");
        store.ensure().unwrap();
        fs::write(store.config_path(), "[font\n").unwrap();
        let config = store.load();
        assert_eq!(config.font.size, Config::default().font.size);
        assert_eq!(config.warnings.len(), 1, "{:?}", config.warnings);
    }

    #[test]
    fn fingerprints_change_with_files() {
        let store = scratch("fingerprint");
        store.ensure().unwrap();
        let settings = store.settings_files();
        let plugins = store.plugin_files();
        fs::write(store.dir().join("themes/x.toml"), "").unwrap();
        assert_ne!(settings, store.settings_files());

        fs::create_dir_all(store.dir().join("plugins/p")).unwrap();
        fs::write(store.dir().join("plugins/p/index.js"), "").unwrap();
        let added = store.plugin_files();
        assert_ne!(plugins, added);
        std::thread::sleep(std::time::Duration::from_millis(20));
        fs::write(store.dir().join("plugins/p/index.js"), "export default () => {}").unwrap();
        assert_ne!(added, store.plugin_files(), "editing a plugin inside its directory counts");
    }
}
