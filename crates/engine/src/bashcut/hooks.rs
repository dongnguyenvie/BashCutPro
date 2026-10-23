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
//! the project), in the plugin's folder, with `BASHCUT_HOOK_EVENT` and `BASHCUT_HOOK_CONTEXT` (JSON) and the
//! plugin's settings (secrets from the OS key store, as the host passes them to its runs; RC3, I2) in its
//! environment, under the shared child supervisor (`bashcut_dirs::child`: TERM, then KILL on timeout), and answers with `bashcut-result/1` on stdout. Exit 0 = ok, 3 = hand-off to the agent (reported, not
//! a failure), anything else = failed; a failed `required` hook stops the command that fired it. `BASHCUT_HOOKS=off`
//! or `hooks: false` turns hooks off. Only plugins installed from the registry (`<data dir>/bashcut-plugins/`) run.

use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

use bashcut_dirs::child::{self, Opts};
use bashcut_manifest::settings::{self, OsKeyStore};
use bashcut_manifest::{Manifest, Os};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, always, bad, bool_p, str_p};
use crate::{EngineError, Result, Session};

pub const EVENTS: &[&str] = &["batch.beforeItem", "batch.afterItem", "import.after", "export.before"];
const MAX_PLUGINS: usize = 200;
const DEFAULT_TIMEOUT: u64 = 300;
const MAX_TIMEOUT: u64 = 3600;
const MAX_OUTPUT: usize = 1 << 20;
/// stderr kept for the row of a failed hook.
const MAX_STDERR: usize = 2048;

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
    /// The plugin's manifest: its settings become the run's environment.
    pub manifest: Manifest,
}

/// The plugins folder: `pluginsDir`, else `<data dir>/bashcut-plugins` (the session's data dir, as in `data.rs`).
fn plugins_dir(s: &Session, p: &Value) -> Option<PathBuf> {
    if let Some(d) = str_p(p, "pluginsDir").filter(|d| !d.is_empty()) {
        return Some(PathBuf::from(d));
    }
    super::data::base(s).map(|d| d.join(bashcut_dirs::PLUGINS_DIR))
}

/// Hooks for `event` declared by the installed plugins, in plugin-folder order. Read through the plugin host's
/// own manifest reader (RC5): a manifest it would refuse declares no hooks either.
pub fn discover(dir: &Path, event: &str) -> Vec<Hook> {
    let Ok(rd) = std::fs::read_dir(dir) else { return Vec::new() };
    let mut folders: Vec<PathBuf> = rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).take(MAX_PLUGINS).collect();
    folders.sort();
    let mut out = Vec::new();
    for folder in folders {
        let m = match Manifest::load(&folder) {
            Ok(m) => m,
            Err(e) if folder.join(bashcut_manifest::FILE).is_file() => {
                log::warn!("plugin {}: {e}", folder.display());
                continue;
            }
            Err(_) => continue,
        };
        let base = m.entry_for(Os::current());
        for h in m.hooks.iter().filter(|h| h.event == event) {
            let Some(action) = m.action(&h.action) else {
                log::warn!("plugin {}: hook names unknown action `{}`", m.id, h.action);
                continue;
            };
            let mut argv = base.to_vec();
            argv.extend(action.args.iter().cloned());
            let project_args = (action.needs == "project")
                .then(|| if m.connect.project.is_empty() { vec!["--project".into(), "{project}".into()] } else { m.connect.project.clone() });
            let secs = h.timeout_seconds.unwrap_or(DEFAULT_TIMEOUT).clamp(1, MAX_TIMEOUT);
            out.push(Hook {
                plugin: m.id.clone(),
                dir: folder.clone(),
                event: event.to_string(),
                action: h.action.clone(),
                argv,
                project_args,
                required: h.required,
                timeout: Duration::from_secs(secs),
                manifest: m.clone(),
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
    // the same environment the host gives the plugin's action runs
    let values = hook.dir.parent().map(|d| settings::load(d, &hook.manifest)).unwrap_or_default();
    let mut env = settings::env_for(&hook.manifest, &values, &OsKeyStore);
    env.push(("BASHCUT_PLUGIN_HOST".into(), "1".into()));
    let mut cmd = Command::new(program);
    cmd.args(&args).current_dir(&hook.dir).envs(env).env("BASHCUT_HOOK_EVENT", &hook.event).env("BASHCUT_HOOK_CONTEXT", context.to_string());
    let mut c = match child::spawn(cmd, &Opts { stdin: None, group: true, stderr: true }) {
        Ok(c) => c,
        Err(e) => return row("failed", json!({"message": format!("{program}: {e}")})),
    };
    // read the pipes on their own threads so a chatty plugin cannot fill one and stall
    let out_rx = child::pump("bashcut-hook-stdout", c.stdout.take(), |out| child::read_lines(out, MAX_OUTPUT, 0, |_, _| {}).0);
    let err_rx = child::pump("bashcut-hook-stderr", c.stderr.take(), |errs| child::tail(errs, MAX_STDERR));
    let exit = child::supervise(&mut c, hook.timeout, None);
    // after a timeout a grandchild may still hold the pipe open: collect waits a bounded time
    let status = exit.status.filter(|_| exit.stop.is_none());
    let stdout = if status.is_some() { child::collect(&out_rx, child::STDOUT_WAIT) } else { Vec::new() };
    let stderr = String::from_utf8_lossy(&child::collect(&err_rx, child::STDERR_WAIT)).trim().to_string();
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
        Some(code) => row("failed", json!({"exit": code, "message": message, "result": result, "stderr": stderr})),
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
        spec! {
            id: "bashcut.hooks.list",
            label: "List Plugin Hooks",
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
        spec! {
            id: "bashcut.hooks.run",
            label: "Run Plugin Hooks",
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
        spec! {
            id: "bashcut.media.import",
            label: "Import (with Plugin Hooks)",
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
