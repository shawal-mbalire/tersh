//! A shell running in a PTY, its terminal state, and the threads that drive them.

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use alacritty_terminal::event::{Event, EventListener, WindowSize};
use alacritty_terminal::grid::Scroll;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::vte::ansi::{Processor, Rgb};
use anyhow::{Context, Result};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize};
use serde::Serialize;
use serde_json::Value;

use crate::config::{parse_hex, Config, Cursor, CursorShape};
use crate::frame::{self, Frame};
use crate::rich::{self, AnchorContext, Chunk, OscScanner, Screen};
use crate::scroll::ScrollTracker;

/// Minimum time between frames. The first frame after a quiet period is sent
/// immediately, so keystroke echo is not delayed.
const FRAME_INTERVAL: Duration = Duration::from_millis(8);

/// Receives everything the session produces. Implemented by the UI layer.
pub trait Sink: Send + Sync + 'static {
    fn frame(&self, frame: Frame);
    /// A rich-media message with its anchor resolved (see [`rich::resolve_anchor`]).
    fn rich(&self, message: Value);
    fn event(&self, event: TermEvent);
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TermEvent {
    Title { title: String },
    ResetTitle,
    Bell,
    /// OSC 52 copy request.
    Clipboard { text: String },
    RichError { error: String },
    Exit { code: Option<u32> },
}

#[derive(Debug, Clone)]
pub struct SessionOptions {
    pub shell: String,
    pub args: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
    /// Accept rich messages embedded in PTY output.
    pub osc_rich: bool,
    /// Accept rich messages on a local socket (exported as `$TERSH_SOCKET`).
    pub socket_rich: bool,
    pub palette: Palette,
    pub cursor: Cursor,
}

impl SessionOptions {
    pub fn from_config(config: &Config, cols: u16, rows: u16) -> Self {
        Self {
            shell: config.shell(),
            args: config.args.clone(),
            cwd: dirs::home_dir(),
            env: config.env.iter().map(|(k, v)| (k.clone(), v.clone())).collect(),
            cols,
            rows,
            scrollback: config.scrollback,
            osc_rich: config.rich.enabled && config.rich.osc,
            socket_rich: config.rich.enabled,
            palette: Palette::from_config(config),
            cursor: config.cursor.clone(),
        }
    }
}

/// Colours reported to programs that query them (OSC 4/10/11/12), e.g. Neovim
/// picking `background=dark`.
#[derive(Debug, Clone)]
pub struct Palette {
    pub foreground: Rgb,
    pub background: Rgb,
    pub cursor: Rgb,
    pub ansi: [Rgb; 16],
}

impl Palette {
    pub fn from_config(config: &Config) -> Self {
        let rgb = |s: &str, fallback: Rgb| parse_hex(s).map(|(r, g, b)| Rgb { r, g, b }).unwrap_or(fallback);
        let defaults = Config::default().theme;
        let mut ansi = [Rgb { r: 0, g: 0, b: 0 }; 16];
        for (i, slot) in ansi.iter_mut().enumerate() {
            let fallback = rgb(&defaults.palette[i], Rgb { r: 0, g: 0, b: 0 });
            *slot = config.theme.palette.get(i).map_or(fallback, |c| rgb(c, fallback));
        }
        let white = Rgb { r: 0xff, g: 0xff, b: 0xff };
        let black = Rgb { r: 0, g: 0, b: 0 };
        Self {
            foreground: rgb(&config.theme.foreground, white),
            background: rgb(&config.theme.background, black),
            cursor: rgb(&config.theme.cursor, white),
            ansi,
        }
    }

    /// Colour at an alacritty palette index.
    pub fn get(&self, index: usize) -> Rgb {
        match index {
            0..=15 => self.ansi[index],
            16..=231 => {
                let i = index - 16;
                let level = |v: usize| if v == 0 { 0 } else { (55 + v * 40) as u8 };
                Rgb { r: level(i / 36), g: level(i / 6 % 6), b: level(i % 6) }
            },
            232..=255 => {
                let v = (8 + (index - 232) * 10) as u8;
                Rgb { r: v, g: v, b: v }
            },
            257 => self.background,
            258 => self.cursor,
            _ => self.foreground,
        }
    }
}

type Writer = Arc<Mutex<Box<dyn Write + Send>>>;

/// Handles requests the emulator can't answer itself.
#[derive(Clone)]
struct Listener {
    writer: Writer,
    sink: Arc<dyn Sink>,
    palette: Arc<Mutex<Palette>>,
    size: Arc<Mutex<WindowSize>>,
}

