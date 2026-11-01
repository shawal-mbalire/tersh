//! The environment tersh passes to the programs it starts.
//!
//! The Linux AppImage's launcher points `PATH`, `LD_LIBRARY_PATH`,
//! `PYTHONHOME`, `XDG_DATA_DIRS`, `GTK_*` and more into the bundle so the app
//! finds its own libraries. A shell must not inherit that: system programs
//! would load the bundle's libraries (`grep: ...libpcre2-8.so.0: no version
//! information`, `symbol lookup error`). [`without_bundle`] undoes it.

/// Variables to remove and to set so a child gets the user's environment back.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct EnvCleanup {
    pub remove: Vec<String>,
    pub set: Vec<(String, String)>,
}

/// Set by the AppImage runtime, its launcher or tersh's wrapper; meaningless to children.
const BUNDLE_ONLY: [&str; 7] =
    ["APPDIR", "APPIMAGE", "ARGV0", "OWD", "APPIMAGE_GTK_THEME", "APPIMAGE_EXTRACT_AND_RUN", "TERSH_WRAPPER"];

/// Changes that strip the bundle from `vars` (the current environment).
///
/// Bundle paths are under `$APPDIR`, under any AppImage mount of tersh
/// (`/tmp/.mount_tersh*`, left over when one tersh restarted into another),
/// or under the installer's extracted copy (`~/.local/share/tersh/app`).
/// Path lists keep their other entries; a variable left empty is removed.
pub fn without_bundle(vars: &[(String, String)]) -> EnvCleanup {
    let get = |key: &str| vars.iter().find(|(k, _)| k == key).map(|(_, v)| v.as_str());
    let mut roots: Vec<String> = Vec::new();
    if let Some(appdir) = get("APPDIR").filter(|d| !d.is_empty()) {
        roots.push(appdir.trim_end_matches('/').to_string());
    }
    let data_home = get("XDG_DATA_HOME")
        .filter(|d| !d.is_empty())
        .map(str::to_string)
        .or_else(|| get("HOME").map(|home| format!("{home}/.local/share")));
    if let Some(data_home) = data_home {
        roots.push(format!("{}/tersh/app", data_home.trim_end_matches('/')));
    }
    let in_bundle = |entry: &str| {
        entry.contains("/.mount_tersh")
            || roots.iter().any(|root| entry.strip_prefix(root.as_str()).is_some_and(|rest| rest.is_empty() || rest.starts_with('/')))
    };

    let mut cleanup = EnvCleanup::default();
    let wrapper_theme = get("APPIMAGE_GTK_THEME");
    for (key, value) in vars {
        if BUNDLE_ONLY.contains(&key.as_str()) || (key == "GTK_THEME" && Some(value.as_str()) == wrapper_theme) {
            cleanup.remove.push(key.clone());
            continue;
        }
        let entries: Vec<&str> = value.split(':').collect();
        if !entries.iter().any(|e| in_bundle(e)) {
            continue;
        }
        let kept: Vec<&str> = entries.into_iter().filter(|e| !e.is_empty() && !in_bundle(e)).collect();
        if kept.is_empty() {
            cleanup.remove.push(key.clone());
        } else {
            cleanup.set.push((key.clone(), kept.join(":")));
        }
    }
    cleanup
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(list: &[(&str, &str)]) -> Vec<(String, String)> {
        list.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect()
    }

    #[test]
    fn leaves_a_normal_environment_alone() {
        let env = vars(&[("HOME", "/home/u"), ("PATH", "/usr/bin:/bin"), ("GTK_THEME", "Adwaita")]);
        assert_eq!(without_bundle(&env), EnvCleanup::default());
    }

    #[test]
    fn strips_the_appimage_launcher() {
        let app = "/home/u/.local/share/tersh/app";
        let env = vars(&[
            ("HOME", "/home/u"),
            ("APPDIR", app),
            ("PATH", &format!("{app}/usr/bin/:{app}/usr/sbin/:/usr/local/bin:/usr/bin")),
            ("LD_LIBRARY_PATH", &format!("{app}/usr/lib/:{app}/usr/lib/x86_64-linux-gnu/")),
            ("PYTHONPATH", &format!("{app}/usr/share/pyshared/:")),
            ("GTK_EXE_PREFIX", &format!("{app}//usr")),
            ("XDG_DATA_DIRS", &format!("{app}/usr/share/:/usr/share:/var/lib/flatpak/exports/share")),
            ("GTK_THEME", "tersh"),
            ("APPIMAGE_GTK_THEME", "tersh"),
            ("TERSH_WRAPPER", "/home/u/.local/bin/tersh"),
            ("EDITOR", "nvim"),
        ]);
        let cleanup = without_bundle(&env);
        let mut removed = cleanup.remove.clone();
        removed.sort();
        assert_eq!(
            removed,
            ["APPDIR", "APPIMAGE_GTK_THEME", "GTK_EXE_PREFIX", "GTK_THEME", "LD_LIBRARY_PATH", "PYTHONPATH", "TERSH_WRAPPER"]
        );
        assert_eq!(
            cleanup.set,
            vars(&[("PATH", "/usr/local/bin:/usr/bin"), ("XDG_DATA_DIRS", "/usr/share:/var/lib/flatpak/exports/share")])
        );
    }

    #[test]
    fn strips_stale_mounts_from_an_earlier_tersh() {
        let env = vars(&[
            ("HOME", "/home/u"),
            ("APPDIR", "/tmp/.mount_tershAbC123"),
            ("PATH", "/tmp/.mount_tershAbC123/usr/bin:/home/u/.local/share/tersh/app/usr/bin:/tmp/.mount_tershXyZ/usr/bin:/usr/bin"),
        ]);
        assert_eq!(without_bundle(&env).set, vars(&[("PATH", "/usr/bin")]));
    }

    #[test]
    fn keeps_lookalike_paths() {
        let env = vars(&[("HOME", "/home/u"), ("PATH", "/home/u/.local/share/tersh/apps/bin:/usr/bin")]);
        assert_eq!(without_bundle(&env), EnvCleanup::default());
    }
}
