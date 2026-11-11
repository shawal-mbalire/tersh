//! `tersh-show --serve`: an HTTP bridge so web servers, scripts in any
//! language, or a browser page can show cards in the tersh window it runs in.
//!
//! `POST /cards` takes one protocol message or an array of them (see
//! docs/protocol.md) and needs `Authorization: Bearer TOKEN`. Requests are
//! handled one at a time and forwarded in order.

use std::io::{BufRead, BufReader, Read, Write};
use std::net::{TcpListener, TcpStream};

use serde_json::{json, Value};

/// Largest request body accepted (a few minutes of compressed video).
const MAX_BODY: usize = 64 * 1024 * 1024;

pub struct Request {
    pub method: String,
    pub path: String,
    pub authorization: Option<String>,
    pub body: Vec<u8>,
}

pub struct Response {
    pub status: u16,
    pub body: Value,
}

impl Response {
    fn new(status: u16, body: Value) -> Self {
        Response { status, body }
    }

    fn error(status: u16, message: &str) -> Self {
        Response::new(status, json!({ "ok": false, "error": message }))
    }
}

/// Serve until the process is stopped. `forward` delivers one message to tersh.
pub fn serve(addr: &str, token: &str, mut forward: impl FnMut(&Value) -> Result<(), String>) -> Result<(), String> {
    let listener = TcpListener::bind(addr).map_err(|e| format!("{addr}: {e}"))?;
    let local = listener.local_addr().map_err(|e| e.to_string())?;
    eprintln!("tersh-show: serving cards on http://{local}/cards");
    eprintln!("tersh-show: token {token}");
    eprintln!(
        "tersh-show: try  curl -H 'Authorization: Bearer {token}' -d '{{\"kind\":\"markdown\",\"data\":\"# hi\"}}' http://{local}/cards"
    );
    for stream in listener.incoming() {
        let Ok(mut stream) = stream else { continue };
        let response = match read_request(&mut stream) {
            Ok(request) => handle(&request, token, &mut forward),
            Err(response) => response,
        };
        let _ = write_response(&mut stream, &response);
    }
    Ok(())
}

pub fn handle(request: &Request, token: &str, forward: &mut impl FnMut(&Value) -> Result<(), String>) -> Response {
    match (request.method.as_str(), request.path.split('?').next().unwrap_or("")) {
        // CORS preflight: browsers may call the bridge; the token is what protects it.
        ("OPTIONS", _) => Response::new(204, Value::Null),
        ("GET", "/") => Response::new(200, json!({ "ok": true, "service": "tersh-show", "version": env!("CARGO_PKG_VERSION") })),
        ("POST", "/cards") => {
            let expected = format!("Bearer {token}");
            if request.authorization.as_deref() != Some(expected.as_str()) {
                return Response::error(401, "missing or wrong `Authorization: Bearer TOKEN`");
            }
            let value: Value = match serde_json::from_slice(&request.body) {
                Ok(value) => value,
                Err(e) => return Response::error(400, &format!("invalid JSON: {e}")),
            };
            let messages = match value {
                Value::Array(list) => list,
                Value::Object(_) => vec![value],
                _ => return Response::error(400, "expected a message object or an array of them"),
            };
            for message in &messages {
                if !message.is_object() {
                    return Response::error(400, "every message must be an object");
                }
                if let Err(e) = forward(message) {
                    return Response::error(502, &e);
                }
            }
            Response::new(200, json!({ "ok": true, "sent": messages.len() }))
        },
        (_, "/" | "/cards") => Response::error(405, "use POST /cards"),
        _ => Response::error(404, "not found: use POST /cards"),
    }
}

fn read_request(stream: &mut TcpStream) -> Result<Request, Response> {
    let bad = |message: &str| Response::error(400, message);
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|_| bad("unreadable request"))?;
    let mut parts = line.split_whitespace();
    let (Some(method), Some(path)) = (parts.next(), parts.next()) else {
        return Err(bad("malformed request line"));
    };
    let (method, path) = (method.to_string(), path.to_string());

    let mut length = 0;
    let mut authorization = None;
    loop {
        line.clear();
        if reader.read_line(&mut line).map_err(|_| bad("unreadable headers"))? == 0 {
            break;
        }
        let header = line.trim_end();
        if header.is_empty() {
            break;
        }
        let Some((name, value)) = header.split_once(':') else { continue };
        let value = value.trim();
        match name.trim().to_ascii_lowercase().as_str() {
            "content-length" => length = value.parse().map_err(|_| bad("bad Content-Length"))?,
            "authorization" => authorization = Some(value.to_string()),
            "transfer-encoding" => return Err(Response::error(411, "send a Content-Length, not chunked data")),
            _ => {},
        }
    }
    if length > MAX_BODY {
        return Err(Response::error(413, "request body too large"));
    }
    let mut body = vec![0; length];
    reader.read_exact(&mut body).map_err(|_| bad("request body shorter than Content-Length"))?;
    Ok(Request { method, path, authorization, body })
}

