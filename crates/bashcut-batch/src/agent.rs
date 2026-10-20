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
//! With the kit (`--kit`, `$BASHCUT_KIT`, #204), Claude Code also gets `--plugin-dir <kit>` and may use
//! [`KIT_TOOLS`] (the kit's skills, its Python scripts, ffmpeg) besides the MCP tools; every agent's
//! prompt says where the kit's skills and scripts are. A step's `allowedTools` adds to the list.
//!
//! The MCP server the agent gets is `bashcut-mcp --headless --project <item> --roots <item folder,
//! outputs>` when the runner knows a `bashcut-mcp` (writes only there, #51), else `filmcraft-cli mcp
//! --project <item>`.

use crate::pipeline::{AgentStep, CommandLine, Vars};
use serde_json::{Value, json};
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};

/// The MCP server name in the agent's config (Claude Code tool names: `mcp__bashcutpro__*`).
pub const SERVER: &str = "bashcutpro";
/// What a Claude Code agent with the kit may use besides the MCP tools: the kit's skills, reading
/// and writing its files (the prompt keeps writes in the item folder and the outputs), the kit's
/// Python scripts and ffmpeg / ffprobe for measuring.
pub const KIT_TOOLS: &[&str] = &["Skill", "Read", "Glob", "Grep", "Write", "Edit", "Bash(python3:*)", "Bash(python:*)", "Bash(ffmpeg:*)", "Bash(ffprobe:*)"];
/// Event logs above this are read from their end only.
const MAX_LOG: u64 = 16 << 20;
/// Findings listed in a fix prompt.
const MAX_FINDINGS: usize = 20;
/// Bins walked below the project root when listing its sequences.
const MAX_BIN_DEPTH: usize = 64;

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
            let kit = kit(l.step, l.vars);
            if kit.is_some() {
                v.extend(KIT_TOOLS.iter().map(|t| t.to_string()));
            }
            v.extend(l.step.allowed_tools.iter().cloned());
            if let Some(k) = kit {
                v.extend(["--plugin-dir".to_string(), k.to_string()]);
            }
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

/// The kit folder this step's agent gets, if any.
fn kit<'a>(step: &AgentStep, vars: &'a Vars) -> Option<&'a str> {
    vars.get("kit").filter(|_| step.kit)
}

/// The first round's prompt.
pub fn first_prompt(step: &AgentStep, vars: &Vars) -> String {
    let brief = vars.expand(&step.brief);
    let kit = match kit(step, vars) {
        Some(_) => " The kit is in {kit}: each skill is {kit}/skills/<name>/SKILL.md, and its scripts run with python3 \
                    (for example python3 {kit}/skills/captions-text/captions_fit.py)."
            .to_string(),
        None => String::new(),
    };
    vars.expand(&format!(
        "Use the {skill} skill from the BashCutPro kit on the project {{project}} (MCP server `{SERVER}`).{kit} {brief} \
         Work only on this project and write files only inside {{itemDir}} or {{outputs}}. Save the project (command \
         file.save) after each finished part and before you finish: if you run out of turns, the item is exported \
         from the last save.",
        skill = step.skill,
    ))
    .trim()
    .to_string()
}

/// A fix round's prompt: the review's blocking findings, each with its sequence when the review
/// covered several (#195). Sequences take turns in the list, so none is left out by the cap.
pub fn fix_prompt(step: &AgentStep, vars: &Vars, review: &Value) -> String {
    let all = blocking(review);
    let mut lines = Vec::new();
    for f in interleave(&all).into_iter().take(MAX_FINDINGS) {
        let at = match (f["start"].as_f64(), f["end"].as_f64()) {
            (Some(a), Some(b)) => format!(" {a:.2}-{b:.2} s"),
            (Some(a), None) => format!(" at {a:.2} s"),
            _ => String::new(),
        };
        let check = f["check"].as_str().unwrap_or("check");
        let message = f["message"].as_str().unwrap_or_default();
        let fix = f["fix"].as_str().map(|x| format!(" (fix: {x})")).unwrap_or_default();
        lines.push(format!("- [{check}]{}{at}: {}{fix}", in_sequence(f), cap(message, 200)));
    }
    if all.len() > MAX_FINDINGS {
        lines.push(format!("- … and {} more", all.len() - MAX_FINDINGS));
    }
    let open =
        if review["sequences"].as_array().is_some_and(|s| s.len() > 1) { " Open each sequence (sequence.open) before you fix its findings." } else { "" };
    vars.expand(&format!(
        "The review (bashcut.review.run) found blocking problems in {{project}}:\n{}\nFix them with the {} skill, keep \
         everything else as it is, save the project (command file.save) and finish.{open}",
        lines.join("\n"),
        step.skill
    ))
}

