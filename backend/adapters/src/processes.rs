//! Process adapter: names and working directories of running processes, from
//! `/proc` on Linux and `ps`/`lsof` on other Unix systems.

use std::path::PathBuf;
use std::process::Command;

use tersh_app::ports::ProcessInspector;

#[derive(Debug, Default, Clone, Copy)]
pub struct SystemProcesses;

impl ProcessInspector for SystemProcesses {
    fn name(&self, pid: u32) -> Option<String> {
        if cfg!(target_os = "linux") {
            let comm = std::fs::read_to_string(format!("/proc/{pid}/comm")).ok()?;
            Some(comm.trim_end().to_string()).filter(|s| !s.is_empty())
        } else {
            let out = command_output(Command::new("ps").args(["-o", "comm=", "-p", &pid.to_string()]))?;
            // `ps` prints a path on macOS; the file name is the program.
            out.lines().next().map(|l| l.rsplit('/').next().unwrap_or(l).trim().to_string()).filter(|s| !s.is_empty())
        }
    }

    fn cwd(&self, pid: u32) -> Option<PathBuf> {
        if cfg!(target_os = "linux") {
            std::fs::read_link(format!("/proc/{pid}/cwd")).ok()
        } else {
            // `lsof -Fn` prints fields one per line; the `n` field is the path.
            let out = command_output(Command::new("lsof").args(["-a", "-d", "cwd", "-p", &pid.to_string(), "-Fn"]))?;
            out.lines().find_map(|l| l.strip_prefix('n')).map(PathBuf::from)
        }
    }

    fn memory(&self, pid: u32) -> Option<u64> {
        let kib: u64 = if cfg!(target_os = "linux") {
            let status = std::fs::read_to_string(format!("/proc/{pid}/status")).ok()?;
            status.lines().find_map(|l| l.strip_prefix("VmRSS:"))?.trim().trim_end_matches("kB").trim().parse().ok()?
        } else if cfg!(unix) {
            command_output(Command::new("ps").args(["-o", "rss=", "-p", &pid.to_string()]))?.trim().parse().ok()?
        } else {
            return None;
        };
        Some(kib * 1024)
    }
}

fn command_output(command: &mut Command) -> Option<String> {
    let output = command.output().ok().filter(|o| o.status.success())?;
    String::from_utf8(output.stdout).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inspects_this_process() {
        let pid = std::process::id();
        let name = SystemProcesses.name(pid).expect("own name");
        assert!(!name.is_empty());
        assert_eq!(SystemProcesses.cwd(pid), std::env::current_dir().ok());
        if cfg!(unix) {
            assert!(SystemProcesses.memory(pid).is_some_and(|bytes| bytes > 0));
        }
    }
}
