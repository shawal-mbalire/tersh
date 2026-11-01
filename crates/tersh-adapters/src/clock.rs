//! The system's monotonic clock.

use std::time::{Duration, Instant};

use tersh_app::ports::TimePort;

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemClock;

impl TimePort for SystemClock {
    fn now(&self) -> Instant {
        Instant::now()
    }

    fn sleep(&self, duration: Duration) {
        std::thread::sleep(duration);
    }
}
