//! The tersh domain: configuration and themes, the rich-card protocol, and
//! the frame model sent to renderers. Pure logic with no I/O; everything that
//! touches the outside world goes through ports in `tersh-app` and is
//! implemented in `tersh-adapters`.

pub mod config;
pub mod env;
pub mod error;
pub mod event;
pub mod frame;
pub mod input;
pub mod palette;
pub mod rich;
pub mod stats;
pub mod theme;
pub mod update;

pub use config::Config;
pub use error::{AppError, Code};
pub use event::{Info, TermEvent, WindowSize};
pub use frame::Frame;
pub use palette::{Palette, Rgb};
pub use stats::Stats;
pub use theme::{Theme, ThemeSource};
