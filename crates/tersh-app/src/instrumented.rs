//! Timing at the edge: a decorator that measures an emulator without the
//! session or the emulator knowing. The entry point wraps the real factory:
//! `Instrumented::new(AlacrittyEmulators, clock)`.

use std::sync::Arc;
use std::time::Instant;

use tersh_domain::rich::AnchorContext;
use tersh_domain::Frame;

use crate::ports::{Emulator, EmulatorFactory, EmulatorOptions, Measurement, Scroll, TermHost, TimePort};

/// Wraps an [`EmulatorFactory`] so every emulator it makes reports how long
/// parsing and frame building take to its host ([`TermHost::measured`]).
pub struct Instrumented<F> {
    inner: F,
    clock: Arc<dyn TimePort>,
}

impl<F: EmulatorFactory> Instrumented<F> {
    pub fn new(inner: F, clock: Arc<dyn TimePort>) -> Self {
        Self { inner, clock }
    }
}

impl<F: EmulatorFactory> EmulatorFactory for Instrumented<F> {
    fn create(&self, options: &EmulatorOptions, host: Arc<dyn TermHost>) -> Box<dyn Emulator> {
        let inner = self.inner.create(options, host.clone());
        Box::new(Measured { inner, host, clock: self.clock.clone() })
    }
}

struct Measured {
    inner: Box<dyn Emulator>,
    host: Arc<dyn TermHost>,
    clock: Arc<dyn TimePort>,
}

impl Emulator for Measured {
    fn advance(&mut self, bytes: &[u8]) {
        let start = self.clock.now();
        self.inner.advance(bytes);
        self.host.measured(Measurement::Advance(self.clock.now() - start));
    }

    fn frame(&mut self, full: bool) -> Frame {
        let start = self.clock.now();
        let frame = self.inner.frame(full);
        self.host.measured(Measurement::Frame(self.clock.now() - start));
        frame
    }

    fn sync_deadline(&self) -> Option<Instant> {
        self.inner.sync_deadline()
    }
    fn flush_sync(&mut self) {
        self.inner.flush_sync();
    }
    fn resize(&mut self, cols: u16, rows: u16) {
        self.inner.resize(cols, rows);
    }
    fn scroll(&mut self, scroll: Scroll) {
        self.inner.scroll(scroll);
    }
    fn anchor_context(&self) -> AnchorContext {
        self.inner.anchor_context()
    }
    fn alt_screen(&self) -> bool {
        self.inner.alt_screen()
    }
    fn set_options(&mut self, options: &EmulatorOptions) {
        self.inner.set_options(options);
    }
}
