//! Run one plugin action as a child process: stderr lines are progress, stdout holds one
//! `bashcut-result/1` object (and, while it runs, `bashcut-view/1` lines that update the plugin's
//! form), exit 3 is a hand-off to the agent. Cancellable, with a timeout and
//! capped output. Every thread runs under `catch_unwind`, and the UI only polls a channel. The
//! process itself is supervised by [`bashcut_dirs::child`] (shared with agent turns and hooks).

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::PathBuf;
use std::process::Child;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

use bashcut_dirs::child::{self, Opts, Stop};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::view::{MAX_VIEW_LINE, View};
use crate::{Error, Result, err};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(30 * 60);
pub const DEFAULT_MAX_STDOUT: usize = 4 * 1024 * 1024;
/// Longest stderr line kept (the rest of the line is dropped).
pub const MAX_LINE: usize = 4096;
pub const HANDOFF_EXIT: i32 = 3;
const TICK: Duration = Duration::from_millis(20);

#[derive(Clone, Debug)]
pub struct RunSpec {
    /// The plugin folder (working directory).
    pub dir: PathBuf,
    pub argv: Vec<String>,
    pub env: Vec<(String, String)>,
    pub timeout: Duration,
    pub max_stdout: usize,
    /// The exit status that means "hand off" (`handoff.exit`, 3).
    pub handoff_exit: i32,
}

impl RunSpec {
    pub fn new(dir: PathBuf, argv: Vec<String>) -> Self {
        Self { dir, argv, env: Vec::new(), timeout: DEFAULT_TIMEOUT, max_stdout: DEFAULT_MAX_STDOUT, handoff_exit: HANDOFF_EXIT }
    }
}

/// How a run ended.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum End {
    Succeeded,
    Failed,
    /// Exit 3: `data` holds what the agent needs to finish the job.
    HandOff,
    Cancelled,
    TimedOut,
    /// Killed by a signal or the host lost it.
    Crashed,
}

