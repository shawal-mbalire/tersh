//! `tersh-show`: display files as rich cards in the tersh terminal.
//!
//! Messages go to `$TERSH_SOCKET` when available, otherwise they are written to
//! the terminal as `OSC 7701` sequences (wrapped for tmux passthrough when
//! running inside tmux).

use std::io::{self, BufRead, BufReader, IsTerminal, Read, Write};
use std::path::Path;
use std::process::ExitCode;

use base64::Engine;
use serde_json::{json, Map, Value};

mod serve;

const USAGE: &str = "\
Usage: tersh-show [OPTIONS] [FILE|-]...

Show files as rich cards in tersh: images, video, SVG, markdown (with LaTeX),
HTML, Plotly figures (JSON), Jupyter notebooks, JSON and text.

Options:
  -k, --kind KIND    Card kind (image, video, svg, markdown, html, plotly,
                     notebook, output, json, text). Guessed from the file name
                     by default.
  -r, --rows N       Rows to reserve below the prompt for an inline card.
  -f, --float        Float the card over the terminal instead of inline.
  -p, --pane         Cover the current tmux pane with the card.
  -t, --title TEXT   Card title.
      --id ID        Card id, to update or close it later.
  -u, --update       Replace the data of the card given by --id.
      --close ID     Close a card.
      --clear        Close all cards.
      --osc          Write escape sequences even if $TERSH_SOCKET is set.
      --serve [ADDR] Accept cards over HTTP (POST /cards) and show them in
                     this terminal. ADDR defaults to 127.0.0.1:7701.
      --token TOKEN  Bearer token --serve requires (default: random).
  -h, --help         Show this help.
";

#[derive(Default)]
struct Args {
    files: Vec<String>,
    kind: Option<String>,
    rows: Option<u16>,
    float: bool,
    pane: bool,
    title: Option<String>,
    id: Option<String>,
    update: bool,
    close: Option<String>,
    clear: bool,
    osc: bool,
    serve: Option<String>,
    token: Option<String>,
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args::default();
    let mut iter = std::env::args().skip(1).peekable();
    while let Some(arg) = iter.next() {
        let mut value = |name: &str| iter.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            },
            "-k" | "--kind" => args.kind = Some(value(&arg)?),
            "-r" | "--rows" => args.rows = Some(value(&arg)?.parse().map_err(|_| "--rows needs a number")?),
            "-f" | "--float" => args.float = true,
            "-p" | "--pane" => args.pane = true,
            "-t" | "--title" => args.title = Some(value(&arg)?),
            "--id" => args.id = Some(value(&arg)?),
            "-u" | "--update" => args.update = true,
            "--close" => args.close = Some(value(&arg)?),
            "--clear" => args.clear = true,
            "--osc" => args.osc = true,
            "--serve" => {
                args.serve = Some(match iter.peek() {
                    Some(next) if !next.starts_with('-') => iter.next().unwrap_or_default(),
                    _ => "127.0.0.1:7701".into(),
                })
            },
            "--token" => args.token = Some(value(&arg)?),
            "-" => args.files.push(arg),
            s if s.starts_with('-') => return Err(format!("unknown option {s}\n\n{USAGE}")),
            _ => args.files.push(arg),
        }
    }
    if args.update && args.id.is_none() {
        return Err("--update needs --id".into());
    }
    if args.files.is_empty() && args.close.is_none() && !args.clear && args.serve.is_none() {
        if io::stdin().is_terminal() {
            return Err(USAGE.into());
        }
        args.files.push("-".into());
    }
    Ok(args)
}

