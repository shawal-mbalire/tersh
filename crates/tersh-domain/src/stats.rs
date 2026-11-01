//! Profiling counters for a session, always collected (they are a handful of
//! atomic adds) and read on demand by the profiler overlay or over the socket.

use serde::Serialize;

/// Totals since the session started. Rates come from comparing two snapshots.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
pub struct Stats {
    /// Milliseconds since the session started.
    pub uptime_ms: u64,
    /// Bytes the program wrote to the terminal, and how many reads that took.
    pub output_bytes: u64,
    pub output_reads: u64,
    /// Time spent parsing program output (VT emulation and the rich-message scanner).
    pub parse_us: u64,
    /// Frames built from the grid, sent to the UI, and skipped because nothing changed.
    pub frames_built: u64,
    pub frames_sent: u64,
    pub frames_skipped: u64,
    /// Viewport rows included in sent frames.
    pub rows_sent: u64,
    /// Time spent building frames, in total and the slowest one.
    pub build_us: u64,
    pub build_max_us: u64,
    /// Bytes typed, pasted or replied to the program, and how many writes.
    pub input_bytes: u64,
    pub input_writes: u64,
    /// Input queued but not yet written (the program isn't reading).
    pub input_pending: u64,
    /// Lines of scrollback currently held.
    pub history: u64,
    /// Resident memory of the terminal process, when the platform reports it.
    pub memory_bytes: Option<u64>,
}
