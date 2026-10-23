//! Child processes: start one with its output in log files, wait with a timeout and a cancel
//! check, and tell whether a process id is alive (for the runner lock and the tripwire).
//!
//! A step runs in its own process group (Unix) and a stop goes through the shared supervisor's
//! [`bashcut_dirs::child::terminate`]: what an agent started (python, ffmpeg) ends with it on cancel,
//! timeout or over the memory cap, and on Windows the kill takes the whole tree (RC13). The batch
//! keeps its own spawn because its output goes to log files, not to pipes.

use crate::book::{Category, ErrorInfo};
use bashcut_dirs::child;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
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
/// ([`Ended::OverLimit`]). The timeout counts only time the machine was awake. Every stop takes the
/// process's group (Unix) or tree (Windows) with it ([`stop`]).
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
    bashcut_dirs::child::no_window(&mut cmd);
    if let Some(cwd) = &spec.cwd {
        cmd.current_dir(cwd);
    }
    for (k, v) in &spec.env {
        cmd.env(k, v);
    }
    // its own group, so a stop also ends its grandchildren (as `child::spawn` with `group: true`)
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
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
                stop(&mut child);
                return Err(ErrorInfo::new(Category::Crashed, format!("{program}: {e}")));
            }
        }
        if cancelled() {
            stop(&mut child);
            return Ok(Ended::Cancelled);
        }
        if spec.timeout.is_some_and(|t| awake >= t) {
            stop(&mut child);
            return Ok(Ended::TimedOut);
        }
        if let Some(why) = watch(pid) {
            stop(&mut child);
            return Ok(Ended::OverLimit(why));
        }
        std::thread::sleep(poll);
    }
}

