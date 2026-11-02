//! The session use case on in-memory adapters: no PTY, no real emulator.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_app::ports::{
    ChildProcess, Emulator, EmulatorFactory, TimePort, EmulatorOptions, ProcessKiller, PtyControl, PtyProcess, PtySystem, Scroll,
    SpawnRequest, TermHost,
};
use tersh_app::instrumented::Instrumented;
use tersh_app::{Ports, Session, SessionOptions, Sink};
use tersh_domain::frame::{Cursor, Shape};
use tersh_domain::rich::{AnchorContext, Screen};
use tersh_domain::{Config, Frame, TermEvent, WindowSize};

/// Program output the test feeds in, chunk by chunk.
struct ChannelReader {
    chunks: Receiver<Vec<u8>>,
    pending: Vec<u8>,
}

impl Read for ChannelReader {
    fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
        if self.pending.is_empty() {
            match self.chunks.recv() {
                Ok(chunk) => self.pending = chunk,
                Err(_) => return Ok(0),
            }
        }
        let n = buf.len().min(self.pending.len());
        buf[..n].copy_from_slice(&self.pending[..n]);
        self.pending.drain(..n);
        Ok(n)
    }
}

#[derive(Clone, Default)]
struct Shared(Arc<Mutex<Vec<u8>>>);

impl Write for Shared {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(buf);
        Ok(buf.len())
    }
    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

/// Hangs up by ending the fake program's output (an empty chunk reads as end-of-file).
struct NoControl(Option<Sender<Vec<u8>>>);
impl PtyControl for NoControl {
    fn resize(&self, _: WindowSize) -> tersh_domain::error::Result<()> {
        Ok(())
    }
    fn foreground_pid(&self) -> Option<u32> {
        Some(77)
    }
    fn hangup(&self) {
        if let Some(output) = &self.0 {
            let _ = output.send(Vec::new());
        }
    }
}

struct Names;
impl tersh_app::ports::ProcessInspector for Names {
    fn name(&self, pid: u32) -> Option<String> {
        (pid == 77).then(|| "nvim".into())
    }
    fn cwd(&self, pid: u32) -> Option<std::path::PathBuf> {
        (pid == 7).then(|| "/work".into())
    }
}

struct FakeChild(Arc<Mutex<Option<Receiver<()>>>>, Sender<()>);
impl ChildProcess for FakeChild {
    fn wait(&mut self) -> Option<u32> {
        let rx = self.0.lock().unwrap().take()?;
        let _ = rx.recv();
        Some(0)
    }
    fn killer(&self) -> Box<dyn ProcessKiller> {
        Box::new(FakeKiller(self.1.clone()))
    }
    fn pid(&self) -> Option<u32> {
        Some(7)
    }
}
struct FakeKiller(Sender<()>);
impl ProcessKiller for FakeKiller {
    fn kill(&mut self) {
        let _ = self.0.send(());
    }
}

#[derive(Default)]
struct FakePty {
    output: Mutex<Option<Receiver<Vec<u8>>>>,
    input: Shared,
    requests: Mutex<Vec<SpawnRequest>>,
    hangup: Mutex<Option<Sender<Vec<u8>>>>,
}

impl PtySystem for FakePty {
    fn spawn(&self, request: &SpawnRequest) -> tersh_domain::error::Result<PtyProcess> {
        self.requests.lock().unwrap().push(request.clone());
        let (exit_tx, exit_rx) = mpsc::channel();
        Ok(PtyProcess {
            output: Box::new(ChannelReader { chunks: self.output.lock().unwrap().take().unwrap(), pending: Vec::new() }),
            input: Box::new(self.input.clone()),
            control: Box::new(NoControl(self.hangup.lock().unwrap().clone())),
            child: Box::new(FakeChild(Arc::new(Mutex::new(Some(exit_rx))), exit_tx)),
            program: "fake".into(),
        })
    }
}

/// Records what it is fed and how it is scrolled; the cursor is always at
/// row 3, column 7 of the alternate screen.
#[derive(Default)]
struct FakeEmulators {
    fed: Shared,
    scrolls: Arc<Mutex<Vec<Scroll>>>,
    /// When set, the next output is parsed only once the test sends on it.
    gate: Arc<Mutex<Option<Receiver<()>>>>,
    host: Mutex<Option<Arc<dyn TermHost>>>,
}

