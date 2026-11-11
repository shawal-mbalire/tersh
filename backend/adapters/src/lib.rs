//! Adapters for tersh's ports:
//!
//! - [`pty`]: native pseudo-terminals
//! - [`emulator`]: VT emulation by alacritty
//! - [`socket`]: the rich-message Unix socket
//! - [`config_fs`]: the user's settings directory
//! - [`fonts`]: the system font directories
//! - [`processes`]: running processes (foreground program, working directory)
//! - [`update`]: GitHub releases and the installer
//! - [`binary`]: the app's own executable, watched for updates
//!
//! [`LocalSessions`] (and [`spawn_local`]) wire the native ones into a session, and [`unbundled`]
//! keeps the AppImage's environment out of the programs tersh starts.

pub mod binary;
pub mod clock;
pub mod config_fs;
pub mod emulator;
pub mod fonts;
pub mod processes;
pub mod pty;
#[cfg(unix)]
pub mod socket;
pub mod toml_format;
pub mod update;

use std::process::Command;
use std::sync::Arc;

use tersh_domain::error::{AppError, Code, Result};
use tersh_app::instrumented::Instrumented;
use tersh_app::ports::{SessionLauncher, TimePort};
use tersh_app::{Ports, Session, SessionOptions, Sink};

/// The process environment, as read once by the entry point.
pub type EnvVars = Vec<(String, String)>;

/// Give `command` the user's environment rather than the AppImage launcher's
/// (see [`tersh_domain::env::without_bundle`]). `env` is this process's environment.
pub fn unbundled<'a>(command: &'a mut Command, env: &[(String, String)]) -> &'a mut Command {
    let cleanup = tersh_domain::env::without_bundle(env);
    for key in &cleanup.remove {
        command.env_remove(key);
    }
    command.envs(cleanup.set)
}

pub use clock::SystemClock;
pub use config_fs::FsConfigStore;
pub use emulator::AlacrittyEmulators;
pub use fonts::SystemFontDirs;
pub use processes::SystemProcesses;
pub use pty::NativePty;

/// Start a session on this machine: a native PTY, alacritty emulation and,
/// when `socket` is set (Unix only), a rich-message socket advertised to the
/// program as `$TERSH_SOCKET`.
pub fn spawn_local(mut options: SessionOptions, socket: bool, sink: Arc<dyn Sink>) -> Result<Session> {
    #[cfg(unix)]
    let bound = if socket {
        socket::BoundSocket::bind().map_err(|e| log::warn!("rich socket: {e}")).ok()
    } else {
        None
    };
    #[cfg(unix)]
    if let Some(bound) = &bound {
        options.rich_socket = Some(bound.path().to_path_buf());
    }
    #[cfg(not(unix))]
    let _ = (socket, &mut options);

    let clock: Arc<dyn TimePort> = Arc::new(SystemClock);
    let emulator = Instrumented::new(AlacrittyEmulators, clock.clone());
    let mut session = Session::spawn(options, Ports { pty: &NativePty, emulator: &emulator, clock }, sink)?;

    #[cfg(unix)]
    if let Some(bound) = bound {
        let server = bound
            .serve(session.rich_inbox())
            .map_err(|e| AppError::new(Code::SocketServe, "could not serve the rich socket").because(e))?;
        session.keep_alive(Box::new(server));
    }
    Ok(session)
}

/// The native [`SessionLauncher`]: [`spawn_local`] behind the port.
#[derive(Debug, Default, Clone, Copy)]
pub struct LocalSessions;

impl SessionLauncher for LocalSessions {
    fn launch(&self, options: SessionOptions, rich: bool, sink: Arc<dyn Sink>) -> Result<Session> {
        spawn_local(options, rich, sink)
    }
}
