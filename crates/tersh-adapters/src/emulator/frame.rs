//! Builds domain [`Frame`]s from alacritty's grid.

use alacritty_terminal::event::EventListener;
use alacritty_terminal::grid::Dimensions;
use alacritty_terminal::index::{Column, Line};
use alacritty_terminal::term::cell::{Cell, Flags};
use alacritty_terminal::term::color::Colors;
use alacritty_terminal::term::{Term, TermDamage, TermMode};
use alacritty_terminal::vte::ansi::{Color, CursorShape, NamedColor};
use tersh_domain::frame::{Cursor, Frame, Row, Run, Shape, RGB_FLAG};

/// Build a frame from the terminal's damage since the last call, then reset it.
pub fn build<T: EventListener>(term: &mut Term<T>, scroll_base: u64, force_full: bool) -> Frame {
    let rows = term.screen_lines();
    let cols = term.columns();
    let display_offset = term.grid().display_offset();

    let damaged: Option<Vec<usize>> = match term.damage() {
        TermDamage::Full => None,
        TermDamage::Partial(iter) => Some(iter.map(|d| d.line).collect()),
    };
    term.reset_damage();

    let full = force_full || damaged.is_none();
    let wanted: Vec<usize> = match damaged {
        Some(lines) if !full => {
            let mut lines: Vec<usize> = lines.into_iter().filter(|&l| l < rows).collect();
            lines.sort_unstable();
            lines.dedup();
            lines
        },
        _ => (0..rows).collect(),
    };

    let colors = term.colors();
    let grid = term.grid();
    let lines = wanted
        .into_iter()
        .map(|y| {
            let line = Line(y as i32 - display_offset as i32);
            Row { y: y as u16, runs: row_runs(&grid[line][..], colors) }
        })
        .collect();

    let content = term.renderable_content();
    let mode = content.mode;
    let point = content.cursor.point;
    let view_y = point.line.0 + display_offset as i32;
    let style = term.cursor_style();
    let shape = if view_y < 0 || view_y >= rows as i32 {
        Shape::Hidden
    } else {
        match content.cursor.shape {
            CursorShape::Block => Shape::Block,
            CursorShape::Underline => Shape::Underline,
            CursorShape::Beam => Shape::Beam,
            CursorShape::HollowBlock => Shape::Hollow,
            CursorShape::Hidden => Shape::Hidden,
        }
    };

    Frame {
        cols: cols as u16,
        rows: rows as u16,
        full,
        lines,
        cursor: Cursor {
            x: point.column.0.min(cols.saturating_sub(1)) as u16,
            y: view_y.clamp(0, rows as i32 - 1) as u16,
            shape,
            blink: style.blinking,
        },
        mode: mode.bits(),
        alt: mode.contains(TermMode::ALT_SCREEN),
        display_offset,
        history: term.grid().history_size(),
        scroll_base,
    }
}

/// Text of a grid line (negative lines are history), used for scroll tracking.
pub fn row_text<T>(term: &Term<T>, line: i32) -> String {
    let cols = term.columns();
    let row = &term.grid()[Line(line)];
    (0..cols).map(|x| row[Column(x)].c).collect()
}

fn encode(color: Color, colors: &Colors) -> u32 {
    let index = match color {
        Color::Spec(rgb) => return rgb_bits(rgb),
        Color::Named(named) => named as usize,
        Color::Indexed(i) => i as usize,
    };
    match colors[index] {
        // Palette entry overridden by the program (OSC 4/10/11).
        Some(rgb) => rgb_bits(rgb),
        None => index as u32,
    }
}

fn rgb_bits(rgb: alacritty_terminal::vte::ansi::Rgb) -> u32 {
    RGB_FLAG | (rgb.r as u32) << 16 | (rgb.g as u32) << 8 | rgb.b as u32
}

/// Flags that do not affect drawing and must not split runs.
const IGNORED: Flags = Flags::WRAPLINE.union(Flags::LEADING_WIDE_CHAR_SPACER);