/// ` in sequence "name" (item N)` for a finding from a review of several sequences.
fn in_sequence(f: &Value) -> String {
    match (f["sequenceName"].as_str(), f["sequence"].as_u64()) {
        (Some(name), Some(id)) => format!(" in sequence \"{}\" (item {id})", cap(name, 80)),
        (None, Some(id)) => format!(" in sequence item {id}"),
        _ => String::new(),
    }
}

/// Findings reordered so each sequence takes a turn: the first of every sequence, then the second…
fn interleave<'a>(findings: &[&'a Value]) -> Vec<&'a Value> {
    let mut groups: Vec<(Value, Vec<&'a Value>)> = Vec::new();
    for f in findings {
        let key = f["sequence"].clone();
        match groups.iter_mut().find(|(k, _)| *k == key) {
            Some((_, g)) => g.push(f),
            None => groups.push((key, vec![f])),
        }
    }
    let longest = groups.iter().map(|(_, g)| g.len()).max().unwrap_or(0);
    (0..longest).flat_map(|i| groups.iter().filter_map(move |(_, g)| g.get(i).copied())).collect()
}

/// Labels for an error message: `check`, or `sequence: check` when the review covered several.
pub fn blocking_labels(review: &Value, max: usize) -> Vec<String> {
    interleave(&blocking(review))
        .into_iter()
        .take(max)
        .filter_map(|f| {
            let check = f["check"].as_str()?;
            Some(match f["sequenceName"].as_str() {
                Some(name) => format!("{}: {check}", cap(name, 80)),
                None => check.to_string(),
            })
        })
        .collect()
}

/// The blocking findings of a `bashcut.review.run` result.
pub fn blocking(review: &Value) -> Vec<&Value> {
    review["findings"].as_array().map(|f| f.iter().filter(|x| x["severity"] == "blocking").collect()).unwrap_or_default()
}

/// How many blocking findings a review has (its `blocking` count, else counted).
pub fn blocking_count(review: &Value) -> u64 {
    review["blocking"].as_u64().unwrap_or_else(|| blocking(review).len() as u64)
}

/// What the job book keeps of a review: verdict, counts, scores, the blocking findings, and each
/// sequence's verdict when it covered several.
pub fn review_summary(review: &Value) -> Value {
    let mut v = json!({
        "verdict": review["verdict"],
        "blocking": blocking_count(review),
        "warnings": review["warnings"],
        "scores": review["scores"],
        "findings": interleave(&blocking(review)).into_iter().take(MAX_FINDINGS).cloned().collect::<Vec<_>>(),
    });
    if let Some(s) = review.get("sequences").filter(|s| s.is_array()) {
        v["sequences"] = s.clone();
    }
    v
}

/// The `result` of every successful `id` line in a `filmcraft-cli run` log, in order.
pub fn results(out_log: &Path, id: &str) -> Vec<Value> {
    read_log(out_log)
        .lines()
        .filter_map(|l| serde_json::from_str::<Value>(l.trim()).ok())
        .filter(|v| v["id"] == id && v["ok"] == true)
        .map(|v| v["result"].clone())
        .collect()
}

/// A sequence of an item's project.
#[derive(Clone, Debug, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Sequence {
    pub id: u64,
    pub name: String,
}

/// The sequences in a `project.inspect` result, every bin walked (multi-camera source sequences and
/// merged clips are sources, not outputs, and are left out).
pub fn sequences(inspect: &Value) -> Vec<Sequence> {
    fn walk(bin: &Value, depth: usize, out: &mut Vec<Sequence>) {
        if depth > MAX_BIN_DEPTH {
            return;
        }
        for c in bin["children"].as_array().map(Vec::as_slice).unwrap_or_default() {
            match (c["item"].as_u64(), c["type"].as_str()) {
                (Some(id), Some("Sequence")) => out.push(Sequence { id, name: c["name"].as_str().unwrap_or_default().to_string() }),
                (None, _) if c.get("children").is_some() => walk(c, depth.saturating_add(1), out),
                _ => {}
            }
        }
    }
    let mut out = Vec::new();
    walk(&inspect["root"], 0, &mut out);
    out
}