impl Listener {
    fn reply(&self, text: &str) {
        let mut writer = lock(&self.writer);
        let _ = writer.write_all(text.as_bytes()).and_then(|_| writer.flush());
    }
}

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        match event {
            Event::PtyWrite(text) => self.reply(&text),
            Event::Title(title) => self.sink.event(TermEvent::Title { title }),
            Event::ResetTitle => self.sink.event(TermEvent::ResetTitle),
            Event::Bell => self.sink.event(TermEvent::Bell),
            Event::ClipboardStore(_, text) => self.sink.event(TermEvent::Clipboard { text }),
            Event::ColorRequest(index, format) => {
                let color = lock(&self.palette).get(index);
                self.reply(&format(color));
            },
            Event::TextAreaSizeRequest(format) => {
                let size = *lock(&self.size);
                self.reply(&format(size));
            },
            _ => {},
        }
    }
}

struct State {
    term: Term<Listener>,
    parser: Processor,
    scanner: OscScanner,
    scroll: ScrollTracker,
    force_full: bool,
}

pub(crate) struct Shared {
    state: Mutex<State>,
    writer: Writer,
    master: Mutex<Box<dyn MasterPty + Send>>,
    killer: Mutex<Box<dyn ChildKiller + Send + Sync>>,
    palette: Arc<Mutex<Palette>>,
    size: Arc<Mutex<WindowSize>>,
    sink: Arc<dyn Sink>,
    wake: mpsc::Sender<()>,
    closed: AtomicBool,
    options: SessionOptions,
    socket_path: Option<PathBuf>,
}

/// A running shell. Dropping it kills the shell.
pub struct Session {
    shared: Arc<Shared>,
}

/// Grid and cell geometry, as reported to rich-protocol clients.
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub version: &'static str,
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub alt_screen: bool,
}

