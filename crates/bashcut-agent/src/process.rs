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

use crate::codex_json;
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
    /// Event format of stdout: [`stream_json::FORMAT`] (the message goes in as a stream-json line) or
    /// [`codex_json::FORMAT`] (the message goes in as plain text).
    pub events: String,
    /// Arguments after everything else (Codex: `-`, read the prompt from stdin).
    pub tail_args: Vec<String>,
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
            tail_args: Vec::new(),
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

    /// Codex (`codex exec --json`, the prompt on stdin), with FilmCraft's MCP server passed as `-c` overrides.
    /// `program` overrides `codex` (from `BASHCUT_CODEX` in the app).
    pub fn codex(program: Option<String>) -> Self {
        let program = program.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "codex".into());
        AgentSpec {
            id: "codex".into(),
            name: "Codex".into(),
            argv: vec![program, "exec".into(), "--json".into(), "--skip-git-repo-check".into()],
            mcp_args: vec![
                "-c".into(),
                "mcp_servers.filmcraft.command='{cli}'".into(),
                "-c".into(),
                "mcp_servers.filmcraft.args=['mcp','--bridge','{bridge}']".into(),
            ],
            resume_args: vec!["resume".into(), "{session}".into()],
            events: codex_json::FORMAT.into(),
            tail_args: vec!["-".into()],
            ..AgentSpec::default()
        }
    }

    pub fn check(&self) -> Result<()> {
        if self.argv.is_empty() || self.argv.len() > 128 {
            return err(format!("agent `{}` has no command", self.id));
        }
        if self.events != stream_json::FORMAT && self.events != codex_json::FORMAT {
            return err(format!(
                "agent `{}`: event format `{}` is not supported (use {} or {})",
                self.id,
                self.events,
                stream_json::FORMAT,
                codex_json::FORMAT
            ));
        }
        Ok(())
    }

    /// The command line of a turn.
    pub fn command_line(&self, mcp_config: Option<&Path>, session: Option<&str>) -> Result<Vec<String>> {
        self.command_line_for(mcp_config, session, None, None)
    }

    /// The command line of a turn; `{cli}` and `{bridge}` in the MCP arguments become `cli` and `bridge`.
    pub fn command_line_for(&self, mcp_config: Option<&Path>, session: Option<&str>, cli: Option<&str>, bridge: Option<&str>) -> Result<Vec<String>> {
        self.check()?;
        let mut argv = self.argv.clone();
        if !self.mcp_args.is_empty() {
            let Some(cfg) = mcp_config else { return err("this agent needs the app's control channel: start BashCutPro with --control <port>") };
            let path = cfg.to_string_lossy();
            let needs = |k: &str| self.mcp_args.iter().any(|a| a.contains(k));
            // the values go inside TOML quotes: refuse what would break out of them
            let quoted = |v: &str| !v.is_empty() && !v.contains(['\'', '"', '\n']);
            let (cli, bridge) = (cli.unwrap_or_default(), bridge.unwrap_or_default());
            if (needs("{cli}") && !quoted(cli)) || (needs("{bridge}") && !quoted(bridge)) {
                return err("this agent needs filmcraft-cli next to the app and the app's control channel (--control <port>)");
            }
            argv.extend(self.mcp_args.iter().map(|a| a.replace("{config}", &path).replace("{cli}", cli).replace("{bridge}", bridge)));
        }
        if let Some(s) = session.filter(|s| !s.is_empty()) {
            if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || s.len() > 128 {
                return err("bad session id");
            }
            argv.extend(self.resume_args.iter().map(|a| a.replace("{session}", s)));
        }
        argv.extend(self.tail_args.iter().cloned());
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
    /// Start `argv` in `cwd` and send `message` (Claude Code stream-json).
    pub fn start(argv: &[String], cwd: &Path, env: &[(String, String)], message: &str, timeout: Duration) -> Result<Turn> {
        Turn::start_as(argv, cwd, env, message, timeout, stream_json::FORMAT)
    }

    /// Start `argv` for an agent speaking `events` ([`stream_json::FORMAT`] or [`codex_json::FORMAT`]).
    pub fn start_as(argv: &[String], cwd: &Path, env: &[(String, String)], message: &str, timeout: Duration, events: &str) -> Result<Turn> {
        let codex = events == codex_json::FORMAT;
        let parse: fn(&str) -> Vec<Event> = if codex { codex_json::parse_line } else { stream_json::parse_line };
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
            let line = if codex { format!("{message}\n") } else { format!("{}\n", stream_json::user_message(message)) };
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
                if catch_unwind(AssertUnwindSafe(|| supervise(child, timeout, &tx, &flag, parse))).is_err() {
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

fn supervise(mut child: Child, timeout: Duration, tx: &Sender<TurnEvent>, stop: &AtomicBool, parse: fn(&str) -> Vec<Event>) {
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
                    for e in parse(line) {
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
    fn codex_command_line() {
        let spec = AgentSpec::codex(None);
        assert!(spec.check().is_ok());
        let cfg = Some(Path::new("/t/m.json"));
        assert!(spec.command_line(cfg, None).is_err(), "Codex needs the CLI and the bridge");
        let argv = spec.command_line_for(cfg, Some("th-1"), Some("/app/filmcraft-cli"), Some("127.0.0.1:9876")).unwrap();
        assert_eq!(&argv[..4], ["codex", "exec", "--json", "--skip-git-repo-check"]);
        assert!(argv.windows(2).any(|w| w == ["-c", "mcp_servers.filmcraft.command='/app/filmcraft-cli'"]));
        assert!(argv.windows(2).any(|w| w == ["-c", "mcp_servers.filmcraft.args=['mcp','--bridge','127.0.0.1:9876']"]));
        assert!(argv.windows(2).any(|w| w == ["resume", "th-1"]));
        assert_eq!(argv.last().map(String::as_str), Some("-"), "the prompt comes from stdin");
        assert!(spec.command_line_for(cfg, None, Some("/x/it's/cli"), Some("127.0.0.1:1")).is_err(), "no quote breaks out of TOML");
    }

    #[test]
    fn a_codex_turn_reads_its_events() {
        // a stand-in agent: prints Codex events whatever it is asked
        let script = r#"{"type":"thread.started","thread_id":"th-9"}
{"type":"item.completed","item":{"id":"m","type":"agent_message","text":"hi"}}
{"type":"turn.completed"}"#;
        let dir = std::env::temp_dir().join(format!("bashcut-codex-turn-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("events.jsonl");
        let _ = std::fs::write(&file, script);
        let argv: Vec<String> = if cfg!(windows) {
            vec!["cmd".into(), "/C".into(), format!("type {}", file.display())]
        } else {
            vec!["cat".into(), file.to_string_lossy().into_owned()]
        };
        let mut turn = Turn::start_as(&argv, &dir, &[], "hello", Duration::from_secs(20), codex_json::FORMAT).unwrap();
        let events = turn.wait(Duration::from_secs(20));
        assert!(events.contains(&TurnEvent::Agent(Event::Text("hi".into()))), "{events:?}");
        assert!(events.iter().any(|e| matches!(e, TurnEvent::Agent(Event::Init { session, .. }) if session == "th-9")));
        assert!(events.contains(&TurnEvent::Ended(TurnEnd::Done)), "{events:?}");
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn lines_are_split_and_capped() {
        let mut got = Vec::new();
        let data = format!("a\n\n{}\nb", "x".repeat(MAX_LINE + 1));
        for_each_line(data.as_bytes(), |l, cut| got.push((l.to_string(), cut)));
        assert_eq!(got, vec![("a".to_string(), false), (String::new(), true), ("b".to_string(), false)]);
    }
}