/// The active sequence in a `project.inspect` result.
pub fn active_sequence(inspect: &Value) -> Option<u64> {
    inspect["activeSequence"].as_u64()
}

/// The sequences a review after an agent round covers (#195): every sequence the agent made (in
/// `now`, not in `before`). Empty when it made none: the agent edited in place, and the active
/// sequence is reviewed as before.
pub fn review_targets(before: &[Sequence], now: &[Sequence]) -> Vec<Sequence> {
    now.iter().filter(|s| !before.iter().any(|b| b.id == s.id)).cloned().collect()
}

/// The review script: for each target `sequence.open` then `bashcut.review.run` on it, and the
/// active sequence opened again at the end; a plain review of the active sequence without targets.
pub fn review_lines(targets: &[Sequence], active: Option<u64>, profile: Option<&str>) -> Vec<CommandLine> {
    let review = |sequence: Option<u64>| {
        let mut params = json!({});
        if let Some(p) = profile {
            params["profile"] = json!(p);
        }
        if let Some(id) = sequence {
            params["sequence"] = json!(id);
        }
        CommandLine { id: "bashcut.review.run".into(), params }
    };
    if targets.is_empty() {
        return vec![review(None)];
    }
    let open = |id: u64| CommandLine { id: "sequence.open".into(), params: json!({"item": id}) };
    let mut lines: Vec<CommandLine> = targets.iter().flat_map(|t| [open(t.id), review(Some(t.id))]).collect();
    if let Some(a) = active {
        lines.push(open(a));
    }
    lines
}

