//! Ports: what the application needs from the outside world (driven ports,
//! implemented in `tersh-adapters`) and what it offers to it (the [`Sink`] a
//! UI implements to receive output).

use std::io::{Read, Write};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Instant, SystemTime};

use anyhow::Result;
use serde_json::Value;
use tersh_domain::config::Cursor;
use tersh_domain::rich::AnchorContext;
use tersh_domain::{Config, Frame, Rgb, TermEvent, WindowSize};

// ---- output to the UI -----------------------------------------------------------

/// Receives everything a session produces. Implemented by UI adapters.
pub trait Sink: Send + Sync + 'static {
    fn frame(&self, frame: Frame);
    /// A rich-media message with its anchor resolved (see `tersh_domain::rich::resolve_anchor`).
    fn rich(&self, message: Value);
    fn event(&self, event: TermEvent);
}

// ---- pseudo-terminals ------------------------------------------------------------

/// What to run in a new pseudo-terminal.
#[derive(Debug, Clone, Default)]
pub struct SpawnRequest {
    /// `None` means the system's default shell.
    pub shell: Option<String>,
    pub args: Vec<String>,
    /// `None` means the user's home directory.
    pub cwd: Option<PathBuf>,
    /// Inherited variables to remove before `env` is applied.
    pub env_remove: Vec<String>,
    pub env: Vec<(String, String)>,
    pub cols: u16,
    pub rows: u16,
}

/// A process running in a pseudo-terminal.
pub struct PtyProcess {
    /// What the program writes to the terminal.
    pub output: Box<dyn Read + Send>,
    /// What the user types.
    pub input: Box<dyn Write + Send>,
    pub control: Box<dyn PtyControl>,
    pub child: Box<dyn ChildProcess>,
    /// The program that was started, for messages.
    pub program: String,
}

pub trait PtyControl: Send {
    fn resize(&self, size: WindowSize) -> Result<()>;
    /// Leader of the terminal's foreground process group (the shell when it is at its prompt).
    fn foreground_pid(&self) -> Option<u32>;
}

pub trait ChildProcess: Send {
    /// Block until the process exits; returns its exit code if it has one.
    fn wait(&mut self) -> Option<u32>;
    fn killer(&self) -> Box<dyn ProcessKiller>;
    fn pid(&self) -> Option<u32>;
}

pub trait ProcessKiller: Send + Sync {
    fn kill(&mut self);
}

/// Starts programs in pseudo-terminals.
pub trait PtySystem: Send + Sync {
    fn spawn(&self, request: &SpawnRequest) -> Result<PtyProcess>;
}

/// Information about running processes.
pub trait ProcessInspector: Send + Sync {
    /// Program name, e.g. `nvim`.
    fn name(&self, pid: u32) -> Option<String>;
    /// Current working directory.
    fn cwd(&self, pid: u32) -> Option<PathBuf>;
}

// ---- terminal emulation ------------------------------------------------------------

/// Settings an emulator can change while running.
#[derive(Debug, Clone)]
pub struct EmulatorOptions {
    pub cols: u16,
    pub rows: u16,
    pub scrollback: usize,
    pub cursor: Cursor,
}

/// What an emulator asks of its host while interpreting output.
pub trait TermHost: Send + Sync {
    /// Send bytes back to the program (answers to device queries).
    fn reply(&self, text: &str);
    fn event(&self, event: TermEvent);
    /// A palette colour, for colour queries (0-255, 256 fg, 257 bg, 258 cursor).
    fn color(&self, index: usize) -> Rgb;
    fn window_size(&self) -> WindowSize;
}

/// Scrolling the viewport through history.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Scroll {
    /// Positive is up, into history.
    Lines(i32),
    Bottom,
}

/// A VT emulator: interprets program output and keeps the screen state.
pub trait Emulator: Send {
    /// Interpret program output.
    fn advance(&mut self, bytes: &[u8]);
    /// When a synchronized update (DEC mode 2026) is buffering output, its deadline.
    fn sync_deadline(&self) -> Option<Instant>;
    /// Apply output held by a synchronized update now.
    fn flush_sync(&mut self);
    fn resize(&mut self, cols: u16, rows: u16);
    fn scroll(&mut self, scroll: Scroll);
    /// How far the viewport is scrolled back into history.
    fn display_offset(&self) -> usize;
    /// Cursor position and screen, for anchoring rich cards.
    fn anchor_context(&self) -> AnchorContext;
    fn alt_screen(&self) -> bool;
    /// The viewport rows changed since the last call (all of them if `full`).
    fn frame(&mut self, full: bool) -> Frame;
    fn set_options(&mut self, options: &EmulatorOptions);
}

pub trait EmulatorFactory: Send + Sync {
    fn create(&self, options: &EmulatorOptions, host: Arc<dyn TermHost>) -> Box<dyn Emulator>;
}

// ---- user settings ------------------------------------------------------------------

/// A user plugin's source.
#[derive(Debug, Clone, serde::Serialize)]
pub struct Plugin {
    pub name: String,
    pub source: String,
    pub style: Option<String>,
}

/// Modification times of a set of files, compared between polls to detect changes.
pub type Fingerprint = Vec<(PathBuf, Option<SystemTime>)>;

/// Something whose files can change while tersh runs.
pub trait Watchable: Send + Sync {
    fn fingerprint(&self) -> Fingerprint;
}

/// Where the user's settings live (normally `~/.config/tersh`).
pub trait ConfigStore: Send + Sync {
    /// Load and parse the config; problems become warnings.
    fn load(&self) -> Config;
    /// Create the config file from the commented template if it doesn't exist.
    fn ensure(&self) -> std::io::Result<()>;
    /// The user stylesheet, if any.
    fn style(&self) -> Option<String>;
    /// Every theme name that can be used.
    fn themes(&self) -> Vec<String>;
    fn plugins(&self, disabled: &[String]) -> Vec<Plugin>;
    fn config_path(&self) -> PathBuf;
    /// Config, theme and stylesheet files.
    fn settings_files(&self) -> Fingerprint;
    /// Plugin sources and styles.
    fn plugin_files(&self) -> Fingerprint;
}

// ---- updates ----------------------------------------------------------------------------

/// Where releases are published.
pub trait ReleaseSource {
    /// The latest release's tag, e.g. `v0.2.0`.
    fn latest(&self) -> Result<String>;
}

/// Installs a release over the current installation.
pub trait Installer {
    fn install(&self, tag: &str) -> Result<()>;
}
