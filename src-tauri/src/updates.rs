//! Updating and restarting.
//!
//! `tersh update` installs the latest release. Every running window watches its
//! own executable; when an update replaces it, the page is told
//! (`app-updated`), and the window restarts into the new version as soon as
//! that is safe (see `tersh_domain::update::restart_decision`): in the same
//! directory, at the same size, re-attaching tmux or zellij if that was running.

use std::thread;
use std::time::Duration;

use tauri::{AppHandle, Emitter, Manager};
use tersh_adapters::binary::AppBinary;
use tersh_adapters::update::{GithubReleases, InstallScript};
use tersh_adapters::SystemProcesses;
use tersh_app::ports::Watchable;
use tersh_app::update::{self, UpdateStatus};
use tersh_domain::update::{restart_decision, Restart};

use crate::cli::Launch;
use crate::{AppState, CmdResult};

const VERSION: &str = env!("CARGO_PKG_VERSION");
const POLL_INTERVAL: Duration = Duration::from_secs(2);

/// `tersh update [--check]`. Returns the process exit code.
pub fn cli(check: bool) -> i32 {
    let releases = GithubReleases::default();
    let result = if check {
        update::check(VERSION, &releases)
    } else {
        println!("Checking for updates…");
        update::update(VERSION, &releases, &InstallScript::default())
    };
    match result {
        Ok(UpdateStatus::UpToDate { current }) => println!("tersh {current} is up to date."),
        Ok(UpdateStatus::Available { current, latest }) => {
            println!("tersh {latest} is available (you have {current}). Run `tersh update` to install it.")
        },
        Ok(UpdateStatus::Installed { previous, installed }) => {
            println!("Updated tersh {previous} → {installed}. Open windows restart into it once their shell is idle.")
        },
        Err(e) => {
            eprintln!("tersh update: {e:#}");
            return 1;
        },
    }
    0
}

/// Restart now, whatever is running (the `restart` action).
#[tauri::command]
pub fn restart_app(app: AppHandle) -> CmdResult<()> {
    let state = app.state::<AppState>();
    let input = match current_decision(&state) {
        Restart::Now { input } => input,
        Restart::Wait { .. } => None,
    };
    relaunch(&app, input)
}

fn current_decision(state: &AppState) -> Restart {
    match state.session.lock().unwrap().as_ref() {
        Some(session) => restart_decision(&session.foreground(&SystemProcesses)),
        None => Restart::Now { input: None },
    }
}

/// Start the binary on disk (possibly a newer version) in this window's place, then exit.
fn relaunch(app: &AppHandle, input: Option<String>) -> CmdResult<()> {
    let binary = AppBinary::current().ok_or("cannot find the tersh executable")?;
    let state = app.state::<AppState>();
    let cwd = state.session.lock().unwrap().as_ref().and_then(|s| s.cwd(&SystemProcesses));
    let size = app.get_webview_window("main").and_then(|w| {
        let scale = w.scale_factor().ok()?;
        let size = w.inner_size().ok()?.to_logical::<f64>(scale);
        Some((size.width, size.height))
    });
    let launch = Launch { cwd, input, size };
    binary.launch(&launch.to_args()).map_err(|e| format!("could not start {}: {e}", binary.path().display()))?;
    app.exit(0);
    Ok(())
}

/// Watch for this executable being replaced by an update.
pub fn watch(app: AppHandle) {
    // In development the binary is rebuilt constantly, and `tauri dev` restarts it anyway.
    if cfg!(debug_assertions) {
        return;
    }
    let Some(binary) = AppBinary::current() else { return };
    let spawned = thread::Builder::new().name("tersh-update-watch".into()).spawn(move || {
        let mut last = binary.fingerprint();
        let mut pending: Option<String> = None;
        loop {
            thread::sleep(POLL_INTERVAL);
            if binary.fingerprint() != last {
                // Let the installer finish writing.
                thread::sleep(Duration::from_secs(1));
                last = binary.fingerprint();
                match binary.version() {
                    Some(version) if version != VERSION => {
                        log::info!("tersh {version} installed");
                        let _ = app.emit("app-updated", &version);
                        pending = Some(version);
                    },
                    _ => {},
                }
            }
            if pending.is_none() {
                continue;
            }
            let state = app.state::<AppState>();
            if !state.config.lock().unwrap().update.auto_restart {
                continue;
            }
            if let Restart::Now { input } = current_decision(&state) {
                if let Err(e) = relaunch(&app, input) {
                    log::warn!("restart after update: {e}");
                    pending = None;
                }
            }
        }
    });
    if let Err(e) = spawned {
        log::warn!("update watcher: {e}");
    }
}
