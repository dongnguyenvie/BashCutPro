//! One agent turn as a child process: the message goes to stdin as one stream-json line, stdout is
//! read line by line into [`Event`]s, and the turn ends when the process exits (or is stopped or
//! times out). The next turn starts a new process that resumes the agent's session.

use std::io::{Read, Write};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::stream_json::{self, Event};
use crate::{Error, Result, err};

/// Longest stdout line read; the rest of a longer line is dropped (and the line reported).
pub const MAX_LINE: usize = 1024 * 1024;
/// stderr kept for the error message of a failed turn.
const MAX_STDERR: usize = 8 * 1024;
const TICK: Duration = Duration::from_millis(25);
const GRACE: Duration = Duration::from_millis(1500);

/// How to start an agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSpec {
    /// Stable id (`claude-code`, or the agent plugin's id).
    pub id: String,
    pub name: String,
    /// Program and arguments.
    pub argv: Vec<String>,
    /// Arguments passing the MCP config file (`{config}` is replaced by its path); empty: no MCP.
    pub mcp_args: Vec<String>,
    /// Arguments resuming a session (`{session}` is replaced by its id).
    pub resume_args: Vec<String>,
    /// Event format of stdout (only [`stream_json::FORMAT`] for now).
    pub events: String,
    /// Extra environment.
    pub env: Vec<(String, String)>,
    /// A turn is stopped after this long.
    pub timeout_secs: u64,
}

impl Default for AgentSpec {
    fn default() -> Self {
        AgentSpec {
            id: String::new(),
            name: String::new(),
            argv: Vec::new(),
            mcp_args: Vec::new(),
            resume_args: Vec::new(),
            events: stream_json::FORMAT.into(),
            env: Vec::new(),
            timeout_secs: 2 * 60 * 60,
        }
    }
}

impl AgentSpec {
    /// Claude Code in print mode with stream-json in and out, FilmCraft's MCP tools allowed.
    /// `program` overrides `claude` (from `BASHCUT_CLAUDE` in the app).
    pub fn claude_code(program: Option<String>) -> Self {
        let program = program.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "claude".into());
        let argv = [&program, "-p", "--output-format", "stream-json", "--input-format", "stream-json", "--verbose", "--allowedTools", "mcp__filmcraft"];
        AgentSpec {
            id: "claude-code".into(),
            name: "Claude Code".into(),
            argv: argv.iter().map(|s| s.to_string()).collect(),
            mcp_args: vec!["--mcp-config".into(), "{config}".into()],
            resume_args: vec!["--resume".into(), "{session}".into()],
            ..AgentSpec::default()
        }
    }

    pub fn check(&self) -> Result<()> {
        if self.argv.is_empty() || self.argv.len() > 128 {
            return err(format!("agent `{}` has no command", self.id));
        }
        if self.events != stream_json::FORMAT {
            return err(format!("agent `{}`: event format `{}` is not supported (use {})", self.id, self.events, stream_json::FORMAT));
        }
        Ok(())
    }

    /// The command line of a turn.
    pub fn command_line(&self, mcp_config: Option<&Path>, session: Option<&str>) -> Result<Vec<String>> {
        self.check()?;
        let mut argv = self.argv.clone();
        if !self.mcp_args.is_empty() {
            let Some(cfg) = mcp_config else { return err("this agent needs the app's control channel: start BashCutPro with --control <port>") };
            let path = cfg.to_string_lossy();
            argv.extend(self.mcp_args.iter().map(|a| a.replace("{config}", &path)));
        }
        if let Some(s) = session.filter(|s| !s.is_empty()) {
            if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || s.len() > 128 {
                return err("bad session id");
            }
            argv.extend(self.resume_args.iter().map(|a| a.replace("{session}", s)));
        }
        Ok(argv)
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum TurnEnd {
    /// The process exited 0.
    Done,
    /// Non-zero exit or a start failure, with what stderr said.
    Failed(String),
    Stopped,
    TimedOut,
}

#[derive(Clone, Debug, PartialEq)]
pub enum TurnEvent {
    Agent(Event),
    Ended(TurnEnd),
}

/// A running turn.
pub struct Turn {
    rx: Receiver<TurnEvent>,
    stop: Arc<AtomicBool>,
    finished: bool,
}

impl Turn {
    /// Start `argv` in `cwd` and send `message`.
    pub fn start(argv: &[String], cwd: &Path, env: &[(String, String)], message: &str, timeout: Duration) -> Result<Turn> {
        let Some((program, args)) = argv.split_first() else { return err("empty agent command") };
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(cwd).envs(env.iter().map(|(k, v)| (k, v))).stdin(Stdio::piped()).stdout(Stdio::piped()).stderr(Stdio::piped());
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            // its own process group, so Stop also ends the MCP server it started
            cmd.process_group(0);
        }
        let mut child = cmd.spawn().map_err(|e| Error(format!("cannot start `{program}`: {e}")))?;
        if let Some(mut stdin) = child.stdin.take() {
            // one message, then EOF: the agent answers it and exits
            let line = format!("{}\n", stream_json::user_message(message));
            if let Err(e) = stdin.write_all(line.as_bytes()) {
                log::warn!("bashcut agent stdin: {e}");
            }
        }
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let done = tx.clone();
        std::thread::Builder::new()
            .name("bashcut-agent-turn".into())
            .spawn(move || {
                if catch_unwind(AssertUnwindSafe(|| supervise(child, timeout, &tx, &flag))).is_err() {
                    let _ = done.send(TurnEvent::Ended(TurnEnd::Failed("agent host thread failed".into())));
                }
            })
            .map_err(|e| Error(format!("cannot start the agent thread: {e}")))?;
        Ok(Turn { rx, stop, finished: false })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
    }

    pub fn is_finished(&self) -> bool {
        self.finished
    }

    /// Events since the last call; never blocks.
    pub fn poll(&mut self) -> Vec<TurnEvent> {
        let mut out = Vec::new();
        loop {
            match self.rx.try_recv() {
                Ok(e) => {
                    if matches!(e, TurnEvent::Ended(_)) {
                        self.finished = true;
                    }
                    out.push(e);
                }
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    if !self.finished {
                        self.finished = true;
                        out.push(TurnEvent::Ended(TurnEnd::Failed("the agent ended unexpectedly".into())));
                    }
                    break;
                }
            }
        }
        out
    }

    /// Block until the turn ends or `max` passes (tests).
    pub fn wait(&mut self, max: Duration) -> Vec<TurnEvent> {
        let start = Instant::now();
        let mut out = Vec::new();
        while start.elapsed() < max && !self.finished {
            out.extend(self.poll());
            std::thread::sleep(TICK);
        }
        out
    }
}

