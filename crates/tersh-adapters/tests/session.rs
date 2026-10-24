#![cfg(unix)]

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::UnixStream;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_adapters::spawn_local;
use tersh_app::{Session, SessionOptions, Sink};
use tersh_domain::{Config, Frame, TermEvent};

#[derive(Default)]
struct Recorder {
    screen: Mutex<Vec<String>>,
    rich: Mutex<Vec<Value>>,
    events: Mutex<Vec<TermEvent>>,
}

impl Sink for Recorder {
    fn frame(&self, frame: Frame) {
        let mut screen = self.screen.lock().unwrap();
        screen.resize(frame.rows as usize, String::new());
        for row in frame.lines {
            let mut text = String::new();
            for run in row.runs {
                while text.chars().count() < run.c as usize {
                    text.push(' ');
                }
                text.push_str(&run.t);
            }
            screen[row.y as usize] = text;
        }
    }

    fn rich(&self, message: Value) {
        self.rich.lock().unwrap().push(message);
    }

    fn event(&self, event: TermEvent) {
        self.events.lock().unwrap().push(event);
    }
}

fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn spawn(recorder: &Arc<Recorder>) -> Session {
    let config = Config { shell: Some("/bin/sh".into()), ..Config::default() };
    let mut options = SessionOptions::from_config(&config, 40, 10);
    options.env.push(("PS1".into(), "$ ".into()));
    spawn_local(options, true, recorder.clone()).expect("spawn shell")
}

#[test]
fn runs_shell_and_extracts_rich_messages() {
    let recorder = Arc::new(Recorder::default());
    let session = spawn(&recorder);

    session.write(b"printf 'ab\\033]7701;{\"op\":\"show\",\"kind\":\"markdown\",\"data\":\"# hi\"}\\007cd\\n'\n").unwrap();

    wait_for("rich message", || !recorder.rich.lock().unwrap().is_empty());
    // The OSC is stripped from the grid: the two halves of the output join up.
    wait_for("output", || recorder.screen.lock().unwrap().iter().any(|l| l == "abcd"));

    let msg = recorder.rich.lock().unwrap()[0].clone();
    assert_eq!(msg["kind"], "markdown");
    assert_eq!(msg["anchor"]["screen"], "primary");
    assert_eq!(msg["anchor"]["col"], 2, "anchored where the cursor was: {msg}");

    session.write(b"exit\n").unwrap();
    wait_for("exit", || recorder.events.lock().unwrap().iter().any(|e| matches!(e, TermEvent::Exit { .. })));
}

#[test]
fn socket_accepts_messages_and_answers_info() {
    let recorder = Arc::new(Recorder::default());
    let session = spawn(&recorder);
    session.resize(50, 12, 9, 18).unwrap();
    let path = session.socket_path().expect("socket").clone();

    let mut stream = UnixStream::connect(&path).unwrap();
    stream.write_all(b"{\"op\":\"info\"}\n{\"op\":\"show\",\"id\":\"x\",\"kind\":\"image\",\"anchor\":{\"mode\":\"cell\",\"row\":3,\"col\":4}}\nnot json\n").unwrap();
    let replies: Vec<Value> = BufReader::new(&stream)
        .lines()
        .take(3)
        .map(|line| serde_json::from_str(&line.unwrap()).unwrap())
        .collect();
    let info = &replies[0];
    assert_eq!((info["cols"].as_u64(), info["rows"].as_u64(), info["cell_width"].as_u64()), (Some(50), Some(12), Some(9)));
    assert_eq!(replies[1]["ok"], true);
    assert_eq!(replies[2]["ok"], false);

    wait_for("socket message", || !recorder.rich.lock().unwrap().is_empty());
    let msg = recorder.rich.lock().unwrap()[0].clone();
    assert_eq!(msg["id"], "x");
    assert_eq!(msg["anchor"]["col"], 4);
    wait_for("error event", || recorder.events.lock().unwrap().iter().any(|e| matches!(e, TermEvent::RichError { .. })));

    drop(session);
    assert!(!path.exists(), "socket removed on drop");
}

#[test]
fn shell_sees_terminal_environment() {
    let recorder = Arc::new(Recorder::default());
    let session = spawn(&recorder);
    session.write(b"echo \"[$TERM_PROGRAM:${TERSH_SOCKET:+sock}]\"\n").unwrap();
    wait_for("env echo", || recorder.screen.lock().unwrap().iter().any(|l| l == "[tersh:sock]"));
}

#[test]
fn knows_when_the_shell_is_idle() {
    use tersh_adapters::SystemProcesses;
    use tersh_domain::update::{restart_decision, Restart};

    let recorder = Arc::new(Recorder::default());
    let session = spawn(&recorder);
    session.write(b"cd /tmp\n").unwrap();
    wait_for("idle shell", || restart_decision(&session.foreground(&SystemProcesses)) == Restart::Now { input: None });
    wait_for("cwd", || session.cwd(&SystemProcesses).is_some_and(|d| d.ends_with("tmp")));

    session.write(b"sleep 30\n").unwrap();
    wait_for("busy", || {
        restart_decision(&session.foreground(&SystemProcesses)) == Restart::Wait { busy: "sleep".into() }
    });
}
