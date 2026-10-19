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

/// Strongly red pixels in the left, middle and right third of the frame.
fn red_by_third(w: usize, h: usize, rgba: &[u8]) -> [usize; 3] {
    let mut n = [0usize; 3];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let (r, g, b) = (rgba[i], rgba[i + 1], rgba[i + 2]);
            if r > 180 && g < 80 && b < 80 {
                n[(x * 3 / w).min(2)] += 1;
            }
        }
    }
    n
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

    // words share 3 s by characters: one 0–0.82 s, two 0.82–1.64 s, three 1.64–3 s (24 fps)
    let mut seen = Vec::new();
    for frame in [10, 29, 58] {
        let (w, h, rgba) = frame_rgba(&mut s, frame, 1.0);
        seen.push(red_by_third(w, h, &rgba));
    }
    let [first, second, third] = [seen[0], seen[1], seen[2]];
    assert!(first[0] > first[2] && first[0] > 0, "word 1 red on the left: {seen:?}");
    assert!(second[1] > 0 && second[1] >= second[0] && second[1] >= second[2], "word 2 red in the middle: {seen:?}");
    assert!(third[2] > third[0] && third[2] > 0, "word 3 red on the right: {seen:?}");

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
