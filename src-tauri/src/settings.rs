//! User settings as the page sees them, and live reloading.
//!
//! `config.toml`, `style.css` and `themes/*.toml` are polled for changes; on
//! a change the config is reloaded, pushed to the running session (colours
//! reported to programs, default cursor) and sent to the page as a
//! `settings-changed` event, which restyles the terminal in place.

use std::path::{Path, PathBuf};
use std::thread;
use std::time::{Duration, SystemTime};

use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager, State};
use tersh_core::{theme, Config};

use crate::{AppState, CmdResult};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[derive(Debug, Clone, Serialize)]
pub struct Settings {
    pub config: Config,
    /// Contents of `style.css`, if any.
    pub style: Option<String>,
    pub config_path: PathBuf,
    /// Theme names that can be used in `theme = "..."`.
    pub themes: Vec<String>,
}

fn settings(config: Config) -> Settings {
    Settings {
        config,
        style: std::fs::read_to_string(Config::style_path()).ok(),
        config_path: Config::path(),
        themes: theme::available(&Config::themes_dir()),
    }
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    settings(state.config.lock().unwrap().clone())
}

/// Reload from disk now and return the result (the event is sent as well).
#[tauri::command]
pub fn reload_config(app: AppHandle) -> Settings {
    reload(&app)
}

/// Open `config.toml` with the system's handler for it, creating it first if needed.
#[tauri::command]
pub fn open_config() -> CmdResult<()> {
    Config::ensure_file().map_err(|e| e.to_string())?;
    open_path(&Config::path())
}

fn open_path(path: &Path) -> CmdResult<()> {
    #[cfg(target_os = "macos")]
    let mut command = std::process::Command::new("open");
    #[cfg(target_os = "windows")]
    let mut command = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]);
        c
    };
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let mut command = std::process::Command::new("xdg-open");

    let mut child = command.arg(path).spawn().map_err(|e| format!("could not open {}: {e}", path.display()))?;
    // Reap it so it doesn't linger as a zombie.
    thread::spawn(move || child.wait());
    Ok(())
}

fn reload(app: &AppHandle) -> Settings {
    let config = Config::load();
    for warning in &config.warnings {
        log::warn!("config: {warning}");
    }
    let state = app.state::<AppState>();
    if let Some(session) = state.session.lock().unwrap().as_ref() {
        session.reconfigure(&config);
    }
    *state.config.lock().unwrap() = config.clone();
    let settings = settings(config);
    if let Err(e) = app.emit("settings-changed", &settings) {
        log::warn!("settings-changed: {e}");
    }
    settings
}

/// Modification times of every file that affects the settings.
fn fingerprint() -> Vec<(PathBuf, Option<SystemTime>)> {
    let mut paths = vec![Config::path(), Config::style_path()];
    if let Ok(entries) = std::fs::read_dir(Config::themes_dir()) {
        paths.extend(entries.flatten().map(|e| e.path()).filter(|p| p.extension().is_some_and(|e| e == "toml")));
    }
    paths.sort();
    paths
        .into_iter()
        .map(|path| {
            let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
            (path, modified)
        })
        .collect()
}

pub fn watch(app: AppHandle) {
    let spawned = thread::Builder::new().name("tersh-config-watch".into()).spawn(move || {
        let mut last = fingerprint();
        loop {
            thread::sleep(POLL_INTERVAL);
            let current = fingerprint();
            if current != last {
                last = current;
                // Editors often write in several steps; let the file settle.
                thread::sleep(Duration::from_millis(50));
                reload(&app);
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!("config watcher: {e}");
    }
}
