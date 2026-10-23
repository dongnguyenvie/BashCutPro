//! Whether a process runs (RC11): the one `pid_alive` for `bashcut-connect` (window, engine lock) and
//! `bashcut-batch` (the book's runner pid).

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
    crate::child::no_window(&mut Command::new("tasklist"))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn our_own_pid_is_alive_and_zero_is_not() {
        assert!(pid_alive(std::process::id()));
        assert!(!pid_alive(0));
    }
}
