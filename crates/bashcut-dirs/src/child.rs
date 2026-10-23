//! The one child-process supervisor (RC3, `bashcut/review/crates.md` D2/I1) of plugin runs
//! (`bashcut-plugin-host` runner, key store, git), agent turns (`bashcut-agent`) and plugin hooks
//! (`engine/src/bashcut/hooks.rs`): [`spawn`] (stdin policy, its own process group on Unix),
//! [`supervise`] (cancel and timeout both stop softly first: TERM, then KILL after [`GRACE`]),
//! [`terminate`], and the capped pipe readers [`pump`] / [`read_lines`] / [`tail`].
//!
//! A fix here (a Windows tree kill, a pipe hang) lands in every runner at once. Nothing here panics:
//! reader threads run under `catch_unwind`, and a grandchild that keeps a pipe open never hangs the
//! caller ([`collect`] waits a bounded time).

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, channel};
use std::time::{Duration, Instant};

/// After a soft stop (cancel or timeout), how long a child has to exit before it is killed.
pub const GRACE: Duration = Duration::from_millis(1500);
/// After the kill, how long to wait for the exit before giving up on the child.
const KILL_WAIT: Duration = Duration::from_secs(5);
const TICK: Duration = Duration::from_millis(20);
/// How long to wait for stdout's reader after the exit (a grandchild may hold the pipe).
pub const STDOUT_WAIT: Duration = Duration::from_secs(2);
/// The same for stderr.
pub const STDERR_WAIT: Duration = Duration::from_millis(500);

/// How [`spawn`] starts the child. stdout is always piped.
#[derive(Clone, Debug, Default)]
pub struct Opts {
    /// Written to stdin, then stdin is closed; `None`: stdin is null.
    pub stdin: Option<String>,
    /// Its own process group on Unix, so a stop also ends what it started (an MCP server).
    pub group: bool,
    /// Pipe stderr (else it goes to null).
    pub stderr: bool,
}

/// Start `cmd` as the caller set it up (args, cwd, env), stdio per `opts`.
pub fn spawn(mut cmd: Command, opts: &Opts) -> std::io::Result<Child> {
    let pipe = |on: bool| if on { Stdio::piped() } else { Stdio::null() };
    cmd.stdin(pipe(opts.stdin.is_some())).stdout(Stdio::piped()).stderr(pipe(opts.stderr));
    #[cfg(unix)]
    if opts.group {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child = cmd.spawn()?;
    if let (Some(text), Some(mut pipe)) = (&opts.stdin, child.stdin.take()) {
        // all of it, then EOF (the pipe drops here)
        if let Err(e) = pipe.write_all(text.as_bytes()) {
            log::warn!("bashcut child stdin: {e}");
        }
    }
    Ok(child)
}

/// Why [`supervise`] stopped the child.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    Cancelled,
    TimedOut,
}

/// How a supervised child ended.
#[derive(Debug)]
pub struct Exit {
    /// `None`: the wait failed, or the child outlived its kill.
    pub status: Option<ExitStatus>,
    pub stop: Option<Stop>,
}

impl Exit {
    pub fn code(&self) -> Option<i32> {
        self.status.and_then(|s| s.code())
    }
}

/// Wait for `child`; on `cancel` or after `timeout`, stop it softly ([`terminate`]), then kill it
/// after [`GRACE`]. Blocks: run it on a worker thread.
pub fn supervise(child: &mut Child, timeout: Duration, cancel: Option<&AtomicBool>) -> Exit {
    let start = Instant::now();
    let mut stop: Option<(Stop, Instant)> = None;
    let mut killed: Option<Instant> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        match (stop, killed) {
            (None, _) => {
                let why = if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
                    Some(Stop::Cancelled)
                } else if start.elapsed() > timeout {
                    Some(Stop::TimedOut)
                } else {
                    None
                };
                if let Some(why) = why {
                    terminate(child, false);
                    stop = Some((why, Instant::now()));
                }
            }
            (Some((_, since)), None) if since.elapsed() > GRACE => {
                terminate(child, true);
                killed = Some(Instant::now());
            }
            (Some(_), Some(at)) if at.elapsed() > KILL_WAIT => break None,
            _ => {}
        }
        std::thread::sleep(TICK);
    };
    Exit { status, stop: stop.map(|(w, _)| w) }
}

