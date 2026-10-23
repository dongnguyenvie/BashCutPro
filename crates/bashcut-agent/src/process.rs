//! One agent turn as a child process: the message goes to stdin (one stream-json line, or plain text
//! for Codex), stdout is read line by line into [`Event`]s, and the turn ends when the process exits
//! (or is stopped or times out). The next turn starts a new process that resumes the agent's session.

use std::io::Read;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::path::{Path, PathBuf};
use std::process::{Child, Command};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, Sender, TryRecvError, channel};
use std::time::Duration;

use bashcut_dirs::child::{self, Opts, Stop};
use serde::{Deserialize, Serialize};

use crate::codex_json;
use crate::stream_json::{self, Event};
use crate::{Error, Result, err};

/// Longest stdout line read; the rest of a longer line is dropped (and the line reported).
pub const MAX_LINE: usize = 1024 * 1024;
/// stderr kept for the error message of a failed turn.
const MAX_STDERR: usize = 8 * 1024;

/// How to start an agent.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct AgentSpec {
    /// Stable id (`claude-code`, or the agent plugin's id).
    pub id: String,
    pub name: String,
    /// Program and arguments.
    pub argv: Vec<String>,
    /// Arguments passing the MCP config: `{config}` is replaced by the config file's path,
    /// `{mcpCommand}` / `{mcpArgs}` by the server's command and arguments as TOML / JSON values
    /// (for agents configured on the command line, like Codex's `-c`); empty: no MCP.
    pub mcp_args: Vec<String>,
    /// Arguments resuming a session (`{session}` is replaced by its id).
    pub resume_args: Vec<String>,
    /// Arguments after everything else (Codex: `-`, the prompt comes from stdin).
    pub prompt_args: Vec<String>,
    /// Event format of stdout: [`stream_json::FORMAT`] or [`codex_json::FORMAT`]; it also sets how
    /// the message is written to stdin.
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
            prompt_args: Vec::new(),
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

    /// Codex in exec mode with JSON events, FilmCraft's MCP server passed as `-c` overrides (Codex
    /// keeps MCP servers in its own config), the prompt on stdin. `program` overrides `codex` (from
    /// `BASHCUT_CODEX` in the app).
    pub fn codex(program: Option<String>) -> Self {
        let program = program.filter(|p| !p.trim().is_empty()).unwrap_or_else(|| "codex".into());
        AgentSpec {
            id: "codex".into(),
            name: "Codex".into(),
            argv: vec![program, "exec".into(), "--json".into(), "--skip-git-repo-check".into()],
            mcp_args: vec!["-c".into(), "mcp_servers.filmcraft.command={mcpCommand}".into(), "-c".into(), "mcp_servers.filmcraft.args={mcpArgs}".into()],
            resume_args: vec!["resume".into(), "{session}".into()],
            prompt_args: vec!["-".into()],
            events: codex_json::FORMAT.into(),
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
        self.check()?;
        let mut argv = self.argv.clone();
        if !self.mcp_args.is_empty() {
            let Some(cfg) = mcp_config else { return err("this agent needs the app's control channel: start BashCutPro with --control <port>") };
            let path = cfg.to_string_lossy();
            let server =
                if self.mcp_args.iter().any(|a| a.contains("{mcpCommand}") || a.contains("{mcpArgs}")) { mcp_server(cfg)? } else { Default::default() };
            argv.extend(self.mcp_args.iter().map(|a| a.replace("{config}", &path).replace("{mcpCommand}", &server.0).replace("{mcpArgs}", &server.1)));
        }
        if let Some(s) = session.filter(|s| !s.is_empty()) {
            if !s.chars().all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_') || s.len() > 128 {
                return err("bad session id");
            }
            argv.extend(self.resume_args.iter().map(|a| a.replace("{session}", s)));
        }
        argv.extend(self.prompt_args.iter().cloned());
        Ok(argv)
    }
}

