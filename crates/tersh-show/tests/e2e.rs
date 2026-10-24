//! Runs `tersh-show` inside a real tersh session and checks what the terminal receives.
#![cfg(unix)]

use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_adapters::spawn_local;
use tersh_app::{SessionOptions, Sink};
use tersh_domain::{Config, Frame, TermEvent};

#[derive(Default)]
struct Recorder {
    rich: Mutex<Vec<Value>>,
    cursor_row: Mutex<u16>,
}

impl Sink for Recorder {
    fn frame(&self, frame: Frame) {
        *self.cursor_row.lock().unwrap() = frame.cursor.y;
    }
    fn rich(&self, message: Value) {
        self.rich.lock().unwrap().push(message);
    }
    fn event(&self, _: TermEvent) {}
}

fn run_in_session(command: &str, use_socket: bool) -> (Value, u16) {
    let recorder = Arc::new(Recorder::default());
    let config = Config { shell: Some("/bin/sh".into()), ..Config::default() };
    let mut options = SessionOptions::from_config(&config, 60, 30);
    options.env.push(("PS1".into(), "$ ".into()));
    let session = spawn_local(options, use_socket, recorder.clone()).unwrap();

    let dir = std::env::temp_dir().join(format!("tersh-show-e2e-{}-{use_socket}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("note.md"), "# Title\n\nsome *text* and $x^2$\n").unwrap();

    let bin = env!("CARGO_BIN_EXE_tersh-show");
    session.write(format!("cd {} && clear && {bin} {command}\n", dir.display()).as_bytes()).unwrap();

    let start = Instant::now();
    while recorder.rich.lock().unwrap().is_empty() {
        assert!(start.elapsed() < Duration::from_secs(10), "no card received");
        std::thread::sleep(Duration::from_millis(20));
    }
    // Let the reserved newlines and the next prompt arrive.
    std::thread::sleep(Duration::from_millis(300));
    let message = recorder.rich.lock().unwrap()[0].clone();
    let cursor_row = *recorder.cursor_row.lock().unwrap();
    (message, cursor_row)
}

#[test]
fn inline_card_over_osc() {
    let (msg, cursor_row) = run_in_session("--osc --rows 6 note.md", false);
    assert_eq!(msg["kind"], "markdown");
    assert_eq!(msg["title"], "note.md");
    assert!(msg["data"].as_str().unwrap().contains("$x^2$"));
    let anchor = &msg["anchor"];
    assert_eq!(anchor["rows"], 6);
    assert_eq!(anchor["col"], 0);
    // `clear` runs before tersh-show and wipes the echoed command, so the card
    // is anchored on row 0 and the prompt returns below the 6 reserved rows.
    let line = anchor["line"].as_u64().unwrap();
    assert_eq!(cursor_row as u64, 6, "card line {line}");
}

#[test]
fn inline_card_over_socket() {
    let (msg, cursor_row) = run_in_session("--rows 4 --id note note.md", true);
    assert_eq!(msg["id"], "note");
    assert_eq!(msg["anchor"]["rows"], 4);
    assert_eq!(cursor_row, 4);
}

#[test]
fn notebooks_float_by_default() {
    let nb = r##"{"cells":[{"cell_type":"markdown","source":["# hi"]}],"metadata":{},"nbformat":4,"nbformat_minor":5}"##;
    let (msg, _) = run_in_session(&format!("--osc - <<'EOF'\n{nb}\nEOF"), false);
    assert_eq!(msg["kind"], "notebook");
    assert_eq!(msg["anchor"]["mode"], "float");
    assert_eq!(msg["data"]["cells"][0]["cell_type"], "markdown");
}
