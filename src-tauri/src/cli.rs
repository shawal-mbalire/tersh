//! Command line: `tersh [OPTIONS]` opens a window; `tersh update` updates.

use std::path::PathBuf;

pub const USAGE: &str = "\
Usage: tersh [OPTIONS]
       tersh update [--check]

A lightweight terminal with rich media cards.

Commands:
  update           Install the latest release. Open windows restart into it
                   when their shell is idle.
  update --check   Only report whether a newer release exists.

Options:
  --cwd DIR        Start the shell in DIR.
  --input TEXT     Type TEXT into the shell once it starts.
  --size WxH       Initial window size in logical pixels, e.g. 1000x640.
  -V, --version    Print the version.
  -h, --help       Show this help.

Settings: ~/.config/tersh/config.toml (Ctrl+, in the app).
";

/// How to start a window.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Launch {
    pub cwd: Option<PathBuf>,
    pub input: Option<String>,
    pub size: Option<(f64, f64)>,
}

impl Launch {
    /// Arguments that reproduce these options (for relaunching).
    pub fn to_args(&self) -> Vec<String> {
        let mut args = Vec::new();
        if let Some(cwd) = &self.cwd {
            args.extend(["--cwd".into(), cwd.display().to_string()]);
        }
        if let Some(input) = &self.input {
            args.extend(["--input".into(), input.clone()]);
        }
        if let Some((w, h)) = self.size {
            args.extend(["--size".into(), format!("{}x{}", w.round(), h.round())]);
        }
        args
    }
}

#[derive(Debug, Clone, PartialEq)]
pub enum Command {
    Run(Launch),
    Update { check: bool },
    Version,
    Help,
}

pub fn parse(args: impl IntoIterator<Item = String>) -> Result<Command, String> {
    let mut args = args.into_iter().peekable();
    if args.peek().map(String::as_str) == Some("update") {
        args.next();
        let mut check = false;
        for arg in args {
            match arg.as_str() {
                "--check" | "-c" => check = true,
                "-h" | "--help" => return Ok(Command::Help),
                other => return Err(format!("unknown option for update: {other}")),
            }
        }
        return Ok(Command::Update { check });
    }

    let mut launch = Launch::default();
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "-h" | "--help" => return Ok(Command::Help),
            "-V" | "--version" => return Ok(Command::Version),
            "--cwd" => launch.cwd = Some(PathBuf::from(value(&arg)?)),
            "--input" => launch.input = Some(value(&arg)?),
            "--size" => {
                let text = value(&arg)?;
                let (w, h) = text.split_once(['x', 'X']).ok_or("--size needs WIDTHxHEIGHT")?;
                let parse = |v: &str| v.trim().parse::<f64>().ok().filter(|v| *v >= 100.0);
                launch.size = Some((parse(w).ok_or("bad --size width")?, parse(h).ok_or("bad --size height")?));
            },
            // Added by macOS when launched from Finder on old systems.
            s if s.starts_with("-psn_") => {},
            other => return Err(format!("unknown argument: {other}")),
        }
    }
    Ok(Command::Run(launch))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse_str(args: &str) -> Result<Command, String> {
        parse(args.split_whitespace().map(String::from))
    }

    #[test]
    fn commands() {
        assert_eq!(parse_str(""), Ok(Command::Run(Launch::default())));
        assert_eq!(parse_str("update"), Ok(Command::Update { check: false }));
        assert_eq!(parse_str("update --check"), Ok(Command::Update { check: true }));
        assert_eq!(parse_str("--version"), Ok(Command::Version));
        assert!(parse_str("update --force").is_err());
        assert!(parse_str("--bogus").is_err());
    }

    #[test]
    fn launch_options_round_trip() {
        let launch = Launch { cwd: Some("/tmp/x".into()), input: Some("tmux attach\r".into()), size: Some((1000.0, 640.0)) };
        assert_eq!(parse(launch.to_args()), Ok(Command::Run(launch)));
        assert!(parse_str("--size 50x50").is_err());
    }
}