/// The `filmcraft` server of an MCP config written by [`write_mcp_config`]: its command as a quoted
/// string and its arguments as an array (JSON strings and arrays of strings are valid TOML too).
fn mcp_server(config: &Path) -> Result<(String, String)> {
    let text = crate::read_capped(config, 64 * 1024)?;
    let v: serde_json::Value = serde_json::from_str(&text).map_err(|e| Error(format!("{}: {e}", config.display())))?;
    let server = v.get("mcpServers").and_then(|s| s.get("filmcraft")).ok_or_else(|| Error(format!("{}: no filmcraft server", config.display())))?;
    let command = server.get("command").and_then(serde_json::Value::as_str).ok_or_else(|| Error(format!("{}: no command", config.display())))?;
    let args: Vec<&str> =
        server.get("args").and_then(serde_json::Value::as_array).map(|a| a.iter().filter_map(serde_json::Value::as_str).collect()).unwrap_or_default();
    Ok((serde_json::json!(command).to_string(), serde_json::json!(args).to_string()))
}

/// Reads an agent's stdout in its event format.
enum Decoder {
    Claude,
    Codex(codex_json::Parser),
}

impl Decoder {
    fn new(events: &str) -> Decoder {
        if events == codex_json::FORMAT { Decoder::Codex(codex_json::Parser::default()) } else { Decoder::Claude }
    }

    fn parse_line(&mut self, line: &str) -> Vec<Event> {
        match self {
            Decoder::Claude => stream_json::parse_line(line),
            Decoder::Codex(p) => p.parse_line(line),
        }
    }
}