fn write_response(stream: &mut TcpStream, response: &Response) -> std::io::Result<()> {
    let reason = match response.status {
        200 => "OK",
        204 => "No Content",
        400 => "Bad Request",
        401 => "Unauthorized",
        404 => "Not Found",
        405 => "Method Not Allowed",
        411 => "Length Required",
        413 => "Payload Too Large",
        _ => "Bad Gateway",
    };
    let body = if response.body.is_null() { String::new() } else { response.body.to_string() };
    write!(
        stream,
        "HTTP/1.1 {} {reason}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\
         Access-Control-Allow-Origin: *\r\nAccess-Control-Allow-Methods: GET, POST, OPTIONS\r\n\
         Access-Control-Allow-Headers: Authorization, Content-Type\r\nConnection: close\r\n\r\n{body}",
        response.status,
        body.len(),
    )?;
    stream.flush()
}

/// A random token for this run (from the OS where available).
pub fn new_token() -> String {
    let mut bytes = [0u8; 16];
    let random = std::fs::File::open("/dev/urandom").and_then(|mut f| f.read_exact(&mut bytes));
    if random.is_err() {
        // No /dev/urandom (Windows): mix the clock and process id.
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_nanos())
            ^ (u128::from(std::process::id()) << 64);
        bytes = seed.to_le_bytes();
    }
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn post(body: &str, auth: Option<&str>) -> Request {
        Request {
            method: "POST".into(),
            path: "/cards".into(),
            authorization: auth.map(str::to_string),
            body: body.as_bytes().to_vec(),
        }
    }

    #[test]
    fn forwards_messages_with_the_token() {
        let mut sent = Vec::new();
        let mut forward = |m: &Value| {
            sent.push(m.clone());
            Ok(())
        };
        let r = handle(&post(r#"[{"kind":"text","data":"a"},{"op":"close","id":"x"}]"#, Some("Bearer t")), "t", &mut forward);
        assert_eq!((r.status, r.body["sent"].as_u64()), (200, Some(2)));
        assert_eq!(sent[1]["op"], "close");
    }

    #[test]
    fn rejects_bad_requests() {
        let mut forward = |_: &Value| Ok(());
        assert_eq!(handle(&post("{}", None), "t", &mut forward).status, 401);
        assert_eq!(handle(&post("{}", Some("Bearer nope")), "t", &mut forward).status, 401);
        assert_eq!(handle(&post("{", Some("Bearer t")), "t", &mut forward).status, 400);
        assert_eq!(handle(&post("[1]", Some("Bearer t")), "t", &mut forward).status, 400);
        let get = Request { method: "GET".into(), path: "/cards".into(), authorization: None, body: vec![] };
        assert_eq!(handle(&get, "t", &mut forward).status, 405);
    }

    #[test]
    fn reports_tersh_errors() {
        let mut forward = |_: &Value| Err("unknown kind".to_string());
        let r = handle(&post("{}", Some("Bearer t")), "t", &mut forward);
        assert_eq!((r.status, r.body["error"].as_str()), (502, Some("unknown kind")));
    }

    #[test]
    fn serves_over_tcp() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        let address = addr.to_string();
        std::thread::spawn(move || serve(&address, "t", |_| Ok(())));
        let mut reply = String::new();
        for _ in 0..50 {
            if let Ok(mut s) = TcpStream::connect(addr) {
                let body = r#"{"kind":"text","data":"hi"}"#;
                write!(s, "POST /cards HTTP/1.1\r\nAuthorization: Bearer t\r\nContent-Length: {}\r\n\r\n{body}", body.len()).unwrap();
                s.read_to_string(&mut reply).unwrap();
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(reply.starts_with("HTTP/1.1 200"), "{reply}");
        assert!(reply.ends_with(r#"{"ok":true,"sent":1}"#), "{reply}");
    }

    #[test]
    fn tokens_are_random_hex() {
        let (a, b) = (new_token(), new_token());
        assert_eq!(a.len(), 32);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, b);
    }
}