struct FakeEmulator {
    fed: Shared,
    scrolls: Arc<Mutex<Vec<Scroll>>>,
    gate: Arc<Mutex<Option<Receiver<()>>>>,
}

impl Emulator for FakeEmulator {
    fn advance(&mut self, bytes: &[u8]) {
        self.fed.write_all(bytes).unwrap();
        let gate = self.gate.lock().unwrap().take();
        if let Some(gate) = gate {
            let _ = gate.recv();
        }
    }
    fn sync_deadline(&self) -> Option<Instant> {
        None
    }
    fn flush_sync(&mut self) {}
    fn resize(&mut self, _: u16, _: u16) {}
    fn scroll(&mut self, scroll: Scroll) {
        self.scrolls.lock().unwrap().push(scroll);
    }
    fn anchor_context(&self) -> AnchorContext {
        AnchorContext { screen: Screen::Alt, cursor_row: 3, cursor_col: 7, scroll_base: 0 }
    }
    fn alt_screen(&self) -> bool {
        true
    }
    fn frame(&mut self, full: bool) -> Frame {
        Frame {
            cols: 80,
            rows: 24,
            full,
            lines: Vec::new(),
            cursor: Cursor { x: 7, y: 3, shape: Shape::Block, blink: false },
            mode: 0,
            alt: true,
            display_offset: 0,
            history: 0,
            scroll_base: 0,
        }
    }
    fn set_options(&mut self, _: &EmulatorOptions) {}
}

impl EmulatorFactory for FakeEmulators {
    fn create(&self, _: &EmulatorOptions, host: Arc<dyn TermHost>) -> Box<dyn Emulator> {
        *self.host.lock().unwrap() = Some(host);
        Box::new(FakeEmulator { fed: self.fed.clone(), scrolls: self.scrolls.clone(), gate: self.gate.clone() })
    }
}

#[derive(Default)]
struct Recorder {
    frames: Mutex<Vec<Frame>>,
    rich: Mutex<Vec<Value>>,
    events: Mutex<Vec<TermEvent>>,
    /// Acknowledge frames, as the desktop page does.
    acks: bool,
}

impl Sink for Recorder {
    fn frame(&self, frame: Frame) {
        self.frames.lock().unwrap().push(frame);
    }
    fn rich(&self, message: Value) {
        self.rich.lock().unwrap().push(message);
    }
    fn event(&self, event: TermEvent) {
        self.events.lock().unwrap().push(event);
    }
    fn acks_frames(&self) -> bool {
        self.acks
    }
}

fn eventually(what: &str, check: impl Fn() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < Duration::from_secs(5), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(5));
    }
}

/// A session plus the handles a test drives it through.
type Rig = (Session, Sender<Vec<u8>>, Arc<FakePty>, Arc<FakeEmulators>, Arc<Recorder>);

/// A clock that only moves when told to, or by `step` on every reading.
struct FakeClock {
    base: Instant,
    offset: Mutex<Duration>,
    step: Duration,
}

impl FakeClock {
    fn new(step: Duration) -> Arc<Self> {
        Arc::new(Self { base: Instant::now(), offset: Mutex::new(Duration::ZERO), step })
    }
    fn advance(&self, by: Duration) {
        *self.offset.lock().unwrap() += by;
    }
}

impl TimePort for FakeClock {
    fn now(&self) -> Instant {
        let mut offset = self.offset.lock().unwrap();
        *offset += self.step;
        self.base + *offset
    }
    fn sleep(&self, duration: Duration) {
        self.advance(duration);
    }
}

fn start() -> Rig {
    start_with(Arc::new(SystemClock), None)
}

/// A session whose UI acknowledges frames.
fn start_acking() -> Rig {
    build(Arc::new(SystemClock), None, Recorder { acks: true, ..Default::default() })
}

struct SystemClock;
impl TimePort for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }
    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}

/// A session on `clock`, its emulators measured on `instrument` when given.
fn start_with(clock: Arc<dyn TimePort>, instrument: Option<Arc<dyn TimePort>>) -> Rig {
    build(clock, instrument, Recorder::default())
}

