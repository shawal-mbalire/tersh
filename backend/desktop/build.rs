//! Tauri's build step, plus the two things it needs from outside this crate,
//! found from the crate's parents rather than with paths that climb out of it:
//!
//! - the page, `frontend/dist`: Tauri reads `frontendDist` relative to
//!   `tauri.conf.json`, so it is given here through the `TAURI_CONFIG` overlay
//!   that both `tauri-build` and `generate_context!` merge into the config;
//! - the third-party notices, `THIRD-PARTY.txt` at the repository root: copied
//!   into this crate (git-ignored), where `bundle > resources` names it, so
//!   every installer ships it next to the app. The Tauri CLI bundles after
//!   cargo has run this script, and `tauri-build` below copies it beside the
//!   binary for development builds.

use std::fs;
use std::path::Path;

use serde_json::{json, Value};

fn main() {
    println!("cargo:rerun-if-env-changed=TAURI_CONFIG");
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("cargo sets CARGO_MANIFEST_DIR");
    // backend/desktop → backend → the repository.
    let repo = Path::new(&manifest).ancestors().nth(2).expect("backend/desktop has a repository above it");
    let dist = repo.join("frontend").join("dist");

    bundle_notices(repo, Path::new(&manifest));

    // The Tauri CLI passes its own overlay (from `--config`) the same way: keep it.
    let mut overlay = match std::env::var("TAURI_CONFIG") {
        Ok(text) => serde_json::from_str(&text).expect("TAURI_CONFIG is a JSON object"),
        Err(_) => json!({}),
    };
    if overlay.pointer("/build/frontendDist").is_none() {
        overlay["build"]["frontendDist"] = Value::String(dist.to_string_lossy().into_owned());
    }
    let overlay = overlay.to_string();
    // For tauri-build below, and for `generate_context!` when the crate compiles.
    std::env::set_var("TAURI_CONFIG", &overlay);
    println!("cargo:rustc-env=TAURI_CONFIG={overlay}");

    tauri_build::build()
}

/// Copy `THIRD-PARTY.txt` into this crate for `bundle > resources`. Written
/// only when it changed, so its timestamp doesn't make cargo rebuild every time.
fn bundle_notices(repo: &Path, krate: &Path) {
    let source = repo.join("THIRD-PARTY.txt");
    let copy = krate.join("THIRD-PARTY.txt");
    println!("cargo:rerun-if-changed={}", source.display());
    let notices = fs::read(&source).unwrap_or_else(|e| panic!("reading {}: {e}", source.display()));
    if fs::read(&copy).ok().as_deref() != Some(notices.as_slice()) {
        fs::write(&copy, notices).unwrap_or_else(|e| panic!("writing {}: {e}", copy.display()));
    }
}
