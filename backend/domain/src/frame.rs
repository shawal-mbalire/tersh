//! Serialisable snapshots of the terminal viewport, sent to the renderer.
//! Emulator adapters produce them; UI adapters draw them.

use serde::Serialize;

/// Bit set on an encoded colour when it carries a literal `0xRRGGBB` value.
/// Otherwise the value is an index into the renderer's palette, using
/// alacritty's layout (0-255 ANSI/cube/gray, 256 fg, 257 bg, 258 cursor,
/// 259-266 dim, 267 bright fg, 268 dim fg).
pub const RGB_FLAG: u32 = 1 << 24;

/// A horizontal run of cells sharing attributes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Run {
    /// Starting column.
    pub c: u16,
    /// Number of cells covered. When `t` has exactly `n` chars each one sits in
    /// its own cell; otherwise `t` is a single cluster (wide char, combining
    /// marks) drawn at `c`.
    pub n: u16,
    pub t: String,
    pub fg: u32,
    pub bg: u32,
    /// Cell attribute bits (bold, italic, underline, inverse, ...), using
    /// `alacritty_terminal`'s flag values (`CellFlag` in `src/types.ts`).
    pub f: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Row {
    /// Viewport row.
    pub y: u16,
    pub runs: Vec<Run>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Shape {
    Block,
    Underline,
    Beam,
    Hollow,
    Hidden,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub struct Cursor {
    pub x: u16,
    pub y: u16,
    pub shape: Shape,
    pub blink: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct Frame {
    pub cols: u16,
    pub rows: u16,
    /// When true every viewport row is included and the renderer should clear first.
    pub full: bool,
    pub lines: Vec<Row>,
    pub cursor: Cursor,
    /// Terminal mode bits (app cursor, mouse modes, bracketed paste, ...), using
    /// `alacritty_terminal`'s `TermMode` values (`Mode` in `src/types.ts`).
    pub mode: u32,
    pub alt: bool,
    /// How far the user has scrolled back into history.
    pub display_offset: usize,
    pub history: usize,
    /// Lines scrolled off the top of the primary screen since start. Used with
    /// `display_offset` to position cards anchored in scrollback.
    pub scroll_base: u64,
}

