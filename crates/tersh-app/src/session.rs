//! The session use case: a program running in a pseudo-terminal, its screen
//! state, and the threads that move bytes between them and the UI.
//!
//! Everything external comes in through ports: the PTY ([`PtySystem`]), the
//! VT emulator ([`EmulatorFactory`]) and the UI ([`Sink`]). Rich messages
//! arrive either embedded in program output (OSC 7701) or through a
//! [`RichInbox`] that a socket adapter feeds.

use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard, Weak};
use std::thread;
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_domain::config::Cursor;
use tersh_domain::rich::{self, AnchorContext, Chunk, OscScanner};
use tersh_domain::error::{AppError, Code, Result};
use tersh_domain::update::Foreground;
use tersh_domain::{Config, Frame, Info, Palette, Rgb, Stats, TermEvent, WindowSize};

use crate::ports::{Measurement, TimePort, 
    Emulator, EmulatorFactory, EmulatorOptions, ProcessInspector, ProcessKiller, PtyControl, PtySystem, Scroll, Sink,
    SpawnRequest, TermHost,
};

/// Minimum time between frames during heavy output (~240 Hz). The first frame
/// after a quiet period is sent immediately, so keystroke echo is never delayed.
const FRAME_INTERVAL: Duration = Duration::from_millis(4);
/// How much program output to read at once.
const READ_BUFFER: usize = 64 * 1024;
/// How long the render thread sleeps when nothing is pending; any output wakes it.
const IDLE_WAIT: Duration = Duration::from_secs(3600);

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
    /// The environment the shell would inherit, filled in by the entry point
    /// (the only place that reads the process environment). Paths into
    /// tersh's own bundle (the AppImage) are removed from it before the shell
    /// starts.
    pub inherited_env: Vec<(String, String)>,
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
            inherited_env: Vec::new(),
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
    pub clock: Arc<dyn TimePort>,
}

/// Profiling counters (see [`Stats`]); plain atomic adds on the hot paths.
#[derive(Default)]
struct Counters {
    output_bytes: AtomicU64,
    output_reads: AtomicU64,
    parse_us: AtomicU64,
    frames_built: AtomicU64,
    frames_sent: AtomicU64,
    frames_skipped: AtomicU64,
    rows_sent: AtomicU64,
    build_us: AtomicU64,
    build_max_us: AtomicU64,
    input_bytes: AtomicU64,
    input_writes: AtomicU64,
    input_pending: AtomicU64,
    history: AtomicU64,
}

fn add(counter: &AtomicU64, n: u64) {
    counter.fetch_add(n, Ordering::Relaxed);
}

fn micros(d: Duration) -> u64 {
    d.as_micros().try_into().unwrap_or(u64::MAX)
}

/// Answers the emulator's questions (device queries, colours, size) and forwards its events.
struct Host {
    /// Bytes for the program, written in order by the `tersh-pty-write` thread
    /// so a program that isn't reading (a large paste) never blocks the caller.
    input: mpsc::Sender<Vec<u8>>,
    counters: Arc<Counters>,
    sink: Arc<dyn Sink>,
    palette: Mutex<Palette>,
    size: Mutex<WindowSize>,
}

impl Host {
    /// Queue bytes for the program. False once the program's input has closed.
    fn send(&self, bytes: &[u8]) -> bool {
        if bytes.is_empty() {
            return true;
        }
        add(&self.counters.input_pending, bytes.len() as u64);
        self.input.send(bytes.to_vec()).is_ok()
    }
}

/// Write queued input to the program until the queue or the program's input closes.
fn write_loop(mut output: Box<dyn Write + Send>, queue: mpsc::Receiver<Vec<u8>>, counters: Arc<Counters>) {
    while let Ok(mut bytes) = queue.recv() {
        // Write whatever else is already queued in the same call.
        while let Ok(more) = queue.try_recv() {
            bytes.extend_from_slice(&more);
        }
        let n = bytes.len() as u64;
        let written = output.write_all(&bytes).and_then(|_| output.flush());
        counters.input_pending.fetch_sub(n, Ordering::Relaxed);
        add(&counters.input_bytes, n);
        add(&counters.input_writes, 1);
        if written.is_err() {
            break;
        }
    }
}

