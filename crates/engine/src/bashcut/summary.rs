//! `bashcut.timeline.summary` (#50): the timeline in a few kilobytes, cheap enough for an agent to read every
//! turn. Tracks in order (V1 up, then A1 down), clips in time order with ids, start / end in seconds, and only
//! what differs from a plain clip: source in-point, speed, reverse, disabled, a label other than its media's,
//! non-intrinsic effects, the link to its partner. Caption tracks give a count, markers a count.
//!
//! [`changes`] compares two summaries; edit commands of our module return it as `changed`, next to
//! `createdId`, so an agent does not have to read the timeline again after an edit.

use filmcraft_project::{ItemId, Sequence, Track, TrackItem, TrackKind};
use filmcraft_time::Tick;
use serde_json::{Map, Value, json};

use crate::commands::{CommandSpec, always, bad};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.timeline.summary";

/// Seconds rounded to the millisecond.
fn s3(t: Tick) -> f64 {
    (t.seconds() * 1000.0).round() / 1000.0
}

fn clip_json(s: &Session, it: &TrackItem) -> Value {
    let mut o = Map::new();
    o.insert("id".into(), json!(it.id.0));
    o.insert("item".into(), json!(it.item.0));
    o.insert("name".into(), json!(it.name));
    o.insert("start".into(), json!(s3(it.start)));
    o.insert("end".into(), json!(s3(it.end())));
    if it.source_in != Tick::ZERO {
        o.insert("in".into(), json!(s3(it.source_in)));
    }
    if (it.speed - 1.0).abs() > 1e-9 {
        o.insert("speed".into(), json!((it.speed * 1000.0).round() / 1000.0));
    }
    if it.reverse {
        o.insert("reverse".into(), json!(true));
    }
    if !it.enabled {
        o.insert("disabled".into(), json!(true));
    }
    if s.project.item(it.item).is_some_and(|p| p.label != it.label) {
        o.insert("label".into(), json!(it.label.name()));
    }
    let effects: Vec<&str> = it.effects.iter().filter_map(|e| e.def()).filter(|d| !d.intrinsic).map(|d| d.name).collect();
    if !effects.is_empty() {
        o.insert("effects".into(), json!(effects));
    }
    if let Some(l) = it.link {
        o.insert("link".into(), json!(l));
    }
    if !it.markers.is_empty() {
        o.insert("markers".into(), json!(it.markers.len()));
    }
    Value::Object(o)
}

fn track_json(s: &Session, t: &Track, n: usize) -> Value {
    let (short, long) = match t.kind {
        TrackKind::Video => ("V", "Video"),
        TrackKind::Audio => ("A", "Audio"),
    };
    let mut o = Map::new();
    o.insert("id".into(), json!(t.id.0));
    o.insert("track".into(), json!(format!("{short}{n}")));
    if !t.name.is_empty() && t.name != format!("{short}{n}") && t.name != format!("{long} {n}") {
        o.insert("name".into(), json!(t.name));
    }
    for (flag, on) in [("locked", t.locked), ("muted", t.muted), ("solo", t.solo), ("off", !t.enabled)] {
        if on {
            o.insert(flag.into(), json!(true));
        }
    }
    let mut items: Vec<&TrackItem> = t.items.iter().collect();
    items.sort_by_key(|i| (i.start, i.id.0));
    if !items.is_empty() {
        o.insert("clips".into(), Value::Array(items.into_iter().map(|i| clip_json(s, i)).collect()));
    }
    Value::Object(o)
}

