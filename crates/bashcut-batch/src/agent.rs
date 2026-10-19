//! Agent steps (#60): how an agent is started headless against an item's project, and what is read
//! back from its event stream. The runner (`runner.rs`) does the rounds, the review and the caps.
//!
//! - **claude** (default): `claude -p <prompt> --output-format stream-json --verbose --max-turns N
//!   --mcp-config <file> --allowedTools mcp__bashcutpro`, fix rounds with `--resume <session>`. The
//!   last `result` event has the session, turns, tokens, cost and whether the turn limit was hit.
//! - **codex**: `codex exec --json --skip-git-repo-check -c mcp_servers.bashcutpro.command=… <prompt>`,
//!   fix rounds with `codex exec resume <thread> …`; usage from `turn.completed` events. No turn
//!   limit of its own: `maxMinutes` caps it.
//! - a pipeline's own `argv` with `{prompt}`, `{session}`, `{mcpConfig}`.
//!
//! The MCP server the agent gets is `bashcut-mcp --headless --project <item> --roots <item folder,
//! outputs>` when the runner knows a `bashcut-mcp` (writes only there, #51), else `filmcraft-cli mcp
//! --project <item>`.

use crate::pipeline::{AgentStep, Vars};
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The MCP server name in the agent's config (Claude Code tool names: `mcp__bashcutpro__*`).
pub const SERVER: &str = "bashcutpro";
/// Event logs above this are read from their end only.
const MAX_LOG: u64 = 16 << 20;
/// Findings listed in a fix prompt.
const MAX_FINDINGS: usize = 20;

/// What the runner needs to start an agent round.
pub struct Launch<'a> {
    pub step: &'a AgentStep,
    pub vars: &'a Vars,
    pub prompt: &'a str,
    pub session: Option<&'a str>,
    pub mcp_config: &'a Path,
}

/// The MCP server command for the agent: (program, args).
pub fn mcp_server(cli: &str, mcp: Option<&Path>, project: &Path, roots: &[PathBuf]) -> (String, Vec<String>) {
    let project = project.to_string_lossy().into_owned();
    match mcp {
        Some(m) => {
            let list = std::env::join_paths(roots).map(|s| s.to_string_lossy().into_owned()).unwrap_or_default();
            let mut args = vec!["--headless".to_string(), "--project".into(), project, "--cli".into(), cli.to_string()];
            if !list.is_empty() {
                args.extend(["--roots".to_string(), list]);
            }
            (m.to_string_lossy().into_owned(), args)
        }
        None => (cli.to_string(), vec!["mcp".to_string(), "--project".into(), project]),
    }
}

/// Claude Code's `--mcp-config` file content.
pub fn mcp_config(program: &str, args: &[String]) -> Value {
    json!({"mcpServers": {SERVER: {"command": program, "args": args}}})
}

/// The round's command line.
pub fn argv(l: &Launch) -> Vec<String> {
    let mcp_config = l.mcp_config.to_string_lossy().into_owned();
    if !l.step.argv.is_empty() {
        let mut vars = l.vars.clone();
        vars.0.push(("prompt", l.prompt.to_string()));
        vars.0.push(("session", l.session.unwrap_or_default().to_string()));
        vars.0.push(("mcpConfig", mcp_config));
        return l.step.argv.iter().map(|a| vars.expand(a)).collect();
    }
    match l.step.agent_name() {
        "codex" => {
            let (program, args) = read_server(l.mcp_config);
            let toml_args = format!("[{}]", args.iter().map(|a| toml_string(a.as_str())).collect::<Vec<_>>().join(", "));
            let mut v: Vec<String> = vec!["codex".into(), "exec".into()];
            if let Some(s) = l.session {
                v.extend(["resume".to_string(), s.to_string()]);
            }
            v.extend([
                "--json".to_string(),
                "--skip-git-repo-check".into(),
                "-c".into(),
                format!("mcp_servers.{SERVER}.command={}", toml_string(&program)),
                "-c".into(),
                format!("mcp_servers.{SERVER}.args={toml_args}"),
                l.prompt.to_string(),
            ]);
            v
        }
        _ => {
            let mut v: Vec<String> = vec![
                "claude".into(),
                "-p".into(),
                l.prompt.to_string(),
                "--output-format".into(),
                "stream-json".into(),
                "--verbose".into(),
                "--max-turns".into(),
                l.step.max_turns().to_string(),
                "--mcp-config".into(),
                mcp_config,
                "--allowedTools".into(),
                format!("mcp__{SERVER}"),
            ];
            if let Some(s) = l.session {
                v.extend(["--resume".to_string(), s.to_string()]);
            }
            v
        }
    }
}

