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
//! [`spawn_local`] wires the native ones into a session, and [`unbundled`]
//! keeps the AppImage's environment out of the programs tersh starts.

pub mod binary;
pub mod config_fs;
pub mod emulator;
pub mod fonts;
pub mod processes;
pub mod pty;
#[cfg(unix)]
pub mod socket;
pub mod update;

use std::process::Command;
use std::sync::Arc;

use anyhow::Result;
use tersh_app::{Ports, Session, SessionOptions, Sink};

/// Give `command` the user's environment rather than the AppImage launcher's
/// (see [`tersh_domain::env::without_bundle`]).
pub fn unbundled(command: &mut Command) -> &mut Command {
    let cleanup = tersh_domain::env::without_bundle(&std::env::vars().collect::<Vec<_>>());
    for key in &cleanup.remove {
        command.env_remove(key);
    }
    command.envs(cleanup.set)
}

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

    let mut session = Session::spawn(options, Ports { pty: &NativePty, emulator: &AlacrittyEmulators }, sink)?;

    #[cfg(unix)]
    if let Some(bound) = bound {
        let server = bound.serve(session.rich_inbox())?;
        session.keep_alive(Box::new(server));
    }
    Ok(session)
}
