//! How agents reach BashCutPro (#51; `bashcut/agent-connect.md`).
//!
//! - [`automation`]: `<data dir>/bashcut/automation.json`, written by the open window while its
//!   control channel runs; [`automation::live`] checks the process and the port.
//! - [`lock`]: one engine per project: `<name>.fcproj.bashcut/engine.lock`.
//! - [`roots`]: where an agent may write (the project's folder and `BASHCUTPRO_ROOTS`).
//! - [`guard`]: checks MCP `tools/call` requests for writes outside those roots.
//!
//! No workspace dependencies; `apps/bashcut-mcp` puts it together.

pub mod automation;
pub mod guard;
pub mod lock;
pub mod roots;

use std::path::PathBuf;

/// FilmCraft's data directory, as the engine finds it (`FILMCRAFT_DATA_DIR`, else the platform's
/// application-data folder; a portable install passes `--data-dir`).
pub fn default_data_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(d) = env("FILMCRAFT_DATA_DIR") {
        return Some(d);
    }
    if cfg!(target_os = "macos") {
        return env("HOME").map(|h| h.join("Library/Application Support/FilmCraft"));
    }
    if cfg!(windows) {
        return env("APPDATA").map(|h| h.join("FilmCraft"));
    }
    env("XDG_DATA_HOME").map(|d| d.join("filmcraft")).or_else(|| env("HOME").map(|h| h.join(".local/share/filmcraft")))
}

/// Whether a process with this id runs (best effort: `kill -0` on Unix, `tasklist` on Windows).
pub fn pid_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    if pid == std::process::id() {
        return true;
    }
    alive(pid)
}

#[cfg(unix)]
fn alive(pid: u32) -> bool {
    use std::process::{Command, Stdio};
    Command::new("kill").arg("-0").arg(pid.to_string()).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success())
}

#[cfg(windows)]
fn alive(pid: u32) -> bool {
    use std::process::{Command, Stdio};
    Command::new("tasklist")
        .args(["/FI", &format!("PID eq {pid}"), "/NH", "/FO", "CSV"])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .is_ok_and(|o| String::from_utf8_lossy(&o.stdout).contains(&format!("\"{pid}\"")))
}

#[cfg(not(any(unix, windows)))]
fn alive(_pid: u32) -> bool {
    false
}
