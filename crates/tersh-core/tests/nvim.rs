//! Runs the Neovim plugin (editors/nvim) inside a real session: cards must be
//! anchored on the alternate screen and follow scrolling. Skipped without `nvim`.
#![cfg(unix)]

use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_core::{Config, Frame, Session, SessionOptions, Sink, TermEvent};

#[derive(Default)]
struct Recorder {
    rich: Mutex<Vec<Value>>,
}

impl Sink for Recorder {
    fn frame(&self, _: Frame) {}
    fn rich(&self, message: Value) {
        self.rich.lock().unwrap().push(message);
    }
    fn event(&self, _: TermEvent) {}
}

fn wait_for(recorder: &Recorder, what: &str, pred: impl Fn(&Value) -> bool) -> Value {
    let start = Instant::now();
    loop {
        if let Some(msg) = recorder.rich.lock().unwrap().iter().find(|m| pred(m)) {
            return msg.clone();
        }
        assert!(start.elapsed() < Duration::from_secs(10), "no {what}; got {:?}", recorder.rich.lock().unwrap());
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn nvim_line_card_follows_scrolling() {
    if std::process::Command::new("nvim").arg("--version").output().is_err() {
        eprintln!("nvim not found; skipping");
        return;
    }
    let plugin = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../editors/nvim").canonicalize().unwrap();
    let dir = std::env::temp_dir().join(format!("tersh-nvim-e2e-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let lines: Vec<String> = (1..=100).map(|i| format!("line {i}")).collect();
    std::fs::write(dir.join("file.txt"), lines.join("\n")).unwrap();
    std::fs::write(
        dir.join("init.lua"),
        format!(
            "vim.opt.rtp:prepend('{}')\nvim.o.shada = ''\nvim.o.swapfile = false\n\
             vim.api.nvim_create_autocmd('VimEnter', {{ callback = function()\n\
               vim.defer_fn(function() require('tersh').attach({{ kind = 'text', data = 'hi', line = 5, rows = 3, id = 'e2e' }}) end, 100)\n\
             end }})\n",
            plugin.display()
        ),
    )
    .unwrap();

    let recorder = Arc::new(Recorder::default());
    let config = Config { shell: Some("/bin/sh".into()), ..Config::default() };
    let mut options = SessionOptions::from_config(&config, 80, 24);
    options.env.push(("PS1".into(), "$ ".into()));
    let session = Session::spawn(options, recorder.clone()).unwrap();
    session
        .write(format!("cd {} && exec nvim -u init.lua -i NONE file.txt\n", dir.display()).as_bytes())
        .unwrap();

    let show = wait_for(&recorder, "show", |m| m["op"] == "show" && m["id"] == "e2e");
    let anchor = &show["anchor"];
    assert_eq!(anchor["screen"], "alt", "{show}");
    assert_eq!(anchor["line"], 5, "{show}");
    assert_eq!(anchor["rows"], 3, "{show}");
    assert_eq!(show["hidden"], false);

    // Ctrl-E twice: the card moves up with its line.
    session.write(b"\x05\x05").unwrap();
    let moved = wait_for(&recorder, "move", |m| m["op"] == "move" && m["anchor"]["line"] == 3);
    assert_eq!(moved["anchor"]["screen"], "alt", "{moved}");

    session.write(b"\x1b:qa!\r").unwrap();
    wait_for(&recorder, "close", |m| m["op"] == "close" && m["id"] == "e2e");
}
