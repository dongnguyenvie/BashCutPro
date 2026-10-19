//! BashCutPro: the curated look pack (#47): golden values, damage checks, the intensity knob, apply / remove.
//!
//! Golden values live in `looks_golden.json` beside this file. After an intended change to a look, re-bless them
//! with `BASHCUT_BLESS=1 cargo test -p filmcraft-engine bashcut::looks_tests` and review the diff.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde_json::{Value, json};

use super::looks::{self, Look, pack};
use crate::Session;
use crate::media_test_util::tmp_dir;

/// Largest difference (display sRGB, 0..1) a golden value may drift by.
const TOLERANCE: f32 = 0.01;

fn golden_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src/bashcut/looks_golden.json")
}

fn round(v: f32) -> f64 {
    (v as f64 * 10_000.0).round() / 10_000.0
}

/// A look's reference colours at its minimum, default and maximum intensity.
fn measured(l: &Look) -> Value {
    let at = |a: f64| json!(looks::graded_patches(l, a).iter().map(|c| c.map(round)).collect::<Vec<_>>());
    json!({"fingerprint": l.fingerprint(1.0), "min": at(l.min), "default": at(1.0), "max": at(l.max)})
}

#[test]
fn pack_is_curated() {
    let p = pack();
    assert!((20..=30).contains(&p.len()), "{} looks", p.len());
    let mut ids: Vec<&str> = p.iter().map(|l| l.id).collect();
    ids.sort_unstable();
    ids.dedup();
    assert_eq!(ids.len(), p.len(), "duplicate look ids");
    for l in p {
        assert!(!l.purpose.is_empty() && l.purpose.len() <= 100, "{}: one-line purpose", l.id);
        assert!(!l.tags.is_empty(), "{}: tags", l.id);
        assert!(l.min > 0.0 && l.min < 1.0 && l.max >= 1.0 && l.max <= looks::MAX_AMOUNT, "{}: safe range", l.id);
        let def = filmcraft_project::find_effect("lumetri").expect("lumetri");
        for (k, _) in &l.params {
            assert!(def.param(k).is_some(), "{}: unknown Lumetri parameter {k}", l.id);
        }
    }
}

#[test]
fn zero_intensity_changes_nothing_and_the_knob_scales() {
    let src = looks::patches();
    for l in pack() {
        let out = looks::graded_patches(l, 0.0);
        for (a, b) in src.iter().zip(&out) {
            for c in 0..3 {
                assert!((a[c] - b[c]).abs() < 0.004, "{} at 0 changed {a:?} to {b:?}", l.id);
            }
        }
        // further along the knob, further from the source
        let dist =
            |amount: f64| -> f32 { looks::graded_patches(l, amount).iter().zip(&src).map(|(o, s)| (0..3).map(|c| (o[c] - s[c]).abs()).sum::<f32>()).sum() };
        assert!(dist(l.min) <= dist(1.0) + 1e-3, "{}: min further than default", l.id);
        assert!(dist(1.0) <= dist(l.max) + 1e-3, "{}: default further than max", l.id);
    }
    let l = &pack()[0];
    assert_eq!(l.clamp(Some(99.0)), l.max);
    assert_eq!(l.clamp(Some(-1.0)), l.min);
    assert_eq!(l.clamp(Some(f64::NAN)), 1.0);
    assert_eq!(l.clamp(None), 1.0);
}

#[test]
fn no_look_damages_the_picture_at_its_default() {
    let mut bad = Vec::new();
    for l in pack() {
        for (name, amount) in [("default", 1.0), ("min", l.min)] {
            let d = looks::damage(l, amount);
            if !d.ok {
                bad.push(format!("{} at {name}: {d:?}", l.id));
            }
        }
    }
    assert!(bad.is_empty(), "damaged looks:\n{}", bad.join("\n"));
}