fn build(clock: Arc<dyn TimePort>, instrument: Option<Arc<dyn TimePort>>, recorder: Recorder) -> Rig {
    let (tx, rx) = mpsc::channel();
    let pty = Arc::new(FakePty {
        output: Mutex::new(Some(rx)),
        hangup: Mutex::new(Some(tx.clone())),
        ..Default::default()
    });
    let emulators = Arc::new(FakeEmulators::default());
    let recorder = Arc::new(recorder);
    let mut options = SessionOptions::from_config(&Config::default(), 80, 24);
    options.rich_socket = Some("/run/tersh.sock".into());
    let session = match instrument {
        Some(measure) => {
            let emulator = Instrumented::new(&*emulators, measure);
            Session::spawn(options, Ports { pty: &*pty, emulator: &emulator, clock }, recorder.clone())
        },
        None => Session::spawn(options, Ports { pty: &*pty, emulator: &*emulators, clock }, recorder.clone()),
    }
    .unwrap();
    (session, tx, pty, emulators, recorder)
}

#[test]
fn osc_messages_are_split_out_and_anchored_at_the_cursor() {
    let (_session, output, _pty, emulators, recorder) = start();
    output.send(b"ab\x1b]7701;{\"kind\":\"text\",\"data\":\"hi\"}\x07cd".to_vec()).unwrap();

    eventually("rich message", || !recorder.rich.lock().unwrap().is_empty());
    eventually("output", || emulators.fed.0.lock().unwrap().as_slice() == b"abcd");
    let msg = recorder.rich.lock().unwrap()[0].clone();
    assert_eq!(msg["anchor"]["screen"], "alt");
    assert_eq!((msg["anchor"]["line"].as_u64(), msg["anchor"]["col"].as_u64()), (Some(3), Some(7)));
}

#[test]
fn frames_that_change_nothing_are_not_sent() {
    let (session, output, _pty, emulators, recorder) = start();
    output.send(b"a".to_vec()).unwrap();
    eventually("first frame", || !recorder.frames.lock().unwrap().is_empty());

    // The fake emulator's frames never change, so this output sends nothing new.
    output.send(b"b".to_vec()).unwrap();
    eventually("output", || emulators.fed.0.lock().unwrap().as_slice() == b"ab");
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(recorder.frames.lock().unwrap().len(), 1);

    // A refresh always repaints.
    session.refresh();
    eventually("full frame", || recorder.frames.lock().unwrap().len() == 2);
    assert!(recorder.frames.lock().unwrap()[1].full);
}

#[test]
fn an_acking_ui_gets_one_frame_in_flight() {
    let (session, output, _pty, _emulators, recorder) = start_acking();
    let frames = || recorder.frames.lock().unwrap().len();
    output.send(b"a".to_vec()).unwrap();
    eventually("first frame", || frames() == 1);

    // Changes made before the UI has drawn the frame are held back...
    session.resize(100, 30, 8, 16).unwrap();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(frames(), 1);
    // ...and sent as soon as it has.
    session.frame_shown();
    eventually("held frame", || frames() == 2);
    assert!(recorder.frames.lock().unwrap()[1].full);

    // A drawn frame with nothing new to show sends nothing.
    session.frame_shown();
    std::thread::sleep(Duration::from_millis(50));
    assert_eq!(frames(), 2);
}

#[test]
fn unacknowledged_frames_are_followed_after_a_timeout() {
    let (session, output, _pty, _emulators, recorder) = start_acking();
    let frames = || recorder.frames.lock().unwrap().len();
    output.send(b"a".to_vec()).unwrap();
    eventually("first frame", || frames() == 1);
    let held = Instant::now();
    session.resize(100, 30, 8, 16).unwrap();
    // The UI never answers (e.g. a minimized window doesn't paint).
    eventually("frame after the timeout", || frames() == 2);
    assert!(held.elapsed() >= Duration::from_millis(200), "sent after {:?}", held.elapsed());
}

#[test]
fn a_refresh_does_not_wait_for_the_old_page() {
    let (session, output, _pty, _emulators, recorder) = start_acking();
    let frames = || recorder.frames.lock().unwrap().len();
    output.send(b"a".to_vec()).unwrap();
    eventually("first frame", || frames() == 1);
    // The page reloaded: the frame in flight will never be acknowledged.
    let start = Instant::now();
    session.refresh();
    eventually("full frame", || frames() == 2);
    assert!(start.elapsed() < Duration::from_millis(200), "waited {:?}", start.elapsed());
}