/// (program, args) back from a config written by [`mcp_config`].
fn read_server(path: &Path) -> (String, Vec<String>) {
    let v: Value = std::fs::read_to_string(path).ok().and_then(|t| serde_json::from_str(&t).ok()).unwrap_or(Value::Null);
    let s = &v["mcpServers"][SERVER];
    let program = s["command"].as_str().unwrap_or_default().to_string();
    let args = s["args"].as_array().map(|a| a.iter().filter_map(Value::as_str).map(str::to_string).collect()).unwrap_or_default();
    (program, args)
}

fn toml_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            c if c.is_control() => out.push(' '),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// What one round reported.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Round {
    pub session: Option<String>,
    pub turns: u64,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cost_usd: f64,
    /// The agent stopped at its turn limit.
    pub hit_turn_limit: bool,
    /// The agent said its run failed.
    pub failed: bool,
    /// Its last words (the `result` text, or an error).
    pub message: String,
}

/// Read a round's event log (Claude Code stream-json or Codex `--json`, one JSON object a line;
/// other lines are skipped).
pub fn read_round(log: &Path) -> Round {
    let mut r = Round::default();
    for line in read_log(log).lines() {
        let Ok(v) = serde_json::from_str::<Value>(line.trim()) else { continue };
        let num = |x: &Value| x.as_u64().unwrap_or(0);
        match v.get("type").and_then(Value::as_str).unwrap_or_default() {
            // Claude Code
            "system" => {
                if let Some(s) = v.get("session_id").and_then(Value::as_str) {
                    r.session = Some(s.to_string());
                }
            }
            "result" => {
                if let Some(s) = v.get("session_id").and_then(Value::as_str) {
                    r.session = Some(s.to_string());
                }
                r.turns = r.turns.saturating_add(num(&v["num_turns"]));
                let u = &v["usage"];
                r.input_tokens = r
                    .input_tokens
                    .saturating_add(num(&u["input_tokens"]))
                    .saturating_add(num(&u["cache_creation_input_tokens"]))
                    .saturating_add(num(&u["cache_read_input_tokens"]));
                r.output_tokens = r.output_tokens.saturating_add(num(&u["output_tokens"]));
                if let Some(c) = v["total_cost_usd"].as_f64().filter(|c| c.is_finite() && *c >= 0.0) {
                    r.cost_usd += c;
                }
                let subtype = v["subtype"].as_str().unwrap_or_default();
                r.hit_turn_limit |= subtype == "error_max_turns";
                r.failed |= v["is_error"].as_bool().unwrap_or(false) || (subtype.starts_with("error") && subtype != "error_max_turns");
                if let Some(t) = v["result"].as_str() {
                    r.message = t.to_string();
                }
            }
            // Codex
            "thread.started" => {
                if let Some(s) = v.get("thread_id").and_then(Value::as_str) {
                    r.session = Some(s.to_string());
                }
            }
            "turn.completed" => {
                r.turns = r.turns.saturating_add(1);
                let u = &v["usage"];
                r.input_tokens = r.input_tokens.saturating_add(num(&u["input_tokens"]));
                r.output_tokens = r.output_tokens.saturating_add(num(&u["output_tokens"]));
            }
            "turn.failed" | "error" => {
                r.failed = true;
                r.message = v["error"]["message"].as_str().or_else(|| v["message"].as_str()).unwrap_or("the agent reported an error").to_string();
            }
            _ => {}
        }
    }
    r.message = cap(&r.message, 600);
    r
}

fn read_log(path: &Path) -> String {
    let Ok(mut f) = std::fs::File::open(path) else { return String::new() };
    let len = f.metadata().map(|m| m.len()).unwrap_or(0);
    if len > MAX_LOG && f.seek(SeekFrom::Start(len - MAX_LOG)).is_err() {
        return String::new();
    }
    let mut buf = Vec::new();
    if f.take(MAX_LOG).read_to_end(&mut buf).is_err() {
        return String::new();
    }
    String::from_utf8_lossy(&buf).into_owned()
}