/// Card kind and, for images, MIME type, from a file name.
fn guess_kind(path: &str, content: &[u8]) -> (String, Option<&'static str>) {
    let ext = Path::new(path).extension().and_then(|e| e.to_str()).unwrap_or("").to_ascii_lowercase();
    let image = |mime| ("image".to_string(), Some(mime));
    let video = |mime| ("video".to_string(), Some(mime));
    match ext.as_str() {
        "png" => image("image/png"),
        "jpg" | "jpeg" => image("image/jpeg"),
        "gif" => image("image/gif"),
        "webp" => image("image/webp"),
        "bmp" => image("image/bmp"),
        "avif" => image("image/avif"),
        "webm" => video("video/webm"),
        "mp4" | "m4v" => video("video/mp4"),
        "ogv" => video("video/ogg"),
        "mov" => video("video/quicktime"),
        "svg" => ("svg".into(), None),
        "md" | "markdown" => ("markdown".into(), None),
        "html" | "htm" => ("html".into(), None),
        "ipynb" => ("notebook".into(), None),
        "json" if looks_like_plotly(content) => ("plotly".into(), None),
        "json" => ("json".into(), None),
        _ => sniff(content),
    }
}

fn sniff(content: &[u8]) -> (String, Option<&'static str>) {
    let start = content.trim_ascii_start();
    if content.starts_with(b"\x89PNG") {
        ("image".into(), Some("image/png"))
    } else if content.starts_with(b"\xff\xd8\xff") {
        ("image".into(), Some("image/jpeg"))
    } else if content.starts_with(b"GIF8") {
        ("image".into(), Some("image/gif"))
    } else if content.starts_with(b"\x1a\x45\xdf\xa3") {
        ("video".into(), Some("video/webm"))
    } else if content.get(4..8) == Some(b"ftyp") {
        ("video".into(), Some("video/mp4"))
    } else if start.starts_with(b"<svg") || (start.starts_with(b"<?xml") && content.windows(4).any(|w| w == b"<svg")) {
        ("svg".into(), None)
    } else if start.starts_with(b"{") && looks_like_plotly(content) {
        ("plotly".into(), None)
    } else if start.starts_with(b"{") && content.windows(8).any(|w| w == b"\"cells\":") {
        ("notebook".into(), None)
    } else {
        ("text".into(), None)
    }
}

fn looks_like_plotly(content: &[u8]) -> bool {
    serde_json::from_slice::<Value>(content).is_ok_and(|v| v.get("data").is_some_and(Value::is_array) && v.get("layout").is_some())
}

/// The `data` field for a card of `kind`.
fn card_data(kind: &str, content: Vec<u8>, mime: Option<&str>) -> Result<Value, String> {
    match kind {
        "image" | "video" => {
            let fallback = if kind == "video" { "video/webm" } else { "image/png" };
            let b64 = base64::engine::general_purpose::STANDARD.encode(&content);
            Ok(Value::String(format!("data:{};base64,{b64}", mime.unwrap_or(fallback))))
        },
        "notebook" | "plotly" | "json" | "output" => {
            serde_json::from_slice(&content).map_err(|e| format!("invalid JSON for {kind} card: {e}"))
        },
        _ => String::from_utf8(content).map(Value::String).map_err(|_| format!("{kind} card needs UTF-8 text")),
    }
}

fn default_rows(kind: &str, data: &Value, screen_rows: u16) -> u16 {
    let budget = (screen_rows as f32 * 0.6) as u16;
    let text_lines = |s: &str| s.lines().count() as u16;
    let rows = match kind {
        "markdown" => data.as_str().map_or(10, |s| text_lines(s) + 3),
        "text" | "json" => data.as_str().map_or(10, |s| text_lines(s) + 2),
        _ => 20,
    };
    rows.clamp(4, budget.max(4))
}

enum Transport {
    #[cfg(unix)]
    Socket(std::os::unix::net::UnixStream),
    Osc { tmux: bool },
}

impl Transport {
    fn open(force_osc: bool) -> Self {
        #[cfg(unix)]
        if !force_osc {
            if let Some(path) = std::env::var_os("TERSH_SOCKET") {
                if let Ok(stream) = std::os::unix::net::UnixStream::connect(path) {
                    return Transport::Socket(stream);
                }
            }
        }
        Transport::Osc { tmux: std::env::var_os("TMUX").is_some() }
    }

