//! Update adapters: releases from GitHub, installed by the project's install
//! script (the same one as `curl ... install.sh | sh`). Both use `curl`,
//! which every supported system has.

use std::path::PathBuf;
use std::process::{Command, Stdio};

use anyhow::{bail, Context, Result};
use tersh_app::ports::{Installer, ReleaseSource};

pub const REPO: &str = "shawal-mbalire/tersh";

/// Releases of a GitHub repository.
#[derive(Debug, Clone)]
pub struct GithubReleases {
    pub repo: String,
}

impl Default for GithubReleases {
    fn default() -> Self {
        Self { repo: REPO.into() }
    }
}

impl ReleaseSource for GithubReleases {
    fn latest(&self) -> Result<String> {
        let url = format!("https://api.github.com/repos/{}/releases/latest", self.repo);
        let output = Command::new("curl")
            .args(["-fsSL", "-H", "Accept: application/vnd.github+json", &url])
            .output()
            .context("running curl (is it installed?)")?;
        if !output.status.success() {
            bail!("could not reach {url}: {}", String::from_utf8_lossy(&output.stderr).trim());
        }
        let release: serde_json::Value = serde_json::from_slice(&output.stdout).context("unexpected reply from GitHub")?;
        release["tag_name"].as_str().map(String::from).context("the latest release has no tag")
    }
}

/// Runs the installer from the release being installed: `install.sh` on
/// macOS and Linux, `install.ps1` on Windows. It updates every component the
/// first install put in place (the installer remembers opt-outs).
#[derive(Debug, Clone)]
pub struct InstallScript {
    pub repo: String,
}

impl Default for InstallScript {
    fn default() -> Self {
        Self { repo: REPO.into() }
    }
}

impl InstallScript {
    /// The command that runs the installer for `tag` on this OS.
    fn command(&self, tag: &str) -> Command {
        let base = format!("https://raw.githubusercontent.com/{}/{tag}", self.repo);
        if cfg!(windows) {
            let mut command = Command::new("powershell");
            command
                .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command"])
                .arg(format!("irm '{base}/install.ps1' | iex"))
                .env("TERSH_VERSION", tag);
            command
        } else {
            let mut command = Command::new("sh");
            command.args(["-c", &format!("curl -fsSL '{base}/install.sh' | sh -s -- --version {tag}")]);
            command
        }
    }
}

impl Installer for InstallScript {
    fn install(&self, tag: &str) -> Result<()> {
        let mut command = self.command(tag);
        // Without the bundle's PATH, the installer can't mistake the app's own
        // binary for another copy of tersh to replace.
        let status = crate::unbundled(&mut command)
            .stdin(Stdio::null())
            .status()
            .context("running the installer")?;
        if !status.success() {
            bail!("the installer failed ({status})");
        }
        Ok(())
    }
}

/// Where the installer keeps the AppImage on Linux, to find the binary when
/// `tersh` was started some other way.
pub fn installed_appimage() -> Option<PathBuf> {
    dirs::data_dir().map(|d| d.join("tersh/tersh.AppImage")).filter(|p| p.exists())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Talks to GitHub: `cargo test -p tersh-adapters -- --ignored`.
    #[test]
    #[ignore]
    fn finds_the_latest_release() {
        let tag = GithubReleases::default().latest().unwrap();
        assert!(tersh_domain::update::Version::parse(&tag).is_some(), "{tag}");
    }
}