#[test]
fn golden_values_hold() {
    let now: BTreeMap<String, Value> = pack().iter().map(|l| (l.id.to_string(), measured(l))).collect();
    if std::env::var_os("BASHCUT_BLESS").is_some() {
        let text = serde_json::to_string_pretty(&now).unwrap();
        std::fs::write(golden_path(), text + "\n").unwrap();
        return;
    }
    let text = std::fs::read_to_string(golden_path())
        .unwrap_or_else(|_| panic!("no {}; bless it: BASHCUT_BLESS=1 cargo test -p filmcraft-engine bashcut::looks_tests", golden_path().display()));
    let golden: BTreeMap<String, Value> = serde_json::from_str(&text).unwrap();
    let mut drift = Vec::new();
    for (id, m) in &now {
        let Some(g) = golden.get(id) else {
            drift.push(format!("{id}: not in the golden file (bless after review)"));
            continue;
        };
        for level in ["min", "default", "max"] {
            let (a, b) = (m[level].as_array().unwrap(), g[level].as_array().unwrap());
            assert_eq!(a.len(), b.len(), "{id} {level}: patch count");
            for (i, (x, y)) in a.iter().zip(b).enumerate() {
                for c in 0..3 {
                    let (x, y) = (x[c].as_f64().unwrap() as f32, y[c].as_f64().unwrap() as f32);
                    if (x - y).abs() > TOLERANCE {
                        drift.push(format!("{id} {level} patch {i} channel {c}: {y} → {x}"));
                    }
                }
            }
        }
    }
    for id in golden.keys().filter(|k| !now.contains_key(*k)) {
        drift.push(format!("{id}: in the golden file but not in the pack"));
    }
    assert!(drift.is_empty(), "looks drifted from looks_golden.json:\n{}", drift.join("\n"));
}

fn demo() -> (Session, u64) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let q = s.execute("sequence.inspect", json!({})).unwrap();
    let clip =
        q["video"].as_array().unwrap().iter().flat_map(|t| t["items"].as_array().unwrap().iter()).next().expect("a video clip")["clip"].as_u64().unwrap();
    (s, clip)
}

fn lumetri_count(s: &mut Session, clip: u64) -> (usize, usize) {
    let q = s.execute("sequence.inspect", json!({})).unwrap();
    let it = q["video"].as_array().unwrap().iter().flat_map(|t| t["items"].as_array().unwrap().iter()).find(|i| i["clip"] == json!(clip)).unwrap().clone();
    let fx = it["effects"].as_array().unwrap();
    let lumetri = fx.iter().filter(|e| e["effect"] == json!("lumetri")).count();
    let ours = fx.iter().filter(|e| e["effect"] == json!("lumetri") && e["params"].get(looks::TAG).is_some()).count();
    (lumetri, ours)
}

#[test]
fn apply_replaces_and_remove_takes_it_off_in_one_undo_step_each() {
    let (mut s, clip) = demo();
    let (before, _) = lumetri_count(&mut s, clip);
    let r = s.execute("bashcut.looks.apply", json!({"look": "teal-orange", "clips": [clip], "intensity": 0.5})).unwrap();
    assert_eq!(r["clips"], json!(1));
    assert_eq!(r["intensity"], json!(0.5));
    assert_eq!(lumetri_count(&mut s, clip), (before + 1, 1));
    // a second look replaces the first, and the knob is clamped to the safe range
    let r = s.execute("bashcut.looks.apply", json!({"look": "Clean Mono", "clips": [clip], "intensity": 9})).unwrap();
    assert_eq!(r["look"], json!("mono-clean"));
    assert_eq!(r["intensity"], json!(1.0));
    assert_eq!(lumetri_count(&mut s, clip), (before + 1, 1));
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(lumetri_count(&mut s, clip), (before + 1, 1), "undo goes back to teal-orange, still one look");
    assert_eq!(s.execute("bashcut.looks.remove", json!({"clips": [clip]})).unwrap()["clips"], json!(1));
    assert_eq!(lumetri_count(&mut s, clip), (before, 0));
    assert_eq!(s.execute("bashcut.looks.remove", json!({"clips": [clip]})).unwrap()["clips"], json!(0), "nothing to remove");
    assert!(s.execute("bashcut.looks.apply", json!({"look": "no-such", "clips": [clip]})).is_err());
    assert!(s.execute("bashcut.looks.apply", json!({"look": "teal-orange", "clips": []})).is_err());
}

