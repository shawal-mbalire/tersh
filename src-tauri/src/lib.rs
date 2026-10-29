//! Desktop UI adapter and composition root: one Tauri window hosting a
//! session (a canvas grid with DOM card overlays). It wires the native
//! adapters from `tersh-adapters` into the `tersh-app` use cases and exposes
//! them to the page as commands and channels.

mod cards;
mod cli;
mod settings;
mod updates;

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder};
use tersh_adapters::{FsConfigStore, SystemFontDirs};
use tersh_app::ports::{ConfigStore, Plugin, Watchable};
use tersh_app::{Session, SessionOptions, Sink};
use tersh_domain::{Config, Frame, Info, TermEvent};

/// Forwards session output to whichever webview channels are current. The
/// channels are swapped when the page reloads, while the shell keeps running.
#[derive(Default)]
struct ChannelSink {
    channels: Mutex<Option<Channels>>,
}

struct Channels {
    frames: Channel<Frame>,
    rich: Channel<Value>,
    events: Channel<TermEvent>,
}

impl Sink for ChannelSink {
    fn frame(&self, frame: Frame) {
        if let Some(c) = self.channels.lock().unwrap().as_ref() {
            let _ = c.frames.send(frame);
        }
    }

    fn rich(&self, message: Value) {
        if let Some(c) = self.channels.lock().unwrap().as_ref() {
            let _ = c.rich.send(message);
        }
    }

    fn event(&self, event: TermEvent) {
        if let Some(c) = self.channels.lock().unwrap().as_ref() {
            let _ = c.events.send(event);
        }
    }
}

struct AppState {
    store: Arc<dyn ConfigStore>,
    fonts: Arc<dyn Watchable>,
    /// The latest loaded config; replaced on reload.
    config: Mutex<Config>,
    sink: Arc<ChannelSink>,
    session: Mutex<Option<Session>>,
    /// Where to start the shell and what to type into it (from the command line, e.g. after a restart).
    launch: Mutex<cli::Launch>,
}

type CmdResult<T> = Result<T, String>;

/// Attach the page to the shell, starting it on first call.
#[tauri::command]
fn session_start(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    cell_width: u16,
    cell_height: u16,
    frames: Channel<Frame>,
    rich: Channel<Value>,
    events: Channel<TermEvent>,
) -> CmdResult<Info> {
    *state.sink.channels.lock().unwrap() = Some(Channels { frames, rich, events });
    let mut session = state.session.lock().unwrap();
    match session.as_ref() {
        Some(existing) => {
            let _ = existing.resize(cols, rows, cell_width, cell_height);
            existing.refresh();
        },
        None => {
            let mut options = SessionOptions::from_config(&state.config.lock().unwrap(), cols, rows);
            options.cell_width = cell_width;
            options.cell_height = cell_height;
            let launch = std::mem::take(&mut *state.launch.lock().unwrap());
            options.cwd = launch.cwd;
            let rich_enabled = state.config.lock().unwrap().rich.enabled;
            let spawned = tersh_adapters::spawn_local(options, rich_enabled, state.sink.clone());
            let spawned = spawned.map_err(|e| format!("{e:#}"))?;
            if let Some(input) = launch.input {
                let _ = spawned.write(input.as_bytes());
            }
            *session = Some(spawned);
        },
    }
    Ok(session.as_ref().unwrap().info())
}

fn with_session<T>(state: &State<'_, AppState>, f: impl FnOnce(&Session) -> T) -> CmdResult<T> {
    state.session.lock().unwrap().as_ref().map(f).ok_or_else(|| "no session".to_string())
}

#[tauri::command]
fn session_write(state: State<'_, AppState>, data: String) -> CmdResult<()> {
    with_session(&state, |s| s.write(data.as_bytes()))?.map_err(|e| e.to_string())
}

#[tauri::command]
fn session_resize(state: State<'_, AppState>, cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> CmdResult<()> {
    with_session(&state, |s| s.resize(cols, rows, cell_width, cell_height))?.map_err(|e| e.to_string())
}

