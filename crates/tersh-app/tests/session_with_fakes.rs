//! The session use case on in-memory adapters: no PTY, no real emulator.

use std::io::{self, Read, Write};
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_app::ports::{
    ChildProcess, Emulator, EmulatorFactory, EmulatorOptions, ProcessKiller, PtyControl, PtyProcess, PtySystem, Scroll,
    SpawnRequest, TermHost,
};
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

struct NoControl;
impl PtyControl for NoControl {
    fn resize(&self, _: WindowSize) -> anyhow::Result<()> {
        Ok(())
    }
    fn foreground_pid(&self) -> Option<u32> {
        Some(77)
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
}

impl PtySystem for FakePty {
    fn spawn(&self, request: &SpawnRequest) -> anyhow::Result<PtyProcess> {
        self.requests.lock().unwrap().push(request.clone());
        let (exit_tx, exit_rx) = mpsc::channel();
        Ok(PtyProcess {
            output: Box::new(ChannelReader { chunks: self.output.lock().unwrap().take().unwrap(), pending: Vec::new() }),
            input: Box::new(self.input.clone()),
            control: Box::new(NoControl),
            child: Box::new(FakeChild(Arc::new(Mutex::new(Some(exit_rx))), exit_tx)),
            program: "fake".into(),
        })
    }
}

/// Records what it is fed; the cursor is always at row 3, column 7 of the alternate screen.
#[derive(Default)]
struct FakeEmulators {
    fed: Shared,
    host: Mutex<Option<Arc<dyn TermHost>>>,
}

struct FakeEmulator(Shared);

impl Emulator for FakeEmulator {
    fn advance(&mut self, bytes: &[u8]) {
        self.0.write_all(bytes).unwrap();
    }
    fn sync_deadline(&self) -> Option<Instant> {
        None
    }
    fn flush_sync(&mut self) {}
    fn resize(&mut self, _: u16, _: u16) {}
    fn scroll(&mut self, _: Scroll) {}
    fn display_offset(&self) -> usize {
        0
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
        Box::new(FakeEmulator(self.fed.clone()))
    }
}

#[derive(Default)]
struct Recorder {
    rich: Mutex<Vec<Value>>,
    events: Mutex<Vec<TermEvent>>,
}

impl Sink for Recorder {
    fn frame(&self, _: Frame) {}
    fn rich(&self, message: Value) {
        self.rich.lock().unwrap().push(message);
    }
    fn event(&self, event: TermEvent) {
        self.events.lock().unwrap().push(event);
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

fn start() -> Rig {
    let (tx, rx) = mpsc::channel();
    let pty = Arc::new(FakePty { output: Mutex::new(Some(rx)), ..Default::default() });
    let emulators = Arc::new(FakeEmulators::default());
    let recorder = Arc::new(Recorder::default());
    let mut options = SessionOptions::from_config(&Config::default(), 80, 24);
    options.rich_socket = Some("/run/tersh.sock".into());
    let session = Session::spawn(options, Ports { pty: &*pty, emulator: &*emulators }, recorder.clone()).unwrap();
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
    let _session = Session::spawn(options, Ports { pty: &pty, emulator: &emulators }, Arc::new(Recorder::default())).unwrap();
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
    assert_eq!(pty.input.0.lock().unwrap().as_slice(), b"\x1b[?62cls\r");
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