#[test]
fn list_filters_by_tag_and_query() {
    let mut s = Session::default();
    let all = s.execute("bashcut.looks.list", json!({})).unwrap();
    assert_eq!(all["looks"].as_array().unwrap().len(), pack().len());
    let th = s.execute("bashcut.looks.list", json!({"tags": "talking-head"})).unwrap();
    let ids: Vec<&str> = th["looks"].as_array().unwrap().iter().map(|l| l["id"].as_str().unwrap()).collect();
    assert!(ids.contains(&"clean-skin") && !ids.contains(&"travel-vivid"), "{ids:?}");
    let q = s.execute("bashcut.looks.list", json!({"query": "sunset"})).unwrap();
    assert!(q["looks"].as_array().unwrap().iter().any(|l| l["id"] == json!("golden-hour")));
    let g = s.execute("bashcut.looks.get", json!({"look": "golden-hour", "intensity": 0.5})).unwrap();
    assert!(g["params"]["temperature"].is_object() || g["params"]["temperature"].is_number(), "{g}");
    assert!(g["damage"]["ok"].is_boolean());
}

#[test]
fn thumbnails_and_luts_are_written() {
    let dir = tmp_dir("bashcut-looks");
    let mut s = Session::default();
    let d = dir.to_string_lossy().to_string();
    let r = s.execute("bashcut.looks.thumbnail", json!({"look": "teal-orange", "width": 64, "dir": d})).unwrap();
    let path = r["path"].as_str().unwrap().to_string();
    assert!(std::path::Path::new(&path).is_file());
    let again = s.execute("bashcut.looks.thumbnail", json!({"look": "teal-orange", "width": 64, "dir": d})).unwrap();
    assert_eq!(again["cached"], json!(true), "cached by fingerprint");
    assert!(s.execute("bashcut.looks.thumbnail", json!({"look": "teal-orange", "frame": true})).is_err(), "no sequence, no frame");

    let cube = dir.join("one.cube");
    s.execute("bashcut.looks.export", json!({"look": "golden-hour", "path": cube.to_string_lossy(), "size": 5})).unwrap();
    let text = std::fs::read_to_string(&cube).unwrap();
    assert!(text.contains("LUT_3D_SIZE 5"));
    assert_eq!(text.lines().filter(|l| l.split_whitespace().count() == 3 && l.chars().next().is_some_and(|c| c.is_ascii_digit())).count(), 125);
    let pack_dir = dir.join("pack");
    let r = s.execute("bashcut.looks.export", json!({"dir": pack_dir.to_string_lossy(), "size": 3})).unwrap();
    assert_eq!(r["looks"], json!(pack().len()));
    assert!(pack_dir.join("looks.json").is_file() && pack_dir.join("clean-skin.cube").is_file());
    let checked = s.execute("bashcut.looks.check", json!({"look": "clean-skin"})).unwrap();
    assert_eq!(checked["looks"].as_array().unwrap().len(), 1);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn the_tag_survives_a_round_trip() {
    let l = looks::find("golden-hour").unwrap();
    let e = looks::instance(l, 0.75);
    assert!(looks::is_look(&e));
    assert_eq!(looks::look_of(&e), Some(("golden-hour".to_string(), 0.75)));
    let back: filmcraft_project::EffectInstance = serde_json::from_value(serde_json::to_value(&e).unwrap()).unwrap();
    assert!(looks::is_look(&back));
}