/// Stop the child (and on Unix its process group): TERM first, KILL when `hard`. On Unix a child
/// without a group gets the signal itself; off Unix a soft stop is a kill too, as there is no TERM to send. On Windows the kill takes
/// the whole tree (`taskkill /T /F`), so grandchildren go too; a job object would need `unsafe`,
/// which only `crates/platform` may use (RC13, `bashcut/decisions.md`).
pub fn terminate(child: &mut Child, hard: bool) {
    #[cfg(unix)]
    {
        let sig = if hard { "-KILL" } else { "-TERM" };
        let pid = child.id().to_string();
        let kill =
            |target: &str| Command::new("kill").args([sig, "--", target]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        // the group first; a child without one (or whose group is gone) gets the signal itself
        if kill(&format!("-{pid}")) || kill(&pid) {
            return;
        }
    }
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        /// No console window flashes up for the helper.
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        let pid = child.id().to_string();
        let ok = Command::new("taskkill")
            .args(["/PID", &pid, "/T", "/F"])
            .creation_flags(CREATE_NO_WINDOW)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status()
            .is_ok_and(|s| s.success());
        if ok {
            return;
        }
    }
    if hard || cfg!(not(unix)) {
        let _ = child.kill();
    }
}

/// Read `r` with `f` on a named thread under `catch_unwind`; the receiver gets what `f` returns
/// (`T::default()` when it panicked or `r` is `None`). Wait for it with [`collect`].
pub fn pump<R, T>(name: &str, r: Option<R>, f: impl FnOnce(R) -> T + Send + 'static) -> Receiver<T>
where
    R: Read + Send + 'static,
    T: Default + Send + 'static,
{
    let (tx, rx) = channel();
    let Some(r) = r else {
        let _ = tx.send(T::default());
        return rx;
    };
    let fallback = tx.clone();
    let started = std::thread::Builder::new().name(name.into()).spawn(move || {
        let _ = tx.send(catch_unwind(AssertUnwindSafe(|| f(r))).unwrap_or_default());
    });
    if started.is_err() {
        let _ = fallback.send(T::default());
    }
    rx
}

/// What a [`pump`] returned, waiting at most `wait` (then `T::default()`).
pub fn collect<T: Default>(rx: &Receiver<T>, wait: Duration) -> T {
    rx.recv_timeout(wait).unwrap_or_default()
}

/// Read `r` to its end (so the child never blocks on a full pipe), keeping its first `keep` bytes
/// (and whether there was more), and call `f` with each line: its bytes (at most `max_line`, without
/// the `\n`) and whether it was longer than that. The last line without `\n` counts when it has bytes.
pub fn read_lines<R: Read>(mut r: R, keep: usize, max_line: usize, mut f: impl FnMut(&[u8], bool)) -> (Vec<u8>, bool) {
    let mut buf = Vec::new();
    let mut truncated = false;
    let mut line: Vec<u8> = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 16 * 1024];
    loop {
        let n = match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        let got = chunk.get(..n).unwrap_or_default();
        let take = n.min(keep.saturating_sub(buf.len()));
        buf.extend_from_slice(got.get(..take).unwrap_or_default());
        truncated |= take < n;
        for &b in got {
            if b == b'\n' {
                f(&line, cut);
                line.clear();
                cut = false;
            } else if line.len() < max_line {
                line.push(b);
            } else {
                cut = true;
            }
        }
    }
    if !line.is_empty() || cut {
        f(&line, cut);
    }
    (buf, truncated)
}