/// The summary of sequence `id` (it must exist).
pub(crate) fn summarize(s: &Session, id: ItemId, q: &Sequence) -> Value {
    let name = s.project.item(id).map(|i| i.name.as_str()).unwrap_or("");
    let tracks: Vec<Value> = q
        .video_tracks
        .iter()
        .enumerate()
        .map(|(i, t)| track_json(s, t, i + 1))
        .chain(q.audio_tracks.iter().enumerate().map(|(i, t)| track_json(s, t, i + 1)))
        .collect();
    let captions: Vec<Value> = q.caption_tracks.iter().map(|c| json!({"id": c.id.0, "name": c.name, "captions": c.captions.len()})).collect();
    let mut o = json!({
        "sequence": id.0,
        "name": name,
        "size": [q.settings.width, q.settings.height],
        "fps": (q.settings.frame_rate.sane().as_f64() * 1000.0).round() / 1000.0,
        "duration": s3(q.duration()),
        "tracks": tracks,
        "units": "seconds",
    });
    if !captions.is_empty() {
        o["captions"] = json!(captions);
    }
    if !q.markers.is_empty() {
        o["markers"] = json!(q.markers.len());
    }
    if s.state.active_sequence == Some(id) {
        o["playhead"] = json!(s3(s.playhead()));
        if !s.state.selection.is_empty() {
            o["selection"] = json!(s.state.selection.iter().map(|c| c.0).collect::<Vec<_>>());
        }
    }
    o
}

/// The summary of the active sequence, if there is one.
pub(crate) fn active(s: &Session) -> Option<Value> {
    let id = s.state.active_sequence?;
    let q = s.project.sequence(id)?;
    Some(summarize(s, id, q))
}

fn clips_of(v: &Value) -> Vec<(String, Value)> {
    let mut out = Vec::new();
    for t in v["tracks"].as_array().into_iter().flatten() {
        let track = t["track"].as_str().unwrap_or("").to_string();
        for c in t["clips"].as_array().into_iter().flatten() {
            let mut c = c.clone();
            c["track"] = json!(track);
            out.push((c["id"].to_string(), c));
        }
    }
    out
}

/// What changed between two summaries: clips `added` and `changed` (as their new entries, with their
/// track), clip ids `removed`, and the new marker / caption counts and duration when they moved. Empty
/// object when nothing changed.
pub(crate) fn changes(before: &Value, after: &Value) -> Value {
    let (b, a) = (clips_of(before), clips_of(after));
    let mut added = Vec::new();
    let mut changed = Vec::new();
    for (id, c) in &a {
        match b.iter().find(|(k, _)| k == id) {
            None => added.push(c.clone()),
            Some((_, old)) if old != c => changed.push(c.clone()),
            Some(_) => {}
        }
    }
    let removed: Vec<Value> = b.iter().filter(|(id, _)| !a.iter().any(|(k, _)| k == id)).map(|(_, c)| c["id"].clone()).collect();
    let mut o = Map::new();
    if !added.is_empty() {
        o.insert("added".into(), Value::Array(added));
    }
    if !changed.is_empty() {
        o.insert("changed".into(), Value::Array(changed));
    }
    if !removed.is_empty() {
        o.insert("removed".into(), Value::Array(removed));
    }
    for k in ["markers", "captions", "duration", "sequence"] {
        if before.get(k) != after.get(k) {
            o.insert(k.into(), after.get(k).cloned().unwrap_or(json!(0)));
        }
    }
    Value::Object(o)
}

fn summary_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let id = match p.get("sequence").filter(|v| !v.is_null()) {
        Some(v) => ItemId(v.as_u64().ok_or_else(|| bad(CMD, "`sequence` must be a sequence id"))?),
        None => s.state.active_sequence.ok_or(EngineError::NoSequence)?,
    };
    let q = s.project.sequence(id).ok_or_else(|| bad(CMD, format!("no sequence {} in the project", id.0)))?;
    Ok(summarize(s, id, q))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Timeline Summary",
        menu: &[],
        shortcut: None,
        params: r#"{"sequence":id? (default the active sequence)} (tracks and clips with ids; start / end / in / duration in seconds, only fields that differ from a plain clip)"#,
        enabled: always,
        run: summary_cmd,
        journal: false,
    }]
}
