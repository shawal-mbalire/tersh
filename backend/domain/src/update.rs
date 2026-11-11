//! Updating: comparing versions, and deciding when a running terminal can
//! restart into a newly installed version without getting in the way.

use std::cmp::Ordering;
use std::fmt;

/// A release version, `MAJOR.MINOR.PATCH`. Tolerates a `v` prefix, a leading
/// program name (`tersh 0.2.0`) and pre-release or build suffixes, which are
/// ignored.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Version {
    pub major: u64,
    pub minor: u64,
    pub patch: u64,
}

impl Version {
    pub fn parse(text: &str) -> Option<Self> {
        let word = text.split_whitespace().last()?;
        let core = word.trim_start_matches(['v', 'V']).split(['-', '+']).next()?;
        let mut parts = core.split('.').map(|p| p.parse::<u64>().ok());
        let major = parts.next()??;
        let minor = parts.next().unwrap_or(Some(0))?;
        let patch = parts.next().unwrap_or(Some(0))?;
        if parts.next().is_some() {
            return None;
        }
        Some(Self { major, minor, patch })
    }
}

impl fmt::Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Whether `latest` is newer than `current`. Unparseable versions never count as newer.
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (Version::parse(latest), Version::parse(current)) {
        (Some(latest), Some(current)) => latest.cmp(&current) == Ordering::Greater,
        _ => false,
    }
}

/// What is running in a terminal, as far as restarting is concerned.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Foreground {
    pub shell_pid: Option<u32>,
    /// Leader of the terminal's foreground process group.
    pub pid: Option<u32>,
    /// Its program name, e.g. `nvim` or `tmux: client`.
    pub name: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Restart {
    /// Safe to restart; type `input` into the new terminal's shell (e.g. to re-attach tmux).
    Now { input: Option<String> },
    /// Something is running; restarting would kill it.
    Wait { busy: String },
}

/// Multiplexers whose sessions outlive the terminal, and how to get back to them.
const REATTACH: &[(&str, &str)] = &[("tmux", "tmux attach\r"), ("zellij", "zellij attach\r")];

/// Decide whether a terminal can restart now.
///
/// The shell sitting at its prompt (it is the foreground process) is safe.
/// So is a terminal multiplexer client: its sessions keep running, and the
/// new terminal re-attaches. Anything else (an editor, a build) means waiting.
pub fn restart_decision(foreground: &Foreground) -> Restart {
    let Some(pid) = foreground.pid else { return Restart::Now { input: None } };
    if foreground.shell_pid == Some(pid) {
        return Restart::Now { input: None };
    }
    let name = foreground.name.clone().unwrap_or_else(|| format!("process {pid}"));
    let program = name.split([':', ' ']).next().unwrap_or_default();
    match REATTACH.iter().find(|(multiplexer, _)| *multiplexer == program) {
        Some((_, input)) => Restart::Now { input: Some((*input).to_string()) },
        None => Restart::Wait { busy: name },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_versions() {
        let v = |major, minor, patch| Some(Version { major, minor, patch });
        assert_eq!(Version::parse("v0.2.0"), v(0, 2, 0));
        assert_eq!(Version::parse("tersh 1.10.3"), v(1, 10, 3));
        assert_eq!(Version::parse("2.1"), v(2, 1, 0));
        assert_eq!(Version::parse("v0.3.0-rc.1"), v(0, 3, 0));
        assert_eq!(Version::parse("nope"), None);
        assert_eq!(Version::parse("1.2.3.4"), None);
        assert_eq!(v(0, 2, 0).unwrap().to_string(), "0.2.0");
    }

    #[test]
    fn compares_versions() {
        assert!(is_newer("v0.10.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.9", "0.2.0"));
        assert!(!is_newer("garbage", "0.2.0"));
    }

    #[test]
    fn restarts_only_when_safe() {
        let fg = |pid: Option<u32>, name: Option<&str>| Foreground { shell_pid: Some(10), pid, name: name.map(String::from) };
        assert_eq!(restart_decision(&fg(Some(10), Some("nu"))), Restart::Now { input: None });
        assert_eq!(restart_decision(&fg(None, None)), Restart::Now { input: None });
        assert_eq!(restart_decision(&fg(Some(42), Some("tmux: client"))), Restart::Now { input: Some("tmux attach\r".into()) });
        assert_eq!(restart_decision(&fg(Some(42), Some("zellij"))), Restart::Now { input: Some("zellij attach\r".into()) });
        assert_eq!(restart_decision(&fg(Some(42), Some("nvim"))), Restart::Wait { busy: "nvim".into() });
        assert_eq!(restart_decision(&fg(Some(42), None)), Restart::Wait { busy: "process 42".into() });
    }
}
