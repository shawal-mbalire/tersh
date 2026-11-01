//! Desktop UI adapter and composition root: one Tauri window hosting a
//! session (a canvas grid with DOM card overlays). It wires the native
//! adapters from `tersh-adapters` into the `tersh-app` use cases and exposes
//! them to the page as commands and channels.

mod cards;
mod cli;
mod environment;
mod settings;
mod updates;

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use serde_json::Value;
use tauri::ipc::Channel;
use tauri::{AppHandle, Manager, RunEvent, State, WebviewUrl, WebviewWindowBuilder};
use tersh_adapters::{FsConfigStore, SystemFontDirs, SystemProcesses};
use tersh_app::ports::{ConfigStore, Plugin, ProcessInspector, Watchable};
use tersh_app::{Session, SessionOptions, Sink};
use tersh_domain::{AppError, Code, Config, Frame, Info, Stats, TermEvent};

/// How long the window may stay hidden waiting for the page's first paint
/// before it is shown anyway (e.g. if the page failed to load).
const SHOW_TIMEOUT: Duration = Duration::from_secs(2);

/// Forwards session output to whichever webview channels are current. The
/// channels are swapped when the page reloads, while the shell keeps running.
/// When the shell exits, the app quits.
#[derive(Default)]
struct ChannelSink {
    channels: Mutex<Option<Channels>>,
    app: OnceLock<AppHandle>,
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
        let exited = matches!(event, TermEvent::Exit { .. });
        if let Some(c) = self.channels.lock().unwrap().as_ref() {
            let _ = c.events.send(event);
        }
        // Quit from here rather than waiting for the page to ask, so the
        // window closes at once (and even if the page is stuck).
        if exited {
            if let Some(app) = self.app.get() {
                app.exit(0);
            }
        }
    }

    /// The page acknowledges each frame once drawn (`session_frame_shown`).
    fn acks_frames(&self) -> bool {
        true
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
    env: environment::Environment,
    /// Whether the window has been shown (it starts hidden; see [`window_ready`]).
    shown: AtomicBool,
    started: std::time::Instant,
}

/// Commands fail with an [`AppError`], which reaches the page as its text.
type CmdResult<T> = tersh_domain::error::Result<T>;

/// Attach the page to the shell, starting it on first call.
#[tauri::command]
#[allow(clippy::too_many_arguments)] // each argument is a named `invoke` parameter from the page
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
            if options.shell.is_none() {
                options.shell = state.env.shell.clone();
            }
            options.inherited_env = state.env.vars.clone();
            let launch = std::mem::take(&mut *state.launch.lock().unwrap());
            options.cwd = launch.cwd;
            let rich_enabled = state.config.lock().unwrap().rich.enabled;
            let spawned = tersh_adapters::spawn_local(options, rich_enabled, state.sink.clone())?;
            if let Some(input) = launch.input {
                let _ = spawned.write(input.as_bytes());
            }
            *session = Some(spawned);
        },
    }
    Ok(session.as_ref().unwrap().info())
}

fn with_session<T>(state: &State<'_, AppState>, f: impl FnOnce(&Session) -> T) -> CmdResult<T> {
    match state.session.lock().unwrap().as_ref() {
        Some(session) => Ok(f(session)),
        None => Err(AppError::new(Code::NoSession, "no session")),
    }
}

#[tauri::command]
fn session_write(state: State<'_, AppState>, data: String) -> CmdResult<()> {
    with_session(&state, |s| s.write(data.as_bytes()))?
}

#[tauri::command]
fn session_resize(state: State<'_, AppState>, cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> CmdResult<()> {
    with_session(&state, |s| s.resize(cols, rows, cell_width, cell_height))?
}

#[tauri::command]
fn session_scroll(state: State<'_, AppState>, lines: i32) -> CmdResult<()> {
    with_session(&state, |s| s.scroll(lines))
}

/// The page has drawn the last frame it was sent.
#[tauri::command]
fn session_frame_shown(state: State<'_, AppState>) -> CmdResult<()> {
    with_session(&state, |s| s.frame_shown())
}

