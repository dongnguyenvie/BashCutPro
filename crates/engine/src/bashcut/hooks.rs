//! BashCutPro: plugin hooks run by our own code paths (#73), never by upstream events.
//!
//! A plugin declares hooks in its `bashcut-plugin.json`:
//!
//! ```json
//! "hooks": [{"event": "export.before", "action": "check-key", "required": false, "timeoutSeconds": 120}]
//! ```
//!
//! Events and who fires them: `batch.beforeItem` / `batch.afterItem` (the batch runner, through `bashcut.hooks.run`),
//! `import.after` (`bashcut.media.import`), `export.before` (`bashcut.profiles.apply`). A hook runs the plugin's
//! action like any other run (the entry command + the action's `args`, plus `--project <file>` when the action needs
//! the project), in the plugin's folder, with `BASHCUT_HOOK_EVENT` and `BASHCUT_HOOK_CONTEXT` (JSON) in its
//! environment, and answers with `bashcut-result/1` on stdout. Exit 0 = ok, 3 = hand-off to the agent (reported, not
//! a failure), anything else = failed; a failed `required` hook stops the command that fired it. `BASHCUT_HOOKS=off`
//! or `hooks: false` turns hooks off. Only plugins installed from the registry (`<data dir>/bashcut-plugins/`) run.

use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use serde_json::{Value, json};

use super::data;
use crate::commands::{CommandSpec, always, bad, bool_p, str_p};
use crate::{EngineError, Result, Session};

pub const EVENTS: &[&str] = &["batch.beforeItem", "batch.afterItem", "import.after", "export.before"];
const MAX_PLUGINS: usize = 200;
const MAX_HOOKS: usize = 32;
const DEFAULT_TIMEOUT: u64 = 300;
const MAX_TIMEOUT: u64 = 3600;
const MAX_OUTPUT: usize = 1 << 20;

#[derive(Clone, Debug)]
pub struct Hook {
    pub plugin: String,
    pub dir: PathBuf,
    pub event: String,
    pub action: String,
    /// Entry command + the action's args.
    pub argv: Vec<String>,
    /// `connect.project` (e.g. `["--project", "{project}"]`) when the action needs the project.
    pub project_args: Option<Vec<String>>,
    pub required: bool,
    pub timeout: Duration,
}

fn strings(v: Option<&Value>) -> Option<Vec<String>> {
    Some(v?.as_array()?.iter().filter_map(Value::as_str).map(str::to_string).collect())
}

fn os_key() -> &'static str {
    match std::env::consts::OS {
        "macos" => "macos",
        "windows" => "windows",
        _ => "linux",
    }
}

/// The plugins folder: `pluginsDir`, else `<data dir>/bashcut-plugins` (the session's data dir, as in `data.rs`).
fn plugins_dir(s: &Session, p: &Value) -> Option<PathBuf> {
    if let Some(d) = str_p(p, "pluginsDir").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    super::data::base(s).map(|d| d.join(bashcut_dirs::PLUGINS_DIR))
}

