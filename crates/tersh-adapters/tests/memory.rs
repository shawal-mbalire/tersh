//! Memory leak checks on the real adapters: the heap must stay flat while a
//! terminal runs for a long time, and return to where it was after sessions
//! are opened and closed.
//!
//! A counting global allocator tracks live heap bytes. That count covers every
//! thread in this binary, so everything runs inside one `#[test]`, which keeps
//! the test harness from running other tests at the same time.

#![cfg(unix)]

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;
use tersh_adapters::{spawn_local, AlacrittyEmulators};
use tersh_app::ports::{EmulatorFactory, EmulatorOptions, TermHost};
use tersh_app::{SessionOptions, Sink};
use tersh_domain::config::Cursor;
use tersh_domain::{Config, Frame, Rgb, TermEvent, WindowSize};

struct Counting;

static LIVE: AtomicIsize = AtomicIsize::new(0);

unsafe impl GlobalAlloc for Counting {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = System.alloc(layout);
        if !ptr.is_null() {
            LIVE.fetch_add(layout.size() as isize, Ordering::Relaxed);
        }
        ptr
    }

    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        System.dealloc(ptr, layout);
        LIVE.fetch_sub(layout.size() as isize, Ordering::Relaxed);
    }

    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, new_size: usize) -> *mut u8 {
        let new = System.realloc(ptr, layout, new_size);
        if !new.is_null() {
            LIVE.fetch_add(new_size as isize - layout.size() as isize, Ordering::Relaxed);
        }
        new
    }
}

#[global_allocator]
static ALLOCATOR: Counting = Counting;

fn live() -> isize {
    LIVE.load(Ordering::Relaxed)
}

const KIB: isize = 1024;

fn wait_for(what: &str, mut check: impl FnMut() -> bool) {
    let start = Instant::now();
    while !check() {
        assert!(start.elapsed() < Duration::from_secs(10), "timed out waiting for {what}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

struct NullHost;

impl TermHost for NullHost {
    fn reply(&self, _: &str) {}
    fn event(&self, _: TermEvent) {}
    fn color(&self, _: usize) -> Rgb {
        Rgb { r: 0, g: 0, b: 0 }
    }
    fn window_size(&self) -> WindowSize {
        WindowSize { cols: 80, rows: 24, cell_width: 8, cell_height: 16 }
    }
}

#[derive(Default)]
struct NullSink;

impl Sink for NullSink {
    fn frame(&self, _: Frame) {}
    fn rich(&self, _: Value) {}
    fn event(&self, _: TermEvent) {}
}

#[test]
fn memory_stays_bounded() {
    emulator_is_flat_once_scrollback_is_full();
    closed_sessions_give_their_memory_back();
    programs_that_ignore_hangup_do_not_keep_sessions_alive();
}

/// Scrollback is capped, so after it fills, more output (coloured lines,
/// wide characters, cursor movement, frames built on the way) must not grow
/// the heap.
fn emulator_is_flat_once_scrollback_is_full() {
    let options = EmulatorOptions { cols: 120, rows: 40, scrollback: 2_000, cursor: Cursor::default() };
    let mut emulator = AlacrittyEmulators.create(&options, Arc::new(NullHost));
    let mut feed = |from: usize, to: usize| {
        for i in from..to {
            let line = format!("\x1b[3{}mline {i} 漢字 {}\x1b[0m\r\n\x1b[2K", i % 8, "x".repeat(i % 100));
            emulator.advance(line.as_bytes());
            if i % 50 == 0 {
                drop(emulator.frame(i % 500 == 0));
            }
        }
    };

    // Fill the scrollback (twice over), then measure.
    feed(0, 4_000);
    let filled = live();
    feed(4_000, 16_000);
    let after = live();
    let growth = after - filled;
    assert!(growth < 256 * KIB, "heap grew by {} KiB over 12,000 lines with full scrollback", growth / KIB);
}

/// Open sessions on a real shell, use them, close them: each one's threads,
/// scrollback and socket must go away.
fn closed_sessions_give_their_memory_back() {
    let config = Config { shell: Some("/bin/sh".into()), ..Config::default() };
    let open_and_close = || {
        let mut options = SessionOptions::from_config(&config, 80, 24);
        options.env.push(("PS1".into(), "$ ".into()));
        let session = spawn_local(options, true, Arc::new(NullSink)).expect("spawn shell");
        let socket = session.socket_path().cloned().expect("socket");
        session.write(b"i=0; while [ $i -lt 300 ]; do echo line $i; i=$((i+1)); done\n").unwrap();
        wait_for("output", || session.stats().output_bytes > 2_000);
        let inbox = session.rich_inbox();
        drop(session);
        wait_for("session released", || inbox.is_released());
        wait_for("socket removed", || !socket.exists());
    };

    // The first session initialises things that live for the whole process.
    open_and_close();
    let baseline = live();
    for _ in 0..15 {
        open_and_close();
    }
    // Threads that already finished may still be unwinding their last
    // allocations; give them a moment.
    let start = Instant::now();
    while live() - baseline > 64 * KIB && start.elapsed() < Duration::from_secs(2) {
        std::thread::sleep(Duration::from_millis(20));
    }
    let growth = live() - baseline;
    assert!(growth < 64 * KIB, "{} KiB still allocated after closing 15 sessions", growth / KIB);
}

/// The session hangs its program up when it closes (SIGHUP), but a shell can
/// ignore that (`trap '' HUP`, `nohup`). Then the program never exits and
/// its output never ends, so the threads waiting on them must not keep the
/// session alive, and closing the terminal must still end the program.
fn programs_that_ignore_hangup_do_not_keep_sessions_alive() {
    let dir = std::env::temp_dir().join(format!("tersh-memory-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let pid_file = dir.join("pid");
    let config = Config { shell: Some("/bin/sh".into()), ..Config::default() };
    let options = SessionOptions::from_config(&config, 80, 24);
    let session = spawn_local(options, true, Arc::new(NullSink)).expect("spawn shell");
    let socket = session.socket_path().cloned().expect("socket");
    session.write(format!("trap '' HUP; echo $$ > {}\n", pid_file.display()).as_bytes()).unwrap();
    wait_for("shell ignoring hangup", || std::fs::read_to_string(&pid_file).is_ok_and(|s| s.ends_with('\n')));
    let pid = std::fs::read_to_string(&pid_file).unwrap().trim().to_string();
    let alive = || std::process::Command::new("kill").args(["-0", &pid]).status().is_ok_and(|s| s.success());

    let inbox = session.rich_inbox();
    drop(session);
    let start = Instant::now();
    while (!inbox.is_released() || alive()) && start.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    let (released, exited) = (inbox.is_released(), !alive());
    let _ = std::process::Command::new("kill").args(["-9", &pid]).status();
    let _ = std::fs::remove_dir_all(&dir);
    assert!(released, "the session was not released while its program ignored the hangup");
    assert!(exited, "the program was still running after its terminal closed");
    wait_for("socket removed", || !socket.exists());
}
