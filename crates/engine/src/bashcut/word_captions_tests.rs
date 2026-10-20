//! BashCutPro: word-by-word caption styles (#49).

use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde_json::json;

use super::word_captions::{plain_text, step_starts, word_ranges, word_starts};
use crate::Session;
use crate::media_test_util::frame_rgba;

fn session_with_caption(text: &str) -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"width": 320, "height": 180, "fps": 24})).unwrap();
    s.execute("captions.add", json!({"text": text, "time": 0, "durationSeconds": 3.0})).unwrap();
    s
}

/// Mean x of the strongly red pixels and how many there are.
fn red_x(w: usize, h: usize, rgba: &[u8]) -> (f64, usize) {
    let (mut sum, mut n) = (0usize, 0usize);
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let (r, g, b) = (rgba[i], rgba[i + 1], rgba[i + 2]);
            if r > 180 && g < 80 && b < 80 {
                sum += x;
                n += 1;
            }
        }
    }
    (if n == 0 { 0.0 } else { sum as f64 / n as f64 }, n)
}

#[test]
fn the_spoken_word_is_highlighted_in_its_own_segment() {
    let mut s = session_with_caption("one two three");
    let undo = s.history.undo.len();
    let r = s.execute("bashcut.captions.wordStyle", json!({"style": {"preset": "clean", "highlight": "#ff0000"}, "useTranscript": false})).unwrap();
    assert_eq!(r["captions"][0]["words"], 3, "{r}");
    assert_eq!(r["captions"][0]["clips"].as_array().unwrap().len(), 3, "{r}");
    assert_eq!(s.history.undo.len(), undo + 1, "one undo step");
    assert_eq!(s.history.undo.last().map(|u| u.0.as_str()), Some("Word Caption Style"));
    let seq = s.active_sequence().unwrap();
    assert!(seq.caption_tracks.iter().all(|t| t.captions.is_empty()), "captions became graphics");

    // segments follow each other on frame boundaries: one word shows at a time
    let mut spans: Vec<(Tick, Tick)> = r["captions"][0]["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| seq.find_item(filmcraft_project::ClipId(c.as_u64().unwrap())).map(|(_, it)| (it.start, it.end())).unwrap())
        .collect();
    spans.sort();
    assert!(spans.windows(2).all(|w| w[0].1 <= w[1].0), "segments overlap: {spans:?}");

    // words share 3 s by characters: one 0–0.82 s, two 0.82–1.64 s, three 1.64–3 s (24 fps); the
    // text is centred, so the red word moves from left to right inside it
    let mut seen = Vec::new();
    for frame in [10, 29, 58] {
        let (w, h, rgba) = frame_rgba(&mut s, frame, 1.0);
        seen.push(red_x(w, h, &rgba));
    }
    assert!(seen.iter().all(|(_, n)| *n > 0), "a red word in every frame: {seen:?}");
    assert!(seen[0].0 < seen[1].0 && seen[1].0 < seen[2].0, "the red word moves left to right: {seen:?}");

    // undo brings the caption back
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(s.active_sequence().unwrap().caption_tracks[0].captions.len(), 1);
}

#[test]
fn word_helpers() {
    assert_eq!(plain_text("<i>Hello</i> <b>big</b> world"), "Hello big world");
    assert_eq!(plain_text("a < b"), "a < b");
    assert_eq!(word_ranges("  one two\nthree "), vec![(2, 5), (6, 9), (10, 15)]);
    assert!(word_ranges("   ").is_empty());
    let sec = |x: f64| Tick::from_seconds_f64(x);
    let words = word_ranges("aa bb");
    // even split by characters
    assert_eq!(word_starts(Tick::ZERO, sec(2.0), &words, &[]), vec![Tick::ZERO, sec(1.0)]);
    // transcript words that line up win
    let tx = [(sec(0.1), sec(0.5)), (sec(1.5), sec(1.9)), (sec(5.0), sec(6.0))];
    assert_eq!(word_starts(Tick::ZERO, sec(2.0), &words, &tx), vec![sec(0.1), sec(1.5)]);
    // a mismatch falls back to the even split
    assert_eq!(word_starts(Tick::ZERO, sec(2.0), &word_ranges("aa bb cc dd"), &tx).len(), 4);
    assert_eq!(Tick(TICKS_PER_SECOND), sec(1.0));
}

#[test]
fn word_style_refuses_hostile_params() {
    let mut s = session_with_caption("one two three");
    for p in [
        json!({"captions": []}),
        json!({"captions": [u64::MAX]}),
        json!({"style": "red"}),
        json!({"style": {"preset": "nope"}}),
        json!({"style": {"highlight": "red"}}),
        json!({"style": {"highlight": "#ff00zz"}}),
    ] {
        assert!(s.execute("bashcut.captions.wordStyle", p.clone()).is_err(), "{p}");
    }
    // out-of-range numbers clamp
    assert!(s.execute("bashcut.captions.wordStyle", json!({"style": {"pop": 1e9, "dim": -4}, "useTranscript": false})).is_ok());
    // a one-word caption is styled as one segment (#199)
    let mut one = session_with_caption("hello");
    let r = one.execute("bashcut.captions.wordStyle", json!({})).unwrap();
    assert_eq!(r["captions"][0]["clips"].as_array().map(Vec::len), Some(1), "{r}");
}

#[test]
fn wrap_text_breaks_at_spaces_and_keeps_every_index() {
    // 10 px per character
    let width = |t: &str| Ok(t.chars().count() as f64 * 10.0);
    let text = "Um, today I want to show you how";
    let wrapped = super::word_captions::wrap_text(text, 120.0, width).unwrap();
    assert_eq!(wrapped, "Um, today I\nwant to show\nyou how");
    assert_eq!(wrapped.chars().count(), text.chars().count());
    assert_eq!(word_ranges(&wrapped), word_ranges(text), "word ranges line up");
    assert_eq!(super::word_captions::wrap_text("short", 120.0, width).unwrap(), "short");
    // one word wider than the limit stays on its own line
    assert_eq!(super::word_captions::wrap_text("a supercalifragilistic b", 120.0, width).unwrap(), "a\nsupercalifragilistic\nb");
}

#[test]
fn a_long_caption_stays_inside_a_phone_frame() {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"width": 1080, "height": 1920, "fps": 30})).unwrap();
    s.execute("captions.add", json!({"text": "Um, today I want to show you how we cut", "time": 0, "durationSeconds": 3.0})).unwrap();
    let r = s.execute("bashcut.captions.wordStyle", json!({"style": {"preset": "pop"}, "useTranscript": false})).unwrap();
    let clips: Vec<u64> = r["captions"][0]["clips"].as_array().unwrap().iter().map(|c| c.as_u64().unwrap()).collect();
    let steps = r["captions"][0]["steps"].as_array().unwrap().clone();
    // 10 words in 3 s: "I" (0.1 s) shares a step with a neighbour (#213)
    assert!((7..10).contains(&clips.len()), "{r}");
    assert_eq!(steps.len(), clips.len(), "{r}");
    for (c, step) in clips.iter().zip(&steps) {
        let (first, last) = (step[0].as_u64().unwrap() as usize, step[1].as_u64().unwrap() as usize);
        let l = s.execute("graphics.list", json!({"clip": c})).unwrap()["layers"][0].clone();
        let xs: Vec<f64> = l["quad"].as_array().unwrap().iter().map(|p| p[0].as_f64().unwrap()).collect();
        assert!(xs.iter().all(|x| (0.0..=1080.0).contains(x)), "step {step} leaves the frame: {l}");
        assert!(l["text"].as_str().unwrap().contains('\n'), "{l}");
        // the styled run is still the spoken word(s) of the step
        let text = l["text"].as_str().unwrap();
        let (a, b) = (word_ranges(text)[first].0, word_ranges(text)[last].1);
        assert_eq!((l["styles"][0]["start"].as_u64(), l["styles"][0]["end"].as_u64()), (Some(a as u64), Some(b as u64)), "{l}");
    }
    assert_eq!(s.history.undo.last().map(|u| u.0.as_str()), Some("Word Caption Style"));
}

