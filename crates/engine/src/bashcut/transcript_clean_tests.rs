//! Tests of #177: Whisper's repeated-text loops on footage with no speech are dropped.

use std::sync::Arc;

use serde_json::json;

use super::transcript_clean::{MIN_REPEATS, Reason, find_doubtful};
use crate::Session;
use filmcraft_project::{Transcript, Word};
use filmcraft_speech::FixedTranscriber;
use filmcraft_time::Tick;

/// Words of `text` laid out from `at` seconds, 0.3 s each, `gap` seconds after each sentence.
fn words(text: &str, at: f64, gap: f64) -> Vec<Word> {
    let mut t = at;
    let mut out = Vec::new();
    for w in text.split_whitespace() {
        out.push(Word::new(w, Tick::from_seconds_f64(t), Tick::from_seconds_f64(t + 0.3)));
        t += 0.3;
        if w.ends_with(['.', '?', '!']) {
            t += gap;
        }
    }
    out
}

/// What whisper-base wrote for 25 s of wind and animals in the bench (#177).
fn beach_loop() -> Vec<Word> {
    let text = "I'm going to have to go to the beach. ".repeat(46) + "I'm sorry.";
    words(&text, 0.0, 0.1)
}

/// A real monologue: varied sentences, a few short ones said more than once.
fn monologue() -> Vec<Word> {
    let mut text = String::new();
    for i in 0..30 {
        text.push_str(&format!("Today we look at part {i} of the build and why it matters. Okay. "));
        if i % 10 == 0 {
            text.push_str("Thank you. ");
        }
    }
    text.push_str("I said no, no, no to that plan and we moved on.");
    words(&text, 0.0, 0.4)
}

#[test]
fn a_loop_on_ambience_is_no_speech() {
    let w = beach_loop();
    assert_eq!(w.len(), 46 * 9 + 2);
    let v = find_doubtful(&w, MIN_REPEATS);
    assert!(v.no_speech);
    assert_eq!(v.dropped(), w.len());
    // the loop itself is named as such, the stray tail as no speech
    assert_eq!(v.drop[0], Some(Reason::Loop));
    assert_eq!(v.drop[w.len() - 1], Some(Reason::NoSpeech));
}

#[test]
fn a_sentence_once_per_window_is_dropped_without_punctuation_or_adjacency() {
    // Whisper writes the same sentence once per 30 s window, with real text in between
    let mut w = Vec::new();
    for k in 0..4 {
        w.extend(words("thanks for watching and see you next time", k as f64 * 30.0, 0.0));
        w.extend(words(&format!("we walked to the old market number {k} today and bought fresh bread."), k as f64 * 30.0 + 10.0, 0.0));
    }
    let v = find_doubtful(&w, MIN_REPEATS);
    // split at the 1.5 s pauses: each "thanks …" sentence counts 4 times
    let repeated = v.drop.iter().filter(|d| **d == Some(Reason::Repeated)).count();
    assert_eq!(repeated, 4 * 8, "{:?}", v.drop.iter().zip(&w).map(|(d, w)| (d, &w.text)).collect::<Vec<_>>());
    assert!(!v.no_speech, "half the words are real speech");
    assert!(v.drop.iter().zip(&w).filter(|(d, _)| d.is_none()).all(|(_, w)| !w.text.starts_with("thanks")));
}

#[test]
fn a_real_monologue_keeps_its_words() {
    let w = monologue();
    let v = find_doubtful(&w, MIN_REPEATS);
    assert_eq!(v.dropped(), 0, "{:?}", v.drop.iter().zip(&w).filter(|(d, _)| d.is_some()).map(|(_, w)| &w.text).collect::<Vec<_>>());
    assert!(!v.no_speech);
}

#[test]
fn odd_input_does_not_panic() {
    assert_eq!(find_doubtful(&[], 0).dropped(), 0);
    let blank = vec![Word::new("...", Tick::ZERO, Tick::ZERO); 50];
    let _ = find_doubtful(&blank, usize::MAX);
    let one = vec![Word::new("la", Tick::ZERO, Tick::ZERO); 5];
    assert_eq!(find_doubtful(&one, 2).dropped(), 0, "a word needs 6 repeats");
    let one = vec![Word::new("la", Tick::ZERO, Tick::ZERO); 6];
    assert!(find_doubtful(&one, 2).no_speech);
}

