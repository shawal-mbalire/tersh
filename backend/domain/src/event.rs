//! What a session reports besides frames and cards.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum TermEvent {
    Title { title: String },
    ResetTitle,
    Bell,
    /// OSC 52 copy request.
    Clipboard { text: String },
    RichError { error: String },
    Exit { code: Option<u32> },
}

/// Grid and cell geometry, as reported to rich-protocol clients (`op: info`).
#[derive(Debug, Clone, Serialize)]
pub struct Info {
    pub version: &'static str,
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
    pub cursor_row: usize,
    pub cursor_col: usize,
    pub alt_screen: bool,
}

/// Size of the text area, in cells and pixels per cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct WindowSize {
    pub cols: u16,
    pub rows: u16,
    pub cell_width: u16,
    pub cell_height: u16,
}
