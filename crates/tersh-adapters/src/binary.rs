//! The app's own executable: watched so a running terminal notices when an
//! update replaced it, and relaunched into the new version.

use std::path::{Path, PathBuf};
use std::process::Command;

use tersh_app::ports::{Fingerprint, Watchable};

use crate::config_fs::modified_times;

/// The file an update replaces: the AppImage when running from one, else the executable.
#[derive(Debug, Clone)]
pub struct AppBinary {
    path: PathBuf,
}

impl AppBinary {
    pub fn current() -> Option<Self> {
        let path = std::env::var_os("APPIMAGE")
            .map(PathBuf::from)
            // Inside an AppImage the executable is a temporary extracted copy; the
            // installed AppImage is the file updates replace.
            .or_else(|| std::env::var_os("APPDIR").and(crate::update::installed_appimage()))
            .or_else(|| std::env::current_exe().ok())?;
        Some(Self { path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Version of the binary on disk now (which may be newer than the running one).
    pub fn version(&self) -> Option<String> {
        let output = Command::new(&self.path).arg("--version").output().ok()?;
        let text = String::from_utf8(output.stdout).ok()?;
        text.lines().next().map(|l| l.trim().trim_start_matches("tersh").trim().to_string()).filter(|v| !v.is_empty())
    }

    /// Start the binary on disk with `args`, detached from this process.
    pub fn launch(&self, args: &[String]) -> std::io::Result<()> {
        let mut command = Command::new(&self.path);
        command.args(args);
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // Its own session, so it survives this process exiting.
            command.process_group(0);
        }
        command.spawn().map(drop)
    }
}

impl Watchable for AppBinary {
    fn fingerprint(&self) -> Fingerprint {
        modified_times(vec![self.path.clone()])
    }
}
