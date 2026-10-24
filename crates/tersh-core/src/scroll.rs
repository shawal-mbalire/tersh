//! Tracks how many lines have scrolled off the top of the primary screen.
//!
//! Cards anchored in the primary screen remember an absolute line number
//! (`scroll_base + row` when they were created), which stays correct while
//! output keeps scrolling. `alacritty_terminal` doesn't expose a scroll counter,
//! so we derive one from history growth and, once history is full and stops
//! growing, from how far the newest history lines have shifted. History is
//! append-only, so that shift is exactly the number of lines scrolled (up to
//! the size of the window we compare).

use std::collections::hash_map::DefaultHasher;
use std::hash::{Hash, Hasher};

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::term::{Term, TermMode};

use crate::frame::row_text;

#[derive(Debug, Default)]
pub struct ScrollTracker {
    base: u64,
    history: usize,
    /// Hashes of the newest history lines, kept only while history is capped.
    rows: Vec<u64>,
}

impl ScrollTracker {
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Bring the counter up to date. Cheap unless scrollback is full.
    pub fn update<T>(&mut self, term: &Term<T>, max_history: usize) {
        // The alternate screen has no history and leaves the primary one untouched.
        if term.mode().contains(TermMode::ALT_SCREEN) {
            return;
        }
        let history = term.grid().history_size();
        if history > self.history {
            self.base += (history - self.history) as u64;
        }
        let capped = history >= max_history && max_history > 0;
        if capped {
            let rows = history_hashes(term, WINDOW.min(history));
            if history == self.history && rows.len() == self.rows.len() {
                self.base += shift(&self.rows, &rows) as u64;
            }
            self.rows = rows;
        } else {
            self.rows.clear();
        }
        self.history = history;
    }

    /// Forget content hashes, e.g. after a resize reflowed the screen.
    pub fn invalidate<T>(&mut self, term: &Term<T>) {
        self.rows.clear();
        if !term.mode().contains(TermMode::ALT_SCREEN) {
            self.history = term.grid().history_size();
        }
    }
}

/// History lines compared when scrollback is full. Bursts larger than this
/// between two updates are undercounted.
const WINDOW: usize = 256;

/// Hashes of the newest `count` history lines, oldest first.
fn history_hashes<T>(term: &Term<T>, count: usize) -> Vec<u64> {
    (1..=count as i32)
        .rev()
        .map(|i| {
            let mut h = DefaultHasher::new();
            row_text(term, -i).hash(&mut h);
            h.finish()
        })
        .collect()
}

/// Smallest `k` such that the old window shifted up by `k` lines matches the new one.
fn shift(old: &[u64], new: &[u64]) -> usize {
    let n = old.len();
    (0..n).find(|&k| old[k..] == new[..n - k]).unwrap_or(n)
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    #[test]
    fn shift_detection() {
        assert_eq!(shift(&[1, 2, 3, 4], &[1, 2, 3, 4]), 0);
        assert_eq!(shift(&[1, 2, 3, 4], &[2, 3, 4, 5]), 1);
        assert_eq!(shift(&[1, 2, 3, 4], &[4, 5, 6, 7]), 3);
        assert_eq!(shift(&[1, 2, 3, 4], &[9, 9, 9, 9]), 4);
        assert_eq!(shift(&[], &[]), 0);
    }

    #[test]
    fn counts_scrolled_lines_past_the_history_cap() {
        let max_history = 5;
        let config = Config { scrolling_history: max_history, ..Config::default() };
        let mut term = Term::new(config, &TermSize::new(10, 3), VoidListener);
        let mut parser: Processor = Processor::new();
        let mut tracker = ScrollTracker::default();

        for i in 0..20 {
            parser.advance(&mut term, format!("line{i}\r\n").as_bytes());
            tracker.update(&term, max_history);
        }
        // 20 newlines on a 3-line screen: the first 18 lines have scrolled off.
        assert_eq!(tracker.base(), 18);

        // Batched output while capped is measured by content shift.
        parser.advance(&mut term, b"a\r\nb\r\n");
        tracker.update(&term, max_history);
        assert_eq!(tracker.base(), 20);

        // Redrawing the screen in place does not count as scrolling.
        parser.advance(&mut term, b"\x1b[Hxxxx\x1b[2;1Hyyyy");
        tracker.update(&term, max_history);
        assert_eq!(tracker.base(), 20);
    }

    #[test]
    fn alt_screen_does_not_count() {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let mut parser: Processor = Processor::new();
        let mut tracker = ScrollTracker::default();
        parser.advance(&mut term, b"\x1b[?1049h1\r\n2\r\n3\r\n4\r\n5\r\n");
        tracker.update(&term, 10_000);
        assert_eq!(tracker.base(), 0);
        parser.advance(&mut term, b"\x1b[?1049lx\r\ny\r\nz\r\n");
        tracker.update(&term, 10_000);
        assert_eq!(tracker.base(), 1);
    }
}
