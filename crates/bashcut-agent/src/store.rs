//! Conversations in `<project>.fcproj.bashcut/conversations/<id>.json`: the transcript and the
//! agent's session id (so a reopened conversation resumes where it was).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::transcript::{Item, Transcript};
use crate::{Error, Result, clip, err};

pub const SCHEMA: &str = "bashcut-conversation/1";
pub const DIR: &str = "conversations";
const MAX_BYTES: u64 = 16 * 1024 * 1024;
const MAX_LISTED: usize = 500;

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Conversation {
    pub schema: String,
    pub id: String,
    pub title: String,
    /// The agent's id (`claude-code`, a plugin id).
    pub agent: String,
    /// The agent's own session id, resumed by the next turn.
    pub session: String,
    pub created: u64,
    pub updated: u64,
    pub transcript: Transcript,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Summary {
    pub id: String,
    pub title: String,
    pub agent: String,
    pub updated: u64,
    pub items: usize,
}

pub fn now() -> u64 {
    SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or_default()
}

/// A conversation id: letters, digits and `-`, at most 64.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
}

impl Conversation {
    pub fn new(agent: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let t = now();
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        Conversation { schema: SCHEMA.into(), id: format!("c{t}-{n}"), agent: agent.into(), created: t, updated: t, ..Conversation::default() }
    }

    /// The title, from the first user message when none was set.
    pub fn title(&self) -> String {
        if !self.title.is_empty() {
            return self.title.clone();
        }
        self.transcript
            .items
            .iter()
            .find_map(|i| match i {
                Item::User { text } => Some(clip(text.lines().next().unwrap_or_default(), 60)),
                _ => None,
            })
            .unwrap_or_default()
    }

    fn summary(&self) -> Summary {
        Summary { id: self.id.clone(), title: self.title(), agent: self.agent.clone(), updated: self.updated, items: self.transcript.items.len() }
    }
}

/// `<sidecar>/conversations`.
pub fn dir(sidecar: &Path) -> PathBuf {
    sidecar.join(DIR)
}

pub fn load(sidecar: &Path, id: &str) -> Result<Conversation> {
    if !valid_id(id) {
        return err(format!("bad conversation id `{id}`"));
    }
    let text = crate::read_capped(&dir(sidecar).join(format!("{id}.json")), MAX_BYTES)?;
    let c: Conversation = serde_json::from_str(&text).map_err(|e| Error(format!("conversation {id}: {e}")))?;
    if c.schema != SCHEMA || c.id != id {
        return err(format!("conversation {id}: not a {SCHEMA} file"));
    }
    Ok(c)
}

pub fn save(sidecar: &Path, c: &Conversation) -> Result<()> {
    if !valid_id(&c.id) {
        return err(format!("bad conversation id `{}`", c.id));
    }
    let text = serde_json::to_string(c).map_err(|e| Error(e.to_string()))?;
    if text.len() as u64 > MAX_BYTES {
        return err("conversation too large to save");
    }
    crate::write_atomic(&dir(sidecar).join(format!("{}.json", c.id)), &text)
}

/// The saved conversations, most recent first.
pub fn list(sidecar: &Path) -> Vec<Summary> {
    let Ok(rd) = std::fs::read_dir(dir(sidecar)) else { return Vec::new() };
    let mut out: Vec<Summary> = rd
        .flatten()
        .take(MAX_LISTED * 2)
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            let id = name.strip_suffix(".json")?;
            load(sidecar, id).ok().map(|c| c.summary())
        })
        .take(MAX_LISTED)
        .collect();
    out.sort_by(|a, b| b.updated.cmp(&a.updated).then_with(|| b.id.cmp(&a.id)));
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn saves_lists_and_rejects_bad_ids() {
        let side = std::env::temp_dir().join(format!("bashcut-store-{}", std::process::id()));
        let mut c = Conversation::new("claude-code");
        c.transcript.push(Item::User { text: "Cắt đoạn mở đầu\nngắn lại".into() });
        save(&side, &c).unwrap();
        let back = load(&side, &c.id).unwrap();
        assert_eq!(back, c);
        assert_eq!(back.title(), "Cắt đoạn mở đầu");
        let l = list(&side);
        assert_eq!(l.len(), 1);
        assert_eq!(l[0].items, 1);
        assert!(load(&side, "../x").is_err());
        std::fs::write(dir(&side).join("bad.json"), "{").unwrap();
        assert_eq!(list(&side).len(), 1, "broken files are skipped");
        let _ = std::fs::remove_dir_all(&side);
    }
}
