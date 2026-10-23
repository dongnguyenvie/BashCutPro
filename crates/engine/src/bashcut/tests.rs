//! Tests of the `bashcut.*` engine commands.

use std::sync::Arc;

use filmcraft_project::ClipId;
use serde_json::{Value, json};

use crate::Session;

const SR: u32 = 48_000;

/// A session with a 320×180 sequence (2 video, 2 audio tracks).
fn session() -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "bashcut", "video": 2, "audio": 2, "width": 320, "height": 180})).unwrap();
    s
}

/// Import mono audio as a stereo wav and place it on A1 at `at` seconds; returns the audio clip.
fn place_audio(s: &mut Session, name: &str, mono: &[f32], at: f64) -> ClipId {
    let inter: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();
    let bytes: Arc<[u8]> = crate::previews::write_wav_f32(&inter, SR).into();
    let item = crate::commands::import_bytes(s, &format!("/{name}.wav"), bytes, None).unwrap();
    let r = s.execute("timeline.place", json!({"item": item.0, "audioTrack": "A1", "seconds": at})).unwrap();
    ClipId(r["clips"][0].as_u64().unwrap())
}

fn sine(amp: f32, freq: f64, seconds: f64) -> Vec<f32> {
    let n = (SR as f64 * seconds) as usize;
    (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / SR as f64).sin() as f32).collect()
}

#[test]
fn bashcut_commands_are_listed_with_units() {
    let ids: Vec<&str> = crate::commands::command_specs().iter().map(|c| c.id).filter(|id| id.starts_with("bashcut.")).collect();
    for id in ["bashcut.audio.loudness", "bashcut.audio.beats", "bashcut.review.pictureChecks"] {
        assert!(ids.contains(&id), "{id} missing from {ids:?}");
        let spec = crate::commands::find(id).unwrap();
        assert!(spec.params.contains("seconds"), "{id} params state no units: {}", spec.params);
    }
}

#[test]
fn loudness_of_a_reference_tone_clip() {
    let mut s = session();
    let amp = 10f32.powf(-23.0 / 20.0);
    let c = place_audio(&mut s, "tone", &sine(amp, 1000.0, 10.0), 0.0);
    let v = s.execute("bashcut.audio.loudness", json!({"clip": c.0, "series": true})).unwrap();
    assert_eq!(v["scope"], "clip");
    let i = v["integratedLufs"].as_f64().unwrap();
    // the wav is dual mono, measured as one channel like essentialSound.autoMatch: a −23 dBFS sine
    // reads about −26 LUFS mono
    assert!(i < -20.0 && i > -30.0, "integrated {i}");
    assert_eq!(v["shortTerm"].as_array().unwrap().len(), 10);
    assert_eq!(v["clippedSamples"], 0);
    // the mix and A1 are measurable too; a range inside the clip
    let mix = s.execute("bashcut.audio.loudness", json!({})).unwrap();
    assert_eq!(mix["scope"], "mix");
    assert!(mix["integratedLufs"].is_f64());
    let tr = s.execute("bashcut.audio.loudness", json!({"track": "A1", "range": {"start": 2.0, "end": 6.0}})).unwrap();
    assert_eq!(tr["scope"], "track");
    assert_eq!((tr["start"].as_f64(), tr["end"].as_f64()), (Some(2.0), Some(6.0)));
    // read-only: no undo step
    assert!(!s.history.can_undo() || s.history.undo.iter().all(|u| !u.0.contains("Loudness")));
}

