//! Tests of #50: `bashcut.timeline.summary`, `changed` / `createdId` from our edit commands, typed errors
//! and units on every `bashcut.*` command.

use serde_json::json;

use super::errors::{Code, code_of, tagged};
use crate::{EngineError, Session};

fn bashcut_ids() -> Vec<&'static str> {
    crate::commands::command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("bashcut.")).collect()
}

#[test]
fn summary_of_the_demo_project_is_small_and_complete() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let v = s.execute("bashcut.timeline.summary", json!({})).unwrap();
    let text = v.to_string();
    assert!(text.len() < 4096, "summary is {} bytes: {text}", text.len());
    let q = s.active_sequence().unwrap();
    let clips: usize = q.video_tracks.iter().chain(&q.audio_tracks).map(|t| t.items.len()).sum();
    let listed: usize = v["tracks"].as_array().unwrap().iter().map(|t| t["clips"].as_array().map_or(0, Vec::len)).sum();
    assert_eq!(listed, clips);
    assert_eq!(v["units"], "seconds");
    assert_eq!(v["tracks"][0]["track"], "V1");
    // clips in time order with seconds, and no default fields
    for t in v["tracks"].as_array().unwrap() {
        let starts: Vec<f64> = t["clips"].as_array().into_iter().flatten().map(|c| c["start"].as_f64().unwrap()).collect();
        assert!(starts.windows(2).all(|w| w[0] <= w[1]), "{starts:?}");
        for c in t["clips"].as_array().into_iter().flatten() {
            assert!(c["end"].as_f64().unwrap() > c["start"].as_f64().unwrap());
            assert!(c.get("speed").is_none_or(|v| v != 1.0));
            assert!(c.get("reverse").is_none_or(|v| v == true));
        }
    }
}

#[test]
fn summary_of_a_named_sequence_and_refusals() {
    let mut s = Session::default();
    let e = s.execute("bashcut.timeline.summary", json!({})).unwrap_err();
    assert_eq!(tagged(&e.to_string()), Some(Code::Refused), "{e}");
    s.execute("file.openDemoProject", json!({})).unwrap();
    let seq = s.state.active_sequence.unwrap().0;
    let v = s.execute("bashcut.timeline.summary", json!({"sequence": seq})).unwrap();
    assert_eq!(v["sequence"], seq);
    let e = s.execute("bashcut.timeline.summary", json!({"sequence": 987_654})).unwrap_err().to_string();
    assert_eq!(tagged(&e), Some(Code::NotFound), "{e}");
    let e = s.execute("bashcut.timeline.summary", json!({"sequence": "x"})).unwrap_err().to_string();
    assert_eq!(tagged(&e), Some(Code::BadParams), "{e}");
}

#[test]
fn batch_returns_what_changed_and_the_created_id() {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "c", "video": 2, "audio": 2, "width": 320, "height": 180, "fps": 30})).unwrap();
    let item = s.execute("file.newColorMatte", json!({"color": "#808080", "seconds": 2.0})).unwrap()["item"].as_u64().unwrap();
    let place = |at: f64| json!({"id": "timeline.place", "params": {"item": item, "track": "V1", "seconds": at, "audio": false}});
    let v = s.execute("bashcut.batch", json!({"commands": [place(0.0), place(3.0)]})).unwrap();
    let added = v["changed"]["added"].as_array().unwrap();
    assert_eq!(added.len(), 2, "{v}");
    assert_eq!(added[1]["start"], 3.0);
    assert_eq!(added[1]["track"], "V1");
    assert!(v["createdId"].is_u64());
    // a query-only batch changes nothing
    let v = s.execute("bashcut.batch", json!({"commands": [{"id": "sequence.inspect"}]})).unwrap();
    assert_eq!(v["changed"], json!({}));
}

#[test]
fn every_bashcut_command_has_params_units_and_typed_errors() {
    let ids = bashcut_ids();
    assert!(ids.len() >= 5, "{ids:?}");
    for id in &ids {
        let spec = crate::commands::find(id).unwrap();
        assert!(!spec.params.trim().is_empty(), "{id}: no params text");
        let p = spec.params.to_ascii_lowercase();
        assert!(["seconds", "ticks", "frames", "lufs", "dbtp", "pixels", "units"].iter().any(|w| p.contains(w)), "{id}: no units note in {}", spec.params);
    }
    // the error path: with no sequence or with hostile params, every failure starts with a code
    let mut empty = Session::default();
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let hostile =
        json!({"clip": "x", "clips": "x", "track": [], "sequence": "x", "range": 5, "commands": 5, "profile": 5, "template": 5, "path": 5, "amount": "x"});
    for id in &ids {
        for (sess, p) in [(&mut empty, json!({})), (&mut s, hostile.clone())] {
            if let Err(e) = sess.execute(id, p) {
                let text = e.to_string();
                // enabled() refusals come from upstream's execute before our code runs
                if matches!(e, EngineError::Disabled(..)) {
                    continue;
                }
                assert!(tagged(&text).is_some(), "{id}: untyped error `{text}`");
            }
        }
    }
}

#[test]
fn error_codes_classify_engine_errors() {
    let bad = crate::commands::bad("x", "need `item`");
    assert_eq!(code_of(&bad), Code::BadParams);
    assert_eq!(code_of(&crate::commands::bad("x", "no clip 5 in the active sequence")), Code::NotFound);
    assert_eq!(code_of(&EngineError::UnknownCommand("a.b".into())), Code::NotFound);
    assert_eq!(code_of(&EngineError::NoSequence), Code::Refused);
    assert_eq!(code_of(&EngineError::Other("an export is still running".into())), Code::Busy);
    assert_eq!(code_of(&EngineError::Other("offline: media is offline".into())), Code::Offline);
    assert_eq!(code_of(&super::errors::fail(Code::Busy, "x", "wait")), Code::Busy);
    let t = super::errors::typed(Err(bad)).unwrap_err().to_string();
    assert!(t.starts_with("badParams: invalid parameters for `x`"), "{t}");
    let again = super::errors::typed(Err(EngineError::Other(t.clone()))).unwrap_err().to_string();
    assert_eq!(again, t, "a typed error is not typed twice");
    assert_eq!(tagged("notFound: x"), Some(Code::NotFound));
    assert_eq!(tagged("notfound: x"), None);
    assert_eq!(tagged(""), None);
}

#[test]
#[ignore = "test ticket: every spec in bashcut/*.rs is built with `spec!` (no raw `CommandSpec {` literal outside mod.rs), so its errors are typed"]
fn every_bashcut_spec_uses_the_spec_macro() {}