/// The message as the agent reads it on stdin.
fn input(events: &str, message: &str) -> String {
    if events == codex_json::FORMAT { message.to_string() } else { format!("{}\n", stream_json::user_message(message)) }
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
    /// Start `argv` in `cwd` and send `message`; stdout is read as `events` (an [`AgentSpec::events`]).
    pub fn start(argv: &[String], cwd: &Path, env: &[(String, String)], message: &str, events: &str, timeout: Duration) -> Result<Turn> {
        let Some((program, args)) = argv.split_first() else { return err("empty agent command") };
        let mut cmd = Command::new(program);
        cmd.args(args).current_dir(cwd).envs(env.iter().map(|(k, v)| (k, v)));
        // one message, then EOF: the agent answers it and exits; its own process group, so Stop
        // also ends the MCP server it started
        let opts = Opts { stdin: Some(input(events, message)), group: true, stderr: true };
        let child = child::spawn(cmd, &opts).map_err(|e| Error(format!("cannot start `{program}`: {e}")))?;
        let (tx, rx) = channel();
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let done = tx.clone();
        let decoder = Decoder::new(events);
        std::thread::Builder::new()
            .name("bashcut-agent-turn".into())
            .spawn(move || {
                if catch_unwind(AssertUnwindSafe(|| supervise(child, decoder, timeout, &tx, &flag))).is_err() {
                    let _ = done.send(TurnEvent::Ended(TurnEnd::Failed("agent host thread failed".into())));
                }
            })
            .map_err(|e| Error(format!("cannot start the agent thread: {e}")))?;
        Ok(Turn { rx, stop, finished: false })
    }

    pub fn stop(&self) {
        self.stop.store(true, Ordering::Relaxed);
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
}

fn supervise(mut child: Child, mut decoder: Decoder, timeout: Duration, tx: &Sender<TurnEvent>, stop: &AtomicBool) {
    let events = tx.clone();
    let out_done = child::pump("bashcut-agent-stdout", child.stdout.take(), move |out| {
        for_each_line(out, |line, cut| {
            if cut {
                let _ = events.send(TurnEvent::Agent(Event::Notice("skipped an event larger than 1 MB".into())));
                return;
            }
            for e in decoder.parse_line(line) {
                let _ = events.send(TurnEvent::Agent(e));
            }
        })
    });
    // keep the tail: the last lines say what went wrong
    let err_rx = child::pump("bashcut-agent-stderr", child.stderr.take(), |errs| String::from_utf8_lossy(&child::tail(errs, MAX_STDERR)).trim().to_string());

    let exit = child::supervise(&mut child, timeout, Some(stop));
    // all events first, then the end (a grandchild holding the pipe must not hang us)
    child::collect(&out_done, child::STDOUT_WAIT);
    let stderr = child::collect(&err_rx, child::STDERR_WAIT);
    let end = match (exit.stop, exit.code()) {
        (Some(Stop::Cancelled), _) => TurnEnd::Stopped,
        (Some(Stop::TimedOut), _) => TurnEnd::TimedOut,
        (None, Some(0)) => TurnEnd::Done,
        (None, code) => {
            let what = code.map_or_else(|| "the agent was killed".to_string(), |c| format!("the agent exited with status {c}"));
            TurnEnd::Failed(if stderr.is_empty() { what } else { format!("{what}: {}", crate::clip(&stderr, 2048)) })
        }
    };
    let _ = tx.send(TurnEvent::Ended(end));
}

/// Call `f` with each line (lossy UTF-8) and whether it was longer than [`MAX_LINE`] (then it is
/// not passed).
fn for_each_line<R: Read>(r: R, mut f: impl FnMut(&str, bool)) {
    child::read_lines(r, 0, MAX_LINE, |line, cut| {
        if cut {
            f("", true);
        } else if !line.is_empty() {
            f(&String::from_utf8_lossy(line), false);
        }
    });
}

/// Where [`write_mcp_config`] puts the config for this engine and bridge: named by pid and a hash of
/// both, so a changed bridge (or a second agent with another one) gets its own file.
pub fn mcp_config_path(cli: &str, bridge: &str) -> PathBuf {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    (cli, bridge).hash(&mut h);
    std::env::temp_dir().join(format!("bashcut-agent-mcp-{}-{:016x}.json", std::process::id(), h.finish()))
}

/// A temporary MCP config for this app: `<engine> mcp --bridge <address>` (`bashcutpro` or `filmcraft-cli`) as server `filmcraft`.
/// Written to the temp dir (it holds no secret) at [`mcp_config_path`] and removed by the caller when the session ends.
pub fn write_mcp_config(cli: &str, bridge: &str) -> Result<PathBuf> {
    let v = serde_json::json!({"mcpServers": {"filmcraft": {"command": cli, "args": ["mcp", "--bridge", bridge]}}});
    let path = mcp_config_path(cli, bridge);
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
    fn codex_gets_the_server_as_overrides_and_the_prompt_from_stdin() {
        let spec = AgentSpec::codex(Some("/opt/codex".into()));
        assert!(spec.check().is_ok());
        assert!(spec.command_line(None, None).is_err(), "Codex needs the bridge");
        let cfg = write_mcp_config("/apps/filmcraft-cli", "127.0.0.1:9876").unwrap();
        let argv = spec.command_line(Some(&cfg), Some("th-1")).unwrap();
        let _ = std::fs::remove_file(&cfg);
        assert_eq!(&argv[..4], ["/opt/codex", "exec", "--json", "--skip-git-repo-check"]);
        assert!(argv.windows(2).any(|w| w == ["-c", r#"mcp_servers.filmcraft.command="/apps/filmcraft-cli""#]), "{argv:?}");
        assert!(argv.windows(2).any(|w| w == ["-c", r#"mcp_servers.filmcraft.args=["mcp","--bridge","127.0.0.1:9876"]"#]), "{argv:?}");
        assert_eq!(&argv[argv.len() - 3..], ["resume", "th-1", "-"]);
        assert_eq!(input(codex_json::FORMAT, "cut it"), "cut it");
        assert!(input(stream_json::FORMAT, "cut it").ends_with("}\n"));
    }

    #[test]
    #[ignore = "test ticket: a new bridge gets its own MCP config file and the session removes the stale one"]
    fn mcp_config_is_named_by_bridge_and_rewritten_on_change() {}

    #[test]
    fn lines_are_split_and_capped() {
        let mut got = Vec::new();
        let data = format!("a\n\n{}\nb", "x".repeat(MAX_LINE + 1));
        for_each_line(data.as_bytes(), |l, cut| got.push((l.to_string(), cut)));
        assert_eq!(got, vec![("a".to_string(), false), (String::new(), true), ("b".to_string(), false)]);
    }
}