/// Stop a step: TERM to its group (on Windows the tree is killed at once), [`child::GRACE`] to
/// exit, then KILL, so a grandchild that ignores TERM or outlives the step goes too.
fn stop(c: &mut Child) {
    child::terminate(c, false);
    let until = Instant::now() + child::GRACE;
    let mut exited = false;
    while Instant::now() < until {
        if let Ok(Some(_)) | Err(_) = c.try_wait() {
            exited = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    if exited {
        // reaped: signal only the group (its id is not reused while a member lives), never the pid
        kill_group(c.id());
    } else {
        child::terminate(c, true);
    }
    let _ = c.wait();
}

/// KILL whatever is left in the process group `pgid` (Unix; on Windows the tree kill already did it).
fn kill_group(pgid: u32) {
    #[cfg(unix)]
    {
        let _ = Command::new("kill").args(["-KILL", "--", &format!("-{pgid}")]).stdin(Stdio::null()).stdout(Stdio::null()).stderr(Stdio::null()).status();
    }
    #[cfg(not(unix))]
    let _ = pgid;
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
    fn cap_cuts_by_chars_and_tail_raw_keeps_whitespace() {
        assert_eq!(cap("abc", 3), "abc");
        assert_eq!(cap("abcd", 3), "abc…");
        assert_eq!(cap("", 0), "");
        assert_eq!(cap("x", 0), "…");
        // characters, not bytes: never cuts inside a multi-byte one
        assert_eq!(cap("tiếng việt", 5), "tiếng…");
        assert_eq!(cap("日本語テキスト", 3), "日本語…");
        assert_eq!(cap("日本語", 3), "日本語");

        let p = std::env::temp_dir().join(format!("bashcut-batch-tail-raw-{}", std::process::id()));
        std::fs::write(&p, b"  head\n  body \n\n").unwrap();
        assert_eq!(tail_raw(&p, 8), " body \n\n");
        assert_eq!(tail_raw(&p, 100), "  head\n  body \n\n");
        assert_eq!(tail(&p, 8), "body");
        assert_eq!(tail_raw(&p, 0), "");
        assert_eq!(tail_raw(&p.with_extension("missing"), 8), "");
        let _ = std::fs::remove_file(&p);
    }

    /// A fake agent that starts a grandchild (its tool process), writes the grandchild's pid to a
    /// file and waits for it; `stop` decides when the step is stopped, from the elapsed time.
    fn grandchild_run(name: &str, argv: impl Fn(&Path) -> Vec<String>, timeout: Option<Duration>, stop: impl Fn(Duration) -> bool) -> (Ended, u32) {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-group-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("grandchild.pid");
        let spec = Spec { argv: argv(&pid_file), timeout, log: dir.join("step"), ..Spec::default() };
        let read_pid = || std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse::<u32>().ok());
        let start = Instant::now();
        // only stop once the grandchild is running
        let ended = run(&spec, Duration::from_millis(20), &mut |_| {}, &|| read_pid().is_some() && stop(start.elapsed())).unwrap();
        let pid = read_pid().unwrap_or_else(|| panic!("no grandchild pid; stderr: {}", tail(&err_path(&spec.log), 400)));
        let _ = std::fs::remove_dir_all(&dir);
        (ended, pid)
    }

    /// The grandchild is gone within a few seconds (once orphaned, init reaps it).
    fn assert_gone(pid: u32, what: &str) {
        let start = Instant::now();
        while crate::pid_alive(pid) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(50));
        }
        assert!(!crate::pid_alive(pid), "{what}: grandchild {pid} still running");
    }

    #[cfg(unix)]
    fn sh_agent(script: &'static str) -> impl Fn(&Path) -> Vec<String> {
        move |pid_file: &Path| vec!["sh".into(), "-c".into(), script.replace("{pid}", &pid_file.display().to_string())]
    }

    #[cfg(unix)]
    #[test]
    fn cancel_and_timeout_end_the_grandchildren() {
        let agent = "sleep 30 & echo $! > '{pid}'; wait";
        let (ended, pid) = grandchild_run("cancel", sh_agent(agent), None, |_| true);
        assert_eq!(ended, Ended::Cancelled);
        assert_gone(pid, "cancel");

        let (ended, pid) = grandchild_run("timeout", sh_agent(agent), Some(Duration::from_millis(300)), |_| false);
        assert_eq!(ended, Ended::TimedOut);
        assert_gone(pid, "timeout");

        // a grandchild that ignores TERM (and outlives the agent, which exits on TERM) is killed after the grace
        let stubborn = "(trap '' TERM; while :; do sleep 1; done) & echo $! > '{pid}'; wait";
        let start = Instant::now();
        let (ended, pid) = grandchild_run("stubborn", sh_agent(stubborn), None, |_| true);
        assert_eq!(ended, Ended::Cancelled);
        assert_gone(pid, "TERM ignored");
        assert!(start.elapsed() < bashcut_dirs::child::GRACE + Duration::from_secs(5), "{:?}", start.elapsed());

        // over the memory cap: the same stop
        let dir = std::env::temp_dir().join(format!("bashcut-batch-group-over-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pid_file = dir.join("grandchild.pid");
        let spec = Spec { argv: sh_agent(agent)(&pid_file), log: dir.join("step"), ..Spec::default() };
        let read_pid = || std::fs::read_to_string(&pid_file).ok().and_then(|s| s.trim().parse::<u32>().ok());
        let ended = run_watched(&spec, Duration::from_millis(20), &mut |_| {}, &|| false, &mut |_| read_pid().map(|_| "over".to_string())).unwrap();
        assert_eq!(ended, Ended::OverLimit("over".into()));
        assert_gone(read_pid().unwrap(), "over the cap");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[cfg(windows)]
    #[test]
    fn cancel_and_timeout_end_the_grandchildren() {
        // the fake agent starts ping (its tool process), writes its pid and waits for it
        let agent = |pid_file: &Path| {
            let script = format!(
                "$p = Start-Process -FilePath ping -ArgumentList '-n','60','127.0.0.1' -PassThru -NoNewWindow; \
                 Set-Content -Encoding ascii -Path '{}' -Value $p.Id; Wait-Process -Id $p.Id",
                pid_file.display()
            );
            vec!["powershell".into(), "-NoProfile".into(), "-NonInteractive".into(), "-Command".into(), script]
        };
        let (ended, pid) = grandchild_run("cancel", agent, None, |_| true);
        assert_eq!(ended, Ended::Cancelled);
        assert_gone(pid, "cancel");

        let (ended, pid) = grandchild_run("timeout", agent, Some(Duration::from_secs(3)), |_| false);
        assert_eq!(ended, Ended::TimedOut);
        assert_gone(pid, "timeout");
    }

    #[test]
    fn empty_command_line_is_a_spawn_error() {
        let spec = Spec { log: std::env::temp_dir().join("bashcut-batch-empty"), ..Spec::default() };
        let r = run(&spec, Duration::from_millis(5), &mut |_| {}, &|| false);
        assert_eq!(r.err().map(|e| e.category), Some(Category::Spawn));
    }
}
