//! Core of the tersh terminal: PTY hosting, VT emulation (via
//! `alacritty_terminal`), viewport serialisation and the rich-media protocol.
//! UI-agnostic; the Tauri app implements [`session::Sink`].

pub mod config;
pub mod frame;
pub mod rich;
pub mod scroll;
pub mod session;
pub mod theme;
#[cfg(unix)]
mod socket;

pub use config::Config;
pub use theme::Theme;
pub use frame::Frame;
pub use session::{Session, SessionOptions, Sink, TermEvent};
