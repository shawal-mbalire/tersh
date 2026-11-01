//! Settings commands and live reloading.
//!
//! The settings files, plugins and system font directories are polled for
//! changes; on a change the config is reloaded, pushed to the running session
//! (colours reported to programs, default cursor) and sent to the page as a
//! `settings-changed` event. The page restyles itself, reloads plugins if
//! their revision changed, and re-measures fonts if the font revision changed.

use std::path::Path;
use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager, State};
use tersh_app::{ChangeDetector, Settings};

use crate::{AppState, CmdResult};

const POLL_INTERVAL: Duration = Duration::from_millis(500);

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    Settings::new(&*state.store, &*state.fonts, state.config.lock().unwrap().clone())
}

/// Reload from disk now and return the result (the event is sent as well).
#[tauri::command]
pub fn reload_config(app: AppHandle) -> Settings {
    reload(&app)
}

/// Open `config.toml` with the system's handler for it, creating it first if needed.
#[tauri::command]
pub fn open_config(state: State<'_, AppState>) -> CmdResult<()> {
    state.store.ensure().map_err(|e| e.to_string())?;
    open_path(&state.store.config_path())
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

    // The editor must not load the AppImage's libraries.
    let mut child = tersh_adapters::unbundled(&mut command).arg(path).spawn().map_err(|e| format!("could not open {}: {e}", path.display()))?;
    // Reap it so it doesn't linger as a zombie.
    thread::spawn(move || child.wait());
    Ok(())
}

fn reload(app: &AppHandle) -> Settings {
    let state = app.state::<AppState>();
    let config = state.store.load();
    for warning in &config.warnings {
        log::warn!("config: {warning}");
    }
    if let Some(session) = state.session.lock().unwrap().as_ref() {
        session.reconfigure(&config);
    }
    *state.config.lock().unwrap() = config.clone();
    let settings = Settings::new(&*state.store, &*state.fonts, config);
    if let Err(e) = app.emit("settings-changed", &settings) {
        log::warn!("settings-changed: {e}");
    }
    settings
}

pub fn watch(app: AppHandle) {
    let spawned = thread::Builder::new().name("tersh-config-watch".into()).spawn(move || {
        let state = app.state::<AppState>();
        let (store, fonts) = (state.store.clone(), state.fonts.clone());
        let mut changes = ChangeDetector::new(&*store, &*fonts);
        loop {
            thread::sleep(POLL_INTERVAL);
            if changes.changed(&*store, &*fonts) {
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