#[test]
fn spawn_request_sets_up_the_environment() {
    let (_session, _output, pty, _emulators, _recorder) = start();
    let request = pty.requests.lock().unwrap()[0].clone();
    assert!(request.env_remove.contains(&"TMUX".to_string()));
    let env: std::collections::HashMap<_, _> = request.env.into_iter().collect();
    assert_eq!(env["TERM_PROGRAM"], "tersh");
    assert_eq!(env["TERSH_SOCKET"], "/run/tersh.sock");
}

#[test]
fn shells_do_not_inherit_the_appimage_bundle() {
    let (tx, rx) = mpsc::channel::<Vec<u8>>();
    let pty = FakePty { output: Mutex::new(Some(rx)), ..Default::default() };
    let emulators = FakeEmulators::default();
    let mut options = SessionOptions::from_config(&Config::default(), 80, 24);
    let mut config = Config::default();
    config.env.insert("LD_LIBRARY_PATH".into(), "/opt/mine".into());
    options.env = config.env.into_iter().collect();
    options.inherited_env = [
        ("HOME", "/home/u"),
        ("APPDIR", "/home/u/.local/share/tersh/app"),
        ("PATH", "/home/u/.local/share/tersh/app/usr/bin:/usr/bin"),
        ("LD_LIBRARY_PATH", "/home/u/.local/share/tersh/app/usr/lib"),
        ("PYTHONHOME", "/home/u/.local/share/tersh/app/usr/"),
    ]
    .map(|(k, v)| (k.to_string(), v.to_string()))
    .to_vec();
    let _session = Session::spawn(options, Ports { pty: &pty, emulator: &emulators, clock: Arc::new(SystemClock) }, Arc::new(Recorder::default())).unwrap();
    drop(tx);

    let request = pty.requests.lock().unwrap()[0].clone();
    for key in ["APPDIR", "PYTHONHOME", "TMUX"] {
        assert!(request.env_remove.contains(&key.to_string()), "{key} not removed");
    }
    // Later entries win when the PTY applies them: the user's `[env]` beats the cleanup.
    let env: std::collections::HashMap<_, _> = request.env.into_iter().collect();
    assert_eq!(env["PATH"], "/usr/bin");
    assert_eq!(env["LD_LIBRARY_PATH"], "/opt/mine");
}

#[test]
fn inbox_answers_info_and_rejects_bad_messages() {
    let (session, _output, _pty, _emulators, recorder) = start();
    session.resize(100, 30, 9, 18).unwrap();
    let inbox = session.rich_inbox();
    let info = inbox.handle(b"{\"op\":\"info\"}").unwrap();
    assert_eq!((info["cols"].as_u64(), info["cursor_col"].as_u64(), info["alt_screen"].as_bool()), (Some(100), Some(7), Some(true)));
    assert_eq!(inbox.handle(b"nope").unwrap()["ok"], false);
    assert!(recorder.events.lock().unwrap().iter().any(|e| matches!(e, TermEvent::RichError { .. })));

    drop(session);
    assert!(inbox.handle(b"{}").is_none(), "inbox closes with the session");
}

#[test]
fn emulator_replies_reach_the_program() {
    let (session, _output, pty, emulators, _recorder) = start();
    let host = emulators.host.lock().unwrap().clone().unwrap();
    host.reply("\x1b[?62c");
    session.write(b"ls\r").unwrap();
    eventually("input", || pty.input.0.lock().unwrap().as_slice() == b"\x1b[?62cls\r");
    assert_eq!(host.color(257), tersh_domain::Palette::from_theme(&Config::default().theme).background);
}

#[test]
fn reports_the_foreground_program_and_shell_directory() {
    let (session, _output, _pty, _emulators, _recorder) = start();
    let fg = session.foreground(&Names);
    assert_eq!((fg.shell_pid, fg.pid, fg.name.as_deref()), (Some(7), Some(77), Some("nvim")));
    assert_eq!(session.cwd(&Names), Some("/work".into()));
    assert_eq!(
        tersh_domain::update::restart_decision(&fg),
        tersh_domain::update::Restart::Wait { busy: "nvim".into() }
    );
}

#[test]
fn input_is_written_in_order() {
    let (session, _output, pty, _emulators, _recorder) = start();
    let mut expected = Vec::new();
    for i in 0..2000 {
        let chunk = format!("{i},");
        session.write(chunk.as_bytes()).unwrap();
        expected.extend_from_slice(chunk.as_bytes());
    }
    eventually("all input", || pty.input.0.lock().unwrap().len() == expected.len());
    assert_eq!(*pty.input.0.lock().unwrap(), expected);
}

