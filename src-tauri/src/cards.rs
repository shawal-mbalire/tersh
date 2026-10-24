//! Serves HTML card documents from a dedicated `tersh-card:` origin.
//!
//! HTML outputs (notebook `text/html`, Bokeh, Plotly exports, ...) need inline
//! scripts, which the main window's CSP forbids. Documents created from
//! `srcdoc`/`blob:` inherit that CSP, so instead each document is served by
//! this protocol with its own policy, and shown in a sandboxed iframe with an
//! opaque origin that can't reach the terminal or Tauri IPC.

use std::borrow::Cow;
use std::collections::HashMap;
use std::sync::Mutex;

use tauri::http::{header, Request, Response, StatusCode};
use tauri::{Manager, Runtime, State, UriSchemeContext};

pub const SCHEME: &str = "tersh-card";

pub struct CardStore {
    documents: Mutex<HashMap<String, String>>,
    csp: &'static str,
}

const CSP_OFFLINE: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' blob: data:; \
    style-src 'unsafe-inline' data:; img-src data: blob:; font-src data:; media-src data: blob:";

const CSP_NETWORK: &str = "default-src 'none'; script-src 'unsafe-inline' 'unsafe-eval' https: blob: data:; \
    style-src 'unsafe-inline' https: data:; img-src https: data: blob:; font-src https: data:; \
    media-src https: data: blob:; connect-src https:";

impl CardStore {
    pub fn new(network: bool) -> Self {
        Self { documents: Mutex::default(), csp: if network { CSP_NETWORK } else { CSP_OFFLINE } }
    }
}

#[tauri::command]
pub fn card_html_put(store: State<'_, CardStore>, id: String, html: String) {
    store.documents.lock().unwrap().insert(id, html);
}

#[tauri::command]
pub fn card_html_drop(store: State<'_, CardStore>, id: String) {
    store.documents.lock().unwrap().remove(&id);
}

pub fn protocol<R: Runtime>(ctx: UriSchemeContext<'_, R>, request: Request<Vec<u8>>) -> Response<Cow<'static, [u8]>> {
    let store = ctx.app_handle().state::<CardStore>();
    let id = request.uri().path().trim_start_matches('/');
    let document = store.documents.lock().unwrap().get(id).cloned();
    match document {
        Some(html) => Response::builder()
            .header(header::CONTENT_TYPE, "text/html; charset=utf-8")
            .header(header::CONTENT_SECURITY_POLICY, store.csp)
            .header(header::CACHE_CONTROL, "no-store")
            .body(Cow::Owned(html.into_bytes()))
            .unwrap(),
        None => Response::builder().status(StatusCode::NOT_FOUND).body(Cow::Borrowed(&b"card not found"[..])).unwrap(),
    }
}
