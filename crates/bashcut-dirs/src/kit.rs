//! The one agent-kit lookup (AG15): the bashcutpro-agent-kit folder that batch agent steps and the in-app
//! Claude Code agent pass to Claude Code as `--plugin-dir`, so the `bcp` skills load only in sessions
//! BashCutPro starts and no global `claude plugin install` is needed.

use std::path::{Path, PathBuf};

/// The folder under the data folder where the kit is kept (`<data dir>/bashcut-kit`).
pub const DATA_SUBDIR: &str = "bashcut-kit";

/// Where the first-run setup clones the kit, under the user's home folder.
pub const HOME_SUBDIR: &str = ".bashcutpro/bashcutpro-agent-kit";

/// `dir` holds the kit (it has `skills/`).
pub fn is_kit(dir: &Path) -> bool {
    dir.join("skills").is_dir()
}

/// The kit in the default places: `<data dir>/bashcut-kit`, then `<home>/.bashcutpro/bashcutpro-agent-kit`.
pub fn kit_in(data_dir: Option<&Path>, home: Option<&Path>) -> Option<PathBuf> {
    data_dir.map(|d| d.join(DATA_SUBDIR)).into_iter().chain(home.map(|h| h.join(HOME_SUBDIR))).find(|k| is_kit(k))
}

/// The kit for this process: `$BASHCUT_KIT`, `$BASHCUTPRO_KIT_DIR` (when they hold a kit), else the default places
/// under [`crate::data_dir`] and the home folder.
pub fn kit_dir() -> Option<PathBuf> {
    let env = |k: &str| std::env::var_os(k).filter(|v| !v.is_empty()).map(PathBuf::from);
    if let Some(k) = ["BASHCUT_KIT", "BASHCUTPRO_KIT_DIR"].into_iter().filter_map(env).find(|k| is_kit(k)) {
        return Some(k);
    }
    let home = env("HOME").or_else(|| env("USERPROFILE"));
    kit_in(crate::data_dir().as_deref(), home.as_deref())
}

/// The kit's skill→command-id index (`scripts/skill-commands.json`, written by the kit's `check.py --write-index`).
pub const INDEX_FILE: &str = "scripts/skill-commands.json";

/// Largest index [`kit_command_gaps`] reads (the kit's own is about 10 KB).
pub const MAX_INDEX_BYTES: usize = 1 << 20;

/// What a kit names that an engine lacks.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct KitGaps {
    /// The kit's version, when the index has one.
    pub kit_version: Option<String>,
    /// `(skill or agent name, ids it names that the engine lacks)`, only entries with gaps, sorted by name.
    pub missing_by_skill: Vec<(String, Vec<String>)>,
}

/// Compares a kit index with the engine's command ids. Only engine ids (`bashcut.<group>.<name>`, three
/// segments) are compared: the index also names core ids, which the engine list the doctor passes
/// (`commands bashcut`) does not carry, and plugin ids (`bashcut.vision`), which are not engine commands.
/// `None` when the text is not an index (not JSON, no `skills` object, over [`MAX_INDEX_BYTES`]).
pub fn kit_command_gaps<S: AsRef<str>>(index_json: &str, engine_ids: &[S]) -> Option<KitGaps> {
    if index_json.len() > MAX_INDEX_BYTES {
        return None;
    }
    let index: serde_json::Value = serde_json::from_str(index_json).ok()?;
    let have: std::collections::HashSet<&str> = engine_ids.iter().map(AsRef::as_ref).collect();
    let mut gaps = KitGaps { kit_version: index.get("kit").and_then(|v| v.as_str()).map(str::to_string), ..KitGaps::default() };
    let mut entries: Vec<(&String, &serde_json::Value)> = Vec::new();
    for group in ["skills", "agents"] {
        match index.get(group) {
            Some(serde_json::Value::Object(m)) => entries.extend(m),
            Some(_) => return None,
            None if group == "skills" => return None,
            None => {}
        }
    }
    for (name, ids) in entries {
        let mut missing: Vec<String> = ids
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|v| v.as_str())
            .filter(|id| id.starts_with("bashcut.") && id.split('.').count() >= 3 && !have.contains(id))
            .map(str::to_string)
            .collect();
        missing.sort();
        missing.dedup();
        if !missing.is_empty() {
            gaps.missing_by_skill.push((name.clone(), missing));
        }
    }
    gaps.missing_by_skill.sort();
    Some(gaps)
}