#[tauri::command]
fn list_plugins(state: State<'_, AppState>) -> Vec<Plugin> {
    let disabled = state.config.lock().unwrap().plugins.disabled.clone();
    state.store.plugins(&disabled)
}

/// Profiling counters for the profiler overlay, with this process's memory.
#[tauri::command]
fn session_stats(state: State<'_, AppState>) -> CmdResult<Stats> {
    let mut stats = with_session(&state, |s| s.stats())?;
    stats.memory_bytes = SystemProcesses.memory(std::process::id());
    Ok(stats)
}

/// The page has painted its first frame: show the window. The window is
/// created hidden so it never appears as an empty (transparent) frame.
#[tauri::command]
fn window_ready(app: AppHandle) {
    show_window(&app);
}

fn show_window(app: &AppHandle) {
    let state = app.state::<AppState>();
    if state.shown.swap(true, Ordering::SeqCst) {
        return;
    }
    log::info!("showing the window {:?} after start", state.started.elapsed());
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}

#[tauri::command]
fn session_scroll_to_bottom(state: State<'_, AppState>) -> CmdResult<()> {
    with_session(&state, |s| s.scroll_to_bottom())
}

/// Entry point: handle the command line, then run the app.
pub fn main() {
    let code = match cli::parse(std::env::args().skip(1)) {
        Ok(cli::Command::Run(launch)) => {
            run(launch, environment::Environment::read());
            0
        },
        Ok(cli::Command::Update { check }) => updates::cli(check, environment::Environment::read()),
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

pub fn run(launch: cli::Launch, env: environment::Environment) {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("warn")).init();
    let store: Arc<dyn ConfigStore> = Arc::new(FsConfigStore::user());
    if let Err(e) = store.ensure() {
        log::warn!("{e}");
    }
    let config = store.load();
    for warning in &config.warnings {
        log::warn!("config: {warning}");
    }

    let sink: Arc<ChannelSink> = Arc::default();
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_clipboard_manager::init())
        .register_uri_scheme_protocol(cards::SCHEME, cards::protocol)
        .manage(cards::CardStore::new(config.rich.html_network))
        .manage(AppState {
            store,
            fonts: Arc::new(SystemFontDirs::new(env.windows_dir.clone())),
            config: Mutex::new(config.clone()),
            sink: sink.clone(),
            session: Mutex::new(None),
            launch: Mutex::new(launch.clone()),
            env,
            shown: AtomicBool::new(false),
            started: std::time::Instant::now(),
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
            session_frame_shown,
            session_stats,
            window_ready,
            cards::card_html_put,
            cards::card_html_drop,
            list_plugins,
            updates::restart_app,
        ])
        .setup(move |app| {
            let _ = sink.app.set(app.handle().clone());
            create_window(app.handle(), &config, launch.size)?;
            let handle = app.handle().clone();
            std::thread::spawn(move || {
                std::thread::sleep(SHOW_TIMEOUT);
                if !handle.state::<AppState>().shown.load(Ordering::SeqCst) {
                    log::warn!("the page did not paint within {SHOW_TIMEOUT:?}; showing the window anyway");
                    show_window(&handle);
                }
            });
            settings::watch(app.handle().clone());
            updates::watch(app.handle().clone());
            Ok(())
        })
        .build(tauri::generate_context!())
        .expect("error while building tersh");

    app.run(|app, event| {
        if let RunEvent::Exit = event {
            // The process exits right after this without dropping managed
            // state, so end the session here: the program gets its hangup and
            // the rich socket file is removed.
            let session = app.state::<AppState>().session.lock().unwrap().take();
            drop(session);
        }
    });
}

fn create_window(app: &AppHandle, config: &Config, size: Option<(f64, f64)>) -> tauri::Result<()> {
    let (width, height) = size.unwrap_or((config.window.width, config.window.height));
    let window = WebviewWindowBuilder::new(app, "main", WebviewUrl::default())
        .title("tersh")
        .inner_size(width, height)
        .min_inner_size(200.0, 100.0)
        .visible(false)
        .decorations(false)
        .transparent(true)
        .background_color(tauri::window::Color(0, 0, 0, 0))
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