    fn send(&mut self, message: &Value) -> Result<(), String> {
        match self {
            #[cfg(unix)]
            Transport::Socket(stream) => {
                let mut line = message.to_string();
                line.push('\n');
                stream.write_all(line.as_bytes()).map_err(|e| format!("socket: {e}"))?;
                // Wait for the acknowledgement so the card is anchored before we print anything.
                let mut reply = String::new();
                BufReader::new(&*stream).read_line(&mut reply).map_err(|e| format!("socket: {e}"))?;
                let reply: Value = serde_json::from_str(&reply).map_err(|e| format!("bad reply: {e}"))?;
                match reply["ok"].as_bool() {
                    Some(true) => Ok(()),
                    _ => Err(reply["error"].as_str().unwrap_or("rejected").to_string()),
                }
            },
            Transport::Osc { tmux } => {
                let payload = base64::engine::general_purpose::STANDARD.encode(message.to_string());
                let sequence = format!("\x1b]7701;{payload}\x1b\\");
                let sequence = if *tmux {
                    // tmux passthrough (needs `set -g allow-passthrough on`): double every ESC.
                    format!("\x1bPtmux;{}\x1b\\", sequence.replace('\x1b', "\x1b\x1b"))
                } else {
                    sequence
                };
                let mut out = io::stdout().lock();
                out.write_all(sequence.as_bytes()).and_then(|_| out.flush()).map_err(|e| e.to_string())
            },
        }
    }
}

/// Cells covered by the current tmux pane: `(row, col, rows, cols)` in the
/// terminal's coordinates, so counting a status line at the top.
fn tmux_pane() -> Result<(u16, u16, u16, u16), String> {
    if std::env::var_os("TMUX").is_none() {
        return Err("--pane only works inside tmux".into());
    }
    let mut cmd = std::process::Command::new("tmux");
    cmd.args(["display-message", "-p"]);
    if let Some(pane) = std::env::var_os("TMUX_PANE") {
        cmd.arg("-t").arg(pane);
    }
    cmd.arg("#{pane_top} #{pane_left} #{pane_height} #{pane_width} #{status} #{status-position}");
    let out = cmd.output().map_err(|e| format!("tmux: {e}"))?;
    let text = String::from_utf8_lossy(&out.stdout);
    parse_pane(&text).ok_or_else(|| format!("unexpected tmux reply: {}", text.trim()))
}

fn parse_pane(text: &str) -> Option<(u16, u16, u16, u16)> {
    let fields: Vec<&str> = text.split_whitespace().collect();
    let [top, left, rows, cols, status, position] = fields[..] else { return None };
    let status_lines = match status {
        "off" => 0,
        "on" => 1,
        n => n.parse().ok()?,
    };
    let offset = if position == "top" { status_lines } else { 0 };
    Some((top.parse::<u16>().ok()? + offset, left.parse().ok()?, rows.parse().ok()?, cols.parse().ok()?))
}

fn read_input(file: &str) -> Result<Vec<u8>, String> {
    if file == "-" {
        let mut buf = Vec::new();
        io::stdin().read_to_end(&mut buf).map_err(|e| format!("stdin: {e}"))?;
        Ok(buf)
    } else {
        std::fs::read(file).map_err(|e| format!("{file}: {e}"))
    }
}

