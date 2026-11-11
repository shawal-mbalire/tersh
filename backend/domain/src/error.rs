//! The one error type every layer returns for expected failures: a bad config
//! file, a shell that won't start, an unreachable release server. It carries
//! what went wrong (a registered [`Code`] and a message), why (the original
//! error as its `cause`), where (the source location that raised it) and
//! details for the log (`context`).
//!
//! Defects (a broken invariant, a bug) panic instead; they are never returned.
//! Adapters turn library errors into an `AppError` at the boundary, keeping the
//! library error as the cause. Every code is listed in `docs/errors.md`.

use std::error::Error;
use std::fmt;
use std::panic::Location;

use serde::{Serialize, Serializer};

pub type Result<T, E = AppError> = std::result::Result<T, E>;

/// What kind of failure a code is, which says who can fix it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Family {
    /// The input was invalid (a config file, a message, an argument).
    Validation,
    /// Something outside tersh failed (the OS, a program, the network).
    External,
    /// Something outside tersh answered in a way it shouldn't have.
    Contract,
}

macro_rules! codes {
    ($($name:ident = $code:literal, $family:ident, $remedy:literal;)*) => {
        /// Every error code. Listed with what to do about each in `docs/errors.md`.
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        pub enum Code { $($name),* }

        impl Code {
            pub const ALL: &'static [Code] = &[$(Code::$name),*];

            /// The stable code, e.g. `CFG-001`.
            pub fn as_str(self) -> &'static str {
                match self { $(Code::$name => $code),* }
            }

            pub fn family(self) -> Family {
                match self { $(Code::$name => Family::$family),* }
            }

            /// One line on how to fix it.
            pub fn remedy(self) -> &'static str {
                match self { $(Code::$name => $remedy),* }
            }
        }
    };
}

codes! {
    ConfigSyntax = "CFG-001", Validation, "Fix the TOML at the reported line in config.toml.";
    ConfigRead = "CFG-002", External, "Check that config.toml is readable.";
    ConfigWrite = "CFG-003", External, "Check that the config directory is writable.";
    ThemeSyntax = "THM-001", Validation, "Fix the TOML in the theme file.";
    ThemeRead = "THM-002", External, "Check that the theme file is readable.";
    RichPayload = "RCH-001", Validation, "Send a JSON object, raw or base64-encoded.";
    SocketServe = "SOC-001", External, "Check that the runtime directory is writable.";
    PtyOpen = "PTY-001", External, "The system may be out of pseudo-terminals.";
    ShellStart = "PTY-002", External, "Check `shell` in config.toml, or $SHELL.";
    PtyResize = "PTY-003", External, "The terminal may have closed.";
    InputClosed = "SES-001", External, "The program has exited; restart the terminal.";
    SessionStart = "SES-002", External, "The system may be out of threads or memory.";
    NoSession = "SES-003", Validation, "Start a session before sending it input.";
    ReleaseLookup = "UPD-001", External, "Check the network connection and that curl is installed.";
    ReleaseReply = "UPD-002", Contract, "GitHub's reply changed; report it as a tersh bug.";
    InstallFailed = "UPD-003", External, "Run the install command by hand to see its output.";
    BinaryMissing = "BIN-001", External, "Reinstall tersh.";
    LaunchFailed = "BIN-002", External, "Check that the file can be run.";
    OpenFailed = "APP-001", External, "Check that a program is set to open the file type.";
    Arguments = "CLI-001", Validation, "Run `tersh --help` for usage.";
}

impl fmt::Display for Code {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// An expected failure. See the module docs.
#[derive(Debug)]
pub struct AppError {
    pub code: Code,
    pub message: String,
    /// Details for the log: ids, paths, inputs.
    pub context: Vec<(&'static str, String)>,
    /// The original error, never discarded.
    pub cause: Option<Box<dyn Error + Send + Sync>>,
    /// Where it was raised.
    pub origin: &'static Location<'static>,
}

impl AppError {
    #[track_caller]
    pub fn new(code: Code, message: impl Into<String>) -> Self {
        Self { code, message: message.into(), context: Vec::new(), cause: None, origin: Location::caller() }
    }

    /// Keep the error that caused this one.
    pub fn because(mut self, cause: impl Into<Box<dyn Error + Send + Sync>>) -> Self {
        self.cause = Some(cause.into());
        self
    }

    /// Add a detail for the log.
    pub fn with(mut self, key: &'static str, value: impl fmt::Display) -> Self {
        self.context.push((key, value.to_string()));
        self
    }

    pub fn family(&self) -> Family {
        self.code.family()
    }
}

/// `message: cause (CODE)`, e.g. `could not start zsh: No such file or directory (PTY-002)`.
impl fmt::Display for AppError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.message)?;
        if let Some(cause) = &self.cause {
            write!(f, ": {cause}")?;
        }
        write!(f, " ({})", self.code)
    }
}

impl Error for AppError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        self.cause.as_deref().map(|e| e as &(dyn Error + 'static))
    }
}

/// UIs and the socket get the rendered text: `message: cause (CODE)`.
impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error> {
        serializer.collect_str(self)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use super::*;

    #[test]
    fn codes_are_unique_and_well_formed() {
        let mut seen = HashSet::new();
        for code in Code::ALL {
            let text = code.as_str();
            assert!(seen.insert(text), "{text} is used twice");
            let (prefix, number) = text.split_once('-').expect("PREFIX-NNN");
            assert!(prefix.len() == 3 && prefix.chars().all(|c| c.is_ascii_uppercase()), "{text}");
            assert!(number.len() == 3 && number.chars().all(|c| c.is_ascii_digit()), "{text}");
            assert!(!code.remedy().is_empty(), "{text} has no remedy");
        }
    }

    /// The registry: every code is documented with what to do about it.
    #[test]
    fn every_code_is_documented() {
        // This crate is backend/domain: the repository is two folders up.
        let manifest = std::path::Path::new(env!("CARGO_MANIFEST_DIR"));
        let repo = manifest.ancestors().nth(2).expect("backend/domain has a repository above it");
        let docs = std::fs::read_to_string(repo.join("docs").join("errors.md")).unwrap();
        for code in Code::ALL {
            let row = format!("| `{}` ", code.as_str());
            assert!(docs.contains(&row), "{} is missing from docs/errors.md", code.as_str());
        }
    }

    #[test]
    fn renders_message_cause_and_code() {
        let io = std::io::Error::new(std::io::ErrorKind::NotFound, "No such file or directory");
        let error = AppError::new(Code::ShellStart, "could not start zsh").because(io).with("shell", "zsh");
        assert_eq!(error.to_string(), "could not start zsh: No such file or directory (PTY-002)");
        assert!(error.source().is_some());
        assert_eq!(error.family(), Family::External);
        assert!(error.origin.file().ends_with("error.rs"));
    }
}
