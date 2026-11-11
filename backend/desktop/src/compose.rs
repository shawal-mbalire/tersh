//! The composition root's one switchboard: every adapter the desktop app runs
//! on is chosen here and handed to the rest of the crate as a trait object.
//! Commands, the config watcher and the updater only ever see the ports, so
//! swapping an implementation (an in-memory config store, a remote session, a
//! different emulator) is a change to this file alone.

use std::sync::Arc;

use tersh_adapters::{FsConfigStore, LocalSessions, SystemFontDirs, SystemProcesses};
use tersh_app::ports::{ConfigStore, ProcessInspector, SessionLauncher, Watchable};

use crate::environment::Environment;

/// The driven adapters behind the desktop app.
#[derive(Clone)]
pub struct Adapters {
    /// The user's settings directory: config, themes, style sheet, plugins.
    pub store: Arc<dyn ConfigStore>,
    /// Installed fonts, watched so the page can pick up new ones.
    pub fonts: Arc<dyn Watchable>,
    /// Foreground program, working directory and memory of processes.
    pub processes: Arc<dyn ProcessInspector>,
    /// Starts the session (PTY, emulator, clock, rich socket).
    pub sessions: Arc<dyn SessionLauncher>,
}

impl Adapters {
    /// The native adapters for this machine.
    pub fn native(env: &Environment) -> Self {
        Self {
            store: Arc::new(FsConfigStore::user()),
            fonts: Arc::new(SystemFontDirs::new(env.windows_dir.clone())),
            processes: Arc::new(SystemProcesses),
            sessions: Arc::new(LocalSessions),
        }
    }
}