impl TermHost for Host {
    fn reply(&self, text: &str) {
        self.send(text.as_bytes());
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

    fn measured(&self, measurement: Measurement) {
        let counters = &self.counters;
        match measurement {
            Measurement::Advance(took) => add(&counters.parse_us, micros(took)),
            Measurement::Frame(took) => {
                let took = micros(took);
                add(&counters.build_us, took);
                counters.build_max_us.fetch_max(took, Ordering::Relaxed);
            },
        }
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
    clock: Arc<dyn TimePort>,
    started: Instant,
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

    /// Whether everything the session held (its threads, screen and
    /// scrollback) has been freed. Becomes true shortly after the session is
    /// dropped; used to check that sessions don't leak.
    pub fn is_released(&self) -> bool {
        self.0.strong_count() == 0
    }
}

impl Session {
    pub fn spawn(options: SessionOptions, ports: Ports<'_>, sink: Arc<dyn Sink>) -> Result<Self> {
        // Undo the AppImage launcher's paths first, so `[env]` in the config still wins.
        let cleanup = tersh_domain::env::without_bundle(&options.inherited_env);
        let mut env_remove: Vec<String> = NOT_INHERITED.map(String::from).to_vec();
        env_remove.extend(cleanup.remove);
        let mut env = cleanup.set;
        env.extend([
            ("TERM".into(), "xterm-256color".into()),
            ("COLORTERM".into(), "truecolor".into()),
            ("TERM_PROGRAM".into(), "tersh".into()),
            ("TERM_PROGRAM_VERSION".into(), env!("CARGO_PKG_VERSION").into()),
        ]);
        if let Some(path) = &options.rich_socket {
            env.push(("TERSH_SOCKET".into(), path.display().to_string()));
        }
        env.extend(options.env.iter().cloned());
        let request = SpawnRequest {
            shell: options.shell.clone(),
            args: options.args.clone(),
            cwd: options.cwd.clone(),
            env_remove,
            env,
            cols: options.cols,
            rows: options.rows,
            cell_width: options.cell_width,
            cell_height: options.cell_height,
        };
        let process = ports.pty.spawn(&request)?;
        log::debug!("started {}", process.program);
        let mut output = process.output;
        let mut child = process.child;
        let counters = Arc::new(Counters::default());
        let (input, input_rx) = mpsc::channel();
        let writer = process.input;
        let c = counters.clone();
        start_thread("tersh-pty-write", move || write_loop(writer, input_rx, c))?;
        let killer = child.killer();
        let shell_pid = child.pid();

        let host = Arc::new(Host {
            input,
            counters,
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
            started: ports.clock.now(),
            clock: ports.clock,
        });

        // The reader and the wait thread hold the session weakly: either can
        // block for as long as something keeps the terminal or the program
        // alive, and must not keep a dropped session's memory with it.
        let weak = Arc::downgrade(&shared);
        start_thread("tersh-pty-read", move || {
            let mut buf = vec![0u8; READ_BUFFER];
            loop {
                let n = match output.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => n,
                };
                let Some(s) = weak.upgrade() else { break };
                s.process(&buf[..n]);
                let _ = s.wake.send(());
            }
        })?;

        let s = shared.clone();
        start_thread("tersh-render", move || s.render_loop(wake_rx))?;

        let weak = Arc::downgrade(&shared);
        start_thread("tersh-wait", move || {
            let code = child.wait();
            let Some(s) = weak.upgrade() else { return };
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
        if !self.shared.host.send(bytes) {
            return Err(AppError::new(Code::InputClosed, "the program's input is closed"));
        }
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

    /// Profiling counters since the session started (`memory_bytes` is left
    /// for the caller, who knows how to measure its own process).
    pub fn stats(&self) -> Stats {
        self.shared.stats()
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
        // A background program may still hold the terminal open; stop reading anyway.
        lock(&self.shared.control).hangup();
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
                Chunk::Oversized => log::warn!("dropping rich payload larger than {} bytes", rich::MAX_PAYLOAD),
            });
        }
        let counters = &self.host.counters;
        add(&counters.output_bytes, bytes.len() as u64);
        add(&counters.output_reads, 1);
        for (payload, ctx) in messages {
            let _ = self.dispatch(rich::decode_payload(&payload), ctx);
        }
    }

    fn handle_message(&self, line: &[u8]) -> Value {
        let message = rich::decode_payload(line);
        if let Ok(msg) = &message {
            let reply = match msg.get("op").and_then(Value::as_str) {
                Some("info") => Some(serde_json::to_value(self.info())),
                Some("stats") => Some(serde_json::to_value(self.stats())),
                _ => None,
            };
            if let Some(reply) = reply {
                let mut reply = reply.unwrap_or_default();
                reply["ok"] = Value::Bool(true);
                return reply;
            }
        }
        let ctx = lock(&self.state).emulator.anchor_context();
        match self.dispatch(message, ctx) {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(error) => serde_json::json!({ "ok": false, "error": error, "code": error.code.as_str() }),
        }
    }

    fn dispatch(&self, message: Result<Value>, ctx: AnchorContext) -> Result<()> {
        match message {
            Ok(mut msg) => {
                rich::resolve_anchor(&mut msg, &ctx);
                self.host.sink.rich(msg);
                Ok(())
            },
            Err(error) => {
                self.host.sink.event(TermEvent::RichError { error: error.to_string() });
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

    fn stats(&self) -> Stats {
        let c = &self.host.counters;
        let get = |counter: &AtomicU64| counter.load(Ordering::Relaxed);
        Stats {
            uptime_ms: (self.clock.now() - self.started).as_millis().try_into().unwrap_or(u64::MAX),
            output_bytes: get(&c.output_bytes),
            output_reads: get(&c.output_reads),
            parse_us: get(&c.parse_us),
            frames_built: get(&c.frames_built),
            frames_sent: get(&c.frames_sent),
            frames_skipped: get(&c.frames_skipped),
            rows_sent: get(&c.rows_sent),
            build_us: get(&c.build_us),
            build_max_us: get(&c.build_max_us),
            input_bytes: get(&c.input_bytes),
            input_writes: get(&c.input_writes),
            input_pending: get(&c.input_pending),
            history: get(&c.history),
            memory_bytes: None,
        }
    }

    fn render_loop(&self, wake: mpsc::Receiver<()>) {
        let mut last_frame = self.clock.now() - FRAME_INTERVAL;
        let mut last_sent: Option<Frame> = None;
        loop {
            // Wake for new output, or when a synchronized update times out.
            let deadline = lock(&self.state).emulator.sync_deadline();
            let wait = deadline.map_or(IDLE_WAIT, |d| d.saturating_duration_since(self.clock.now()));
            match wake.recv_timeout(wait) {
                Ok(()) | Err(RecvTimeoutError::Timeout) => {},
                Err(RecvTimeoutError::Disconnected) => return,
            }
            let since = self.clock.now() - last_frame;
            if since < FRAME_INTERVAL {
                self.clock.sleep(FRAME_INTERVAL - since);
            }
            while wake.try_recv().is_ok() {}

            let mut frame = {
                let mut state = lock(&self.state);
                let State { emulator, force_full, .. } = &mut *state;
                if let Some(deadline) = emulator.sync_deadline() {
                    if self.clock.now() < deadline {
                        // Mid synchronized update: wait for it to finish.
                        continue;
                    }
                    emulator.flush_sync();
                }
                let frame = emulator.frame(*force_full);
                *force_full = false;
                let counters = &self.host.counters;
                add(&counters.frames_built, 1);
                counters.history.store(frame.history as u64, Ordering::Relaxed);
                frame
            };
            last_frame = self.clock.now();
            // Output that changed nothing visible (e.g. a redraw of identical
            // text) needs no trip to the UI.
            if last_sent.as_ref().is_none_or(|last| !unchanged(last, &frame)) {
                let lines = std::mem::take(&mut frame.lines);
                last_sent = Some(frame.clone());
                frame.lines = lines;
                add(&self.host.counters.frames_sent, 1);
                add(&self.host.counters.rows_sent, frame.lines.len() as u64);
                self.host.sink.frame(frame);
            } else {
                add(&self.host.counters.frames_skipped, 1);
            }

            if self.closed.load(Ordering::SeqCst) {
                return;
            }
        }
    }
}

/// Whether `next` repaints nothing and moves nothing compared to `last`.
fn unchanged(last: &Frame, next: &Frame) -> bool {
    !next.full
        && next.lines.is_empty()
        && next.cursor == last.cursor
        && next.mode == last.mode
        && (next.cols, next.rows, next.alt) == (last.cols, last.rows, last.alt)
        && (next.display_offset, next.history, next.scroll_base) == (last.display_offset, last.history, last.scroll_base)
}

/// Lock, recovering from poisoning: a panic elsewhere shouldn't freeze the terminal.
/// Start a named thread for the session.
fn start_thread(name: &str, body: impl FnOnce() + Send + 'static) -> Result<()> {
    thread::Builder::new()
        .name(name.into())
        .spawn(body)
        .map(drop)
        .map_err(|e| AppError::new(Code::SessionStart, format!("could not start the {name} thread")).because(e))
}

fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
