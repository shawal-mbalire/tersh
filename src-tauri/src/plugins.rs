//! Discovers user plugins in `~/.config/tersh/plugins`.
//!
//! A plugin is either a single `name.js` file or a `name/` directory holding
//! `index.js` and optionally `style.css`. Plugins are ES modules evaluated in
//! the terminal window; see `docs/plugins.md` for the API they receive.

use std::fs;
use std::path::Path;

use serde::Serialize;
use tauri::State;
use tersh_core::Config;

use crate::AppState;

#[derive(Debug, Serialize)]
pub struct Plugin {
    pub name: String,
    pub source: String,
    pub style: Option<String>,
}

#[tauri::command]
pub fn list_plugins(state: State<'_, AppState>) -> Vec<Plugin> {
    let disabled = state.config.lock().unwrap().plugins.disabled.clone();
    discover(&Config::plugin_dir(), &disabled)
}

pub fn discover(dir: &Path, disabled: &[String]) -> Vec<Plugin> {
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
            } else if let Some(stem) = file_name.strip_suffix(".js") {
                (stem.to_string(), path, None)
            } else {
                return None;
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