fn cap(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { s.chars().take(max).collect::<String>() + "…" }
}

/// The first round's prompt.
pub fn first_prompt(step: &AgentStep, vars: &Vars) -> String {
    let brief = vars.expand(&step.brief);
    vars.expand(&format!(
        "Use the {skill} skill from the BashCutPro kit on the project {{project}} (MCP server `{SERVER}`). {brief} \
         Work only on this project and write files only inside {{itemDir}} or {{outputs}}. Save the project (command \
         file.save) before you finish.",
        skill = step.skill,
    ))
    .trim()
    .to_string()
}

/// A fix round's prompt: the review's blocking findings.
pub fn fix_prompt(step: &AgentStep, vars: &Vars, review: &Value) -> String {
    let mut lines = Vec::new();
    for f in blocking(review).into_iter().take(MAX_FINDINGS) {
        let at = match (f["start"].as_f64(), f["end"].as_f64()) {
            (Some(a), Some(b)) => format!(" {a:.2}-{b:.2} s"),
            (Some(a), None) => format!(" at {a:.2} s"),
            _ => String::new(),
        };
        let check = f["check"].as_str().unwrap_or("check");
        let message = f["message"].as_str().unwrap_or_default();
        let fix = f["fix"].as_str().map(|x| format!(" (fix: {x})")).unwrap_or_default();
        lines.push(format!("- [{check}]{at}: {}{fix}", cap(message, 200)));
    }
    vars.expand(&format!(
        "The review (bashcut.review.run) found blocking problems in {{project}}:\n{}\nFix them with the {} skill, keep \
         everything else as it is, save the project (command file.save) and finish.",
        lines.join("\n"),
        step.skill
    ))
}

/// The blocking findings of a `bashcut.review.run` result.
pub fn blocking(review: &Value) -> Vec<&Value> {
    review["findings"].as_array().map(|f| f.iter().filter(|x| x["severity"] == "blocking").collect()).unwrap_or_default()
}

/// How many blocking findings a review has (its `blocking` count, else counted).
pub fn blocking_count(review: &Value) -> u64 {
    review["blocking"].as_u64().unwrap_or_else(|| blocking(review).len() as u64)
}

/// What the job book keeps of a review: verdict, counts, scores, the blocking findings.
pub fn review_summary(review: &Value) -> Value {
    json!({
        "verdict": review["verdict"],
        "blocking": blocking_count(review),
        "warnings": review["warnings"],
        "scores": review["scores"],
        "findings": blocking(review).into_iter().take(MAX_FINDINGS).cloned().collect::<Vec<_>>(),
    })
}