fn supervise(mut child: Child, timeout: Duration, tx: &Sender<TurnEvent>, stop: &AtomicBool) {
    let (out_done_tx, out_done) = channel::<()>();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        let _ = std::thread::Builder::new().name("bashcut-agent-stdout".into()).spawn(move || {
            let _ = catch_unwind(AssertUnwindSafe(|| {
                for_each_line(out, |line, cut| {
                    if cut {
                        let _ = tx.send(TurnEvent::Agent(Event::Notice("skipped an event larger than 1 MB".into())));
                        return;
                    }
                    for e in stream_json::parse_line(line) {
                        let _ = tx.send(TurnEvent::Agent(e));
                    }
                })
            }));
            let _ = out_done_tx.send(());
        });
    }
    let (err_tx, err_rx) = channel::<String>();
    if let Some(mut errs) = child.stderr.take() {
        let _ = std::thread::Builder::new().name("bashcut-agent-stderr".into()).spawn(move || {
            let text = catch_unwind(AssertUnwindSafe(|| {
                // keep the tail: the last lines say what went wrong
                let mut tail: Vec<u8> = Vec::new();
                let mut chunk = [0u8; 4096];
                while let Ok(n) = errs.read(&mut chunk) {
                    if n == 0 {
                        break;
                    }
                    tail.extend_from_slice(chunk.get(..n).unwrap_or_default());
                    if tail.len() > MAX_STDERR * 2 {
                        let cut = tail.len() - MAX_STDERR;
                        tail.drain(..cut);
                    }
                }
                let start = tail.len().saturating_sub(MAX_STDERR);
                String::from_utf8_lossy(tail.get(start..).unwrap_or_default()).trim().to_string()
            }));
            let _ = err_tx.send(text.unwrap_or_default());
        });
    }

    let start = Instant::now();
    let mut ending: Option<(TurnEnd, Instant)> = None;
    let status = loop {
        match child.try_wait() {
            Ok(Some(s)) => break Some(s),
            Ok(None) => {}
            Err(_) => break None,
        }
        if ending.is_none() {
            if stop.load(Ordering::Relaxed) {
                terminate(&mut child, false);
                ending = Some((TurnEnd::Stopped, Instant::now()));
            } else if start.elapsed() > timeout {
                terminate(&mut child, false);
                ending = Some((TurnEnd::TimedOut, Instant::now()));
            }
        } else if ending.as_ref().is_some_and(|(_, since)| since.elapsed() > GRACE) {
            terminate(&mut child, true);
        }
        std::thread::sleep(TICK);
    };
    // all events first, then the end (a grandchild holding the pipe must not hang us)
    let _ = out_done.recv_timeout(Duration::from_secs(2));
    let stderr = err_rx.recv_timeout(Duration::from_millis(500)).unwrap_or_default();
    let end = match (ending, status.and_then(|s| s.code())) {
        (Some((end, _)), _) => end,
        (None, Some(0)) => TurnEnd::Done,
        (None, code) => {
            let what = code.map_or_else(|| "the agent was killed".to_string(), |c| format!("the agent exited with status {c}"));
            TurnEnd::Failed(if stderr.is_empty() { what } else { format!("{what}: {}", crate::clip(&stderr, 2048)) })
        }
    };
    let _ = tx.send(TurnEvent::Ended(end));
}