#[tauri::command]
fn session_scroll(state: State<'_, AppState>, lines: i32) -> CmdResult<()> {
    with_session(&state, |s| s.scroll(lines))
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Vec<Plugin> {
    let disabled = state.config.lock().unwrap().plugins.disabled.clone();
    state.store.plugins(&disabled)
}

#[tauri::command]
fn session_scroll_to_bottom(state: State<'_, AppState>) -> CmdResult<()> {
    with_session(&state, |s| s.scroll_to_bottom())
}

/// Entry point: handle the command line, then run the app.
pub fn main() {
    #[cfg(target_os = "linux")]
    if std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER").is_none() {
        std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1");
    }

    let code = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Run(launch)) => {
            run(launch);
            0
        },
        Ok(cli::Command::Update { check }) => updates::cli(check),
        Ok(cli::Command::Version) => {
            println!("tersh {}", env!("CARGO_PKG_VERSION"));
            0
        },
        Ok(cli::Command::Help) => {
            print!("{}", cli::USAGE);
            0
        },
        Err(e) => {
            eprintln!("tersh: {e}\n\n{}", cli::USAGE);
            2
        },
    };
    std::process::exit(code);
}

pub fn run(launch: cli::Launch) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let store: Arc<dyn ConfigStore> = Arc::new(FsConfigStore::user());
    if let Err(e) = store.ensure() {
        log::warn!("could not create {}: {e}", store.config_path().display());
    }
    let config = store.load();
    for warning in &config.warnings {
        log::warn!("config: {warning}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol(cards::SCHEME, cards::protocol)
        .manage(cards::CardStore::new(config.rich.html_network))
        .manage(AppState {
            store,
            fonts: Arc::new(SystemFontDirs::new()),
            config: Mutex::new(config.clone()),
            sink: Arc::default(),
            session: Mutex::new(None),
            launch: Mutex::new(launch.clone()),
        })
        .invoke_handler(tauri::generate_handler![
            settings::get_settings,
            settings::reload_config,
            settings::open_config,
            session_start,
            session_write,
            session_resize,
            session_scroll,
            session_scroll_to_bottom,
            cards::card_html_put,
            cards::card_html_drop,
            list_plugins,
            updates::restart_app,
        ])
        .setup(move |app| {
            create_window(app.handle(), &config, launch.size)?;
            settings::watch(app.handle().clone());
            updates::watch(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tersh");
}

fn create_window(app: &AppHandle, config: &Config, size: Option<(f64, f64)>) -> tauri::Result<()> {
    let (width, height) = size.unwrap_or((config.window.width, config.window.height));
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("tersh")
        .inner_size(width, height)
        .min_inner_size(200.0, 100.0)
        .decorations(false)
        .transparent(true)
        .shadow(true)
        .build()?;

    if config.window.blur {
        apply_blur(&window);
    }
    Ok(())
}

#[cfg(target_os = "macos")]
fn apply_blur(window: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_vibrancy, NSVisualEffectMaterial};
    if let Err(e) = apply_vibrancy(window, NSVisualEffectMaterial::HudWindow, None, None) {
        log::warn!("vibrancy unavailable: {e}");
    }
}

#[cfg(target_os = "windows")]
fn apply_blur(window: &tauri::WebviewWindow) {
    use window_vibrancy::{apply_acrylic, apply_blur};
    if apply_acrylic(window, Some((18, 18, 18, 125))).is_err() {
        if let Err(e) = apply_blur(window, Some((18, 18, 18, 125))) {
            log::warn!("blur unavailable: {e}");
        }
    }
}

/// On Linux the backdrop blur is up to the compositor (e.g. KDE's blur effect or
/// Hyprland's `decoration:blur`); the window itself is transparent.
#[cfg(not(any(target_os = "macos", target_os = "windows")))]
fn apply_blur(_window: &tauri::WebviewWindow) {}
