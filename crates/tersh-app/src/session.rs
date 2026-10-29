//! The session use case: a program running in a pseudo-terminal, its screen
//! state, and the threads that move bytes between them and the UI.
//!
//! Everything external comes in through ports: the PTY ([`PtySystem`]), the
//! VT emulator ([`EmulatorFactory`]) and the UI ([`Sink`]). Rich messages
//! arrive either embedded in program output (OSC 7701) or through a
//! [`RichInbox`] that a socket adapter feeds.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use anyhow::{Context, Result};
use serde_json::Value;
use tersh_domain::config::Cursor;
use tersh_domain::rich::{self, AnchorContext, Chunk, OscScanner};
use tersh_domain::update::Foreground;
use tersh_domain::{Config, Info, Palette, Rgb, TermEvent, WindowSize};

use crate::ports::{
    Emulator, EmulatorFactory, EmulatorOptions, ProcessInspector, ProcessKiller, PtyControl, PtySystem, Scroll, Sink,
    SpawnRequest, TermHost,
};

/// Minimum time between frames during heavy output (~240 Hz). The first frame
/// after a quiet period is sent immediately, so keystroke echo is never delayed.
const FRAME_INTERVAL: Duration = Duration::from_millis(4);

/// Variables a new terminal must not inherit from whatever launched the app:
/// it is not inside that tmux, nor inside another tersh.
const NOT_INHERITED: [&str; 3] = ["TMUX", "TMUX_PANE", "TERSH_SOCKET"];

#[derive(Debug, Clone)]
pub struct SessionOptions {
    /// `None` runs the system's default shell.
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// `None` starts in the home directory.
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub scrollback: usize,
    /// Accept rich messages embedded in program output.
    pub osc_rich: bool,
    /// A rich-message socket to advertise to programs as `$TERSH_SOCKET`.
    pub rich_socket: Option<PathBuf>,
    pub palette: Palette,
    pub cursor: Cursor,
}

impl SessionOptions {
    pub fn from_config(config: &Config, cols: u16, rows: u16) -> Self {
        Self {
            shell: config.shell().map(String::from),
            args: config.args.clone(),
            cwd: None,
            env: config.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            cols,
            rows,
            cell_width: 0,
            cell_height: 0,
            scrollback: config.scrollback,
            osc_rich: config.rich.enabled && config.rich.osc,
            rich_socket: None,
            palette: Palette::from_theme(&config.theme),
            cursor: config.cursor.clone(),
        }
    }

    fn emulator_options(&self) -> EmulatorOptions {
        EmulatorOptions { cols: self.cols, rows: self.rows, scrollback: self.scrollback, cursor: self.cursor.clone() }
    }
}

/// The adapters a session runs on.
pub struct Ports<'a> {
    pub pty: &'a dyn PtySystem,
    pub emulator: &'a dyn EmulatorFactory,
}

type Input = Arc<Mutex<Box<dyn Write + Send>>>;

/// Answers the emulator's questions (device queries, colours, size) and forwards its events.
struct Host {
    input: Input,
    sink: Arc<dyn Sink>,
    palette: Mutex<Palette>,
    size: Mutex<WindowSize>,
}

impl TermHost for Host {
    fn reply(&self, text: &str) {
        let mut input = lock(&self.input);
        let _ = input.write_all(text.as_bytes()).and_then(|_| input.flush());
    }

    fn event(&self, event: TermEvent) {
        self.sink.event(event);
    }

    fn color(&self, index: usize) -> Rgb {
        lock(&self.palette).get(index)
    }

    fn window_size(&self) -> WindowSize {
        *lock(&self.size)
    }
}

struct State {
    emulator: Box<dyn Emulator>,
    scanner: OscScanner,
    force_full: bool,
}

struct Shared {
    state: Mutex<State>,
    host: Arc<Host>,
    control: Mutex<Box<dyn PtyControl>>,
    killer: Mutex<Box<dyn ProcessKiller>>,
    shell_pid: Option<u32>,
    wake: mpsc::Sender<()>,
    closed: AtomicBool,
    options: SessionOptions,
}

/// A running program. Dropping it kills the program.
pub struct Session {
    shared: Arc<Shared>,
    /// Adapter resources that live as long as the session (e.g. its socket server).
    guards: Vec<Box<dyn Send>>,
}

/// Accepts rich messages from outside the PTY stream, e.g. from a socket server.
/// Holds the session weakly: once the session is gone, [`RichInbox::handle`] returns `None`.
#[derive(Clone)]
pub struct RichInbox(Weak<Shared>);

impl RichInbox {
    /// Handle one message. The reply is `{"ok":true}`, `{"ok":false,"error":...}`,
    /// or for `{"op":"info"}` the grid description.
    pub fn handle(&self, line: &[u8]) -> Option<Value> {
        let shared = self.0.upgrade().filter(|s| !s.closed.load(Ordering::SeqCst))?;
        Some(shared.handle_message(line))
    }

