//! Tauri host: one window, one PTY session, a canvas grid and DOM card overlays.

mod cards;
mod plugins;
mod settings;

use std::sync::{Arc, Mutex};

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, State, WebviewUrl, WebviewWindowBuilder};
use tersh_core::session::Info;
use tersh_core::{Config, Frame, Session, SessionOptions, Sink, TermEvent};

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
    /// The latest loaded config; replaced on reload.
    config: Mutex<Config>,
    sink: Arc<ChannelSink>,
    session: Mutex<Option<Session>>,
}

type CmdResult<T> = Result<T, String>;

/// Attach the page to the shell, starting it on first call.
#[tauri::command]
fn session_start(
    state: State<'_, AppState>,
    cols: u16,
    rows: u16,
    frames: Channel<Frame>,
    rich: Channel<Value>,
    events: Channel<TermEvent>,
) -> CmdResult<Info> {
    *state.sink.channels.lock().unwrap() = Some(Channels { frames, rich, events });
    let mut session = state.session.lock().unwrap();
    match session.as_ref() {
        Some(existing) => existing.refresh(),
        None => {
            let options = SessionOptions::from_config(&state.config.lock().unwrap(), cols, rows);
            *session = Some(Session::spawn(options, state.sink.clone()).map_err(|e| format!("{e:#}"))?);
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
fn session_scroll_to_bottom(state: State<'_, AppState>) -> CmdResult<()> {
    with_session(&state, |s| s.scroll_to_bottom())
}

pub fn run() {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    if let Err(e) = Config::ensure_file() {
        log::warn!("could not create {}: {e}", Config::path().display());
    }
    let config = Config::load();
    for warning in &config.warnings {
        log::warn!("config: {warning}");
    }

    tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol(cards::SCHEME, cards::protocol)
        .manage(cards::CardStore::new(config.rich.html_network))
        .manage(AppState { config: Mutex::new(config.clone()), sink: Arc::default(), session: Mutex::new(None) })
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
            plugins::list_plugins,
        ])
        .setup(move |app| {
            create_window(app.handle(), &config)?;
            settings::watch(app.handle().clone());
            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running tersh");
}

fn create_window(app: &AppHandle, config: &Config) -> tauri::Result<()> {
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("tersh")
        .inner_size(config.window.width, config.window.height)
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
