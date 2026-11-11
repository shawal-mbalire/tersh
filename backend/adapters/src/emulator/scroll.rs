//! Tracks how many lines have scrolled off the top of the primary screen.
//!
//! Cards anchored in the primary screen remember an absolute line number
//! (`scroll_base + row` when they were created), which stays correct while
//! output keeps scrolling. `alacritty_terminal` doesn't expose a scroll counter,
//! so we derive one from history growth and, once history is full and stops
//! growing, from how far the newest history lines have shifted. History is
//! append-only, so that shift is exactly the number of lines scrolled (up to
//! the size of the window we remember).
//!
//! This runs after every read of program output, so it must stay cheap next
//! to parsing: it hashes only the lines it needs to find the previous newest
//! line again, checks a few lines after it, and keeps the rest of the window.

use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::Line;
use alacritty_terminal::term::{Term, TermMode};

#[derive(Debug, Default)]
pub struct ScrollTracker {
    base: u64,
    history: usize,
    /// Hashes of the newest history lines, newest first (`rows[j]` is line
    /// `-(j + 1)`), kept only while history is capped.
    rows: Vec<u64>,
}

impl ScrollTracker {
    pub fn base(&self) -> u64 {
        self.base
    }

    /// Bring the counter up to date. Cheap unless scrollback is full, and
    /// then proportional to the lines scrolled since the last update.
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
            let window = WINDOW.min(history);
            if history == self.history && self.rows.len() == window {
                self.base += self.follow(term) as u64;
            } else {
                self.rows = (0..window).map(|j| row_hash(term, j)).collect();
            }
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

    /// How many lines the capped history shifted since the window was taken,
    /// updating the window. The smallest shift `k` whose newest line `-(k + 1)`
    /// and the [`VERIFY`] lines before it match the old window wins; older
    /// lines are taken from the old window instead of being hashed again.
    fn follow<T>(&mut self, term: &Term<T>) -> usize {
        let old = &self.rows;
        let n = old.len();
        // `fresh[j]` is the hash of line `-(j + 1)` now, filled in as needed.
        let mut fresh: Vec<u64> = Vec::new();
        let at = |fresh: &mut Vec<u64>, j: usize| {
            while fresh.len() <= j {
                fresh.push(row_hash(term, fresh.len()));
            }
            fresh[j]
        };
        for k in 0..n {
            if at(&mut fresh, k) != old[0] {
                continue;
            }
            if (1..VERIFY.min(n - k)).all(|j| at(&mut fresh, k + j) == old[j]) {
                fresh.truncate(k);
                fresh.extend_from_slice(&old[..n - k]);
                self.rows = fresh;
                return k;
            }
        }
        // No overlap: more than the window scrolled.
        at(&mut fresh, n - 1);
        self.rows = fresh;
        n
    }
}

/// History lines remembered when scrollback is full. Bursts larger than this
/// between two updates are undercounted.
const WINDOW: usize = 256;

/// Lines that must match, counting the old newest one, before a shift is
/// accepted. Output that repeats in blocks this short can be undercounted.
const VERIFY: usize = 16;

/// Hash of the characters of history line `-(j + 1)` (FxHash-style: only
/// compared for equality, so it needs speed, not resistance to attack).
fn row_hash<T>(term: &Term<T>, j: usize) -> u64 {
    const K: u64 = 0x51_7c_c1_b7_27_22_0a_95;
    let row = &term.grid()[Line(-(j as i32) - 1)];
    row[..].iter().fold(0u64, |h, cell| (h.rotate_left(5) ^ cell.c as u64).wrapping_mul(K))
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

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
    fn counts_bursts_of_any_size_up_to_the_window() {
        let max_history = 300;
        let config = Config { scrolling_history: max_history, ..Config::default() };
        let mut term = Term::new(config, &TermSize::new(20, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        let mut tracker = ScrollTracker::default();
        let mut written = 0u64;
        // Fill the history past its cap (that update only sees the growth),
        // then scroll by bursts of mixed sizes.
        let mut offset = None;
        for burst in [400, 1, 0, 2, 15, 16, 17, 100, 255, 256, 3, 0, 64] {
            let mut out = String::new();
            for _ in 0..burst {
                out.push_str(&format!("line {written}\r\n"));
                written += 1;
            }
            parser.advance(&mut term, out.as_bytes());
            tracker.update(&term, max_history);
            // Every line written since has scrolled off, less what the screen still shows.
            let offset = *offset.get_or_insert(written - tracker.base());
            assert_eq!(tracker.base() + offset, written, "after a burst of {burst}");
        }
    }

    #[test]
    fn bursts_beyond_the_window_count_the_window() {
        let max_history = 1000;
        let config = Config { scrolling_history: max_history, ..Config::default() };
        let mut term = Term::new(config, &TermSize::new(20, 4), VoidListener);
        let mut parser: Processor = Processor::new();
        let mut tracker = ScrollTracker::default();
        let lines = |from: usize, to: usize| (from..to).map(|i| format!("{i}\r\n")).collect::<String>();
        parser.advance(&mut term, lines(0, 1200).as_bytes());
        tracker.update(&term, max_history);
        let before = tracker.base();
        parser.advance(&mut term, lines(1200, 1700).as_bytes());
        tracker.update(&term, max_history);
        assert_eq!(tracker.base() - before, WINDOW as u64);
        // The window is rebuilt, so the next burst is exact again.
        parser.advance(&mut term, lines(1700, 1710).as_bytes());
        tracker.update(&term, max_history);
        assert_eq!(tracker.base() - before, WINDOW as u64 + 10);
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
