//! BashCutPro: word edges moved out of the pauses Whisper hides (#203).

use std::sync::Arc;

use filmcraft_project::Word;
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::word_edges::{pauses, refine};
use crate::Session;

const SR: u32 = 48_000;

fn sec(x: f64) -> Tick {
    Tick::from_seconds_f64(x)
}

fn word(text: &str, a: f64, b: f64) -> Word {
    Word::new(text, sec(a), sec(b))
}

#[test]
fn a_pause_inside_a_word_moves_the_side_without_voice() {
    // the issue's words: "Now" starts where "panel" ended, over a pause from 2.0 to 5.2 s
    let words = [word("panel", 1.40, 1.90), word("Now", 1.90, 5.34), word("trailing", 6.0, 9.0), word("all", 10.0, 10.5)];
    let found = [(sec(2.0), sec(5.2)), (sec(6.5), sec(8.9)), (sec(9.5), sec(11.0))];
    let (out, moved) = refine(&words, &found);
    assert_eq!((out[0].start, out[0].end), (sec(1.40), sec(1.90)), "no pause in it");
    assert_eq!((out[1].start, out[1].end), (sec(5.2), sec(5.34)), "starts after the pause");
    assert_eq!((out[2].start, out[2].end), (sec(6.0), sec(6.5)), "more voice before: ends at the pause");
    assert_eq!((out[3].start, out[3].end), (sec(10.0), sec(10.5)), "a pause over the whole word leaves it");
    assert_eq!(moved.len(), 2);
}

#[test]
fn pauses_need_a_clear_split_and_a_minimum_length() {
    // speech at -20 dB, noise at -60 dB: a 0.4 s pause counts, a 0.2 s one does not
    let mut levels = vec![-20.0f32; 300];
    for l in levels.iter_mut().skip(100).take(40) {
        *l = -60.0;
    }
    for l in levels.iter_mut().skip(200).take(20) {
        *l = -60.0;
    }
    assert_eq!(pauses(&levels, 30), vec![(100, 140)]);
    // music-like: everything within a few dB, no pauses found
    let flat: Vec<f32> = (0..300).map(|i| -20.0 - (i % 7) as f32).collect();
    assert!(pauses(&flat, 30).is_empty());
    assert!(pauses(&[], 30).is_empty());
}

fn import_wav(s: &mut Session, name: &str, mono: &[f32]) -> u64 {
    let inter: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();
    let bytes: Arc<[u8]> = crate::previews::write_wav_f32(&inter, SR).into();
    crate::commands::import_bytes(s, &format!("/{name}.wav"), bytes, None).unwrap().0
}

/// 15 s of "speech" (a warbling tone at -20 dBFS) with digital silence at 2.0–5.2 and 8.8–12.9 s.
fn narration() -> Vec<f32> {
    let n = (SR as f64 * 15.0) as usize;
    (0..n)
        .map(|i| {
            let t = i as f64 / SR as f64;
            if (2.0..5.2).contains(&t) || (8.8..12.9).contains(&t) {
                0.0
            } else {
                (0.1 * (2.0 * std::f64::consts::PI * (220.0 + 40.0 * (7.0 * t).sin()) * t).sin()) as f32
            }
        })
        .collect()
}

/// The narration with a Whisper-like transcript (each word after a pause starts where the last one ended), on A1
/// from its start when `place`.
fn session_with(place: bool) -> (Session, u64) {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "talk", "video": 1, "audio": 1, "width": 320, "height": 180})).unwrap();
    let item = import_wav(&mut s, "talk", &narration());
    let t = |x: f64| sec(x).0;
    let words: Vec<Value> = [
        ("Open", 0.2, 0.9),
        ("the", 0.9, 1.4),
        ("panel", 1.4, 1.9),
        ("Now", 1.9, 5.34),
        ("type", 5.4, 7.9),
        ("6000", 7.9, 8.6),
        ("Next,", 8.6, 13.55),
        ("done.", 13.6, 14.5),
    ]
    .iter()
    .map(|(w, a, b)| json!({"text": w, "start": t(*a), "end": t(*b)}))
    .collect();
    s.execute("transcript.set", json!({"item": item, "transcript": {"language": "en", "words": words}})).unwrap();
    if place {
        s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "video": false})).unwrap();
    }
    (s, item)
}

