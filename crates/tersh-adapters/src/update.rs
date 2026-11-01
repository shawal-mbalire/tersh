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

/// Runs `install.sh` from the release being installed. Keeps the Neovim
/// plugin up to date too if the installer put it there before.
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
    fn nvim_plugin_installed() -> bool {
        dirs::data_dir().is_some_and(|d| d.join("nvim/site/pack/tersh/start/tersh.nvim").is_dir())
    }
}

impl Installer for InstallScript {
    fn install(&self, tag: &str) -> Result<()> {
        if cfg!(windows) {
            bail!("on Windows, install the new version from https://github.com/{}/releases/tag/{tag}", self.repo);
        }
        let script = format!("https://raw.githubusercontent.com/{}/{tag}/install.sh", self.repo);
        let mut args = vec!["--version".to_string(), tag.to_string()];
        if Self::nvim_plugin_installed() {
            args.push("--nvim".into());
        }
        let pipeline = format!("curl -fsSL '{script}' | sh -s -- {}", args.join(" "));
        // Without the bundle's PATH, the installer can't mistake the app's own
        // binary for another copy of tersh to replace.
        let status = crate::unbundled(&mut Command::new("sh"))
            .args(["-c", &pipeline])
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