impl Session {
    pub fn spawn(options: SessionOptions, sink: Arc<dyn Sink>) -> Result<Self> {
        let pty = portable_pty::native_pty_system()
            .openpty(PtySize { rows: options.rows, cols: options.cols, pixel_width: 0, pixel_height: 0 })
            .context("opening pty")?;

        #[cfg(unix)]
        let socket = if options.socket_rich { crate::socket::bind().map_err(|e| log::warn!("rich socket: {e}")).ok() } else { None };
        #[cfg(unix)]
        let socket_path = socket.as_ref().map(|(_, path)| path.clone());
        #[cfg(not(unix))]
        let socket_path: Option<PathBuf> = None;

        let mut cmd = CommandBuilder::new(&options.shell);
        cmd.args(&options.args);
        if let Some(cwd) = &options.cwd {
            cmd.cwd(cwd);
        }
        cmd.env("TERM", "xterm-256color");
        cmd.env("COLORTERM", "truecolor");
        cmd.env("TERM_PROGRAM", "tersh");
        cmd.env("TERM_PROGRAM_VERSION", env!("CARGO_PKG_VERSION"));
        // A new terminal is not inside whatever tmux (or tersh) launched the app.
        for inherited in ["TMUX", "TMUX_PANE", "TERSH_SOCKET"] {
            cmd.env_remove(inherited);
        }
        if let Some(path) = &socket_path {
            cmd.env("TERSH_SOCKET", path);
        }
        for (key, value) in &options.env {
            cmd.env(key, value);
        }

        let mut child = pty.slave.spawn_command(cmd).with_context(|| format!("starting {}", options.shell))?;
        drop(pty.slave);
        let killer = child.clone_killer();
        let mut reader = pty.master.try_clone_reader()?;
        let writer: Writer = Arc::new(Mutex::new(pty.master.take_writer()?));

        let size = Arc::new(Mutex::new(WindowSize {
            num_lines: options.rows,
            num_cols: options.cols,
            cell_width: 0,
            cell_height: 0,
        }));
        let palette = Arc::new(Mutex::new(options.palette.clone()));
        let listener = Listener { writer: writer.clone(), sink: sink.clone(), palette: palette.clone(), size: size.clone() };
        let term = Term::new(term_config(&options), &TermSize::new(options.cols as usize, options.rows as usize), listener);

        let (wake, wake_rx) = mpsc::channel();
        let shared = Arc::new(Shared {
            state: Mutex::new(State {
                term,
                parser: Processor::new(),
                scanner: OscScanner::new(),
                scroll: ScrollTracker::default(),
                force_full: true,
            }),
            writer,
            master: Mutex::new(pty.master),
            killer: Mutex::new(killer),
            palette,
            size,
            sink,
            wake,
            closed: AtomicBool::new(false),
            options,
            socket_path,
        });

        let s = shared.clone();
        thread::Builder::new().name("tersh-pty-read".into()).spawn(move || {
            let mut buf = vec![0u8; 64 * 1024];
            loop {
                match reader.read(&mut buf) {
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
            let code = child.wait().ok().map(|status| status.exit_code());
            s.closed.store(true, Ordering::SeqCst);
            let _ = s.wake.send(());
            s.sink.event(TermEvent::Exit { code });
        })?;

        #[cfg(unix)]
        if let Some((listener, _)) = socket {
            let s = shared.clone();
            thread::Builder::new()
                .name("tersh-socket".into())
                .spawn(move || crate::socket::serve(listener, Arc::downgrade(&s)))?;
        }

        Ok(Self { shared })
    }

    /// Send input (keystrokes, pastes, mouse reports) to the shell.
    pub fn write(&self, bytes: &[u8]) -> Result<()> {
        {
            let mut state = lock(&self.shared.state);
            if state.term.grid().display_offset() != 0 {
                state.term.scroll_display(Scroll::Bottom);
                drop(state);
                let _ = self.shared.wake.send(());
            }
        }
        let mut writer = lock(&self.shared.writer);
        writer.write_all(bytes)?;
        writer.flush()?;
        Ok(())
    }

    pub fn resize(&self, cols: u16, rows: u16, cell_width: u16, cell_height: u16) -> Result<()> {
        let cols = cols.max(2);
        let rows = rows.max(1);
        *lock(&self.shared.size) = WindowSize { num_lines: rows, num_cols: cols, cell_width, cell_height };
        {
            let mut state = lock(&self.shared.state);
            let state = &mut *state;
            state.term.resize(TermSize::new(cols as usize, rows as usize));
            state.scroll.invalidate(&state.term);
            state.force_full = true;
        }
        lock(&self.shared.master).resize(PtySize {
            rows,
            cols,
            pixel_width: cols * cell_width,
            pixel_height: rows * cell_height,
        })?;
        let _ = self.shared.wake.send(());
        Ok(())
    }

    /// Scroll the viewport through history by `lines` (positive is up).
    pub fn scroll(&self, lines: i32) {
        lock(&self.shared.state).term.scroll_display(Scroll::Delta(lines));
        let _ = self.shared.wake.send(());
    }

    pub fn scroll_to_bottom(&self) {
        lock(&self.shared.state).term.scroll_display(Scroll::Bottom);
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

    /// Apply the parts of a reloaded config that a running shell can pick up:
    /// the colours reported to programs and the default cursor. (Shell,
    /// environment and scrollback only affect new sessions.)
    pub fn reconfigure(&self, config: &Config) {
        *lock(&self.shared.palette) = Palette::from_config(config);
        let options = SessionOptions { cursor: config.cursor.clone(), ..self.shared.options.clone() };
        let mut state = lock(&self.shared.state);
        state.term.set_options(term_config(&options));
        state.force_full = true;
        drop(state);
        let _ = self.shared.wake.send(());
    }

    pub fn socket_path(&self) -> Option<&PathBuf> {
        self.shared.socket_path.as_ref()
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        self.shared.closed.store(true, Ordering::SeqCst);
        let _ = lock(&self.shared.killer).kill();
        let _ = self.shared.wake.send(());
        #[cfg(unix)]
        if let Some(path) = &self.shared.socket_path {
            crate::socket::shutdown(path);
        }
    }
}

impl Shared {
    fn process(&self, bytes: &[u8]) {
        let mut messages = Vec::new();
        {
            let mut state = lock(&self.state);
            let State { term, parser, scanner, scroll, .. } = &mut *state;
            let max_history = self.options.scrollback;
            let osc_rich = self.options.osc_rich;
            scanner.feed(bytes, |chunk| match chunk {
                Chunk::Bytes(bytes) => parser.advance(term, bytes),
                Chunk::Payload(payload) if osc_rich => {
                    // Apply output buffered by a synchronized update first, so
                    // the cursor is where the program thinks it is.
                    if parser.sync_timeout().sync_timeout().is_some() {
                        parser.stop_sync(term);
                    }
                    scroll.update(term, max_history);
                    messages.push((payload, anchor_context(term, scroll.base())));
                },
                Chunk::Payload(_) => {},
            });
            scroll.update(term, max_history);
        }
        for (payload, ctx) in messages {
            let _ = self.dispatch(rich::decode_payload(&payload), ctx);
        }
    }

    /// Handle a message from the rich socket.
    pub(crate) fn is_closed(&self) -> bool {
        self.closed.load(Ordering::SeqCst)
    }

    /// Handle a message from the rich socket. Every message gets a one-line
    /// reply: `{"ok":true}`, `{"ok":false,"error":...}`, or for `info` the
    /// grid description. Clients can wait for it to order their own output.
    pub(crate) fn handle_socket_message(&self, line: &[u8]) -> Value {
        let message = rich::decode_payload(line);
        if let Ok(msg) = &message {
            if msg.get("op").and_then(Value::as_str) == Some("info") {
                let mut info = serde_json::to_value(self.info()).unwrap_or_default();
                info["ok"] = Value::Bool(true);
                return info;
            }
        }
        let ctx = {
            let mut state = lock(&self.state);
            let State { term, scroll, .. } = &mut *state;
            scroll.update(term, self.options.scrollback);
            anchor_context(term, scroll.base())
        };
        match self.dispatch(message, ctx) {
            Ok(()) => serde_json::json!({ "ok": true }),
            Err(error) => serde_json::json!({ "ok": false, "error": error }),
        }
    }

    fn dispatch(&self, message: Result<Value, String>, ctx: AnchorContext) -> Result<(), String> {
        match message {
            Ok(mut msg) => {
                rich::resolve_anchor(&mut msg, &ctx);
                self.sink.rich(msg);
                Ok(())
            },
            Err(error) => {
                self.sink.event(TermEvent::RichError { error: error.clone() });
                Err(error)
            },
        }
    }

    fn info(&self) -> Info {
        let size = *lock(&self.size);
        let state = lock(&self.state);
        let cursor = state.term.grid().cursor.point;
        Info {
            version: env!("CARGO_PKG_VERSION"),
            cols: size.num_cols,
            rows: size.num_lines,
            cell_width: size.cell_width,
            cell_height: size.cell_height,
            cursor_row: cursor.line.0.max(0) as usize,
            cursor_col: cursor.column.0,
            alt_screen: state.term.mode().contains(TermMode::ALT_SCREEN),
        }
    }

    fn render_loop(&self, wake: mpsc::Receiver<()>) {
        let mut last_frame = Instant::now() - FRAME_INTERVAL;
        loop {
            // Wake for new output, or when a synchronized update times out.
            let deadline = lock(&self.state).parser.sync_timeout().sync_timeout();
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
                let State { term, parser, scroll, force_full, .. } = &mut *state;
                if let Some(deadline) = parser.sync_timeout().sync_timeout() {
                    if Instant::now() < deadline {
                        // Mid synchronized update: wait for it to finish.
                        continue;
                    }
                    parser.stop_sync(term);
                }
                let frame = frame::build(term, scroll.base(), *force_full);
                *force_full = false;
                frame
            };
            last_frame = Instant::now();
            self.sink.frame(frame);

            if self.closed.load(Ordering::SeqCst) {
                return;
            }
        }
    }
}

fn term_config(options: &SessionOptions) -> term::Config {
    use alacritty_terminal::vte::ansi::{CursorShape as Shape, CursorStyle};
    let shape = match options.cursor.shape {
        CursorShape::Block => Shape::Block,
        CursorShape::Beam => Shape::Beam,
        CursorShape::Underline => Shape::Underline,
    };
    term::Config {
        scrolling_history: options.scrollback,
        default_cursor_style: CursorStyle { shape, blinking: options.cursor.blink },
        ..Default::default()
    }
}

fn anchor_context<T>(term: &Term<T>, scroll_base: u64) -> AnchorContext {
    let point = term.grid().cursor.point;
    AnchorContext {
        screen: if term.mode().contains(TermMode::ALT_SCREEN) { Screen::Alt } else { Screen::Primary },
        cursor_row: point.line.0.max(0) as usize,
        cursor_col: point.column.0,
        scroll_base,
    }
}

/// Lock, recovering from poisoning: a panic elsewhere shouldn't freeze the terminal.
fn lock<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    mutex.lock().unwrap_or_else(|e| e.into_inner())
}
