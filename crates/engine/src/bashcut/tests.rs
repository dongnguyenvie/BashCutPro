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