/// The demo project with a transcriber that returns `words` (fit to the first A1 clip's media).
fn session(words: Vec<Word>) -> (Session, u64) {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let a = &s.active_sequence().unwrap().audio_tracks[0].items[0];
    let (item, sin) = (a.item, a.source_in);
    let mut t = Transcript { language: "en".into(), ..Default::default() };
    // squeeze into 4 s of media so every word maps into the clip
    let n = words.len().max(1) as i64;
    let step = Tick::from_seconds_f64(4.0).0 / n;
    for (i, mut w) in words.into_iter().enumerate() {
        w.start = sin + Tick(step * i as i64);
        w.end = sin + Tick(step * (i as i64 + 1));
        t.words.push(w);
    }
    t.normalize();
    s.transcriber = Some(Arc::new(FixedTranscriber { transcript: t, id: "whisper-base".into() }));
    (s, item.0)
}

#[test]
fn generate_on_ambience_gives_no_words_and_a_warning() {
    let (mut s, item) = session(beach_loop());
    let r = s.execute("bashcut.transcript.generate", json!({"items": [item]})).unwrap();
    assert_eq!(r["items"][0]["words"], 0, "{r}");
    assert_eq!(r["items"][0]["noSpeech"], true);
    assert!(r["warning"].as_str().unwrap().contains("No speech found"), "{r}");
    let t = s.project.transcripts.values().next().unwrap();
    assert!(t.words.is_empty(), "the transcript stays, empty");
    // one undo step takes the whole transcribe back
    s.execute("edit.undo", json!({})).unwrap();
    assert!(s.project.transcripts.is_empty());
}

#[test]
fn generate_keeps_a_monologue_and_keep_doubtful_keeps_a_loop() {
    let (mut s, item) = session(monologue());
    let n = monologue().len();
    let r = s.execute("bashcut.transcript.generate", json!({"items": [item]})).unwrap();
    assert_eq!(r["items"][0]["words"], n);
    assert_eq!(r["items"][0]["dropped"], 0);
    assert!(r.get("warning").is_none());

    let (mut s, item) = session(beach_loop());
    let r = s.execute("bashcut.transcript.generate", json!({"items": [item], "keepDoubtful": true})).unwrap();
    assert_eq!(r["items"][0]["words"], beach_loop().len());
}

#[test]
fn clean_runs_on_existing_transcripts_and_skips_hand_made_ones() {
    let (mut s, item) = session(beach_loop());
    s.execute("transcript.generate", json!({"items": [item]})).unwrap();
    let dry = s.execute("bashcut.transcript.clean", json!({"dryRun": true})).unwrap();
    assert_eq!(dry["items"][0]["dropped"], beach_loop().len());
    assert!(dry["items"][0]["doubtful"].as_array().unwrap().len() <= 20);
    assert_eq!(s.project.transcripts.values().next().unwrap().words.len(), beach_loop().len(), "dry run changes nothing");
    s.execute("bashcut.transcript.clean", json!({})).unwrap();
    assert!(s.project.transcripts.values().next().unwrap().words.is_empty());

    // an imported transcript is left alone unless named
    let (mut s, item) = session(Vec::new());
    let loop_words: Vec<serde_json::Value> =
        beach_loop().iter().take(60).enumerate().map(|(i, w)| json!({"text": w.text, "start": i as i64 * 1000, "end": i as i64 * 1000 + 900})).collect();
    s.execute("transcript.set", json!({"item": item, "transcript": {"language": "en", "words": loop_words}})).unwrap();
    let r = s.execute("bashcut.transcript.clean", json!({})).unwrap();
    assert_eq!(r["items"].as_array().unwrap().len(), 0);
    let r = s.execute("bashcut.transcript.clean", json!({"items": [item]})).unwrap();
    assert_eq!(r["items"][0]["kept"], 0);
    assert!(s.execute("bashcut.transcript.clean", json!({"minRepeats": 1})).is_err());
}
