//! Emulator adapter: VT emulation by `alacritty_terminal`.

mod frame;
mod scroll;

use std::sync::Arc;
use std::time::Instant;

use alacritty_terminal::event::{Event, EventListener, WindowSize as AlacrittySize};
use alacritty_terminal::grid::Scroll as GridScroll;
use alacritty_terminal::term::test::TermSize;
use alacritty_terminal::term::{self, Term, TermMode};
use alacritty_terminal::vte::ansi::{CursorShape as Shape, CursorStyle, Processor, Rgb};
use tersh_app::ports::{Emulator, EmulatorFactory, EmulatorOptions, Scroll, TermHost};
use tersh_domain::config::CursorShape;
use tersh_domain::rich::{AnchorContext, Screen};
use tersh_domain::{Frame, TermEvent};

use scroll::ScrollTracker;

/// Creates [`AlacrittyEmulator`]s.
#[derive(Debug, Default, Clone, Copy)]
pub struct AlacrittyEmulators;

impl EmulatorFactory for AlacrittyEmulators {
    fn create(&self, options: &EmulatorOptions, host: Arc<dyn TermHost>) -> Box<dyn Emulator> {
        let size = TermSize::new(options.cols as usize, options.rows as usize);
        Box::new(AlacrittyEmulator {
            term: Term::new(term_config(options), &size, Listener(host)),
            parser: Processor::new(),
            scroll: ScrollTracker::default(),
            max_history: options.scrollback,
        })
    }
}

pub struct AlacrittyEmulator {
    term: Term<Listener>,
    parser: Processor,
    scroll: ScrollTracker,
    max_history: usize,
}

impl Emulator for AlacrittyEmulator {
    fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.scroll.update(&self.term, self.max_history);
    }

    fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    fn flush_sync(&mut self) {
        self.parser.stop_sync(&mut self.term);
        self.scroll.update(&self.term, self.max_history);
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(TermSize::new(cols as usize, rows as usize));
        self.scroll.invalidate(&self.term);
    }

    fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(match scroll {
            Scroll::Lines(lines) => GridScroll::Delta(lines),
            Scroll::Bottom => GridScroll::Bottom,
        });
    }

    fn display_offset(&self) -> usize {
        self.term.grid().display_offset()
    }

    fn anchor_context(&self) -> AnchorContext {
        let point = self.term.grid().cursor.point;
        AnchorContext {
            screen: if self.alt_screen() { Screen::Alt } else { Screen::Primary },
            cursor_row: point.line.0.max(0) as usize,
            cursor_col: point.column.0,
            scroll_base: self.scroll.base(),
        }
    }

    fn alt_screen(&self) -> bool {
        self.term.mode().contains(TermMode::ALT_SCREEN)
    }

    fn frame(&mut self, full: bool) -> Frame {
        frame::build(&mut self.term, self.scroll.base(), full)
    }

    fn set_options(&mut self, options: &EmulatorOptions) {
        self.max_history = options.scrollback;
        self.term.set_options(term_config(options));
    }
}

fn term_config(options: &EmulatorOptions) -> term::Config {
    let shape = match options.cursor.shape {
        CursorShape::Block => Shape::Block,
        CursorShape::Beam => Shape::Beam,
        CursorShape::Underline => Shape::Underline,
    };
    term::Config {
        scrolling_history: options.scrollback,
        default_cursor_style: CursorStyle { shape, blinking: options.cursor.blink },
        ..Default::default()
    }
}

/// Forwards alacritty's requests and events to the session's [`TermHost`].
struct Listener(Arc<dyn TermHost>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        let host = &self.0;
        match event {
            Event::PtyWrite(text) => host.reply(&text),
            Event::Title(title) => host.event(TermEvent::Title { title }),
            Event::ResetTitle => host.event(TermEvent::ResetTitle),
            Event::Bell => host.event(TermEvent::Bell),
            Event::ClipboardStore(_, text) => host.event(TermEvent::Clipboard { text }),
            Event::ColorRequest(index, format) => {
                let c = host.color(index);
                host.reply(&format(Rgb { r: c.r, g: c.g, b: c.b }));
            },
            Event::TextAreaSizeRequest(format) => {
                let size = host.window_size();
                host.reply(&format(AlacrittySize {
                    num_lines: size.rows,
                    num_cols: size.cols,
                    cell_width: size.cell_width,
                    cell_height: size.cell_height,
                }));
            },
            _ => {},
        }
    }
}