/// `bashcut-result/1`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct PluginResult {
    pub ok: bool,
    pub message: String,
    pub data: Value,
    pub error: Option<ResultError>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ResultError {
    pub code: String,
    pub message: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Outcome {
    pub end: End,
    pub exit_code: Option<i32>,
    pub result: Option<PluginResult>,
    /// stdout went over the cap; the result was looked for in what was kept.
    pub stdout_truncated: bool,
    /// What went wrong on the host's side (start failure, timeout, no result).
    pub error: Option<String>,
}

impl Outcome {
    /// One line for a list: the result's message or error, else the host's.
    pub fn summary(&self) -> String {
        if let Some(r) = &self.result {
            if let Some(e) = &r.error {
                return if e.code.is_empty() { e.message.clone() } else { format!("{}: {}", e.code, e.message) };
            }
            if !r.message.is_empty() {
                return r.message.clone();
            }
        }
        self.error.clone().unwrap_or_default()
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum Event {
    /// One stderr line: `fraction` in 0..=1 when the line carries one.
    Progress {
        fraction: Option<f32>,
        message: String,
    },
    /// A `bashcut-view/1` line on stdout: the plugin's form as it is now.
    View(View),
    Done(Outcome),
}

/// A started run.
pub struct Running {
    rx: Receiver<Event>,
    cancel: Arc<AtomicBool>,
    finished: bool,
}

impl Running {
    /// Start the process and its supervisor thread.
    pub fn start(spec: RunSpec) -> Result<Running> {
        let Some((program, args)) = spec.argv.split_first() else { return err("empty command line") };
        let mut cmd = std::process::Command::new(program);
        cmd.args(args).current_dir(&spec.dir).envs(spec.env.iter().map(|(k, v)| (k, v)));
        // its own process group, so Cancel also stops what the plugin started (filmcraft-cli mcp)
        let child = child::spawn(cmd, &Opts { stdin: None, group: true, stderr: true }).map_err(|e| Error(format!("cannot start `{program}`: {e}")))?;
        let (tx, rx) = channel();
        let cancel = Arc::new(AtomicBool::new(false));
        let flag = cancel.clone();
        let done = tx.clone();
        std::thread::Builder::new()
            .name("bashcut-plugin-run".into())
            .spawn(move || {
                let r = catch_unwind(AssertUnwindSafe(|| supervise(child, &spec, &tx, &flag)));
                if r.is_err() {
                    let _ = done.send(Event::Done(Outcome {
                        end: End::Crashed,
                        exit_code: None,
                        result: None,
                        stdout_truncated: false,
                        error: Some("plugin host thread failed".into()),
                    }));
                }
            })
            .map_err(|e| Error(format!("cannot start the run thread: {e}")))?;
        Ok(Running { rx, cancel, finished: false })
    }

    /// Ask the run to stop (TERM, then KILL after a grace period).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    /// Events since the last call; never blocks.
    pub fn poll(&mut self) -> Vec<Event> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(e) => {
                    if matches!(e, Event::Done(_)) {
                        self.finished = true;
                    }
                    out.push(e);
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => break,
                Err(std::sync::mpsc::TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.finished = true;
                        let error = Some("the run ended without a result".to_string());
                        out.push(Event::Done(Outcome { end: End::Crashed, exit_code: None, result: None, stdout_truncated: false, error }));
                    }
                    break;
                }
            }
        }
        out
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Block until the run ends (tests, CLI); `None` after `max`.
    pub fn wait(&mut self, max: Duration) -> (Vec<Event>, Option<Outcome>) {
        let start = Instant::now();
        let mut events = Vec::new();
        while start.elapsed() < max {
            match self.rx.recv_timeout(TICK) {
                Ok(Event::Done(o)) => {
                    self.finished = true;
                    return (events, Some(o));
                }
                Ok(e) => events.push(e),
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {}
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }
        (events, None)
    }
}

fn supervise(mut child: Child, spec: &RunSpec, tx: &Sender<Event>, cancel: &AtomicBool) {
    let cap = spec.max_stdout;
    let views = tx.clone();
    let out_rx = child::pump("bashcut-plugin-stdout", child.stdout.take(), move |out| {
        read_stdout(out, cap, |line| {
            if let Some(v) = View::from_line(line) {
                let _ = views.send(Event::View(v));
            }
        })
    });
    let progress = tx.clone();
    let err_rx = child::pump("bashcut-plugin-stderr", child.stderr.take(), move |errs| {
        for_each_line(errs, |line| {
            let (fraction, message) = parse_progress(line);
            let _ = progress.send(Event::Progress { fraction, message });
        })
    });

    let exit = child::supervise(&mut child, spec.timeout, Some(cancel));
    // the readers end when the pipes close; a grandchild holding them open must not hang us
    let (stdout, truncated) = child::collect(&out_rx, child::STDOUT_WAIT);
    child::collect(&err_rx, child::STDERR_WAIT);

    let text = String::from_utf8_lossy(&stdout);
    let result = parse_result(&text);
    let exit_code = exit.code();
    let (end, error) = match (exit.stop, exit_code) {
        (Some(Stop::TimedOut), _) => (End::TimedOut, Some(format!("timed out after {} s", spec.timeout.as_secs()))),
        (Some(Stop::Cancelled), _) => (End::Cancelled, None),
        (None, Some(c)) if c == spec.handoff_exit => (End::HandOff, None),
        (None, Some(0)) if result.as_ref().is_some_and(|r| !r.ok) => (End::Failed, None),
        (None, Some(0)) => (End::Succeeded, None),
        (None, Some(c)) => (End::Failed, if result.is_none() { Some(format!("exit status {c}")) } else { None }),
        (None, None) => (End::Crashed, Some("the plugin was killed".into())),
    };
    let error =
        error.or_else(|| (result.is_none() && matches!(end, End::Succeeded | End::HandOff)).then(|| "no bashcut-result/1 object on stdout".to_string()));
    let _ = tx.send(Event::Done(Outcome { end, exit_code, result, stdout_truncated: truncated, error }));
}

/// Read up to `cap` bytes, then keep draining (so the child never blocks) and report truncation;
/// `on_line` gets each line as it arrives (lines longer than [`MAX_VIEW_LINE`] are skipped, not
/// cut: half a view is no view).
fn read_stdout<R: Read>(r: R, cap: usize, mut on_line: impl FnMut(&str)) -> (Vec<u8>, bool) {
    child::read_lines(r, cap, MAX_VIEW_LINE, |line, cut| {
        if !cut {
            on_line(&String::from_utf8_lossy(line));
        }
    })
}

/// Call `f` with each line (lossy UTF-8, at most [`MAX_LINE`] bytes, the rest dropped).
fn for_each_line<R: Read>(r: R, mut f: impl FnMut(&str)) {
    child::read_lines(r, 0, MAX_LINE, |line, _| {
        let text = String::from_utf8_lossy(line);
        let text = text.trim_end_matches('\r');
        if !text.trim().is_empty() {
            f(text);
        }
    });
}

/// A progress line: a JSON object (`progress` / `fraction` 0..1 or a percentage, `message` /
/// `stage`), or text with an `NN%` in it. A leading `[plugin-name] ` tag is dropped.
pub fn parse_progress(line: &str) -> (Option<f32>, String) {
    let t = line.trim();
    if t.starts_with('{')
        && let Ok(Value::Object(o)) = serde_json::from_str::<Value>(t)
    {
        let frac = o.get("progress").or_else(|| o.get("fraction")).and_then(Value::as_f64).map(|p| if p > 1.0 { p / 100.0 } else { p });
        let msg = o.get("message").or_else(|| o.get("stage")).and_then(Value::as_str).unwrap_or_default().to_string();
        return (frac.filter(|f| f.is_finite()).map(|f| f.clamp(0.0, 1.0) as f32), msg);
    }
    let msg = match t.strip_prefix('[').and_then(|r| r.split_once("] ")) {
        Some((tag, rest)) if !tag.contains(' ') => rest.to_string(),
        _ => t.to_string(),
    };
    (percent(&msg), msg)
}

fn percent(s: &str) -> Option<f32> {
    let i = s.find('%')?;
    let head = s.get(..i)?;
    let start = head.rfind(|c: char| !(c.is_ascii_digit() || c == '.')).map_or(0, |j| j + 1);
    let n: f32 = head.get(start..)?.parse().ok()?;
    (n.is_finite() && (0.0..=100.0).contains(&n)).then_some(n / 100.0)
}

/// The `bashcut-result/1` object: the whole of stdout, else its last line that is an object with `ok`.
pub fn parse_result(stdout: &str) -> Option<PluginResult> {
    let whole = stdout.trim();
    let as_result = |t: &str| -> Option<PluginResult> {
        let v: Value = serde_json::from_str(t).ok()?;
        v.get("ok")?.as_bool()?;
        serde_json::from_value(v).ok()
    };
    if whole.starts_with('{')
        && let Some(r) = as_result(whole)
    {
        return Some(r);
    }
    whole.lines().rev().map(str::trim).filter(|l| l.starts_with('{')).find_map(as_result)
}

#[cfg(test)]
mod tests {
    #[test]
    #[ignore = "test ticket: a run with RunContext.control_token and a bridge has FILMCRAFT_CONTROL_TOKEN in its env, none without a token or a bridge"]
    fn control_token_reaches_the_run_env() {}

    use super::*;

    #[test]
    fn progress_lines() {
        assert_eq!(parse_progress("[translate-captions] 40% translating"), (Some(0.4), "40% translating".to_string()));
        assert_eq!(parse_progress(r#"{"progress": 0.25, "message": "half"}"#), (Some(0.25), "half".to_string()));
        assert_eq!(parse_progress(r#"{"progress": 50}"#).0, Some(0.5));
        assert_eq!(parse_progress("reading captions").0, None);
        assert_eq!(parse_progress("999% nonsense").0, None);
    }

    #[test]
    fn results() {
        let r = parse_result("noise\n{\"ok\": true, \"message\": \"done\", \"data\": {\"n\": 3}}\n").unwrap();
        assert!(r.ok);
        assert_eq!(r.data["n"], 3);
        let r = parse_result(r#"{"ok": false, "error": {"code": "no_key", "message": "set a key"}}"#).unwrap();
        assert_eq!(r.error.unwrap().code, "no_key");
        assert!(parse_result("{\"no\": 1}").is_none());
        assert!(parse_result("").is_none());
    }

    #[test]
    fn lines_are_capped() {
        let long = format!("{}\nshort\n", "x".repeat(MAX_LINE * 3));
        let mut got = Vec::new();
        for_each_line(long.as_bytes(), |l| got.push(l.len()));
        assert_eq!(got, [MAX_LINE, 5]);
        let (buf, truncated) = read_stdout(&b"0123456789"[..], 4, |_| {});
        assert_eq!((buf.as_slice(), truncated), (&b"0123"[..], true));
    }

    #[test]
    fn stdout_lines_reach_the_view_reader() {
        let text = format!("a\n{}\nb\r\nlast", "x".repeat(MAX_VIEW_LINE + 1));
        let mut lines = Vec::new();
        let (buf, truncated) = read_stdout(text.as_bytes(), 3, |l| lines.push(l.to_string()));
        assert_eq!(lines, ["a", "b\r", "last"], "the over-long line is skipped");
        assert_eq!((buf.as_slice(), truncated), (&b"a\nx"[..], true));
    }
}
