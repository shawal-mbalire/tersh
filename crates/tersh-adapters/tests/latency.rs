//! Latency budget checks against the 4 ms frame interval (~240 Hz):
//! key encoding, VT parsing, differential/full frame building, and live PTY
//! round-trip echo must stay within the 4 ms frame budget even in debug builds.

#![cfg(unix)]

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_adapters::{spawn_local, AlacrittyEmulators};
use tersh_app::ports::{EmulatorFactory, EmulatorOptions, TermHost};
use tersh_app::{SessionOptions, Sink};
use tersh_domain::config::Cursor;
use tersh_domain::input::{encode_key, mode, KeyInput};
use tersh_domain::{Config, Frame, Rgb, TermEvent, WindowSize};

/// The 240 Hz frame budget (`FRAME_INTERVAL` in `tersh-app`).
const FRAME_BUDGET: Duration = Duration::from_millis(4);

struct NullHost;

impl TermHost for NullHost {
    fn reply(&self, _: &str) {}
    fn event(&self, _: TermEvent) {}
    fn color(&self, _: usize) -> Rgb {
        Rgb { r: 0, g: 0, b: 0 }
    }
    fn window_size(&self) -> WindowSize {
        WindowSize { cols: 120, rows: 40, cell_width: 9, cell_height: 18 }
    }
}

struct FrameNotify {
    tx: Mutex<mpsc::Sender<Instant>>,
}

impl Sink for FrameNotify {
    fn frame(&self, _frame: Frame) {
        let _ = self.tx.lock().unwrap().send(Instant::now());
    }
    fn rich(&self, _: Value) {}
    fn event(&self, _: TermEvent) {}
    fn acks_frames(&self) -> bool {
        true
    }
}

#[test]
fn key_encoding_and_emulator_frame_build_stay_within_4ms_budget() {
    // 1. 1,000 key encodings must finish within a single 4 ms frame budget.
    let key = KeyInput {
        key: "Unidentified".into(),
        code: "Tab".into(),
        shift: true,
        ..Default::default()
    };
    let start = Instant::now();
    for _ in 0..1_000 {
        let encoded = encode_key(&key, mode::DISAMBIGUATE_KEYS | mode::REPORT_EVENT_TYPES);
        assert_eq!(encoded.as_deref(), Some("\x1b[9;2u"));
    }
    let key_batch = start.elapsed();
    assert!(
        key_batch < FRAME_BUDGET,
        "1,000 key encodings took {key_batch:?}, exceeding the {FRAME_BUDGET:?} frame budget"
    );

    // 2. Populate a 120x40 screen with 24-bit coloured text.
    let options = EmulatorOptions { cols: 120, rows: 40, scrollback: 10_000, cursor: Cursor::default() };
    let mut emulator = AlacrittyEmulators.create(&options, Arc::new(NullHost));
    let mut screen_ansi = Vec::new();
    screen_ansi.extend_from_slice(b"\x1b[H");
    for r in 0..40 {
        for c in 0..120 {
            screen_ansi.extend_from_slice(
                format!("\x1b[38;2;{};{};{}m{}", (r * 6) % 255, (c * 2) % 255, 180, (b'A' + ((r + c) % 26) as u8) as char)
                    .as_bytes(),
            );
        }
        if r < 39 {
            screen_ansi.extend_from_slice(b"\r\n");
        }
    }
    emulator.advance(&screen_ansi);
    emulator.advance(b"\x1b[H");
    let _ = emulator.frame(true);

    // 3. Single-keystroke VT parse + differential frame build (`frame(false)`)
    //    must average well under 100 µs (and stay within the 4 ms budget).
    let iterations = 500;
    let start = Instant::now();
    for i in 0..iterations {
        let ch = [b'a' + (i % 26) as u8];
        emulator.advance(&ch);
        let frame = emulator.frame(false);
        assert_eq!(frame.lines.len(), 1);
        if i % 80 == 79 {
            emulator.advance(b"\r");
            let _ = emulator.frame(false);
        }
    }
    let avg_diff = start.elapsed() / iterations;
    assert!(
        avg_diff < Duration::from_micros(100),
        "single-char advance + diff frame averaged {avg_diff:?} (theoretical budget < 100 µs)"
    );

    // 4. Full-screen 120x40 (4,800 truecolor cells) `frame(true)` build must
    //    complete within the 4 ms frame budget.
    let full_iters = 50;
    let start = Instant::now();
    for _ in 0..full_iters {
        let frame = emulator.frame(true);
        assert_eq!(frame.lines.len(), 40);
    }
    let avg_full = start.elapsed() / full_iters;
    assert!(
        avg_full < FRAME_BUDGET,
        "120x40 full frame build averaged {avg_full:?}, exceeding the {FRAME_BUDGET:?} frame budget"
    );
}

#[test]
fn live_pty_keystroke_round_trip_stays_within_4ms_frame_budget() {
    let (tx, rx) = mpsc::channel();
    let sink = Arc::new(FrameNotify { tx: Mutex::new(tx) });
    let config = Config { shell: Some("/bin/cat".into()), ..Config::default() };
    let options = SessionOptions::from_config(&config, 120, 40);
    let session = spawn_local(options, true, sink).expect("spawn cat");

    // Let the initial frame settle so the render loop is idle-ready.
    std::thread::sleep(Duration::from_millis(20));
    while rx.try_recv().is_ok() {}
    session.frame_shown();

    let mut samples = Vec::with_capacity(40);
    for i in 0..40 {
        // Space keystrokes past FRAME_INTERVAL (4 ms) so each wakes the render loop immediately.
        std::thread::sleep(Duration::from_millis(5));
        while rx.try_recv().is_ok() {}
        let ch = ((b'a' + (i % 26) as u8) as char).to_string();
        let event = KeyInput { key: ch.clone(), ..Default::default() };
        let start = Instant::now();
        // Plain printable keys return Ok(false) from `key` and are sent via `write`.
        assert!(!session.key(&event).unwrap());
        session.write(ch).unwrap();
        let arrived = rx.recv_timeout(Duration::from_secs(1)).expect("echo frame arrived");
        session.frame_shown();
        samples.push(arrived.duration_since(start));
    }

    samples.sort();
    let median = samples[samples.len() / 2];
    let avg: Duration = samples.iter().copied().sum::<Duration>() / samples.len() as u32;
    assert!(
        median < FRAME_BUDGET && avg < FRAME_BUDGET,
        "live PTY key -> frame round-trip exceeded {FRAME_BUDGET:?} budget (median {median:?}, avg {avg:?})"
    );
}
