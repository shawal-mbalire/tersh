//! PTY adapter: pseudo-terminals from `portable-pty`, and the system defaults
//! for what to run (`$SHELL`, the home directory).

use anyhow::{Context, Result};
use portable_pty::{Child, ChildKiller, CommandBuilder, MasterPty, PtySize};
use tersh_app::ports::{ChildProcess, ProcessKiller, PtyControl, PtyProcess, PtySystem, SpawnRequest};
use tersh_domain::WindowSize;

/// The platform's native pseudo-terminals.
#[derive(Debug, Default, Clone, Copy)]
pub struct NativePty;

/// The shell to run when the config doesn't name one.
pub fn default_shell() -> String {
    if cfg!(windows) {
        std::env::var("COMSPEC").unwrap_or_else(|_| "powershell.exe".into())
    } else {
        std::env::var("SHELL").unwrap_or_else(|_| "/bin/sh".into())
    }
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
            .context("opening pty")?;

        let program = request.shell.clone().unwrap_or_else(default_shell);
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

        let child = pty.slave.spawn_command(cmd).with_context(|| format!("starting {program}"))?;
        drop(pty.slave);
        Ok(PtyProcess {
            output: pty.master.try_clone_reader()?,
            input: pty.master.take_writer()?,
            control: Box::new(Control(pty.master)),
            child: Box::new(Process(child)),
            program,
        })
    }
}

struct Control(Box<dyn MasterPty + Send>);

impl PtyControl for Control {
    fn resize(&self, size: WindowSize) -> Result<()> {
        self.0.resize(PtySize {
            rows: size.rows,
            cols: size.cols,
            pixel_width: size.cols * size.cell_width,
            pixel_height: size.rows * size.cell_height,
        })
    }

    #[cfg(unix)]
    fn foreground_pid(&self) -> Option<u32> {
        self.0.process_group_leader().and_then(|pid| u32::try_from(pid).ok())
    }

    #[cfg(not(unix))]
    fn foreground_pid(&self) -> Option<u32> {
        None
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
