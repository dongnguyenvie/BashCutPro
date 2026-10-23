//! Tests of `bashcut.batch` (#44): one undo step, all or nothing.

use std::sync::Arc;
use std::time::Instant;

use serde_json::{Value, json};

use crate::Session;

/// A session with a sequence and a 2 s colour matte in the project; returns the matte's item id.
fn session() -> (Session, u64) {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "batch", "video": 2, "audio": 2, "width": 320, "height": 180})).unwrap();
    let item = s.execute("file.newColorMatte", json!({"color": "#808080", "seconds": 2.0})).unwrap()["item"].as_u64().unwrap();
    (s, item)
}

fn clips(s: &Session) -> usize {
    s.active_sequence().unwrap().video_tracks.iter().map(|t| t.items.len()).sum()
}

fn place(item: u64, at: f64) -> Value {
    json!({"id": "timeline.place", "params": {"item": item, "track": "V1", "seconds": at, "audio": false}})
}

#[test]
fn three_edits_are_one_undo_step() {
    let (mut s, item) = session();
    let undo = s.history.undo.len();
    let v = s.execute("bashcut.batch", json!({"commands": [place(item, 0.0), place(item, 4.0), place(item, 8.0)], "label": "Lay out"})).unwrap();
    assert_eq!(v["ok"], true);
    assert_eq!(v["count"], 3);
    assert_eq!(v["undoSteps"], 1);
    assert!(v["lastCreatedId"].is_u64(), "{v}");
    assert_eq!(clips(&s), 3);
    assert_eq!(s.history.undo.len(), undo + 1);
    assert_eq!(s.history.undo.last().map(|u| u.0.as_str()), Some("Lay out"));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(clips(&s), 0);
    s.execute("edit.redo", json!({})).unwrap();
    assert_eq!(clips(&s), 3);
}

#[test]
fn an_error_in_the_second_step_changes_nothing() {
    let (mut s, item) = session();
    s.execute("bashcut.batch", json!({"commands": [place(item, 0.0)]})).unwrap();
    let before = s.project.clone();
    let undo = s.history.undo.len();
    let redo = s.history.redo.len();
    let e = s.execute("bashcut.batch", json!({"commands": [place(item, 4.0), place(999_999, 6.0), place(item, 8.0)]})).unwrap_err().to_string();
    assert!(e.contains("step 2 of 3"), "{e}");
    assert!(e.contains("timeline.place"), "{e}");
    assert!(Arc::ptr_eq(&s.project, &before), "the project is the one from before the batch");
    assert_eq!(clips(&s), 1);
    assert_eq!((s.history.undo.len(), s.history.redo.len()), (undo, redo));
    // the edit before the batch is still undoable on its own
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(clips(&s), 0);
}

#[test]
fn not_atomic_keeps_what_ran_as_one_step() {
    let (mut s, item) = session();
    let undo = s.history.undo.len();
    let v = s.execute("bashcut.batch", json!({"commands": [place(item, 0.0), place(999_999, 6.0), place(item, 8.0)], "atomic": false})).unwrap();
    assert_eq!(v["ok"], false);
    assert_eq!(v["failed"], 1);
    assert_eq!(v["results"][1]["ok"], false);
    assert_eq!(clips(&s), 2);
    assert_eq!(s.history.undo.len(), undo + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(clips(&s), 0);
}

#[test]
fn a_thousand_commands_in_one_step_and_in_time() {
    let (mut s, _) = session();
    let undo = s.history.undo.len();
    let steps: Vec<Value> = (0..1000).map(|i| json!({"id": "markers.add", "params": {"seconds": i as f64 * 0.1, "name": format!("m{i}")}})).collect();
    let t = Instant::now();
    let v = s.execute("bashcut.batch", json!({"commands": steps})).unwrap();
    let took = t.elapsed();
    assert!(took.as_secs() < 60, "1,000 commands took {took:?}");
    assert_eq!(v["count"], 1000);
    assert_eq!(s.active_sequence().unwrap().markers.len(), 1000);
    // more steps than the history limit still fold into one entry
    assert_eq!(s.history.undo.len(), undo + 1);
    assert_eq!(s.history.limit, 200);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active_sequence().unwrap().markers.len(), 0);
}

#[test]
fn queries_only_leave_no_undo_step() {
    let (mut s, _) = session();
    let undo = s.history.undo.len();
    let v = s.execute("bashcut.batch", json!({"commands": [{"id": "sequence.inspect"}, {"id": "project.inspect"}]})).unwrap();
    assert_eq!(v["undoSteps"], 0);
    assert_eq!(s.history.undo.len(), undo);
}

#[test]
fn hostile_batches_are_refused_without_changes() {
    let (mut s, item) = session();
    let before = s.project.clone();
    let too_many: Vec<Value> = (0..super::batch::MAX_STEPS + 1).map(|_| json!({"id": "sequence.inspect"})).collect();
    let bad = [
        json!({}),
        json!({"commands": "timeline.place"}),
        json!({"commands": []}),
        json!({"commands": [{}]}),
        json!({"commands": [{"id": 5}]}),
        json!({"commands": [{"id": "no.such.command"}]}),
        json!({"commands": [{"id": "bashcut.batch", "params": {"commands": []}}]}),
        json!({"commands": [place(item, 0.0), {"id": "edit.undo"}]}),
        json!({"commands": [{"id": "history.list"}]}),
        json!({"commands": [{"id": "timeline.place", "params": [1, 2]}]}),
        json!({"commands": [place(item, 0.0)], "atomic": "yes"}),
        json!({"commands": too_many}),
    ];
    for p in bad {
        let short = p.to_string().chars().take(120).collect::<String>();
        assert!(s.execute("bashcut.batch", p).is_err(), "should refuse {short}");
        assert!(Arc::ptr_eq(&s.project, &before), "{short} changed the project");
    }
    // hostile params inside a step fail that step, and the batch restores
    let r = s.execute("bashcut.batch", json!({"commands": [place(item, 0.0), {"id": "timeline.place", "params": {"item": item, "seconds": -1e300}}]}));
    assert!(r.is_err());
    assert!(Arc::ptr_eq(&s.project, &before));
    // a label of any length is cut
    let v = s.execute("bashcut.batch", json!({"commands": [place(item, 0.0)], "label": "x".repeat(10_000)})).unwrap();
    assert_eq!(v["label"].as_str().unwrap().chars().count(), 80);
}

#[test]
#[ignore = "test ticket: a batch whose step opens a project (history reset) reports undoSteps 0 and keeps the new history; a failing atomic batch restores project, state, undo and redo"]
fn batch_undo_fold_after_history_reset() {}
