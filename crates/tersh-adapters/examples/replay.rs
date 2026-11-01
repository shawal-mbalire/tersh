//! Replay recorded program output through the emulator, to profile parsing and
//! frame building or to debug how a program's output ends up on screen.
//!
//! ```sh
//! script -q -f out.raw -c claude          # record (inside any terminal)
//! cargo run --release -p tersh-adapters --example replay -- out.raw 120 40 [--screen]
//! ```
//!
//! Prints the time spent and the final cursor; `--screen` also prints the
//! screen. Cut the recording short to see the state at an earlier moment.

use std::sync::Arc;
use std::time::Instant;

use tersh_adapters::AlacrittyEmulators;
use tersh_app::ports::{EmulatorFactory, EmulatorOptions, TermHost};
use tersh_domain::config::Cursor;
use tersh_domain::{Rgb, TermEvent, WindowSize};

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

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let usage = "usage: replay FILE COLS ROWS [--screen]";
    let (Some(path), Some(cols), Some(rows)) = (args.first(), args.get(1), args.get(2)) else {
        eprintln!("{usage}");
        std::process::exit(2);
    };
    let cols: u16 = cols.parse().expect("COLS");
    let rows: u16 = rows.parse().expect("ROWS");
    let screen = args.iter().any(|a| a == "--screen");
    let bytes = std::fs::read(path).expect("read recording");

    let options = EmulatorOptions { cols, rows, scrollback: 10_000, cursor: Cursor::default() };
    let mut emulator = AlacrittyEmulators.create(&options, Arc::new(NullHost));
    let start = Instant::now();
    for chunk in bytes.chunks(4096) {
        emulator.advance(chunk);
    }
    let parsed = start.elapsed();
    let start = Instant::now();
    let frame = emulator.frame(true);
    let built = start.elapsed();

    println!("{} bytes parsed in {parsed:?} ({:.1} MiB/s), full frame built in {built:?}", bytes.len(), bytes.len() as f64 / 1048576.0 / parsed.as_secs_f64());
    println!("cursor: column {} row {} ({:?}, mode {:#x})", frame.cursor.x, frame.cursor.y, frame.cursor.shape, frame.mode);
    if screen {
        for row in &frame.lines {
            let mut line = String::new();
            for run in &row.runs {
                while line.chars().count() < run.c as usize {
                    line.push(' ');
                }
                line.push_str(&run.t);
            }
            println!("{:3}|{line}", row.y);
        }
    }
}
