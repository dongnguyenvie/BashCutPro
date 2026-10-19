//! `bashcut.batch` (#44): many commands as one undo step, all or nothing.
//!
//! Upstream's `command_batch` makes every edit its own undo step and stops half-way on an error. Here the
//! steps run through [`Session::execute`] as usual, then the undo entries they pushed are folded into one
//! (the project before the batch, labelled with the batch's label). With `atomic` (the default) an error
//! puts the project, the editor state and the history back as they were before the batch and names the
//! failing step. No hook: `History` is public and stores whole-project snapshots.
//!
//! Only the project and the editor state are restored: side effects outside them (files written,
//! preferences, background jobs) stay, so steps that have them (`file.*`, `prefs.*`, exports) belong
//! outside an atomic batch.

use std::sync::Arc;

use serde_json::{Value, json};

use super::errors::{code_of, fail};
use super::summary;
use crate::commands::{CommandSpec, always, bad};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.batch";
/// At most this many steps in one batch.
pub const MAX_STEPS: usize = 10_000;
/// Labels are cut to this many characters.
const MAX_LABEL: usize = 80;

/// Steps a batch refuses: they rewrite the history the batch folds, or nest batches.
fn refused(id: &str) -> bool {
    id == CMD || id == "edit.undo" || id == "edit.redo" || id.starts_with("history.")
}

/// The id a step's result names as created (or last of a list it created), if any.
pub(crate) fn created_id(v: &Value) -> Option<u64> {
    for k in ["createdId", "clip", "item", "marker", "track", "sequence", "caption", "id"] {
        if let Some(id) = v.get(k).and_then(Value::as_u64) {
            return Some(id);
        }
    }
    for k in ["clips", "items", "markers", "tracks", "captions"] {
        if let Some(id) = v.get(k).and_then(Value::as_array).and_then(|a| a.iter().rev().find_map(Value::as_u64)) {
            return Some(id);
        }
    }
    None
}

/// One parsed step.
struct Step {
    id: String,
    params: Value,
}

fn steps_p(p: &Value) -> Result<Vec<Step>> {
    let list = p.get("commands").and_then(Value::as_array).ok_or_else(|| bad(CMD, "need `commands`: [{\"id\": str, \"params\": {}}]"))?;
    if list.is_empty() {
        return Err(bad(CMD, "`commands` is empty"));
    }
    if list.len() > MAX_STEPS {
        return Err(bad(CMD, format!("at most {MAX_STEPS} commands in one batch, got {}", list.len())));
    }
    let mut out = Vec::with_capacity(list.len());
    for (i, c) in list.iter().enumerate() {
        let id = c.get("id").and_then(Value::as_str).ok_or_else(|| bad(CMD, format!("commands[{i}] needs an `id` string")))?;
        if refused(id) {
            return Err(bad(CMD, format!("commands[{i}]: `{id}` cannot run inside a batch (it changes the history the batch groups)")));
        }
        if crate::commands::find(id).is_none() {
            return Err(bad(CMD, format!("commands[{i}]: unknown command `{id}`")));
        }
        let params = match c.get("params") {
            None | Some(Value::Null) => json!({}),
            Some(v) if v.is_object() => v.clone(),
            Some(_) => return Err(bad(CMD, format!("commands[{i}]: `params` must be an object"))),
        };
        out.push(Step { id: id.to_string(), params });
    }
    Ok(out)
}

/// What the session looked like before the batch.
struct Before {
    project: Arc<filmcraft_project::Project>,
    state: crate::EditorState,
    undo_len: usize,
    redo: Vec<(String, Arc<filmcraft_project::Project>)>,
    limit: usize,
}

impl Before {
    fn take(s: &mut Session) -> Before {
        let b =
            Before { project: s.project.clone(), state: s.state.clone(), undo_len: s.history.undo.len(), redo: s.history.redo.clone(), limit: s.history.limit };
        // no step may fold into an edit made before the batch, and no step may push the batch's own
        // first entries out of the history
        s.history.merge_key = None;
        s.history.limit = usize::MAX;
        b
    }

    /// Put everything back as it was.
    fn restore(self, s: &mut Session) {
        s.project = self.project;
        s.state = self.state;
        s.history.undo.truncate(self.undo_len);
        s.history.redo = self.redo;
        s.history.merge_key = None;
        s.history.limit = self.limit;
        s.fix_state();
        s.bump();
    }

    /// Fold the undo entries the steps pushed into one. Returns the number of undo steps the batch
    /// left (0 when nothing undoable happened, else 1).
    fn fold(self, s: &mut Session, label: &str) -> usize {
        s.history.limit = self.limit;
        s.history.merge_key = None;
        let pushed = s.history.undo.len().saturating_sub(self.undo_len);
        if pushed == 0 && Arc::ptr_eq(&s.project, &self.project) {
            return 0;
        }
        if s.history.undo.len() < self.undo_len {
            // a step reset the history (a project was opened): nothing to fold into
            return pushed.min(1);
        }
        s.history.undo.truncate(self.undo_len);
        s.history.undo.push((label.to_string(), self.project));
        let over = s.history.undo.len().saturating_sub(s.history.limit.max(1));
        if over > 0 {
            s.history.undo.drain(..over);
        }
        1
    }
}

fn batch(s: &mut Session, p: &Value) -> Result<Value> {
    let steps = steps_p(p)?;
    let atomic = match p.get("atomic") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| bad(CMD, "`atomic` must be true or false"))?,
    };
    let label = match p.get("label").and_then(Value::as_str).map(str::trim).filter(|l| !l.is_empty()) {
        Some(l) => l.chars().take(MAX_LABEL).collect(),
        None => format!("Batch ({} commands)", steps.len()),
    };
    let total = steps.len();
    let summary_before = summary::active(s);
    let before = Before::take(s);
    let mut results = Vec::with_capacity(steps.len());
    let mut last_created = None;
    let mut failed: Option<(usize, String, EngineError)> = None;
    for (i, st) in steps.into_iter().enumerate() {
        match s.execute(&st.id, st.params) {
            Ok(v) => {
                if let Some(id) = created_id(&v) {
                    last_created = Some(id);
                }
                results.push(json!({"ok": true, "id": st.id, "result": v}));
            }
            Err(e) => {
                if atomic {
                    failed = Some((i, st.id, e));
                    break;
                }
                results.push(json!({"ok": false, "id": st.id, "error": e.to_string()}));
            }
        }
    }
    if let Some((i, id, e)) = failed {
        before.restore(s);
        return Err(fail(code_of(&e), CMD, format!("step {} of {} (`{id}`) failed: {e}; nothing was changed", i + 1, total)));
    }
    let undo_steps = before.fold(s, &label);
    let errors = results.iter().filter(|r| r["ok"] == false).count();
    let changed = match (summary_before, summary::active(s)) {
        (Some(b), Some(a)) => summary::changes(&b, &a),
        (_, a) => a.unwrap_or(Value::Null),
    };
    Ok(json!({
        "ok": errors == 0,
        "label": label,
        "count": results.len(),
        "failed": errors,
        "undoSteps": undo_steps,
        "createdId": last_created,
        "lastCreatedId": last_created,
        "changed": changed,
        "results": results,
    }))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Run Batch",
        menu: &[],
        shortcut: None,
        params: r#"{"commands":[{"id":str,"params":{}?}] (at most 10000),"atomic":bool=true,"label":str?} (one undo step; atomic: an error restores the project and names the failing step; times inside each step's params use that command's units)"#,
        enabled: always,
        run: batch,
        // the steps are journaled themselves; journaling the batch too would replay them twice
        journal: false,
    }]
}