#[test]
fn beats_of_a_click_track_and_markers_in_one_undo_step() {
    let mut s = session();
    let (x, _) = bashcut_analysis::beats::click_track(SR, 120.0, 12.0);
    let c = place_audio(&mut s, "click", &x, 1.0);
    let v = s.execute("bashcut.audio.beats", json!({"clip": c.0})).unwrap();
    assert_eq!(v["found"], true);
    let bpm = v["bpm"].as_f64().unwrap();
    assert!((bpm - 120.0).abs() < 1.5, "bpm {bpm}");
    let beats = v["beats"].as_array().unwrap();
    assert!(beats.len() >= 20, "{} beats", beats.len());
    // in sequence seconds: the clip starts at 1 s
    assert!(beats[0].as_f64().unwrap() >= 0.99);
    assert_eq!(v["markers"], 0);
    let markers = |s: &Session| s.active_sequence().unwrap().markers.len();
    assert_eq!(markers(&s), 0);

    let undo_before = s.history.undo.len();
    let v = s.execute("bashcut.audio.beats", json!({"clip": c.0, "writeMarkers": true})).unwrap();
    let n = v["markers"].as_u64().unwrap() as usize;
    assert_eq!(n, v["beats"].as_array().unwrap().len());
    assert_eq!(markers(&s), n);
    assert_eq!(s.history.undo.len(), undo_before + 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(markers(&s), 0);
}

#[test]
fn no_beats_in_silence() {
    let mut s = session();
    let c = place_audio(&mut s, "silence", &vec![0.0; SR as usize * 4], 0.0);
    let v = s.execute("bashcut.audio.beats", json!({"clip": c.0, "writeMarkers": true})).unwrap();
    assert_eq!(v["found"], false);
    assert_eq!(s.active_sequence().unwrap().markers.len(), 0);
}

fn place_generator(s: &mut Session, id: &str, params: Value, at: f64) {
    let item = s.execute(id, params).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": item, "track": "V1", "seconds": at, "audio": false})).unwrap();
}

#[test]
fn picture_checks_find_a_black_stretch() {
    let mut s = session();
    place_generator(&mut s, "file.newColorMatte", json!({"color": "#a0a0a0", "seconds": 3.0}), 0.0);
    place_generator(&mut s, "file.newBlackVideo", json!({"seconds": 2.0}), 3.0);
    place_generator(&mut s, "file.newColorMatte", json!({"color": "#6080c0", "seconds": 3.0}), 5.0);
    let v = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 4})).unwrap();
    let black: Vec<&Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "black").collect();
    assert_eq!(black.len(), 1, "{v}");
    let (a, b) = (black[0]["start"].as_f64().unwrap(), black[0]["end"].as_f64().unwrap());
    assert!((a - 3.0).abs() <= 0.26 && (b - 5.0).abs() <= 0.26, "black {a}..{b}");
    assert_eq!(v["counts"]["black"], 1);
    assert_eq!(v["unrendered"], 0);
    // a range that leaves the black out finds none
    let v = s.execute("bashcut.review.pictureChecks", json!({"range": {"start": 5.0}})).unwrap();
    assert_eq!(v["counts"]["black"], 0, "{v}");
}

#[test]
fn hostile_params_are_errors_not_crashes() {
    let mut empty = Session::default();
    for id in ["bashcut.audio.loudness", "bashcut.audio.beats", "bashcut.review.pictureChecks"] {
        assert!(empty.execute(id, json!({})).is_err(), "{id} without a sequence");
    }
    let mut s = session();
    let c = place_audio(&mut s, "tone", &sine(0.1, 440.0, 2.0), 0.0);
    let bad = [
        ("bashcut.audio.loudness", json!({"clip": "x"})),
        ("bashcut.audio.loudness", json!({"clip": 999_999})),
        ("bashcut.audio.loudness", json!({"clip": -1})),
        ("bashcut.audio.loudness", json!({"track": "A99"})),
        ("bashcut.audio.loudness", json!({"track": "V1"})),
        ("bashcut.audio.loudness", json!({"range": 5})),
        ("bashcut.audio.loudness", json!({"range": {"start": -1.0}})),
        ("bashcut.audio.loudness", json!({"range": {"start": 1e300}})),
        ("bashcut.audio.loudness", json!({"range": {"start": 1.5, "end": 1.0}})),
        ("bashcut.audio.loudness", json!({"clip": c.0, "range": {"start": 50.0}})),
        ("bashcut.audio.beats", json!({"clip": "x"})),
        ("bashcut.audio.beats", json!({"clip": 999_999})),
        ("bashcut.audio.beats", json!({"clip": c.0, "writeMarkers": "yes"})),
        ("bashcut.review.pictureChecks", json!({"samplesPerSecond": 0})),
        ("bashcut.review.pictureChecks", json!({"samplesPerSecond": 1e9})),
        ("bashcut.review.pictureChecks", json!({"samplesPerSecond": "fast"})),
        ("bashcut.review.pictureChecks", json!({"range": {"end": "soon"}})),
    ];
    for (id, p) in bad {
        assert!(s.execute(id, p.clone()).is_err(), "{id} {p} should be refused");
    }
    // a huge range is clipped to what exists
    let v = s.execute("bashcut.audio.loudness", json!({"range": {"start": 0.0, "end": 9e6}})).unwrap();
    assert!(v["end"].as_f64().unwrap() <= 2.01);
}

#[test]
fn bashcut_commands_doc_matches_the_engine() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../bashcut/bashcut-commands.md");
    let doc = std::fs::read_to_string(&path).unwrap();
    // table rows: | `bashcut.x.y` | Label | Params |
    let listed: Vec<&str> = doc.lines().filter_map(|l| l.strip_prefix("| `")?.split('`').next()).filter(|id| id.starts_with("bashcut.")).collect();
    let engine: Vec<&str> = super::commands().iter().map(|c| c.id).collect();
    assert_eq!(listed, engine, "bashcut/bashcut-commands.md is out of date: regenerate it (see its header)");
    assert!(doc.contains(&format!("Our own engine commands ({})", engine.len())), "the count in the doc's first line");
}

