//! The process environment, read once at startup. This is the only place the
//! desktop app reads environment variables; everything else gets these values
//! passed in.

use std::path::PathBuf;

use tersh_adapters::EnvVars;

/// What the app takes from its environment.
#[derive(Debug, Clone)]
pub struct Environment {
    /// Every variable, for working out what a launched program should inherit.
    pub vars: EnvVars,
    /// The user's shell (`$SHELL`, or `%COMSPEC%` on Windows), when set.
    pub shell: Option<String>,
    /// `%WINDIR%`, for the system font directory on Windows.
    pub windows_dir: Option<PathBuf>,
    pub current_exe: Option<PathBuf>,
}

impl Environment {
    pub fn read() -> Self {
        let vars: EnvVars = std::env::vars().collect();
        Self {
            shell: tersh_adapters::pty::shell_from(&vars),
            windows_dir: std::env::var_os("WINDIR").map(PathBuf::from),
            current_exe: std::env::current_exe().ok(),
            vars,
        }
    }
}