/// Stop the child (and on Unix its process group): TERM first, KILL when `hard`.
fn terminate(child: &mut Child, hard: bool) {
    #[cfg(unix)]
    {
        let sig = if hard { "-KILL" } else { "-TERM" };
        let group = format!("-{}", child.id());
        let ok = Command::new("kill").args([sig, "--", &group]).stdout(Stdio::null()).stderr(Stdio::null()).status().is_ok_and(|s| s.success());
        if ok {
            return;
        }
    }
    if hard || cfg!(not(unix)) {
        let _ = child.kill();
    }
}

/// Call `f` with each line (lossy UTF-8) and whether it was longer than [`MAX_LINE`] (then it is
/// not passed).
fn for_each_line<R: Read>(mut r: R, mut f: impl FnMut(&str, bool)) {
    let mut line: Vec<u8> = Vec::new();
    let mut cut = false;
    let mut chunk = [0u8; 16 * 1024];
    let mut emit = |line: &mut Vec<u8>, cut: &mut bool| {
        if *cut {
            f("", true);
        } else if !line.is_empty() {
            f(&String::from_utf8_lossy(line), false);
        }
        line.clear();
        *cut = false;
    };
    loop {
        let n = match r.read(&mut chunk) {
            Ok(0) | Err(_) => break,
            Ok(n) => n,
        };
        for &b in chunk.get(..n).unwrap_or_default() {
            if b == b'\n' {
                emit(&mut line, &mut cut);
            } else if line.len() < MAX_LINE {
                line.push(b);
            } else {
                cut = true;
            }
        }
    }
    emit(&mut line, &mut cut);
}

/// A temporary MCP config for this app: `filmcraft-cli mcp --bridge <address>` as server `filmcraft`.
/// Written to the temp dir (it holds no secret) and removed by the caller when the session ends.
pub fn write_mcp_config(cli: &str, bridge: &str) -> Result<PathBuf> {
    let v = serde_json::json!({"mcpServers": {"filmcraft": {"command": cli, "args": ["mcp", "--bridge", bridge]}}});
    let path = std::env::temp_dir().join(format!("bashcut-agent-mcp-{}.json", std::process::id()));
    crate::write_atomic(&path, &v.to_string())?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn command_line_adds_mcp_and_resume() {
        let spec = AgentSpec::claude_code(None);
        assert!(spec.command_line(None, None).is_err(), "Claude Code needs the bridge");
        let argv = spec.command_line(Some(Path::new("/t/m.json")), Some("abc-1")).unwrap();
        assert_eq!(argv[0], "claude");
        assert!(argv.windows(2).any(|w| w == ["--mcp-config", "/t/m.json"]));
        assert!(argv.windows(2).any(|w| w == ["--resume", "abc-1"]));
        assert!(spec.command_line(Some(Path::new("/t")), Some("x; rm -rf /")).is_err());
        let bad = AgentSpec { events: "other".into(), ..spec };
        assert!(bad.check().is_err());
    }

    #[test]
    fn lines_are_split_and_capped() {
        let mut got = Vec::new();
        let data = format!("a\n\n{}\nb", "x".repeat(MAX_LINE + 1));
        for_each_line(data.as_bytes(), |l, cut| got.push((l.to_string(), cut)));
        assert_eq!(got, vec![("a".to_string(), false), (String::new(), true), ("b".to_string(), false)]);
    }
}