fn row_runs(cells: &[Cell], colors: &Colors) -> Vec<Run> {
    let mut runs: Vec<Run> = Vec::new();
    // Whether the last run may be extended with more single-width cells.
    let mut open = false;

    for (x, cell) in cells.iter().enumerate() {
        if cell.flags.contains(Flags::WIDE_CHAR_SPACER) {
            continue;
        }
        let fg = encode(cell.fg, colors);
        let bg = encode(cell.bg, colors);
        let f = (cell.flags - IGNORED).bits();
        let zerowidth = cell.zerowidth().filter(|z| !z.is_empty());
        let wide = cell.flags.contains(Flags::WIDE_CHAR);
        let ch = if cell.c == '\0' { ' ' } else { cell.c };

        if wide || zerowidth.is_some() {
            let mut t = String::from(ch);
            if let Some(z) = zerowidth {
                t.extend(z.iter());
            }
            let n = if wide { 2 } else { 1 };
            runs.push(Run { c: x as u16, n, t, fg, bg, f });
            open = false;
            continue;
        }

        match runs.last_mut() {
            Some(last) if open && last.fg == fg && last.bg == bg && last.f == f => {
                last.t.push(ch);
                last.n += 1;
            },
            _ => {
                runs.push(Run { c: x as u16, n: 1, t: String::from(ch), fg, bg, f });
                open = true;
            },
        }
    }

    // Trailing default-coloured blanks carry no information.
    if let Some(last) = runs.last_mut() {
        let default_bg = encode(Color::Named(NamedColor::Background), colors);
        if open && last.f == 0 && last.bg == default_bg {
            let trimmed = last.t.trim_end_matches(' ');
            let removed = last.t.len() - trimmed.len();
            if removed > 0 {
                last.t.truncate(trimmed.len());
                last.n -= removed as u16;
            }
            if last.n == 0 {
                runs.pop();
            }
        }
    }
    runs
}

#[cfg(test)]
mod tests {
    use super::*;
    use alacritty_terminal::event::VoidListener;
    use alacritty_terminal::term::test::TermSize;
    use alacritty_terminal::term::Config;
    use alacritty_terminal::vte::ansi::Processor;

    fn term_with(input: &[u8]) -> Term<VoidListener> {
        let mut term = Term::new(Config::default(), &TermSize::new(10, 3), VoidListener);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, input);
        term
    }

    #[test]
    fn merges_runs_and_trims_blanks() {
        let mut term = term_with(b"ab\x1b[31mcd\x1b[0m");
        let frame = build(&mut term, 0, false);
        assert!(frame.full);
        assert_eq!(frame.lines.len(), 3);
        let runs = &frame.lines[0].runs;
        assert_eq!(runs.len(), 2);
        assert_eq!((runs[0].c, runs[0].n, runs[0].t.as_str(), runs[0].fg), (0, 2, "ab", 256));
        assert_eq!((runs[1].c, runs[1].n, runs[1].t.as_str(), runs[1].fg), (2, 2, "cd", 1));
        assert!(frame.lines[1].runs.is_empty());
        assert_eq!((frame.cursor.x, frame.cursor.y), (4, 0));
    }

    #[test]
    fn wide_chars_get_their_own_run() {
        let mut term = term_with("a漢b".as_bytes());
        let runs = &build(&mut term, 0, false).lines[0].runs;
        let summary: Vec<_> = runs.iter().map(|r| (r.c, r.n, r.t.as_str())).collect();
        assert_eq!(summary, vec![(0, 1, "a"), (1, 2, "漢"), (3, 1, "b")]);
    }

    #[test]
    fn truecolor_is_flagged() {
        let mut term = term_with(b"\x1b[38;2;1;2;3mx");
        let run = &build(&mut term, 0, false).lines[0].runs[0];
        assert_eq!(run.fg, RGB_FLAG | 0x010203);
    }

    #[test]
    fn partial_damage_after_reset() {
        let mut term = term_with(b"x");
        build(&mut term, 0, false);
        let mut parser: Processor = Processor::new();
        parser.advance(&mut term, b"\r\n\r\nz");
        let frame = build(&mut term, 0, false);
        assert!(!frame.full);
        let ys: Vec<u16> = frame.lines.iter().map(|r| r.y).collect();
        assert!(ys.contains(&2), "{ys:?}");
        assert!(!ys.contains(&1) || ys.len() <= 3);
    }
}
