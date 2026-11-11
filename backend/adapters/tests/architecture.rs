//! The layering rules, checked on the source: the inner crates do no I/O of
//! their own, and only entry points read the process environment. The
//! frontend's equivalent is `frontend/src/__tests__/architecture.test.ts`.

use std::fs;
use std::path::{Path, PathBuf};

fn sources(crate_dir: &str) -> Vec<(PathBuf, String)> {
    // The crates are siblings in the backend workspace.
    let backend = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let root = backend.join(crate_dir).join("src");
    let mut files = Vec::new();
    let mut dirs = vec![root];
    while let Some(dir) = dirs.pop() {
        for entry in fs::read_dir(&dir).unwrap().flatten() {
            let path = entry.path();
            if path.is_dir() {
                dirs.push(path);
            } else if path.extension().is_some_and(|e| e == "rs") {
                let text = fs::read_to_string(&path).unwrap();
                // Unit tests may use anything.
                let code = text.split("#[cfg(test)]").next().unwrap_or_default().to_string();
                files.push((path, code));
            }
        }
    }
    files
}

fn forbid(crate_dir: &str, rules: &[(&str, &str)]) {
    let mut broken = Vec::new();
    for (path, code) in sources(crate_dir) {
        for (pattern, why) in rules {
            for (n, line) in code.lines().enumerate() {
                let line = line.trim_start();
                if !line.starts_with("//") && line.contains(pattern) {
                    broken.push(format!("{}:{}: `{pattern}`: {why}", path.display(), n + 1));
                }
            }
        }
    }
    assert!(broken.is_empty(), "layering rules broken:\n{}", broken.join("\n"));
}

const NO_ENV: (&str, &str) = ("std::env::var", "only entry points read the environment");
const NO_VARS: (&str, &str) = ("std::env::vars", "only entry points read the environment");

#[test]
fn the_domain_does_no_io() {
    forbid(
        "domain",
        &[
            NO_ENV,
            NO_VARS,
            ("std::fs", "the domain does no I/O"),
            ("std::net", "the domain does no I/O"),
            ("std::process", "the domain does no I/O"),
            ("std::thread", "the domain does no I/O"),
            ("Instant::now", "time comes through TimePort"),
            ("SystemTime::now", "time comes through TimePort"),
            ("log::", "the domain reports problems as values, not logs"),
            ("toml::", "file formats are decoded by adapters"),
        ],
    );
}

#[test]
fn the_use_cases_reach_the_world_through_ports() {
    forbid(
        "application",
        &[
            NO_ENV,
            NO_VARS,
            ("std::fs", "use a port"),
            ("std::net", "use a port"),
            ("std::process", "use a port"),
            ("Instant::now", "time comes through TimePort"),
            ("thread::sleep", "time comes through TimePort"),
            ("anyhow", "fail with AppError"),
        ],
    );
}

#[test]
fn adapters_take_their_configuration() {
    forbid("adapters", &[NO_ENV, NO_VARS, ("anyhow::", "translate errors to AppError")]);
}
