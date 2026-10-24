//! Socket adapter: rich messages over a local Unix socket, as newline-delimited
//! JSON with the same schema as the OSC payloads. Suited to large payloads and
//! to tools (like Neovim plugins) that would rather not write escape sequences
//! through the PTY. Each message is answered with one JSON line.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::os::unix::fs::PermissionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::thread;

use tersh_app::RichInbox;
use tersh_domain::rich::MAX_PAYLOAD;

static NEXT_ID: AtomicUsize = AtomicUsize::new(0);

/// A bound socket, not yet serving. Bind before starting the session so its
/// path can be given to the program; serve once the session exists.
pub struct BoundSocket {
    listener: UnixListener,
    path: PathBuf,
}

/// A serving socket. Dropping it stops the server and removes the socket file.
pub struct SocketServer {
    path: PathBuf,
}

impl BoundSocket {
    /// Bind a private (mode 0600) socket in the runtime directory.
    pub fn bind() -> io::Result<Self> {
        let dir = dirs::runtime_dir().unwrap_or_else(std::env::temp_dir);
        let id = NEXT_ID.fetch_add(1, Ordering::Relaxed);
        let path = dir.join(format!("tersh-{}-{id}.sock", std::process::id()));
        let _ = std::fs::remove_file(&path);
        let listener = UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        Ok(Self { listener, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn serve(self, inbox: RichInbox) -> io::Result<SocketServer> {
        let Self { listener, path } = self;
        thread::Builder::new().name("tersh-socket".into()).spawn(move || accept_loop(listener, inbox))?;
        Ok(SocketServer { path })
    }
}

impl Drop for SocketServer {
    fn drop(&mut self) {
        // Unblock the accept loop, which then sees the closed inbox and exits.
        let _ = UnixStream::connect(&self.path);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn accept_loop(listener: UnixListener, inbox: RichInbox) {
    for stream in listener.incoming() {
        if !inbox.is_open() {
            break;
        }
        let Ok(stream) = stream else { continue };
        let inbox = inbox.clone();
        let _ = thread::Builder::new().name("tersh-socket-conn".into()).spawn(move || handle(stream, inbox));
    }
}

fn handle(stream: UnixStream, inbox: RichInbox) {
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
        let Some(reply) = inbox.handle(&line) else { return };
        let mut reply = reply.to_string();
        reply.push('\n');
        if out.write_all(reply.as_bytes()).is_err() {
            return;
        }
    }
}