fn run() -> Result<(), String> {
    let args = parse_args()?;
    let mut transport = Transport::open(args.osc);

    if args.clear {
        transport.send(&json!({ "op": "clear" }))?;
    }
    if let Some(addr) = &args.serve {
        let token = args.token.clone().unwrap_or_else(serve::new_token);
        return serve::serve(addr, &token, |message| transport.send(message));
    }
    if let Some(id) = &args.close {
        transport.send(&json!({ "op": "close", "id": id }))?;
    }

    let screen_rows = terminal_size::terminal_size().map_or(24, |(_, h)| h.0);
    let pid = std::process::id();

    for (index, file) in args.files.iter().enumerate() {
        let content = read_input(file)?;
        let (guessed, mime) = guess_kind(file, &content);
        let kind = args.kind.clone().unwrap_or(guessed);
        let data = card_data(&kind, content, mime)?;

        let id = match &args.id {
            Some(id) if args.files.len() == 1 => id.clone(),
            Some(id) => format!("{id}-{index}"),
            None => format!("show-{pid}-{index}"),
        };
        let title = args.title.clone().unwrap_or_else(|| {
            if file == "-" { kind.clone() } else { Path::new(file).file_name().map_or(file.clone(), |n| n.to_string_lossy().into_owned()) }
        });

        let mut message = Map::new();
        message.insert("op".into(), json!(if args.update { "update" } else { "show" }));
        message.insert("id".into(), json!(id));
        message.insert("kind".into(), json!(kind));
        message.insert("title".into(), json!(title));
        message.insert("data".into(), data);

        if args.update {
            transport.send(&Value::Object(message))?;
            continue;
        }

        if args.pane {
            let (row, col, rows, cols) = tmux_pane()?;
            message.insert("anchor".into(), json!({ "mode": "cell", "row": row, "col": col, "rows": rows, "cols": cols }));
            transport.send(&Value::Object(message))?;
            continue;
        }

        let float = args.float || (kind == "notebook" && args.rows.is_none());
        if float {
            message.insert("anchor".into(), json!({ "mode": "float", "top": "5%", "left": "5%", "width": "90%", "height": "90%" }));
            transport.send(&Value::Object(message))?;
            continue;
        }

        let rows = args.rows.unwrap_or_else(|| default_rows(&kind, &message["data"], screen_rows)).max(1);
        message.insert("anchor".into(), json!({ "mode": "cursor", "rows": rows }));
        transport.send(&Value::Object(message))?;
        // Leave room for the card: the prompt continues below it.
        let mut out = io::stdout().lock();
        out.write_all("\n".repeat(rows as usize).as_bytes()).and_then(|_| out.flush()).map_err(|e| e.to_string())?;
    }
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) => {
            eprintln!("tersh-show: {e}");
            ExitCode::FAILURE
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn guesses_kinds() {
        assert_eq!(guess_kind("a.PNG", b"").0, "image");
        assert_eq!(guess_kind("a.ipynb", b"").0, "notebook");
        assert_eq!(guess_kind("fig.json", br#"{"data":[],"layout":{}}"#).0, "plotly");
        assert_eq!(guess_kind("x.json", br#"{"a":1}"#).0, "json");
        assert_eq!(guess_kind("-", b"\x89PNG\r\n").1, Some("image/png"));
        assert_eq!(guess_kind("-", b"  <svg xmlns=''/>").0, "svg");
        assert_eq!(guess_kind("-", b"hello").0, "text");
        assert_eq!(guess_kind("clip.webm", b""), ("video".into(), Some("video/webm")));
        assert_eq!(guess_kind("-", b"\x1a\x45\xdf\xa3\x01").0, "video");
        assert_eq!(guess_kind("-", b"\0\0\0\x20ftypisom").1, Some("video/mp4"));
    }

    #[test]
    fn builds_data() {
        assert_eq!(card_data("image", vec![1, 2, 3], Some("image/png")).unwrap(), "data:image/png;base64,AQID");
        assert!(card_data("notebook", b"nope".to_vec(), None).is_err());
        assert_eq!(card_data("markdown", b"# hi".to_vec(), None).unwrap(), "# hi");
    }

    #[test]
    fn pane_geometry_counts_a_top_status_line() {
        assert_eq!(parse_pane("0 81 40 80 on top\n"), Some((1, 81, 40, 80)));
        assert_eq!(parse_pane("5 0 20 100 2 top"), Some((7, 0, 20, 100)));
        assert_eq!(parse_pane("5 0 20 100 on bottom"), Some((5, 0, 20, 100)));
        assert_eq!(parse_pane("garbage"), None);
    }

    #[test]
    fn row_defaults_fit_the_screen() {
        assert_eq!(default_rows("markdown", &json!("a\nb"), 40), 5);
        assert_eq!(default_rows("image", &json!(""), 40), 20);
        assert_eq!(default_rows("image", &json!(""), 10), 6);
        assert_eq!(default_rows("text", &json!("x\n".repeat(100)), 40), 24);
    }
}