fn session() -> (Session, u64) {
    session_with(true)
}

fn starts(s: &Session, item: u64) -> Vec<(String, f64, f64)> {
    let t = &s.project.transcripts[&filmcraft_project::ItemId(item)];
    t.words.iter().map(|w| (w.text.clone(), (w.start.seconds() * 100.0).round() / 100.0, (w.end.seconds() * 100.0).round() / 100.0)).collect()
}

#[test]
fn refined_words_show_the_pauses_to_remove_pauses_and_the_review() {
    let (mut s, item) = session();
    // before: no gap between words, so nothing to remove
    let r = s.execute("transcript.removePauses", json!({"minSeconds": 0.8, "keepSeconds": 0.3})).unwrap();
    assert_eq!(r["removed"], 0, "{r}");

    let undo = s.history.undo.len();
    let r = s.execute("bashcut.transcript.refineEdges", json!({"items": [item]})).unwrap();
    assert_eq!(r["items"][0]["pauses"], 2, "{r}");
    assert_eq!(r["items"][0]["moved"], 2, "{r}");
    assert_eq!(s.history.undo.len(), undo + 1, "one undo step");
    let w = starts(&s, item);
    assert!((w[3].1 - 5.2).abs() < 0.02 && (w[3].2 - 5.34).abs() < 0.01, "\"Now\" starts after the pause: {w:?}");
    assert!((w[6].1 - 12.9).abs() < 0.02, "\"Next,\" starts after the pause: {w:?}");
    assert_eq!(w[2], ("panel".into(), 1.4, 1.9), "{w:?}");

    // a cut in the silence is on a word edge now
    let v = s.execute("bashcut.review.run", json!({"checks": ["edit"], "writeMarkers": false})).unwrap();
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["check"] == "cutInWord"), "{v}");
    // and the pauses are there to remove
    let r = s.execute("transcript.removePauses", json!({"minSeconds": 0.8, "keepSeconds": 0.3})).unwrap();
    assert_eq!(r["removed"], 2, "{r}");
}

#[test]
fn a_cut_in_the_hidden_pause_is_no_longer_inside_a_word() {
    // the issue's step 2: the clip plays the source from 4.0 to 8.85 s, both cuts in pauses that Whisper gave to "Now"
    // and "Next,"
    let (mut s, item) = session_with(false);
    s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "sourceIn": sec(4.0).0, "duration": sec(4.85).0, "video": false}))
        .unwrap();
    let cut = |s: &mut Session| {
        let v = s.execute("bashcut.review.run", json!({"checks": ["edit"], "writeMarkers": false})).unwrap();
        v["findings"].as_array().unwrap().iter().filter(|f| f["check"] == "cutInWord").count()
    };
    assert!(cut(&mut s) >= 1, "Whisper's times put the cut inside \"Now\"");
    s.execute("bashcut.transcript.refineEdges", json!({"items": [item]})).unwrap();
    assert_eq!(cut(&mut s), 0);
}

#[test]
fn hostile_params_are_refused() {
    let (mut s, item) = session();
    for v in [0.0, -1.0, 99.0] {
        let r = s.execute("bashcut.transcript.refineEdges", json!({"items": [item], "minSilenceSeconds": v}));
        assert!(r.is_err(), "{v}");
    }
    let r = s.execute("bashcut.transcript.refineEdges", json!({"items": [999_999]})).unwrap();
    assert_eq!(r["items"], json!([]));
    let dry = s.execute("bashcut.transcript.refineEdges", json!({"items": [item], "dryRun": true})).unwrap();
    assert_eq!(dry["items"][0]["moved"], 2);
    assert!((starts(&s, item)[3].1 - 1.9).abs() < 0.01, "a dry run changes nothing");
}
