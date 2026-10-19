//! BashCutPro: project templates with slots (#42).

use filmcraft_media::DemoScene;
use filmcraft_project::{ItemId, MediaRef};
use serde_json::{Value, json};

use crate::Session;
use crate::media_test_util::{make_movie, session_with, tmp_dir};

/// (start, duration, source_in, effects as JSON) of every clip, track by track.
fn clip_layout(s: &Session) -> Vec<(i64, i64, i64, Value)> {
    let seq = s.state.active_sequence.and_then(|id| s.project.sequence(id)).unwrap();
    seq.video_tracks
        .iter()
        .chain(seq.audio_tracks.iter())
        .flat_map(|t| t.items.iter())
        .map(|c| (c.start.0, c.duration.0, c.source_in.0, serde_json::to_value(&c.effects).unwrap()))
        .collect()
}

fn media_path(s: &Session, id: ItemId) -> String {
    match &s.project.item(id).unwrap().as_media().unwrap().media {
        MediaRef::File { path } => path.clone(),
        other => panic!("not a file: {other:?}"),
    }
}

#[test]
fn template_with_two_slots_keeps_timing_and_effects() {
    let dir = tmp_dir("bashcut-template");
    let (a, b) = (dir.join("a.mov"), dir.join("b.mov"));
    make_movie(&a, DemoScene::Aurora, 64, 36, 12);
    make_movie(&b, DemoScene::Dunes, 64, 36, 12);
    let (mut s, items, _) = session_with(&[a.as_path(), b.as_path()]);
    let first = s.project.sequence(s.state.active_sequence.unwrap()).unwrap().video_tracks[0].items[0].id;
    s.execute("effects.apply", json!({"clips": [first.0], "effect": "Invert"})).unwrap();
    let before = clip_layout(&s);

    let bundle = dir.join("bundle");
    let r = s
        .execute(
            "bashcut.template.save",
            json!({"path": bundle.to_string_lossy(), "slots": [items[0].0, {"item": items[1].0, "name": "B-roll"}], "poster": false}),
        )
        .unwrap();
    assert_eq!(r["template"]["slots"].as_array().unwrap().len(), 2, "{r}");
    assert!(bundle.join("template.json").is_file());
    assert!(bundle.join("project.fcproj").is_file());
    assert!(bundle.join("assets/slot1.mov").is_file());

    let (c, d) = (dir.join("c.mov"), dir.join("d.mov"));
    make_movie(&c, DemoScene::Forest, 64, 36, 12);
    make_movie(&d, DemoScene::Plasma, 64, 36, 12);
    let out = dir.join("out.fcproj");
    let r = s
        .execute(
            "bashcut.template.instantiate",
            json!({
                "template": bundle.to_string_lossy(),
                "fills": [{"slot": "slot1", "file": c.to_string_lossy()}, {"slot": "B-roll", "file": d.to_string_lossy()}],
                "saveAs": out.to_string_lossy(),
            }),
        )
        .unwrap();
    assert_eq!(r["slots"].as_array().unwrap().len(), 2, "{r}");
    assert_eq!(clip_layout(&s), before, "clips keep timing and effects");
    assert_eq!(media_path(&s, items[0]), c.to_string_lossy());
    assert_eq!(media_path(&s, items[1]), d.to_string_lossy());
    assert_eq!(s.history.undo.last().map(|u| u.0.as_str()), Some("Instantiate Template"));
    assert_eq!(s.history.undo.iter().filter(|u| u.0 == "Instantiate Template").count(), 1, "one undo step");
    assert!(out.is_file());
    assert_eq!(s.path.as_deref(), Some(out.to_string_lossy().as_ref()));

    // a missing fill keeps the template's media (its copy in assets/)
    let r = s
        .execute("bashcut.template.instantiate", json!({"template": bundle.to_string_lossy(), "fills": [{"slot": "slot1", "file": c.to_string_lossy()}]}))
        .unwrap();
    assert_eq!(r["slots"][1]["from"], "template", "{r}");
    assert_eq!(media_path(&s, items[0]), c.to_string_lossy());
    assert!(media_path(&s, items[1]).ends_with("slot2.mov"));
    assert!(s.path.is_none(), "never saves over the template's project");

    let listed = s.execute("bashcut.template.list", json!({"dir": dir.to_string_lossy()})).unwrap();
    assert_eq!(listed["templates"][0]["slots"].as_array().unwrap().len(), 2, "{listed}");
}

#[test]
fn template_commands_refuse_hostile_params() {
    let dir = tmp_dir("bashcut-template-hostile");
    let a = dir.join("a.mov");
    make_movie(&a, DemoScene::Aurora, 32, 18, 4);
    let (mut s, items, _) = session_with(&[a.as_path()]);
    let bundle = dir.join("t").to_string_lossy().into_owned();
    for p in [
        json!({}),
        json!({"path": bundle}),
        json!({"path": bundle, "slots": []}),
        json!({"path": bundle, "slots": "x"}),
        json!({"path": bundle, "slots": [u64::MAX]}),
        json!({"path": bundle, "slots": [-1]}),
        json!({"path": bundle, "slots": [items[0].0, items[0].0]}),
        json!({"path": bundle, "slots": vec![items[0].0; 1000]}),
    ] {
        assert!(s.execute("bashcut.template.save", p.clone()).is_err(), "{p}");
    }
    s.execute("bashcut.template.save", json!({"path": bundle, "slots": [items[0].0], "poster": false})).unwrap();
    assert!(s.execute("bashcut.template.save", json!({"path": bundle, "slots": [items[0].0]})).is_err(), "no silent overwrite");
    for p in [
        json!({}),
        json!({"template": dir.join("nope").to_string_lossy()}),
        json!({"template": "../../etc", "dir": dir.to_string_lossy()}),
        json!({"template": bundle, "fills": "x"}),
        json!({"template": bundle, "fills": [{"slot": "slot9", "file": a.to_string_lossy()}]}),
        json!({"template": bundle, "fills": [{"slot": "slot1"}]}),
        json!({"template": bundle, "fills": [{"slot": "slot1", "file": dir.join("missing.mov").to_string_lossy()}]}),
        json!({"template": bundle, "fills": [{"slot": "slot1", "file": a.to_string_lossy()}, {"slot": "slot1", "file": a.to_string_lossy()}]}),
    ] {
        assert!(s.execute("bashcut.template.instantiate", p.clone()).is_err(), "{p}");
    }
    // a broken manifest is an error, not a crash
    std::fs::write(dir.join("t").join("template.json"), b"{\"schema\":\"bashcut-template/1\",\"slots\":[{\"id\":7}]}").unwrap();
    let _ = s.execute("bashcut.template.instantiate", json!({"template": bundle}));
    std::fs::write(dir.join("t").join("template.json"), b"not json").unwrap();
    assert!(s.execute("bashcut.template.instantiate", json!({"template": bundle})).is_err());
    assert!(s.execute("bashcut.template.list", json!({"dir": dir.join("none").to_string_lossy()})).is_ok());
}