/// The results whose numbers `data::round_to` / `data::secs` round (summary, beats, loudness, picture checks,
/// review, readable captions) on fixed inputs: a click track and a black stretch, and the demo project with a
/// transcript.
fn golden_outputs() -> Value {
    let mut s = session();
    let (x, _) = bashcut_analysis::beats::click_track(SR, 120.0, 12.0);
    let c = place_audio(&mut s, "click", &x, 1.0);
    place_generator(&mut s, "file.newColorMatte", json!({"color": "#a0a0a0", "seconds": 3.0}), 0.0);
    place_generator(&mut s, "file.newBlackVideo", json!({"seconds": 2.0}), 3.0);
    place_generator(&mut s, "file.newColorMatte", json!({"color": "#6080c0", "seconds": 3.0}), 5.0);
    let beats = s.execute("bashcut.audio.beats", json!({"clip": c.0})).unwrap();
    let loudness = s.execute("bashcut.audio.loudness", json!({"clip": c.0, "series": true})).unwrap();
    let picture = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 4})).unwrap();
    let summary = s.execute("bashcut.timeline.summary", json!({})).unwrap();

    let mut d = Session::default();
    d.execute("file.openDemoProject", json!({})).unwrap();
    let q = d.active_sequence().unwrap();
    let a = &q.audio_tracks[0].items[0];
    let (item, sin) = (a.item, a.source_in);
    let words = [
        ("So,", 0.0, 0.37),
        ("here", 0.54, 0.66),
        ("is", 0.66, 0.8),
        ("the", 0.8, 0.9),
        ("thing", 0.9, 1.04),
        ("podcasts.", 1.04, 2.35),
        ("Um,", 2.52, 2.78),
        ("most", 2.93, 3.12),
        ("people", 3.12, 3.38),
        ("cut", 3.38, 3.58),
        ("it.", 3.58, 3.7),
    ];
    let ws: Vec<Value> = words
        .iter()
        .map(|(t, a, b)| json!({"text": t, "start": (sin + filmcraft_time::Tick::from_seconds_f64(*a)).0, "end": (sin + filmcraft_time::Tick::from_seconds_f64(*b)).0}))
        .collect();
    d.execute("transcript.set", json!({"item": item.0, "transcript": {"language": "en", "words": ws}})).unwrap();
    let captions = d.execute("bashcut.captions.create", json!({"maxChars": 20, "maxCps": 4, "dryRun": true})).unwrap();
    let review = d.execute("bashcut.review.run", json!({"writeMarkers": false})).unwrap();
    let demo_summary = d.execute("bashcut.timeline.summary", json!({})).unwrap();
    json!({"beats": beats, "loudness": loudness, "picture": picture, "summary": summary, "captions": captions, "review": review, "demoSummary": demo_summary})
}

/// Every number in `v` with its JSON path, in key order.
fn numbers(v: &Value, path: String, out: &mut Vec<(String, f64)>) {
    match v {
        Value::Number(n) => out.push((path, n.as_f64().unwrap())),
        Value::Array(a) => a.iter().enumerate().for_each(|(i, x)| numbers(x, format!("{path}[{i}]"), out)),
        Value::Object(o) => o.iter().for_each(|(k, x)| numbers(x, format!("{path}.{k}"), out)),
        _ => {}
    }
}

/// The one rounding rule (`data::round_to`, #275) changed no reported number: `golden_numbers.json` holds
/// [`golden_outputs`] as the engine gave it at 272dcc32, the commit before #275. Only numbers are compared, so
/// reworded messages do not break it; an intended change to a number re-records the file.
#[test]
fn reported_numbers_match_the_golden_numbers_from_before_the_rounding_rule() {
    let golden: Value = serde_json::from_str(include_str!("golden_numbers.json")).unwrap();
    let (mut want, mut got) = (Vec::new(), Vec::new());
    numbers(&golden, String::new(), &mut want);
    numbers(&golden_outputs(), String::new(), &mut got);
    assert!(want.len() > 200, "{} golden numbers", want.len());
    let paths = |v: &[(String, f64)]| v.iter().map(|(p, _)| p.clone()).collect::<Vec<_>>();
    assert_eq!(paths(&got), paths(&want), "the same numbers are reported");
    // parsing the file may be one ulp off; a rounding change moves a number by far more
    let drift: Vec<String> =
        want.iter().zip(&got).filter(|((_, w), (_, g))| (w - g).abs() > 1e-9 * w.abs().max(1.0)).map(|((p, w), (_, g))| format!("{p}: {w} -> {g}")).collect();
    assert!(drift.is_empty(), "numbers drifted from golden_numbers.json:\n{}", drift.join("\n"));
}
