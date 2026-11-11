//! Emulator adapter: VT emulation by `alacritty_terminal`.

mod frame;
mod scroll;

use std::sync::{Arc, Mutex};
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
        let events = Arc::new(Mutex::new(Vec::new()));
        Box::new(AlacrittyEmulator {
            term: Term::new(term_config(options), &size, Listener(events.clone())),
            parser: Processor::new(),
            scroll: ScrollTracker::default(),
            max_history: options.scrollback,
            host,
            events,
        })
    }
}

pub struct AlacrittyEmulator {
    term: Term<Listener>,
    parser: Processor,
    scroll: ScrollTracker,
    max_history: usize,
    host: Arc<dyn TermHost>,
    events: Arc<Mutex<Vec<Event>>>,
}

impl AlacrittyEmulator {
    fn dispatch_events(&self) {
        let mut events = self.events.lock().unwrap_or_else(|e| e.into_inner());
        for event in events.drain(..) {
            let host = &self.host;
            match event {
                Event::PtyWrite(text) => host.reply(&text),
                Event::Title(title) => host.event(TermEvent::Title { title }),
                Event::ResetTitle => host.event(TermEvent::ResetTitle),
                Event::Bell => host.event(TermEvent::Bell),
                Event::ClipboardStore(_, text) => host.event(TermEvent::Clipboard { text }),
                Event::ColorRequest(index, format) => {
                    let c = self.term.colors()[index]
                        .map(|c| tersh_domain::Rgb { r: c.r, g: c.g, b: c.b })
                        .unwrap_or_else(|| host.color(index));
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
}

impl Emulator for AlacrittyEmulator {
    fn advance(&mut self, bytes: &[u8]) {
        self.parser.advance(&mut self.term, bytes);
        self.scroll.update(&self.term, self.max_history);
        self.dispatch_events();
    }

    fn sync_deadline(&self) -> Option<Instant> {
        self.parser.sync_timeout().sync_timeout()
    }

    fn flush_sync(&mut self) {
        self.parser.stop_sync(&mut self.term);
        self.scroll.update(&self.term, self.max_history);
        self.dispatch_events();
    }

    fn resize(&mut self, cols: u16, rows: u16) {
        self.term.resize(TermSize::new(cols as usize, rows as usize));
        self.scroll.invalidate(&self.term);
        self.dispatch_events();
    }

    fn scroll(&mut self, scroll: Scroll) {
        self.term.scroll_display(match scroll {
            Scroll::Lines(lines) => GridScroll::Delta(lines),
            Scroll::Bottom => GridScroll::Bottom,
        });
        self.dispatch_events();
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

    fn mode(&self) -> u32 {
        self.term.mode().bits()
    }

    fn frame(&mut self, full: bool) -> Frame {
        frame::build(&mut self.term, self.scroll.base(), full)
    }

    fn set_options(&mut self, options: &EmulatorOptions) {
        self.max_history = options.scrollback;
        self.term.set_options(term_config(options));
        self.dispatch_events();
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
        // Lets programs that ask for it (Neovim does at startup) tell keys like
        // Shift+Enter and Ctrl+Enter apart from Enter. Others get legacy bytes.
        kitty_keyboard: true,
        ..Default::default()
    }
}

/// Queues alacritty's requests and events so [`AlacrittyEmulator`] can inspect
/// [`Term`] state (such as dynamic OSC 4/10/11/12 colours) while answering them.
struct Listener(Arc<Mutex<Vec<Event>>>);

impl EventListener for Listener {
    fn send_event(&self, event: Event) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tersh_domain::config::Cursor;
    use tersh_domain::WindowSize;

    #[derive(Default)]
    struct RecordingHost {
        replies: Mutex<Vec<String>>,
    }

    impl TermHost for RecordingHost {
        fn reply(&self, text: &str) {
            self.replies.lock().unwrap().push(text.to_owned());
        }
        fn event(&self, _: TermEvent) {}
        fn color(&self, _: usize) -> tersh_domain::Rgb {
            tersh_domain::Rgb { r: 0x10, g: 0x20, b: 0x30 }
        }
        fn window_size(&self) -> WindowSize {
            WindowSize { cols: 80, rows: 24, cell_width: 8, cell_height: 16 }
        }
    }

    #[test]
    fn color_requests_reflect_dynamic_osc_colors_and_keep_reply_order() {
        let host = Arc::new(RecordingHost::default());
        let options = EmulatorOptions { cols: 80, rows: 24, scrollback: 100, cursor: Cursor::default() };
        let mut emu = AlacrittyEmulators.create(&options, host.clone());

        // Set background (OSC 11) to #aabbcc, query OSC 11 followed by DSR (CSI 6 n), then reset (OSC 111) and query again.
        emu.advance(b"\x1b]11;rgb:aa/bb/cc\x07\x1b]11;?\x07\x1b[6n");
        emu.advance(b"\x1b]111\x07\x1b]11;?\x07");

        let replies = host.replies.lock().unwrap();
        assert_eq!(replies.len(), 3);
        assert!(replies[0].contains("rgb:aaaa/bbbb/cccc"), "got {:?}", replies[0]);
        assert_eq!(replies[1], "\x1b[1;1R");
        assert!(replies[2].contains("rgb:1010/2020/3030"), "got {:?}", replies[2]);
    }
}
