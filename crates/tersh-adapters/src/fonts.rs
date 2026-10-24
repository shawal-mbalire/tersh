//! Fonts adapter: the system's font directories, watched so newly installed
//! or removed fonts are picked up without a restart.

use std::path::PathBuf;

use tersh_app::ports::{Fingerprint, Watchable};

use crate::config_fs::{files_in, modified_times};

/// The platform's font directories.
#[derive(Debug, Clone)]
pub struct SystemFontDirs {
    dirs: Vec<PathBuf>,
}

impl SystemFontDirs {
    pub fn new() -> Self {
        let home = dirs::home_dir();
        let mut dirs: Vec<PathBuf> = Vec::new();
        if cfg!(target_os = "macos") {
            dirs.extend(home.iter().map(|h| h.join("Library/Fonts")));
            dirs.extend(["/Library/Fonts", "/System/Library/Fonts"].map(PathBuf::from));
        } else if cfg!(windows) {
            dirs.extend(dirs::data_local_dir().map(|d| d.join("Microsoft/Windows/Fonts")));
            dirs.push(PathBuf::from(std::env::var("WINDIR").unwrap_or_else(|_| "C:\\Windows".into())).join("Fonts"));
        } else {
            dirs.extend(dirs::data_dir().map(|d| d.join("fonts")));
            dirs.extend(home.iter().map(|h| h.join(".fonts")));
            dirs.extend(["/usr/share/fonts", "/usr/local/share/fonts"].map(PathBuf::from));
        }
        Self { dirs }
    }
}

impl Default for SystemFontDirs {
    fn default() -> Self {
        Self::new()
    }
}

impl Watchable for SystemFontDirs {
    /// Directory times only (two levels deep): adding or removing a font file
    /// changes its directory's time, and it stays cheap with thousands of fonts.
    fn fingerprint(&self) -> Fingerprint {
        let mut paths = Vec::new();
        for dir in &self.dirs {
            paths.push(dir.clone());
            paths.extend(files_in(dir, 1).into_iter().filter(|p| p.is_dir()));
        }
        modified_times(paths)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn notices_new_fonts() {
        let root = std::env::temp_dir().join(format!("tersh-fonts-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Family")).unwrap();
        let fonts = SystemFontDirs { dirs: vec![root.clone()] };
        let before = fonts.fingerprint();
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("Family/New.ttf"), b"").unwrap();
        assert_ne!(before, fonts.fingerprint());
    }
}