#[test]
fn input_and_scrolling_never_wait_for_output_parsing() {
    let (session, output, pty, emulators, _recorder) = start();
    let (release, gate) = mpsc::channel();
    *emulators.gate.lock().unwrap() = Some(gate);
    output.send(b"slow".to_vec()).unwrap();
    eventually("parsing to start", || emulators.fed.0.lock().unwrap().as_slice() == b"slow");

    // The reader is stuck parsing, holding the screen. On another thread, so
    // a session that waits for it fails the test rather than hanging it.
    let ui = std::thread::spawn(move || {
        session.scroll(3);
        session.write(b"x").unwrap();
        session
    });
    eventually("input", || pty.input.0.lock().unwrap().as_slice() == b"x");
    assert!(emulators.scrolls.lock().unwrap().is_empty(), "scrolls wait for the parse");

    release.send(()).unwrap();
    // Typing brings the scrolled viewport back to the bottom.
    eventually("scrolls", || *emulators.scrolls.lock().unwrap() == [Scroll::Lines(3), Scroll::Bottom]);
    let session = ui.join().unwrap();
    session.write(b"y").unwrap();
    eventually("more input", || pty.input.0.lock().unwrap().as_slice() == b"xy");
    // Not scrolled since: nothing more to undo.
    std::thread::sleep(Duration::from_millis(20));
    assert_eq!(emulators.scrolls.lock().unwrap().len(), 2);
}

#[test]
fn stats_count_output_frames_and_input() {
    let (session, output, _pty, _emulators, _recorder) = start();
    output.send(b"hello".to_vec()).unwrap();
    session.write(b"ls\r").unwrap();
    eventually("stats", || {
        let stats = session.stats();
        stats.output_bytes == 5 && stats.frames_sent >= 1 && stats.input_bytes == 3
    });
    let stats = session.stats();
    assert_eq!((stats.output_reads, stats.input_pending), (1, 0));
    assert!(stats.frames_built >= stats.frames_sent);

    let reply = session.rich_inbox().handle(br#"{"op":"stats"}"#).unwrap();
    assert_eq!(reply["ok"], true);
    assert_eq!(reply["output_bytes"], 5);
}

/// Memory leak check: once dropped, a session's threads finish and its
/// screen, scrollback and buffers are freed.
#[test]
fn dropped_sessions_are_released() {
    for _ in 0..20 {
        let (session, output, _pty, _emulators, _recorder) = start();
        output.send(b"some output".to_vec()).unwrap();
        session.write(b"input").unwrap();
        let inbox = session.rich_inbox();
        drop(session);
        // The program's output ends once it has been killed.
        drop(output);
        eventually("session released", || inbox.is_released());
        assert!(inbox.handle(br#"{"op":"info"}"#).is_none());
    }
}

#[test]
fn sessions_are_released_while_a_background_program_holds_the_terminal() {
    // `output` stays open, like a program started with `&` that outlives the shell.
    let (session, output, _pty, _emulators, _recorder) = start();
    output.send(b"some output".to_vec()).unwrap();
    let inbox = session.rich_inbox();
    drop(session);
    eventually("session released", || inbox.is_released());
    // The reader thread was woken and has finished (it dropped its end).
    eventually("reader stopped", || output.send(Vec::new()).is_err());
}

#[test]
fn the_instrumented_emulator_times_parsing_and_frames() {
    // Every reading of this clock is 1ms after the last, so each measured call takes 1ms.
    let measure = FakeClock::new(Duration::from_millis(1));
    let (session, output, _pty, _emulators, _recorder) = start_with(Arc::new(SystemClock), Some(measure));
    output.send(b"hello".to_vec()).unwrap();
    eventually("a frame", || session.stats().frames_sent >= 1);
    let stats = session.stats();
    assert_eq!(stats.parse_us, 1_000, "one advance call");
    assert_eq!(stats.build_us, 1_000 * stats.frames_built);
    assert_eq!(stats.build_max_us, 1_000);
}

#[test]
fn uptime_comes_from_the_clock() {
    let clock = FakeClock::new(Duration::ZERO);
    let (session, _output, _pty, _emulators, _recorder) = start_with(clock.clone(), None);
    let before = session.stats().uptime_ms;
    clock.advance(Duration::from_secs(90));
    assert!(session.stats().uptime_ms >= before + 90_000);
}