    pub fn is_open(&self) -> bool {
        self.0.upgrade().is_some_and(|s| !s.closed.load(Ordering::SeqCst))
    }
}

impl Session {
    pub fn spawn(options: SessionOptions, ports: Ports<'_>, sink: Arc<dyn Sink>) -> Result<Self> {
        let mut env: Vec<(String, String)> = vec![
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
            ("TERM_PROGRAM".into(), "tersh".into()),
            ("TERM_PROGRAM_VERSION".into(), env!("CARGO_PKG_VERSION").into()),
        ];
        if let Some(path) = &options.rich_socket {
            env.push(("TERSH_SOCKET".into(), path.display().to_string()));
        }
        env.extend(options.env.iter().cloned());
        let request = SpawnRequest {
            shell: options.shell.clone(),
            args: options.args.clone(),
            cwd: options.cwd.clone(),
            env_remove: NOT_INHERITED.map(String::from).to_vec(),
            env,
            cols: options.cols,
            rows: options.rows,
            cell_width: options.cell_width,
            cell_height: options.cell_height,
        };
        let process = ports.pty.spawn(&request).context("starting the shell")?;
        log::debug!("started {}", process.program);
        let mut output = process.output;
        let mut child = process.child;
        let killer = child.killer();
        let shell_pid = child.pid();

        let host = Arc::new(Host {
            input: Arc::new(Mutex::new(process.input)),
            sink,
            palette: Mutex::new(options.palette.clone()),
            size: Mutex::new(WindowSize {
                cols: options.cols,
                rows: options.rows,
                cell_width: options.cell_width,
                cell_height: options.cell_height,
            }),
        });
        let emulator = ports.emulator.create(&options.emulator_options(), host.clone());

        let (wake, wake_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(State { emulator, scanner: OscScanner::new(), force_full: true }),
            host,
            control: Mutex::new(process.control),
            killer: Mutex::new(killer),
            shell_pid,
            wake,
            closed: AtomicBool::new(false),
            options,
        });

        let s = shared.clone();
        thread::Builder::new().name("tersh-pty-read".into()).spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match output.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => s.process(&buf[..n]),
                }
                let _ = s.wake.send(());
            }
        })?;

        let s = shared.clone();
        thread::Builder::new().name("tersh-render".into()).spawn(move || s.render_loop(wake_rx))?;

        let s = shared.clone();
        thread::Builder::new().name("tersh-wait".into()).spawn(move || {
            let code = child.wait();
            s.closed.store(true, Ordering::SeqCst);
            let _ = s.wake.send(());
            s.host.sink.event(TermEvent::Exit { code });
        })?;

        Ok(Self { shared, guards: Vec::new() })
    }

    /// Keep an adapter resource alive until the session ends (it is dropped after the program is killed).
    pub fn keep_alive(&mut self, guard: Box<dyn Send>) {
        self.guards.push(guard);
    }

    pub fn rich_inbox(&self) -> RichInbox {
        RichInbox(Arc::downgrade(&self.shared))
    }

    pub fn socket_path(&self) -> Option<&PathBuf> {
        self.shared.options.rich_socket.as_ref()
    }

    /// Send input (keystrokes, pastes, mouse reports) to the program.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        {
            let mut state = lock(&self.shared.state);
            if state.emulator.display_offset() != 0 {
                state.emulator.scroll(Scroll::Bottom);
                drop(state);
                let _ = self.shared.wake.send(());
            }
        }
        let mut input = lock(&self.shared.host.input);
        input.write_all(bytes)?;
        input.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> Result<()> {
        let size = WindowSize { cols: cols.max(2), rows: rows.max(1), cell_width, cell_height };
        *lock(&self.shared.host.size) = size;
        {
            let mut state = lock(&self.shared.state);
            state.emulator.resize(size.cols, size.rows);
            state.force_full = true;
        }
        lock(&self.shared.control).resize(size)?;
        let _ = self.shared.wake.send(());
        Ok(())
    }

    /// Scroll the viewport through history by `lines` (positive is up).
    pub fn scroll(&self, lines: i32) {
        lock(&self.shared.state).emulator.scroll(Scroll::Lines(lines));
        let _ = self.shared.wake.send(());
    }

    pub fn scroll_to_bottom(&self) {
        lock(&self.shared.state).emulator.scroll(Scroll::Bottom);
        let _ = self.shared.wake.send(());
    }

    /// Resend the whole viewport, e.g. after the renderer reloaded.
    pub fn refresh(&self) {
        lock(&self.shared.state).force_full = true;
        let _ = self.shared.wake.send(());
    }

    pub fn info(&self) -> Info {
        self.shared.info()
    }

    /// What is in the foreground (see `tersh_domain::update::restart_decision`).
    pub fn foreground(&self, processes: &dyn ProcessInspector) -> Foreground {
        let pid = lock(&self.shared.control).foreground_pid();
        Foreground { shell_pid: self.shared.shell_pid, pid, name: pid.and_then(|pid| processes.name(pid)) }
    }

    /// The shell's working directory.
    pub fn cwd(&self, processes: &dyn ProcessInspector) -> Option<PathBuf> {
        self.shared.shell_pid.and_then(|pid| processes.cwd(pid))
    }

    /// Apply the parts of a reloaded config that a running program can pick up:
    /// the colours reported to programs and the default cursor. (Shell,
    /// environment and scrollback only affect new sessions.)
    pub fn reconfigure(&self, config: &Config) {
        *lock(&self.shared.host.palette) = Palette::from_theme(&config.theme);
        let size = *lock(&self.shared.host.size);
        let options = EmulatorOptions {
            cols: size.cols,
            rows: size.rows,
            scrollback: self.shared.options.scrollback,
            cursor: config.cursor.clone(),
        };
        let mut state = lock(&self.shared.state);
        state.emulator.set_options(&options);
        state.force_full = true;
        drop(state);
        let _ = self.shared.wake.send(());
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        lock(&self.shared.killer).kill();
        let _ = self.shared.wake.send(());
        // `guards` drop after this, once the program is gone.
    }
}

