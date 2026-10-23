//! Child processes: start one with its output in log files, wait with a timeout and a cancel
//! check, and tell whether a process id is alive (for the runner lock and the tripwire).

use crate::book::{Category, ErrorInfo};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

/// A process to run for one step.
#[derive(Clone, Debug, Default)]
pub struct Spec {
    pub argv: Vec<String>,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
    pub timeout: Option<Duration>,
    /// stdout and stderr go to `<log>.out` and `<log>.err`.
    pub log: PathBuf,
}

/// How a process ended.
#[derive(Clone, Debug, PartialEq)]
pub enum Ended {
    /// Exit status (`None`: killed by a signal).
    Exited(Option<i32>),
    TimedOut,
    Cancelled,
    /// Stopped by the watch (over the memory cap, #61), with why.
    OverLimit(String),
}

/// A gap this long between two polls means the machine slept: it does not count toward a timeout.
const SLEEP_GAP: Duration = Duration::from_secs(30);

pub fn out_path(log: &Path) -> PathBuf {
    with_suffix(log, ".out")
}

pub fn err_path(log: &Path) -> PathBuf {
    with_suffix(log, ".err")
}

fn with_suffix(p: &Path, suffix: &str) -> PathBuf {
    let mut s = p.as_os_str().to_owned();
    s.push(suffix);
    PathBuf::from(s)
}

#[cfg(test)]
/// Start `spec`, report its pid through `started`, and wait. `cancelled` is asked every `poll`.
pub fn run(spec: &Spec, poll: Duration, started: &mut dyn FnMut(u32), cancelled: &dyn Fn() -> bool) -> Result<Ended, ErrorInfo> {
    run_watched(spec, poll, started, cancelled, &mut |_| None)
}

/// Start `spec`, report its pid through `started` and wait, also asking `watch` with the pid every `poll`: `Some(why)` stops the process
/// ([`Ended::OverLimit`]). The timeout counts only time the machine was awake.
pub fn run_watched(
    spec: &Spec,
    poll: Duration,
    started: &mut dyn FnMut(u32),
    cancelled: &dyn Fn() -> bool,
    watch: &mut dyn FnMut(u32) -> Option<String>,
) -> Result<Ended, ErrorInfo> {
    let Some((program, args)) = spec.argv.split_first() else {
        return Err(ErrorInfo::new(Category::Spawn, "empty command line"));
    };
    let io = |e: std::io::Error, p: &Path| ErrorInfo::new(Category::Io, format!("{}: {e}", p.display()));
    let (out, err) = (out_path(&spec.log), err_path(&spec.log));
    let stdout = std::fs::File::create(&out).map_err(|e| io(e, &out))?;
    let stderr = std::fs::File::create(&err).map_err(|e| io(e, &err))?;
    let mut cmd = Command::new(program);
    cmd.args(args).stdin(Stdio::null()).stdout(stdout).stderr(stderr);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    let mut child = cmd.spawn().map_err(|e| ErrorInfo::new(Category::Spawn, format!("{program}: {e}")))?;
    let pid = child.id();
    started(pid);
    let mut awake = Duration::ZERO;
    let mut last = Instant::now();
    loop {
        let gap = last.elapsed();
        last = Instant::now();
        // after a sleep only one poll counts
        awake = awake.saturating_add(if gap >= SLEEP_GAP { poll } else { gap });
        match child.try_wait() {
            Ok(Some(status)) => return Ok(Ended::Exited(status.code())),
            Ok(None) => {}
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Err(ErrorInfo::new(Category::Crashed, format!("{program}: {e}")));
            }
        }
        if cancelled() {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Ended::Cancelled);
        }
        if spec.timeout.is_some_and(|t| awake >= t) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Ended::TimedOut);
        }
        if let Some(why) = watch(pid) {
            let _ = child.kill();
            let _ = child.wait();
            return Ok(Ended::OverLimit(why));
        }
        std::thread::sleep(poll);
    }
}

/// The last `max` bytes of a file as text (lossy), trimmed.
pub fn tail(path: &Path, max: u64) -> String {
    tail_raw(path, max).trim().to_string()
}

/// The last `max` bytes of a file as text (lossy), as read; empty when it cannot be read.
pub fn tail_raw(path: &Path, max: u64) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if f.seek(SeekFrom::Start(len.saturating_sub(max))).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.take(max).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

/// `s` cut to at most `max` characters, with `…` appended when it was cut.
pub fn cap(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect::<String>() + "…" }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tail_reads_the_end() {
        let p = std::env::temp_dir().join(format!("bashcut-batch-tail-{}", std::process::id()));
        std::fs::write(&p, b"0123456789\n").unwrap();
        assert_eq!(tail(&p, 4), "789");
        assert_eq!(tail(&p, 100), "0123456789");
        assert_eq!(tail(&p.with_extension("missing"), 4), "");
        let _ = std::fs::remove_file(&p);
    }

    #[test]
    fn empty_command_line_is_a_spawn_error() {
        let spec = Spec { log: std::env::temp_dir().join("bashcut-batch-empty"), ..Spec::default() };
        let r = run(&spec, Duration::from_millis(5), &mut |_| {}, &|| false);
        assert_eq!(r.err().map(|e| e.category), Some(Category::Spawn));
    }
}
