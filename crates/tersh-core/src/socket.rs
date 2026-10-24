//! Local socket for rich messages: newline-delimited JSON, same schema as the
//! OSC payloads. Suited to large payloads and to tools (like Neovim plugins)
//! that would rather not write escape sequences through the PTY.
//!
//! Each message is answered with one JSON line (see `handle_socket_message`).

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Weak;
use std::thread;

use crate::rich::MAX_PAYLOAD;
use crate::session::Shared;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

pub(crate) fn bind() -> io::Result<(UnixListener, PathBuf)> {
    let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
    let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
    let path = dir.join(format!("tersh-{}-{id}.sock", std::process::id()));
    let _ = std::fs::remove_file(&path);
    let listener = UnixListener::bind(&path)?;
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
    Ok((listener, path))
}

pub(crate) fn serve(listener: UnixListener, shared: Weak<Shared>) {
    for stream in listener.incoming() {
        match shared.upgrade() {
            Some(s) if !s.is_closed() => {},
            _ => break,
        }
        let Ok(stream) = stream else { continue };
        let shared = shared.clone();
        let _ = thread::Builder::new().name("tersh-socket-conn".into()).spawn(move || handle(stream, shared));
    }
}

fn handle(stream: UnixStream, shared: Weak<Shared>) {
    let Ok(mut out) = stream.try_clone() else { return };
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    loop {
        line.clear();
        match (&mut reader).take(MAX_PAYLOAD as u64 + 1).read_until(b'\n', &mut line) {
            Ok(0) | Err(_) => return,
            Ok(_) => {},
        }
        if line.len() > MAX_PAYLOAD {
            log::warn!("rich socket: message too large, closing connection");
            return;
        }
        if line.trim_ascii().is_empty() {
            continue;
        }
        let Some(shared) = shared.upgrade() else { return };
        let mut reply = shared.handle_socket_message(&line).to_string();
        reply.push('\n');
        if out.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}

/// Unblock the accept loop and remove the socket file.
pub(crate) fn shutdown(path: &Path) {
    let _ = UnixStream::connect(path);
    let _ = std::fs::remove_file(path);
}