/// Hooks for `event` declared by the installed plugins, in plugin-folder order.
pub fn discover(dir: &Path, event: &str) -> Vec<Hook> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).take(MAX_PLUGINS).collect();
    folders.sort();
    let mut out = Vec::new();
    for folder in folders {
        let Ok(m) = data::read_json(&folder.join("bashcut-plugin.json"), 1 << 20) else { continue };
        if m.get("schema").and_then(Value::as_str) != Some("bashcut-plugin/1") {
            continue;
        }
        let plugin = m.get("id").and_then(Value::as_str).unwrap_or_default().to_string();
        let entry = m.get("entry");
        let Some(base) = strings(entry.and_then(|e| e.get(os_key()).or_else(|| e.get("command")))).filter(|v| !v.is_empty()) else { continue };
        let actions = m.get("actions").and_then(Value::as_array).cloned().unwrap_or_default();
        let hooks = m.get("hooks").and_then(Value::as_array).cloned().unwrap_or_default();
        for h in hooks.iter().take(MAX_HOOKS) {
            if h.get("event").and_then(Value::as_str) != Some(event) {
                continue;
            }
            let Some(action_id) = h.get("action").and_then(Value::as_str) else { continue };
            let Some(action) = actions.iter().find(|a| a.get("id").and_then(Value::as_str) == Some(action_id)) else {
                log::warn!("plugin {plugin}: hook names unknown action `{action_id}`");
                continue;
            };
            let mut argv = base.clone();
            argv.extend(strings(action.get("args")).unwrap_or_default());
            let needs_project = action.get("needs").and_then(Value::as_str) == Some("project");
            let project_args =
                needs_project.then(|| strings(m.get("connect").and_then(|c| c.get("project"))).unwrap_or_else(|| vec!["--project".into(), "{project}".into()]));
            let secs = h.get("timeoutSeconds").and_then(Value::as_u64).unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT);
            out.push(Hook {
                plugin: plugin.clone(),
                dir: folder.clone(),
                event: event.to_string(),
                action: action_id.to_string(),
                argv,
                project_args,
                required: h.get("required").and_then(Value::as_bool).unwrap_or(false),
                timeout: Duration::from_secs(secs),
            });
        }
    }
    out
}