impl Shared {
    fn process(&self, bytes: &[u8]) {
        let mut messages = Vec::new();
        {
            let mut state = lock(&self.state);
            let State { emulator, scanner, .. } = &mut *state;
            let osc_rich = self.options.osc_rich;
            scanner.feed(bytes, |chunk| match chunk {
                Chunk::Bytes(bytes) => emulator.advance(bytes),
                Chunk::Payload(payload) if osc_rich => {
                    // Apply output buffered by a synchronized update first, so
                    // the cursor is where the program thinks it is.
                    if emulator.sync_deadline().is_some() {
                        emulator.flush_sync();
                    }
                    messages.push((payload, emulator.anchor_context()));
                },
                Chunk::Payload(_) => {},
            });
        }
        for (payload, ctx) in messages {
            let _ = self.dispatch(rich::decode_payload(&payload), ctx);
        }
    }

    fn handle_message(&self, line: &[u8]) -> Value {
        let message = rich::decode_payload(line);
        if let Ok(msg) = &message {
            if msg.get("op").and_then(Value::as_str) == Some("info") {
                let mut info = serde_json::to_value(self.info()).unwrap_or_default();
                info["ok"] = Value::Bool(true);
                return info;
            }
        }
        let ctx = lock(&self.state).emulator.anchor_context();
        match self.dispatch(message, ctx) {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        }
    }

    fn dispatch(&self, message: Result<Value, String>, ctx: AnchorContext) -> Result<(), String> {
        match message {
            Ok(mut msg) => {
                rich::resolve_anchor(&mut msg, &ctx);
                self.host.sink.rich(msg);
                Ok(())
            },
            Err(error) => {
                self.host.sink.event(TermEvent::RichError { error: error.clone() });
                Err(error)
            },
        }
    }

    fn info(&self) -> Info {
        let size = *lock(&self.host.size);
        let state = lock(&self.state);
        let ctx = state.emulator.anchor_context();
        Info {
            version: env!("CARGO_PKG_VERSION"),
            cols: size.cols,
            rows: size.rows,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
            cursor_row: ctx.cursor_row,
            cursor_col: ctx.cursor_col,
            alt_screen: state.emulator.alt_screen(),
        }
    }

    fn render_loop(&self, wake: mpsc::Receiver<()>) {
        let mut last_frame = Instant::now() - FRAME_INTERVAL;
        loop {
            // Wake for new output, or when a synchronized update times out.
            let deadline = lock(&self.state).emulator.sync_deadline();
            let wait = deadline.map_or(Duration::from_secs(3600), |d| d.saturating_duration_since(Instant::now()));
            match wake.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {},
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let since = last_frame.elapsed();
            if since < FRAME_INTERVAL {
                thread::sleep(FRAME_INTERVAL - since);
            }
            while wake.try_recv().is_ok() {}

            let frame = {
                let mut state = lock(&self.state);
                let State { emulator, force_full, .. } = &mut *state;
                if let Some(deadline) = emulator.sync_deadline() {
                    if Instant::now() < deadline {
                        // Mid synchronized update: wait for it to finish.
                        continue;
                    }
                    emulator.flush_sync();
                }
                let frame = emulator.frame(*force_full);
                *force_full = false;
                frame
            };
            last_frame = Instant::now();
            self.host.sink.frame(frame);

            if self.closed.load(Ordering::SeqCst) {
                return;
            }
        }
    }
}

/// Lock, recovering from poisoning: a panic elsewhere shouldn't freeze the terminal.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