fn undo_labels(s: &Session) -> Vec<String> {
    s.history.undo.iter().map(|u| u.0.clone()).collect()
}

fn all_captions(s: &Session) -> Vec<u64> {
    s.active_sequence().unwrap().caption_tracks.iter().flat_map(|t| t.captions.iter().map(|c| c.id.0)).collect()
}

#[test]
fn a_long_word_style_is_one_undo_step_after_the_one_before() {
    // #199: 25 captions of 6 words push more steps than the history keeps (200): the step before the word style was
    // lost and a stray "Character Style" stayed
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"width": 320, "height": 180, "fps": 24})).unwrap();
    let commands: Vec<_> = (0..25)
        .map(|i| json!({"id": "captions.add", "params": {"text": format!("one two three four five six{i}"), "time": Tick::from_seconds_f64(i as f64 * 3.0).0, "durationSeconds": 3.0}}))
        .collect();
    s.execute("bashcut.batch", json!({"label": "Captions", "commands": commands})).unwrap();
    let before = undo_labels(&s);
    assert_eq!(before.last().map(String::as_str), Some("Captions"));
    let ids = all_captions(&s);
    let r = s.execute("bashcut.captions.wordStyle", json!({"captions": ids, "style": {"preset": "pop", "pop": 1.2}, "useTranscript": false})).unwrap();
    assert_eq!(r["captions"].as_array().unwrap().len(), 25, "{r}");
    let mut want = before.clone();
    want.push("Word Caption Style".into());
    assert_eq!(undo_labels(&s), want);
    assert_eq!(s.history.limit, 200, "the limit is back");
    s.execute("edit.undo", json!({})).unwrap();
    let seq = s.active_sequence().unwrap();
    assert_eq!(seq.caption_tracks.iter().map(|t| t.captions.len()).sum::<usize>(), 25, "undo brings the captions back");
    assert_eq!(undo_labels(&s), before);
}