/// Run one hook; the JSON row reports what happened (`status`: ok | handoff | failed | skipped).
pub fn run_hook(hook: &Hook, project: Option<&str>, context: &Value) -> Value {
    let row = |status: &str, extra: Value| {
        let mut v = json!({"plugin": hook.plugin, "action": hook.action, "event": hook.event, "required": hook.required, "status": status});
        if let (Some(o), Value::Object(e)) = (v.as_object_mut(), extra) {
            o.extend(e);
        }
        v
    };
    let mut args: Vec<String> = hook.argv.iter().skip(1).cloned().collect();
    if let Some(pa) = &hook.project_args {
        let Some(project) = project else { return row("skipped", json!({"message": "the action needs a saved project"})) };
        args.extend(pa.iter().map(|a| a.replace("{project}", project)));
    }
    let Some(program) = hook.argv.first() else { return row("failed", json!({"message": "no entry command"})) };
    let child = Command::new(program)
        .args(&args)
        .current_dir(&hook.dir)
        .env("BASHCUT_HOOK_EVENT", &hook.event)
        .env("BASHCUT_HOOK_CONTEXT", context.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(c) => c,
        Err(e) => return row("failed", json!({"message": format!("{program}: {e}")})),
    };
    // read stdout on its own thread so a chatty plugin cannot fill the pipe and stall
    let reader = child.stdout.take().map(|mut out| {
        std::thread::spawn(move || {
            let mut buf = Vec::new();
            let _ = (&mut out).take(MAX_OUTPUT as u64).read_to_end(&mut buf);
            buf
        })
    });
    let start = Instant::now();
    let status = loop {
        match child.try_wait() {
            Ok(Some(st)) => break Some(st),
            Ok(None) if start.elapsed() >= hook.timeout => {
                let _ = child.kill();
                let _ = child.wait();
                break None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(_) => break None,
        }
    };
    // after a timeout a grandchild may still hold the pipe open: leave the reader thread behind instead of waiting
    let stdout = if status.is_some() { reader.and_then(|r| r.join().ok()).unwrap_or_default() } else { Vec::new() };
    let result = String::from_utf8_lossy(&stdout)
        .lines()
        .rev()
        .find_map(|l| serde_json::from_str::<Value>(l.trim()).ok().filter(Value::is_object))
        .unwrap_or(Value::Null);
    let message = result.get("message").cloned().unwrap_or(Value::Null);
    match status.and_then(|s| s.code()) {
        None => row("failed", json!({"message": format!("timed out after {} s", hook.timeout.as_secs()), "result": result})),
        Some(0) => row("ok", json!({"message": message, "result": result})),
        Some(3) => row("handoff", json!({"message": message, "result": result})),
        Some(code) => row("failed", json!({"exit": code, "message": message, "result": result})),
    }
}

fn hooks_on(p: &Value) -> bool {
    let env_off = std::env::var("BASHCUT_HOOKS").is_ok_and(|v| matches!(v.to_ascii_lowercase().as_str(), "0" | "off" | "false" | "no"));
    !env_off && bool_p(p, "hooks").unwrap_or(true)
}

/// Fire `event` for the calling command. A failed required hook is an error (the caller stops); the rest is reported.
pub fn fire(s: &Session, p: &Value, event: &str, context: Value, cmd: &str) -> Result<Value> {
    if !hooks_on(p) {
        return Ok(Value::Array(Vec::new()));
    }
    let Some(dir) = plugins_dir(s, p) else { return Ok(Value::Array(Vec::new())) };
    let mut rows = Vec::new();
    for hook in discover(&dir, event) {
        let row = run_hook(&hook, s.path.as_deref(), &context);
        let failed = row.get("status").and_then(Value::as_str) == Some("failed");
        if failed && hook.required {
            return Err(EngineError::Other(format!(
                "{cmd}: required {event} hook {} / {} failed: {}",
                hook.plugin,
                hook.action,
                row.get("message").unwrap_or(&Value::Null)
            )));
        }
        rows.push(row);
    }
    Ok(Value::Array(rows))
}

pub fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "bashcut.hooks.list",
            label: "List Plugin Hooks",
            menu: &[],
            shortcut: None,
            params: r#"{"event":"batch.beforeItem|batch.afterItem|import.after|export.before"?,"pluginsDir":str?}"#,
            enabled: always,
            run: |s, p| {
                let Some(dir) = plugins_dir(s, p) else { return Ok(json!({"hooks": []})) };
                let events: Vec<&str> = match str_p(p, "event") {
                    Some(e) if EVENTS.contains(&e) => vec![e],
                    Some(e) => return Err(bad("bashcut.hooks.list", format!("unknown event `{e}` ({})", EVENTS.join(", ")))),
                    None => EVENTS.to_vec(),
                };
                let hooks: Vec<Value> = events
                    .iter()
                    .flat_map(|e| discover(&dir, e))
                    .map(|h| json!({"plugin": h.plugin, "event": h.event, "action": h.action, "required": h.required, "timeoutSeconds": h.timeout.as_secs()}))
                    .collect();
                Ok(json!({"dir": dir.to_string_lossy(), "hooks": hooks}))
            },
            journal: false,
        },
        CommandSpec {
            id: "bashcut.hooks.run",
            label: "Run Plugin Hooks",
            menu: &[],
            shortcut: None,
            params: r#"{"event":"batch.beforeItem|batch.afterItem|import.after|export.before","context":{}?,"hooks":bool=true,"pluginsDir":str?}  (the batch runner fires the batch.* events)"#,
            enabled: always,
            run: |s, p| {
                const CMD: &str = "bashcut.hooks.run";
                let event = str_p(p, "event").filter(|e| EVENTS.contains(e)).ok_or_else(|| bad(CMD, format!("need `event`: {}", EVENTS.join(", "))))?;
                let context = p.get("context").cloned().unwrap_or_else(|| json!({}));
                Ok(json!({"event": event, "hooks": fire(s, p, event, context, CMD)?}))
            },
            journal: false,
        },
        CommandSpec {
            id: "bashcut.media.import",
            label: "Import (with Plugin Hooks)",
            menu: &[],
            shortcut: None,
            params: r#"{"paths":[str], …file.import params, "hooks":bool=true,"pluginsDir":str?}  (file.import, then the import.after hooks)"#,
            enabled: always,
            run: |s, p| {
                let mut q = p.clone();
                if let Some(o) = q.as_object_mut() {
                    o.remove("hooks");
                    o.remove("pluginsDir");
                }
                let imported = s.execute("file.import", q)?;
                let context = json!({"paths": p.get("paths").cloned().unwrap_or(Value::Null), "items": imported.get("items").cloned().unwrap_or(Value::Null)});
                let hooks = fire(s, p, "import.after", context, "bashcut.media.import")?;
                let mut out = imported;
                if let Some(o) = out.as_object_mut() {
                    o.insert("hooks".into(), hooks);
                }
                Ok(out)
            },
            journal: true,
        },
    ]
}
