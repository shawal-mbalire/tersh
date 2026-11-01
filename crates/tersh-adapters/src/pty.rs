//! PTY adapter: pseudo-terminals from `portable-pty`, and the system defaults
//! for what to run (`$SHELL`, the home directory).

use std::io::{Read, Write};
#[cfg(unix)]
use std::sync::Arc;

use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tersh_app::ports::{ChildProcess, ProcessKiller, PtyControl, PtyProcess, PtySystem, SpawnRequest};
use tersh_domain::error::{AppError, Code, Result};
use tersh_domain::WindowSize;

/// The platform's native pseudo-terminals.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePty;

/// The shell to run when neither the config nor the environment names one
/// (the entry point reads `$SHELL` / `%COMSPEC%` into `SpawnRequest::shell`).
pub const FALLBACK_SHELL: &str = if cfg!(windows) { "powershell.exe" } else { "/bin/sh" };

/// The user's shell named by an environment (`$SHELL`, or `%COMSPEC%` on Windows).
pub fn shell_from(env: &[(String, String)]) -> Option<String> {
    let key = if cfg!(windows) { "COMSPEC" } else { "SHELL" };
    env.iter().find(|(k, v)| k == key && !v.is_empty()).map(|(_, v)| v.clone())
}

impl PtySystem for NativePty {
    fn spawn(&self, request: &SpawnRequest) -> Result<PtyProcess> {
        let pty = portable_pty::native_pty_system()
            .openpty(PtySize {
                rows: request.rows,
                cols: request.cols,
                pixel_width: request.cols * request.cell_width,
                pixel_height: request.rows * request.cell_height,
            })
            .map_err(|e| AppError::new(Code::PtyOpen, "could not open a pseudo-terminal").because(e))?;

        let program = request.shell.clone().unwrap_or_else(|| FALLBACK_SHELL.into());
        let mut cmd = CommandBuilder::new(&program);
        cmd.args(&request.args);
        if let Some(cwd) = request.cwd.clone().or_else(dirs::home_dir) {
            cmd.cwd(cwd);
        }
        for key in &request.env_remove {
            cmd.env_remove(key);
        }
        for (key, value) in &request.env {
            cmd.env(key, value);
        }

        let child = pty
            .slave
            .spawn_command(cmd)
            .map_err(|e| AppError::new(Code::ShellStart, format!("could not start {program}")).because(e))?;
        drop(pty.slave);
        #[cfg(unix)]
        let (output, input, hangup): (Box<dyn Read + Send>, Box<dyn Write + Send>, _) = {
            let open = |e: std::io::Error| AppError::new(Code::PtyOpen, "could not open the pseudo-terminal").because(e);
            let fd = pty.master.as_raw_fd().ok_or_else(|| open(std::io::Error::other("the pty has no descriptor")))?;
            let terminal = Arc::new(unix::Terminal::new(fd).map_err(open)?);
            (Box::new(unix::Output(terminal.clone())), Box::new(unix::Input(terminal.clone())), terminal)
        };
        // On Windows the output ends when the pseudo console closes, which
        // happens when `Control` is dropped with the session.
        #[cfg(not(unix))]
        let (output, input, hangup) = {
            let open = |e| AppError::new(Code::PtyOpen, "could not open the pseudo-terminal").because(e);
            (pty.master.try_clone_reader().map_err(open)?, pty.master.take_writer().map_err(open)?, ())
        };
        Ok(PtyProcess {
            output,
            input,
            control: Box::new(Control { master: pty.master, hangup }),
            child: Box::new(Process(child)),
            program,
        })
    }
}

struct Control {
    master: Box<dyn MasterPty + Send>,
    #[cfg(unix)]
    hangup: Arc<unix::Terminal>,
    #[cfg(not(unix))]
    hangup: (),
}

impl PtyControl for Control {
    fn resize(&self, size: WindowSize) -> Result<()> {
        self.master.resize(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: size.cols * size.cell_width,
            pixel_height: size.rows * size.cell_height,
            })
            .map_err(|e| AppError::new(Code::PtyResize, "could not resize the terminal").because(e))
    }

    #[cfg(unix)]
    fn foreground_pid(&self) -> Option<u32> {
        self.master.process_group_leader().and_then(|pid| u32::try_from(pid).ok())
    }

    #[cfg(not(unix))]
    fn foreground_pid(&self) -> Option<u32> {
        None
    }

    #[cfg(unix)]
    fn hangup(&self) {
        self.hangup.hangup();
    }

    #[cfg(not(unix))]
    fn hangup(&self) {
        let () = self.hangup;
    }
}

struct Process(Box<dyn Child + Send + Sync>);

impl ChildProcess for Process {
    fn wait(&mut self) -> Option<u32> {
        self.0.wait().ok().map(|status| status.exit_code())
    }

    fn killer(&self) -> Box<dyn ProcessKiller> {
        Box::new(Killer(self.0.clone_killer()))
    }

    fn pid(&self) -> Option<u32> {
        self.0.process_id()
    }
}

struct Killer(Box<dyn ChildKiller + Send + Sync>);

impl ProcessKiller for Killer {
    fn kill(&mut self) {
        let _ = self.0.kill();
    }
}

/// Reading and writing the terminal without blocking forever.
///
/// A blocking read of the pty master only ends when the program writes or
/// when every copy of the terminal's other end is closed, and a program left
/// running in the background (`cmd &`, `nohup`) can hold one open long after
/// the shell has gone. So reads and writes wait for the terminal and for a
/// hang-up pipe together, and [`Terminal::hangup`] ends them.
#[cfg(unix)]
mod unix {
    use std::io::{self, PipeReader, PipeWriter, Read, Write};
    use std::os::fd::{AsRawFd, BorrowedFd, OwnedFd, RawFd};
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::{Arc, Mutex};

