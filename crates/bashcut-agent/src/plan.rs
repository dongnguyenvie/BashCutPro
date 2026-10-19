//! Gates from `<project>.fcproj.bashcut/plan.json` (`bashcut/in-app-ui.md` §6). The kit's
//! edit-workflow skill writes `{"gates": [{stage, summary, evidence, state}]}` (other keys are its
//! own and kept as they are); the panel shows the first `pending` gate as a card, and Approve /
//! Change write the answer back here before it goes to the agent as the next message.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{Error, Result, clip, err};

pub const FILE: &str = "plan.json";
const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_GATES: usize = 500;

pub const PENDING: &str = "pending";
pub const APPROVED: &str = "approved";
pub const CHANGES: &str = "changes";

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Gate {
    /// Where it sits in `gates`.
    #[serde(skip)]
    pub index: usize,
    pub stage: String,
    pub summary: String,
    /// What the user looks at: a string, or a list of strings / `{label, ...}` objects.
    pub evidence: Value,
    pub state: String,
    pub note: String,
}

impl Gate {
    /// The evidence as lines.
    pub fn evidence_lines(&self) -> Vec<String> {
        let line = |v: &Value| match v {
            Value::String(s) => clip(s, 1024),
            Value::Object(o) => ["label", "text", "title", "path"]
                .iter()
                .find_map(|k| o.get(*k).and_then(Value::as_str))
                .map_or_else(|| clip(&v.to_string(), 1024), |s| clip(s, 1024)),
            other => clip(&other.to_string(), 1024),
        };
        match &self.evidence {
            Value::Null => Vec::new(),
            Value::Array(a) => a.iter().take(50).map(line).collect(),
            other => vec![line(other)],
        }
    }
}

/// `<sidecar>/plan.json`.
pub fn file(sidecar: &Path) -> PathBuf {
    sidecar.join(FILE)
}

fn read(sidecar: &Path) -> Result<Value> {
    let text = crate::read_capped(&file(sidecar), MAX_BYTES)?;
    serde_json::from_str(&text).map_err(|e| Error(format!("plan.json: {e}")))
}

/// Every gate in the plan (none when there is no plan or it is broken).
pub fn gates(sidecar: &Path) -> Vec<Gate> {
    let Ok(v) = read(sidecar) else { return Vec::new() };
    let Some(Value::Array(list)) = v.get("gates") else { return Vec::new() };
    list.iter()
        .take(MAX_GATES)
        .enumerate()
        .filter_map(|(i, g)| serde_json::from_value::<Gate>(g.clone()).ok().map(|g| Gate { index: i, ..g }))
        .map(|g| Gate { stage: clip(&g.stage, 256), summary: clip(&g.summary, 4096), ..g })
        .collect()
}

/// The first gate waiting for the user.
pub fn pending(sidecar: &Path) -> Option<Gate> {
    gates(sidecar).into_iter().find(|g| g.state == PENDING)
}

/// Answer the gate at `index` (it must still be pending) and return the message for the agent.
pub fn answer(sidecar: &Path, index: usize, approve: bool, note: &str) -> Result<String> {
    let note = note.trim();
    if !approve && note.is_empty() {
        return err("say what to change");
    }
    let mut v = read(sidecar)?;
    let Some(g) = v.get_mut("gates").and_then(Value::as_array_mut).and_then(|a| a.get_mut(index)) else { return err("no such gate") };
    let Some(obj) = g.as_object_mut() else { return err("no such gate") };
    if obj.get("state").and_then(Value::as_str) != Some(PENDING) {
        return err("that gate was already answered");
    }
    let stage = obj.get("stage").and_then(Value::as_str).map(|s| clip(s, 256)).unwrap_or_default();
    let state = if approve { APPROVED } else { CHANGES };
    obj.insert("state".into(), json!(state));
    obj.insert("note".into(), json!(clip(note, 4096)));
    let text = serde_json::to_string_pretty(&v).map_err(|e| Error(e.to_string()))?;
    crate::write_atomic(&file(sidecar), &text)?;
    Ok(match (approve, note.is_empty()) {
        (true, true) => format!("Gate `{stage}` approved. Go on."),
        (true, false) => format!("Gate `{stage}` approved. Note: {note}"),
        (false, _) => format!("Changes requested at gate `{stage}`: {note}"),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_gate_round_trips() {
        let dir = std::env::temp_dir().join(format!("bashcut-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert!(pending(&dir).is_none(), "no plan yet");
        std::fs::write(
            file(&dir),
            r#"{"brief":"keep me","gates":[{"stage":"story","state":"approved"},{"stage":"cut","summary":"12 clips","evidence":["a.png",{"label":"cut list"}],"state":"pending"},7]}"#,
        )
        .unwrap();
        let g = pending(&dir).unwrap();
        assert_eq!((g.index, g.stage.as_str()), (1, "cut"));
        assert_eq!(g.evidence_lines(), vec!["a.png".to_string(), "cut list".to_string()]);
        assert!(answer(&dir, 1, false, "  ").is_err(), "changes need a note");
        let msg = answer(&dir, 1, false, "shorter intro").unwrap();
        assert!(msg.contains("shorter intro"));
        assert!(answer(&dir, 1, true, "").is_err(), "already answered");
        assert!(answer(&dir, 9, true, "").is_err());
        let v = read(&dir).unwrap();
        assert_eq!(v["brief"], "keep me");
        assert_eq!(v["gates"][1]["state"], CHANGES);
        assert!(pending(&dir).is_none());
        std::fs::write(file(&dir), "{broken").unwrap();
        assert!(gates(&dir).is_empty());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