/// The `result` of the `bashcut.review.run` line in a `filmcraft-cli run` log.
pub fn review_result(out_log: &Path) -> Option<Value> {
    read_log(out_log)
        .lines()
        .rev()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .find(|v| v["id"] == "bashcut.review.run" && v["ok"] == true)
        .map(|v| v["result"].clone())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(v: Value) -> AgentStep {
        serde_json::from_value(v).unwrap()
    }

    fn vars() -> Vars {
        Vars(vec![("project", "/p/a.fcproj".into()), ("itemDir", "/j/items/a".into()), ("outputs", "/o".into()), ("name", "a".into())])
    }

    #[test]
    fn claude_rounds_resume_the_session() {
        let s = step(json!({"skill": "rough-cut", "maxTurns": 7}));
        let v = vars();
        let cfg = PathBuf::from("/j/items/a/step-1.mcp.json");
        let first = argv(&Launch { step: &s, vars: &v, prompt: "go", session: None, mcp_config: &cfg });
        assert_eq!(first[..3], ["claude".to_string(), "-p".into(), "go".into()]);
        assert!(first.windows(2).any(|w| w[0] == "--max-turns" && w[1] == "7"));
        assert!(first.windows(2).any(|w| w[0] == "--mcp-config" && w[1] == "/j/items/a/step-1.mcp.json"));
        assert!(!first.contains(&"--resume".to_string()));
        let next = argv(&Launch { step: &s, vars: &v, prompt: "fix", session: Some("abc"), mcp_config: &cfg });
        assert!(next.windows(2).any(|w| w[0] == "--resume" && w[1] == "abc"));
    }

    #[test]
    fn own_argv_gets_prompt_session_and_config() {
        let s = step(json!({"skill": "x", "argv": ["my-agent", "{prompt}", "--s={session}", "{mcpConfig}", "{name}"]}));
        let v = vars();
        let cfg = PathBuf::from("/c.json");
        assert_eq!(argv(&Launch { step: &s, vars: &v, prompt: "p", session: None, mcp_config: &cfg }), vec!["my-agent", "p", "--s=", "/c.json", "a"]);
    }

    #[test]
    fn mcp_server_prefers_bashcut_mcp_with_roots() {
        let (prog, args) = mcp_server("fc", Some(Path::new("/bin/bashcut-mcp")), Path::new("/p/a.fcproj"), &[PathBuf::from("/j/items/a")]);
        assert_eq!(prog, "/bin/bashcut-mcp");
        assert_eq!(args[..5], ["--headless".to_string(), "--project".into(), "/p/a.fcproj".into(), "--cli".into(), "fc".into()]);
        assert_eq!(args.last().map(String::as_str), Some("/j/items/a"));
        let (prog, args) = mcp_server("fc", None, Path::new("/p/a.fcproj"), &[]);
        assert_eq!((prog.as_str(), args), ("fc", vec!["mcp".to_string(), "--project".into(), "/p/a.fcproj".into()]));
        let cfg = mcp_config("fc", &["mcp".to_string()]);
        assert_eq!(cfg["mcpServers"]["bashcutpro"]["command"], "fc");
    }

    #[test]
    fn rounds_are_read_from_claude_and_codex_events() {
        let dir = std::env::temp_dir().join(format!("bashcut-batch-agent-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let claude = dir.join("claude.out");
        std::fs::write(
            &claude,
            [
                json!({"type": "system", "subtype": "init", "session_id": "s1"}).to_string(),
                "not json".to_string(),
                json!({"type": "result", "subtype": "error_max_turns", "is_error": false, "num_turns": 7, "session_id": "s1", "total_cost_usd": 0.25, "usage": {"input_tokens": 10, "cache_read_input_tokens": 5, "output_tokens": 3}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let r = read_round(&claude);
        assert_eq!((r.session.as_deref(), r.turns, r.input_tokens, r.output_tokens), (Some("s1"), 7, 15, 3));
        assert!(r.hit_turn_limit && !r.failed);
        assert!((r.cost_usd - 0.25).abs() < 1e-9);
        let codex = dir.join("codex.out");
        std::fs::write(
            &codex,
            [
                json!({"type": "thread.started", "thread_id": "t9"}).to_string(),
                json!({"type": "turn.completed", "usage": {"input_tokens": 100, "output_tokens": 20}}).to_string(),
            ]
            .join("\n"),
        )
        .unwrap();
        let r = read_round(&codex);
        assert_eq!((r.session.as_deref(), r.turns, r.input_tokens, r.output_tokens), (Some("t9"), 1, 100, 20));
        assert_eq!(read_round(&dir.join("missing.out")), Round::default());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn fix_prompts_list_the_blocking_findings() {
        let review = json!({"verdict": "needsFix", "blocking": 1, "findings": [
            {"category": "captions", "check": "captions.readingSpeed", "severity": "blocking", "start": 1.0, "end": 2.5, "message": "24 cps", "fix": "split the caption"},
            {"category": "sound", "check": "sound.silence", "severity": "warning", "start": 4.0, "end": 9.0, "message": "long silence"}
        ]});
        let p = fix_prompt(&step(json!({"skill": "captions-text"})), &vars(), &review);
        assert!(p.contains("/p/a.fcproj"));
        assert!(p.contains("[captions.readingSpeed] 1.00-2.50 s: 24 cps (fix: split the caption)"), "{p}");
        assert!(!p.contains("long silence"));
        assert_eq!(blocking_count(&review), 1);
        assert_eq!(review_summary(&review)["findings"].as_array().map(Vec::len), Some(1));
        let first = first_prompt(&step(json!({"skill": "rough-cut", "brief": "Cut {name} to 60 s."})), &vars());
        assert!(first.contains("rough-cut") && first.contains("Cut a to 60 s.") && first.contains("/j/items/a"), "{first}");
    }
}
