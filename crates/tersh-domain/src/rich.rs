//! The tersh rich-media protocol.
//!
//! Programs ask the terminal to show rich cards (markdown, images, plots,
//! notebooks, ...) by writing an OSC sequence:
//!
//! ```text
//! ESC ] 7701 ; <payload> BEL        or        ESC ] 7701 ; <payload> ESC \
//! ```
//!
//! `<payload>` is either a JSON object or the base64 encoding of one. The same
//! JSON messages can also be sent, one per line, to the Unix socket named in
//! `$TERSH_SOCKET` (better for large payloads such as whole notebooks).
//!
//! The scanner strips these sequences out of the PTY byte stream before it
//! reaches the VT parser, so the core never has to fork the emulator.

use base64::Engine;
use serde_json::Value;

/// OSC number reserved for the rich protocol.
pub const OSC_NUMBER: &str = "7701";

const INTRO: &[u8] = b"7701;";
const ESC: u8 = 0x1b;
const BEL: u8 = 0x07;

/// Payloads larger than this are discarded (the sequence is still consumed).
pub const MAX_PAYLOAD: usize = 64 * 1024 * 1024;

/// A piece of the PTY stream after scanning.
#[derive(Debug, PartialEq, Eq)]
pub enum Chunk<'a> {
    /// Ordinary terminal output, to be fed to the VT parser.
    Bytes(&'a [u8]),
    /// The body of a rich OSC sequence.
    Payload(Vec<u8>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    Ground,
    Esc,
    /// Matched this many bytes of `INTRO` after `ESC ]`.
    Intro(usize),
    Payload,
    PayloadEsc,
}

/// Incremental, allocation-light scanner for rich OSC sequences.
///
/// Sequences may be split across arbitrary read boundaries.
#[derive(Debug)]
pub struct OscScanner {
    state: State,
    /// Bytes held back while we decide whether an escape is ours.
    prefix: Vec<u8>,
    payload: Vec<u8>,
    overflow: bool,
}

impl Default for OscScanner {
    fn default() -> Self {
        Self::new()
    }
}

impl OscScanner {
    pub fn new() -> Self {
        Self { state: State::Ground, prefix: Vec::with_capacity(8), payload: Vec::new(), overflow: false }
    }

    /// Scan `input`, calling `out` with pass-through bytes and complete payloads in stream order.
    pub fn feed(&mut self, input: &[u8], mut out: impl FnMut(Chunk<'_>)) {
        let mut run_start = if self.state == State::Ground { Some(0) } else { None };
        let mut i = 0;

        while i < input.len() {
            let b = input[i];
            match self.state {
                State::Ground => {
                    if b == ESC {
                        if let Some(start) = run_start.take() {
                            if start < i {
                                out(Chunk::Bytes(&input[start..i]));
                            }
                        }
                        self.prefix.push(ESC);
                        self.state = State::Esc;
                    } else if run_start.is_none() {
                        run_start = Some(i);
                    }
                },
                State::Esc => {
                    if b == b']' {
                        self.prefix.push(b);
                        self.state = State::Intro(0);
                    } else {
                        self.mismatch(&mut out);
                        run_start = Some(i);
                        continue;
                    }
                },
                State::Intro(k) => {
                    if b == INTRO[k] {
                        self.prefix.push(b);
                        if k + 1 == INTRO.len() {
                            self.prefix.clear();
                            self.payload.clear();
                            self.overflow = false;
                            self.state = State::Payload;
                        } else {
                            self.state = State::Intro(k + 1);
                        }
                    } else {
                        self.mismatch(&mut out);
                        // Reprocess this byte from the ground state.
                        continue;
                    }
                },
                State::Payload => match b {
                    BEL => self.finish(&mut out),
                    ESC => self.state = State::PayloadEsc,
                    _ => self.push_payload(b),
                },
                State::PayloadEsc => {
                    self.finish(&mut out);
                    if b != b'\\' {
                        // Any other escape also terminates the OSC; keep the
                        // ESC and let the new sequence be scanned normally.
                        self.prefix.push(ESC);
                        self.state = State::Esc;
                        continue;
                    }
                },
            }
            if self.state == State::Ground && run_start.is_none() && b != ESC {
                // Just left a sequence; output resumes after this byte.
                run_start = Some(i + 1);
            }
            i += 1;
        }

        if self.state == State::Ground {
            if let Some(start) = run_start {
                if start < input.len() {
                    out(Chunk::Bytes(&input[start..]));
                }
            }
        }
    }

    fn mismatch(&mut self, out: &mut impl FnMut(Chunk<'_>)) {
        out(Chunk::Bytes(&self.prefix));
        self.prefix.clear();
        self.state = State::Ground;
    }

    fn push_payload(&mut self, b: u8) {
        if self.payload.len() < MAX_PAYLOAD {
            self.payload.push(b);
        } else {
            self.overflow = true;
        }
    }

    fn finish(&mut self, out: &mut impl FnMut(Chunk<'_>)) {
        let payload = std::mem::take(&mut self.payload);
        if self.overflow {
            log::warn!("dropping rich payload larger than {MAX_PAYLOAD} bytes");
        } else {
            out(Chunk::Payload(payload));
        }
        self.overflow = false;
        self.state = State::Ground;
    }
}

/// Decode a payload (raw JSON or base64 JSON) into a message object.
pub fn decode_payload(payload: &[u8]) -> Result<Value, String> {
    let trimmed = payload.trim_ascii();
    let json = if trimmed.first() == Some(&b'{') {
        std::borrow::Cow::Borrowed(trimmed)
    } else {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(trimmed)
            .map_err(|e| format!("payload is neither JSON nor base64: {e}"))?;
        std::borrow::Cow::Owned(decoded)
    };
    let value: Value = serde_json::from_slice(&json).map_err(|e| format!("invalid JSON: {e}"))?;
    if !value.is_object() {
        return Err("message must be a JSON object".into());
    }
    Ok(value)
}

/// Which screen buffer a card is attached to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Screen {
    Primary,
    Alt,
}

/// Where the cursor was when a message arrived, used to resolve anchors.
#[derive(Debug, Clone, Copy)]
pub struct AnchorContext {
    pub screen: Screen,
    /// Cursor line, relative to the top of the screen.
    pub cursor_row: usize,
    pub cursor_col: usize,
    /// Lines scrolled off the top of the primary screen since start.
    pub scroll_base: u64,
}

/// Resolve the message's `anchor` into terminal-absolute coordinates.
///
/// Accepted anchor forms (`rows`/`cols` size hints pass through untouched):
///
/// * `{"mode": "cursor"}` (default) — at the cursor when the message arrived.
/// * `{"mode": "cell", "row": r, "col": c}` — screen-relative cell.
/// * `{"mode": "float", ...}` — not tied to the grid; left for the renderer.
///
/// Grid anchors get `screen` and `line` added. On the primary screen `line`
/// counts from the very first line of output so it survives scrolling; on the
/// alternate screen it is simply the screen row.
pub fn resolve_anchor(msg: &mut Value, ctx: &AnchorContext) {
    let Some(obj) = msg.as_object_mut() else { return };
    let op = obj.get("op").and_then(Value::as_str).unwrap_or("show");
    if !matches!(op, "show" | "move" | "update") {
        return;
    }
    // Updates and moves without an anchor (e.g. just hiding a card) keep the old one.
    if (op == "update" || op == "move") && !obj.contains_key("anchor") {
        return;
    }

    let anchor = obj.entry("anchor").or_insert_with(|| Value::Object(Default::default()));
    if !anchor.is_object() {
        *anchor = Value::Object(Default::default());
    }
    let anchor = anchor.as_object_mut().expect("anchor is an object");

    let mode = anchor.get("mode").and_then(Value::as_str).unwrap_or("cursor").to_owned();
    let (row, col) = match mode.as_str() {
        "float" => return,
        "cell" => (
            anchor.get("row").and_then(Value::as_u64).unwrap_or(ctx.cursor_row as u64),
            anchor.get("col").and_then(Value::as_u64).unwrap_or(0),
        ),
        _ => (ctx.cursor_row as u64, ctx.cursor_col as u64),
    };

    let line = match ctx.screen {
        Screen::Primary => ctx.scroll_base + row,
        Screen::Alt => row,
    };
    anchor.insert("mode".into(), Value::from(mode));
    anchor.insert("screen".into(), serde_json::to_value(ctx.screen).unwrap());
    anchor.insert("line".into(), Value::from(line));
    anchor.insert("col".into(), Value::from(col));
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(chunks: &[&[u8]]) -> (Vec<u8>, Vec<Vec<u8>>) {
        let mut scanner = OscScanner::new();
        let mut bytes = Vec::new();
        let mut payloads = Vec::new();
        for chunk in chunks {
            scanner.feed(chunk, |c| match c {
                Chunk::Bytes(b) => bytes.extend_from_slice(b),
                Chunk::Payload(p) => payloads.push(p),
            });
        }
        (bytes, payloads)
    }

    #[test]
    fn passes_plain_output() {
        let (bytes, payloads) = scan(&[b"hello \x1b[1mworld\x1b[0m\r\n"]);
        assert_eq!(bytes, b"hello \x1b[1mworld\x1b[0m\r\n");
        assert!(payloads.is_empty());
    }

    #[test]
    fn extracts_bel_and_st_terminated() {
        let (bytes, payloads) = scan(&[b"a\x1b]7701;{\"x\":1}\x07b\x1b]7701;{}\x1b\\c"]);
        assert_eq!(bytes, b"abc");
        assert_eq!(payloads, vec![b"{\"x\":1}".to_vec(), b"{}".to_vec()]);
    }

    #[test]
    fn leaves_other_oscs_alone() {
        let input = b"\x1b]0;title\x07\x1b]770;x\x07\x1b]77011;y\x07";
        let (bytes, payloads) = scan(&[input]);
        assert_eq!(bytes, input);
        assert!(payloads.is_empty());
    }

    #[test]
    fn handles_split_reads() {
        let input = b"pre\x1b]7701;{\"op\":\"clear\"}\x1b\\post\x1b]0;t\x07";
        for split in 0..input.len() {
            let (a, b) = input.split_at(split);
            let (bytes, payloads) = scan(&[a, b]);
            assert_eq!(bytes, b"prepost\x1b]0;t\x07", "split at {split}");
            assert_eq!(payloads, vec![b"{\"op\":\"clear\"}".to_vec()], "split at {split}");
        }
    }

    #[test]
    fn byte_at_a_time() {
        let input = b"\x1b[31mx\x1b]7701;e30=\x07\x1b\x1b]7701;{}\x07y";
        let chunks: Vec<&[u8]> = input.chunks(1).collect();
        let (bytes, payloads) = scan(&chunks);
        assert_eq!(bytes, b"\x1b[31mx\x1by");
        assert_eq!(payloads, vec![b"e30=".to_vec(), b"{}".to_vec()]);
    }

    #[test]
    fn escape_inside_payload_terminates_it() {
        let (bytes, payloads) = scan(&[b"\x1b]7701;{}\x1b[0mz"]);
        assert_eq!(bytes, b"\x1b[0mz");
        assert_eq!(payloads, vec![b"{}".to_vec()]);
    }

    #[test]
    fn decodes_json_and_base64() {
        assert_eq!(decode_payload(b" {\"op\":\"clear\"} ").unwrap()["op"], "clear");
        assert_eq!(decode_payload(b"eyJvcCI6ImNsZWFyIn0=").unwrap()["op"], "clear");
        assert!(decode_payload(b"[1]").is_err());
        assert!(decode_payload(b"!!").is_err());
    }

    #[test]
    fn resolves_anchors() {
        let ctx = AnchorContext { screen: Screen::Primary, cursor_row: 3, cursor_col: 7, scroll_base: 100 };
        let mut msg = serde_json::json!({"op": "show", "kind": "image"});
        resolve_anchor(&mut msg, &ctx);
        assert_eq!(msg["anchor"]["line"], 103);
        assert_eq!(msg["anchor"]["col"], 7);
        assert_eq!(msg["anchor"]["screen"], "primary");

        let alt = AnchorContext { screen: Screen::Alt, ..ctx };
        let mut msg = serde_json::json!({"op": "move", "id": "a", "anchor": {"mode": "cell", "row": 5, "col": 2, "rows": 4}});
        resolve_anchor(&mut msg, &alt);
        assert_eq!(msg["anchor"]["line"], 5);
        assert_eq!(msg["anchor"]["rows"], 4);
        assert_eq!(msg["anchor"]["screen"], "alt");

        let mut msg = serde_json::json!({"op": "show", "anchor": {"mode": "float", "x": 0.5}});
        resolve_anchor(&mut msg, &ctx);
        assert!(msg["anchor"].get("line").is_none());

        let mut msg = serde_json::json!({"op": "update", "id": "a", "data": 1});
        resolve_anchor(&mut msg, &ctx);
        assert!(msg.get("anchor").is_none());
    }
}