/// One review over several sequences: counts summed, each score the worst sequence's, findings
/// tagged with their `sequence` / `sequenceName`, and each sequence's verdict under `sequences`.
pub fn merge_reviews(reviews: &[(Sequence, Value)]) -> Value {
    let mut blocking_total = 0u64;
    let mut warnings = 0u64;
    let mut info = 0u64;
    let mut scores = serde_json::Map::new();
    let mut findings = Vec::new();
    let mut per = Vec::new();
    for (s, r) in reviews {
        let b = blocking_count(r);
        blocking_total = blocking_total.saturating_add(b);
        warnings = warnings.saturating_add(r["warnings"].as_u64().unwrap_or(0));
        info = info.saturating_add(r["info"].as_u64().unwrap_or(0));
        for (k, v) in r["scores"].as_object().into_iter().flatten() {
            let Some(n) = v.as_f64() else { continue };
            let worse = scores.get(k).and_then(Value::as_f64).is_none_or(|old| n < old);
            if worse {
                scores.insert(k.clone(), v.clone());
            }
        }
        for f in r["findings"].as_array().map(Vec::as_slice).unwrap_or_default() {
            let mut f = f.clone();
            if let Some(m) = f.as_object_mut() {
                m.insert("sequence".into(), json!(s.id));
                m.insert("sequenceName".into(), json!(s.name));
            }
            findings.push(f);
        }
        per.push(json!({"sequence": s.id, "name": s.name, "verdict": r["verdict"], "blocking": b, "warnings": r["warnings"], "scores": r["scores"]}));
    }
    json!({
        "ok": blocking_total == 0,
        "verdict": if blocking_total == 0 { "pass" } else { "needsFix" },
        "blocking": blocking_total,
        "warnings": warnings,
        "info": info,
        "scores": scores,
        "findings": findings,
        "sequences": per,
    })
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
    fn the_kit_is_a_plugin_dir_with_its_tools_and_named_in_the_prompt() {
        let s = step(json!({"skill": "captions-text", "allowedTools": ["Bash(sox:*)"]}));
        let mut v = vars();
        v.0.push(("kit", "/k".into()));
        let cfg = PathBuf::from("/c.json");
        let a = argv(&Launch { step: &s, vars: &v, prompt: "go", session: None, mcp_config: &cfg });
        assert!(a.windows(2).any(|w| w[0] == "--plugin-dir" && w[1] == "/k"));
        let tools = &a[a.iter().position(|x| x == "--allowedTools").unwrap() + 1..];
        assert!(tools.starts_with(&["mcp__bashcutpro".to_string(), "Skill".into()]));
        assert!(tools.contains(&"Bash(python3:*)".to_string()) && tools.contains(&"Bash(sox:*)".to_string()));
        let p = first_prompt(&s, &v);
        assert!(p.contains("/k/skills/<name>/SKILL.md") && p.contains("python3 /k/skills/captions-text/captions_fit.py"), "{p}");
        // no kit, or the step opts out: MCP tools only (plus the step's own), no plugin dir
        let off = step(json!({"skill": "x", "kit": false}));
        let a = argv(&Launch { step: &off, vars: &v, prompt: "go", session: None, mcp_config: &cfg });
        assert!(!a.contains(&"--plugin-dir".to_string()) && !a.contains(&"Skill".to_string()));
        assert!(!first_prompt(&off, &v).contains("/k"));
        let a = argv(&Launch { step: &s, vars: &vars(), prompt: "go", session: None, mcp_config: &cfg });
        assert!(!a.contains(&"--plugin-dir".to_string()) && a.contains(&"Bash(sox:*)".to_string()));
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

    #[test]
    fn reviews_of_every_new_sequence_are_merged_and_named_in_the_fix_prompt() {
        let seq = |id: u64, name: &str| json!({"item": id, "name": name, "type": "Sequence"});
        let inspect = json!({"activeSequence": 4, "root": {"children": [
            {"item": 1, "name": "talk.mp4", "type": "Movie"}, seq(2, "talk"),
            {"item": 6, "name": "cam", "type": "Multi-Camera Source Sequence"},
            {"id": 9, "name": "Clips", "children": [seq(3, "clip-1"), seq(4, "clip-2"), {"id": 10, "children": [seq(5, "clip-3")]}]}
        ]}});
        let now = sequences(&inspect);
        assert_eq!(now.iter().map(|s| s.id).collect::<Vec<_>>(), vec![2, 3, 4, 5]);
        let before = vec![Sequence { id: 2, name: "talk".into() }];
        let targets = review_targets(&before, &now);
        assert_eq!(targets.iter().map(|s| s.name.as_str()).collect::<Vec<_>>(), vec!["clip-1", "clip-2", "clip-3"]);
        // each target opened and reviewed, the active one opened again at the end
        let lines = review_lines(&targets, active_sequence(&inspect), Some("youtube"));
        let ids: Vec<String> = lines.iter().map(|l| format!("{} {}", l.id, l.params)).collect();
        assert_eq!(ids.len(), 7);
        assert_eq!(ids[0], r#"sequence.open {"item":3}"#);
        assert_eq!(ids[1], r#"bashcut.review.run {"profile":"youtube","sequence":3}"#);
        assert_eq!(ids[6], r#"sequence.open {"item":4}"#);
        // nothing new: the active sequence, as before
        let plain = review_lines(&review_targets(&now, &now), Some(4), None);
        assert_eq!(plain, vec![CommandLine { id: "bashcut.review.run".into(), params: json!({}) }]);

        let finding =
            |at: f64| json!({"check": "captions.readingSpeed", "severity": "blocking", "start": at, "end": at + 1.0, "message": "35 characters a second"});
        let bad = |n: usize| json!({"verdict": "needsFix", "blocking": n, "warnings": 1, "scores": {"captions": 40, "sound": 100}, "findings": (0..n).map(|i| finding(i as f64)).collect::<Vec<_>>()});
        let good = json!({"verdict": "pass", "blocking": 0, "warnings": 2, "scores": {"captions": 90, "sound": 80}, "findings": []});
        let merged = merge_reviews(&[(targets[0].clone(), bad(30)), (targets[1].clone(), good), (targets[2].clone(), bad(3))]);
        assert_eq!((merged["verdict"].as_str(), blocking_count(&merged), merged["warnings"].as_u64()), (Some("needsFix"), 33, Some(4)));
        assert_eq!(merged["scores"], json!({"captions": 40, "sound": 80}));
        assert_eq!(merged["sequences"][1]["verdict"], "pass");
        let p = fix_prompt(&step(json!({"skill": "captions-text"})), &vars(), &merged);
        assert!(p.contains(r#"[captions.readingSpeed] in sequence "clip-1" (item 3) 0.00-1.00 s"#), "{p}");
        // clip-1's 30 findings do not crowd clip-3 out of the list
        assert!(p.contains(r#"in sequence "clip-3" (item 5) 2.00-3.00 s"#), "{p}");
        assert!(!p.contains("clip-2") && p.contains("and 13 more") && p.contains("sequence.open"), "{p}");
        assert_eq!(blocking_labels(&merged, 2), vec!["clip-1: captions.readingSpeed", "clip-3: captions.readingSpeed"]);
        let summary = review_summary(&merged);
        assert_eq!(summary["sequences"].as_array().map(Vec::len), Some(3));
        assert_eq!(summary["findings"][1]["sequenceName"], "clip-3");
    }
}
