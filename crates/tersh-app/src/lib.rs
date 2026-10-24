//! The tersh application layer: use cases (a terminal [`session`], user
//! [`settings`], [`update`]s) and the [`ports`] they need. Adapters in `tersh-adapters`
//! implement the ports; UIs such as the Tauri app drive the use cases.

pub mod ports;
pub mod session;
pub mod settings;
pub mod update;

pub use ports::Sink;
pub use session::{Ports, RichInbox, Session, SessionOptions};
pub use settings::{ChangeDetector, Settings};
