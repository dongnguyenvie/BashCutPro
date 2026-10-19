//! BashCutPro: word-by-word caption styles (#49).

use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde_json::json;

use super::word_captions::{plain_text, word_ranges, word_starts};
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
    // a one-word caption is reported, not styled
    let mut one = session_with_caption("hello");
    let r = one.execute("bashcut.captions.wordStyle", json!({})).unwrap();
    assert!(r["captions"][0]["skipped"].is_string(), "{r}");
}