    pub struct Terminal {
        /// Our own non-blocking copy of the master.
        fd: OwnedFd,
        /// Closing the write end makes the read end ready for good.
        stop: PipeReader,
        stop_writer: Mutex<Option<PipeWriter>>,
        stopped: AtomicBool,
    }

    impl Terminal {
        pub fn new(master: RawFd) -> io::Result<Self> {
            // SAFETY: `master` is the open master of a pty we own for the duration of this call.
            let fd = unsafe { BorrowedFd::borrow_raw(master) }.try_clone_to_owned()?;
            // SAFETY: plain fcntl calls on a descriptor we own.
            unsafe {
                let flags = libc::fcntl(fd.as_raw_fd(), libc::F_GETFL);
                if flags < 0 || libc::fcntl(fd.as_raw_fd(), libc::F_SETFL, flags | libc::O_NONBLOCK) < 0 {
                    return Err(io::Error::last_os_error());
                }
            }
            let (stop, stop_writer) = io::pipe()?;
            Ok(Self { fd, stop, stop_writer: Mutex::new(Some(stop_writer)), stopped: AtomicBool::new(false) })
        }

        pub fn hangup(&self) {
            self.stopped.store(true, Ordering::SeqCst);
            self.stop_writer.lock().unwrap_or_else(|e| e.into_inner()).take();
        }

        fn stopped(&self) -> bool {
            self.stopped.load(Ordering::SeqCst)
        }

        /// Wait until the terminal can be read (or written); false once hung up.
        fn wait(&self, write: bool) -> io::Result<bool> {
            loop {
                match ready(self.fd.as_raw_fd(), write, self.stop.as_raw_fd()) {
                    Err(e) if e.kind() == io::ErrorKind::Interrupted => continue,
                    Err(e) => return Err(e),
                    Ok(stop) => return Ok(!stop && !self.stopped()),
                }
            }
        }
    }

    /// Block until `fd` is ready or `stop` is; returns whether `stop` is.
    #[cfg(not(any(target_os = "macos", target_os = "ios")))]
    fn ready(fd: RawFd, write: bool, stop: RawFd) -> io::Result<bool> {
        let events = if write { libc::POLLOUT } else { libc::POLLIN };
        let mut fds = [
            libc::pollfd { fd, events, revents: 0 },
            libc::pollfd { fd: stop, events: libc::POLLIN, revents: 0 },
        ];
        // SAFETY: `fds` is a valid array of two pollfds.
        if unsafe { libc::poll(fds.as_mut_ptr(), 2, -1) } < 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(fds[1].revents != 0)
    }

    /// macOS can't poll() terminals; select() works on them.
    #[cfg(any(target_os = "macos", target_os = "ios"))]
    fn ready(fd: RawFd, write: bool, stop: RawFd) -> io::Result<bool> {
        if fd.max(stop) >= libc::FD_SETSIZE as RawFd {
            return Err(io::Error::other("descriptor too large for select()"));
        }
        // SAFETY: both descriptors are below FD_SETSIZE and the sets are initialised by FD_ZERO.
        unsafe {
            let mut reads: libc::fd_set = std::mem::zeroed();
            let mut writes: libc::fd_set = std::mem::zeroed();
            libc::FD_ZERO(&mut reads);
            libc::FD_ZERO(&mut writes);
            libc::FD_SET(stop, &mut reads);
            libc::FD_SET(fd, if write { &mut writes } else { &mut reads });
            let r = libc::select(fd.max(stop) + 1, &mut reads, &mut writes, std::ptr::null_mut(), std::ptr::null_mut());
            if r < 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(libc::FD_ISSET(stop, &reads))
        }
    }

    /// What the program writes; ends (reads 0) on hang-up.
    pub struct Output(pub Arc<Terminal>);

    impl Read for Output {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            loop {
                if self.0.stopped() {
                    return Ok(0);
                }
                // SAFETY: `buf` is valid for `buf.len()` bytes.
                let n = unsafe { libc::read(self.0.fd.as_raw_fd(), buf.as_mut_ptr().cast(), buf.len()) };
                if n >= 0 {
                    return Ok(n as usize);
                }
                let err = io::Error::last_os_error();
                match err.kind() {
                    io::ErrorKind::Interrupted => {},
                    io::ErrorKind::WouldBlock => {
                        if !self.0.wait(false)? {
                            return Ok(0);
                        }
                    },
                    // EIO: every copy of the other end has closed.
                    _ => return Err(err),
                }
            }
        }
    }

    /// What the user types; fails with `BrokenPipe` after hang-up.
    pub struct Input(pub Arc<Terminal>);

    impl Write for Input {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            loop {
                if self.0.stopped() {
                    return Err(io::ErrorKind::BrokenPipe.into());
                }
                // SAFETY: `buf` is valid for `buf.len()` bytes.
                let n = unsafe { libc::write(self.0.fd.as_raw_fd(), buf.as_ptr().cast(), buf.len()) };
                if n >= 0 {
                    return Ok(n as usize);
                }
                let err = io::Error::last_os_error();
                match err.kind() {
                    io::ErrorKind::Interrupted => {},
                    io::ErrorKind::WouldBlock => {
                        if !self.0.wait(true)? {
                            return Err(io::ErrorKind::BrokenPipe.into());
                        }
                    },
                    _ => return Err(err),
                }
            }
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }
}