/// The last `cap` bytes of `r` (read to its end): what a failed child said last.
pub fn tail<R: Read>(mut r: R, cap: usize) -> Vec<u8> {
    let mut tail: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 4096];
    while let Ok(n) = r.read(&mut chunk) {
        if n == 0 {
            break;
        }
        tail.extend_from_slice(chunk.get(..n).unwrap_or_default());
        if tail.len() > cap.saturating_mul(2) {
            let cut = tail.len().saturating_sub(cap);
            tail.drain(..cut);
        }
    }
    let start = tail.len().saturating_sub(cap);
    tail.split_off(start)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_keep_cut_and_tail() {
        let mut got = Vec::new();
        let (buf, truncated) = read_lines(&b"ab\nxxxxx\n\nend"[..], 4, 3, |l, cut| got.push((l.to_vec(), cut)));
        assert_eq!(got, [(b"ab".to_vec(), false), (b"xxx".to_vec(), true), (Vec::new(), false), (b"end".to_vec(), false)]);
        assert_eq!((buf.as_slice(), truncated), (&b"ab\nx"[..], true));
        assert_eq!(tail(&b"0123456789"[..], 3), b"789");
    }

    #[cfg(unix)]
    fn sh(script: &str) -> Command {
        let mut cmd = Command::new("sh");
        cmd.args(["-c", script]);
        cmd
    }

    #[cfg(unix)]
    #[test]
    fn timeout_is_soft_first_then_kill() {
        use std::os::unix::process::ExitStatusExt;
        let opts = Opts { group: true, ..Opts::default() };
        // TERM is enough for a child that honours it: it ends well before GRACE
        let mut c = spawn(sh("sleep 30"), &opts).unwrap();
        let start = Instant::now();
        let exit = supervise(&mut c, Duration::from_millis(100), None);
        assert_eq!((exit.stop, exit.status.and_then(|s| s.signal())), (Some(Stop::TimedOut), Some(15)));
        assert!(start.elapsed() < GRACE, "{:?}", start.elapsed());
        // a child (and its group) ignoring TERM is killed after GRACE
        let mut c = spawn(sh("trap '' TERM; sleep 30"), &opts).unwrap();
        let start = Instant::now();
        let exit = supervise(&mut c, Duration::from_millis(100), None);
        assert_eq!((exit.stop, exit.status.and_then(|s| s.signal())), (Some(Stop::TimedOut), Some(9)));
        assert!(start.elapsed() >= GRACE && start.elapsed() < GRACE + Duration::from_secs(5), "{:?}", start.elapsed());
        // without a group the child itself gets TERM, so it still ends well before GRACE
        let mut c = spawn(sh("exec sleep 30"), &Opts::default()).unwrap();
        let start = Instant::now();
        let exit = supervise(&mut c, Duration::from_millis(100), None);
        assert_eq!((exit.stop, exit.status.and_then(|s| s.signal())), (Some(Stop::TimedOut), Some(15)));
        assert!(start.elapsed() < GRACE, "no group: {:?}", start.elapsed());
        // cancel stops it the same way
        let mut c = spawn(sh("sleep 30"), &opts).unwrap();
        let exit = supervise(&mut c, Duration::from_secs(60), Some(&AtomicBool::new(true)));
        assert_eq!(exit.stop, Some(Stop::Cancelled));
        // a child that ends in time is not stopped
        let mut c = spawn(sh("exit 3"), &opts).unwrap();
        let exit = supervise(&mut c, Duration::from_secs(60), None);
        assert_eq!((exit.stop, exit.code()), (None, Some(3)));
    }

    #[cfg(unix)]
    #[test]
    fn grandchild_pipe_does_not_hang() {
        // the child exits at once, its background grandchild keeps stdout open for 30 s
        let mut c = spawn(sh("sleep 30 & echo hi"), &Opts { group: true, ..Opts::default() }).unwrap();
        let reader = pump("test-stdout", c.stdout.take(), |out| read_lines(out, 1024, 1024, |_, _| {}).0);
        let exit = supervise(&mut c, Duration::from_secs(10), None);
        assert_eq!((exit.stop, exit.code()), (None, Some(0)));
        let start = Instant::now();
        let out = collect(&reader, STDOUT_WAIT);
        let waited = start.elapsed();
        assert!(waited >= STDOUT_WAIT && waited < STDOUT_WAIT + Duration::from_secs(2), "{waited:?}");
        assert!(out.is_empty(), "still being read: the default");
        // the grandchild is still in the group: end it
        terminate(&mut c, true);
    }

    /// The pids of `pid`'s children (PowerShell's CIM query: no `unsafe`, no extra crate).
    #[cfg(windows)]
    fn children(pid: u32) -> Vec<u32> {
        let query = format!("(Get-CimInstance Win32_Process -Filter 'ParentProcessId={pid}').ProcessId");
        let out = Command::new("powershell").args(["-NoProfile", "-NonInteractive", "-Command", &query]).output().unwrap();
        String::from_utf8_lossy(&out.stdout).split_whitespace().filter_map(|p| p.parse().ok()).collect()
    }

    #[cfg(windows)]
    #[test]
    fn windows_terminate_kills_tree() {
        // cmd is the child, ping.exe its grandchild: a cancel has to end both (taskkill /T)
        let mut cmd = Command::new("cmd");
        cmd.args(["/c", "ping", "-n", "120", "127.0.0.1"]);
        let mut c = spawn(cmd, &Opts { group: true, ..Opts::default() }).unwrap();
        let reader = pump("test-stdout", c.stdout.take(), |out| read_lines(out, 1024, 1024, |_, _| {}).0);
        let start = Instant::now();
        let ping = loop {
            if let Some(&p) = children(c.id()).first() {
                break p;
            }
            assert!(start.elapsed() < Duration::from_secs(20), "ping.exe never started");
            std::thread::sleep(Duration::from_millis(100));
        };
        assert!(crate::process::pid_alive(ping));
        let start = Instant::now();
        let exit = supervise(&mut c, Duration::from_secs(60), Some(&AtomicBool::new(true)));
        assert_eq!(exit.stop, Some(Stop::Cancelled));
        assert!(exit.status.is_some(), "cmd ended");
        assert!(start.elapsed() < GRACE + Duration::from_secs(5), "{:?}", start.elapsed());
        // no ping.exe left holding the pipe: stdout reaches EOF well before STDOUT_WAIT
        let start = Instant::now();
        let _ = collect(&reader, STDOUT_WAIT);
        assert!(start.elapsed() < STDOUT_WAIT, "stdout still held: {:?}", start.elapsed());
        let start = Instant::now();
        while crate::process::pid_alive(ping) && start.elapsed() < Duration::from_secs(5) {
            std::thread::sleep(Duration::from_millis(100));
        }
        assert!(!crate::process::pid_alive(ping), "ping.exe {ping} outlived the cancel");
    }
}