#[test]
fn a_one_word_caption_becomes_a_graphic_too() {
    // #199: "it." and "read." stayed plain captions while the rest became graphics
    let mut s = session_with_caption("Most people cut it");
    s.execute("captions.add", json!({"text": "Yes.", "time": Tick::from_seconds_f64(4.0).0, "durationSeconds": 1.0})).unwrap();
    let ids = all_captions(&s);
    let r = s
        .execute("bashcut.captions.wordStyle", json!({"captions": ids, "style": {"preset": "clean", "highlight": "#ff0000"}, "useTranscript": false}))
        .unwrap();
    assert!(r["captions"].as_array().unwrap().iter().all(|c| c.get("skipped").is_none()), "{r}");
    let seq = s.active_sequence().unwrap();
    assert!(seq.caption_tracks.iter().all(|t| t.captions.is_empty()), "every cue became a graphic: {r}");
    let one = r["captions"].as_array().unwrap().iter().find(|c| c["words"] == 1).expect("the one-word cue");
    assert_eq!(one["clips"].as_array().unwrap().len(), 1, "{r}");
    // its word is red
    let (w, h, rgba) = frame_rgba(&mut s, 4 * 24 + 12, 1.0);
    assert!(red_x(w, h, &rgba).1 > 20, "the word is highlighted");
}

#[test]
fn step_starts_merge_fast_words() {
    let sec = |x: f64| Tick::from_seconds_f64(x);
    let min = sec(0.25);
    // 0.1 s words: a new step opens once 0.25 s have passed
    let starts: Vec<Tick> = (0..10).map(|i| sec(i as f64 * 0.1)).collect();
    assert_eq!(step_starts(Tick::ZERO, sec(1.0), &starts, min), vec![0, 3, 6]);
    // slow words keep their own steps
    let slow: Vec<Tick> = (0..4).map(|i| sec(i as f64 * 0.5)).collect();
    assert_eq!(step_starts(Tick::ZERO, sec(2.0), &slow, min), vec![0, 1, 2, 3]);
    // a short last word joins the step before it
    assert_eq!(step_starts(Tick::ZERO, sec(1.6), &slow, min), vec![0, 1, 2]);
    // a caption shorter than a step is one step
    assert_eq!(step_starts(Tick::ZERO, sec(0.2), &starts[..2], min), vec![0]);
    assert_eq!(step_starts(Tick::ZERO, sec(1.0), &[], min), vec![0]);
}

#[test]
fn fast_speech_makes_no_flash_frames() {
    // #213: 6 words/s (12 words in 2 s, short and long ones) made 2-frame word clips that review flagged as shortClip
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"width": 640, "height": 360, "fps": 30})).unwrap();
    let text = "so I a do it to me, we go on up the extraordinarily";
    s.execute("captions.add", json!({"text": text, "time": 0, "durationSeconds": 2.0})).unwrap();
    let undo = s.history.undo.len();
    let r = s.execute("bashcut.captions.wordStyle", json!({"style": {"preset": "pop"}, "useTranscript": false})).unwrap();
    assert_eq!(s.history.undo.len(), undo + 1, "one undo step");
    let clips = r["captions"][0]["clips"].as_array().unwrap();
    assert!(clips.len() > 1 && clips.len() < 13, "{r}");
    let seq = s.active_sequence().unwrap();
    for c in clips {
        let (_, it) = seq.find_item(filmcraft_project::ClipId(c.as_u64().unwrap())).unwrap();
        assert!(it.duration >= Tick::from_seconds_f64(0.24), "a {} s step: {r}", it.duration.seconds());
    }
    // every word is highlighted in exactly one step, in order
    let steps = r["captions"][0]["steps"].as_array().unwrap();
    let mut next = 0;
    for st in steps {
        assert_eq!(st[0].as_u64(), Some(next), "{r}");
        next = st[1].as_u64().unwrap() + 1;
    }
    assert_eq!(next, 13, "{r}");
    let v = s.execute("bashcut.review.run", json!({"checks": ["picture"], "writeMarkers": false})).unwrap();
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["check"] == "shortClip" || f["check"] == "shortGap"), "{v}");
}
