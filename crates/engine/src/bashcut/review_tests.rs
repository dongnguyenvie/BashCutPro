//! Tests of `bashcut.review.run` (#57): planted problems (a loud segment, a black frame, a fast caption, a cut
//! inside a word) are each found once with the right range; the demo has no blocking picture, caption or
//! edit finding; review markers replace the previous review's and undo in one step.

use std::sync::Arc;

use filmcraft_time::Tick;
use serde_json::{Value, json};

use crate::Session;

const SR: u32 = 48_000;

fn session() -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "review", "video": 2, "audio": 2, "width": 320, "height": 180})).unwrap();
    s
}

fn import_wav(s: &mut Session, name: &str, mono: &[f32]) -> u64 {
    let inter: Vec<f32> = mono.iter().flat_map(|v| [*v, *v]).collect();
    let bytes: Arc<[u8]> = crate::previews::write_wav_f32(&inter, SR).into();
    crate::commands::import_bytes(s, &format!("/{name}.wav"), bytes, None).unwrap().0
}

fn tone(amp_db: f32, seconds: f64) -> Vec<f32> {
    let amp = 10f32.powf(amp_db / 20.0);
    (0..(SR as f64 * seconds) as usize).map(|i| amp * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / SR as f64).sin() as f32).collect()
}

fn findings<'a>(v: &'a Value, check: &str) -> Vec<&'a Value> {
    v["findings"].as_array().unwrap().iter().filter(|f| f["check"] == check).collect()
}

fn range(f: &Value) -> (f64, f64) {
    (f["start"].as_f64().unwrap(), f["end"].as_f64().unwrap())
}

#[test]
fn a_loud_segment_is_found_once() {
    let mut s = session();
    // 12 s at about −20 dBFS with 2 s at −3 dBFS from 5 s
    let mut x = tone(-20.0, 12.0);
    for v in x.iter_mut().skip(5 * SR as usize).take(2 * SR as usize) {
        *v *= 10f32.powf(17.0 / 20.0);
    }
    let item = import_wav(&mut s, "loud", &x);
    s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "video": false})).unwrap();
    let v = s.execute("bashcut.review.run", json!({"checks": ["sound"], "writeMarkers": false})).unwrap();
    let loud = findings(&v, "loudSegment");
    assert_eq!(loud.len(), 1, "{v}");
    let (a, b) = range(loud[0]);
    assert!(a <= 5.0 && b >= 7.0 && a >= 2.0 && b <= 10.0, "loud {a}..{b}");
    assert!(v["measured"]["sound"]["integratedLufs"].is_f64());
}

#[test]
fn a_black_frame_is_found_once_with_its_range() {
    let mut s = session();
    let place = |s: &mut Session, id: &str, p: Value, at: f64| {
        let item = s.execute(id, p).unwrap()["item"].as_u64().unwrap();
        s.execute("timeline.place", json!({"item": item, "track": "V1", "seconds": at, "audio": false})).unwrap();
    };
    place(&mut s, "file.newColorMatte", json!({"color": "#909090", "seconds": 3.0}), 0.0);
    place(&mut s, "file.newBlackVideo", json!({"seconds": 0.5}), 3.0);
    place(&mut s, "file.newColorMatte", json!({"color": "#4060a0", "seconds": 3.0}), 3.5);
    let v = s.execute("bashcut.review.run", json!({"checks": ["picture", "edit"], "samplesPerSecond": 10, "writeMarkers": false})).unwrap();
    let black = findings(&v, "black");
    assert_eq!(black.len(), 1, "{v}");
    assert_eq!(black[0]["severity"], "blocking");
    let (a, b) = range(black[0]);
    assert!((a - 3.0).abs() <= 0.11 && (b - 3.5).abs() <= 0.11, "black {a}..{b}");
    assert_eq!(v["verdict"], "needsFix");
    assert!(findings(&v, "gap").is_empty(), "{v}");
}

#[test]
fn a_fast_caption_is_found_once() {
    let mut s = session();
    let matte = s.execute("file.newColorMatte", json!({"color": "#909090", "seconds": 8.0})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 0.0, "audio": false})).unwrap();
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 3.0, "text": "A calm caption."})).unwrap();
    s.execute("captions.add", json!({"seconds": 5.0, "durationSeconds": 1.0, "text": "This caption has far too many words to read in one second"})).unwrap();
    let v = s.execute("bashcut.review.run", json!({"checks": ["captions"], "writeMarkers": false})).unwrap();
    let fast = findings(&v, "readingSpeed");
    assert_eq!(fast.len(), 1, "{v}");
    let (a, b) = range(fast[0]);
    assert!((a - 5.0).abs() < 0.05 && (b - 6.0).abs() < 0.05, "fast {a}..{b}");
    assert_eq!(fast[0]["severity"], "blocking");
    assert_eq!(v["measured"]["captions"]["captions"], 2);
}

#[test]
fn a_cut_inside_a_word_is_found_once() {
    let mut s = session();
    let item = import_wav(&mut s, "speech", &tone(-20.0, 6.0));
    let t = |x: f64| Tick::from_seconds_f64(x).0;
    let words = json!({"language": "en", "words": [
        {"text": "hello", "start": t(1.0), "end": t(1.6), "confidence": 1.0},
        {"text": "world", "start": t(2.0), "end": t(2.5), "confidence": 1.0},
    ]});
    s.execute("transcript.set", json!({"item": item, "transcript": words})).unwrap();
    // source 1.3 s … 1.9 s: the head cuts "hello", the tail sits in the pause
    s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 2.0, "sourceIn": t(1.3), "duration": t(0.6), "video": false})).unwrap();
    let v = s.execute("bashcut.review.run", json!({"checks": ["edit"], "writeMarkers": false})).unwrap();
    let cut = findings(&v, "cutInWord");
    assert_eq!(cut.len(), 1, "{v}");
    assert_eq!(cut[0]["measured"], "hello");
    let (a, b) = range(cut[0]);
    assert!((a - 1.7).abs() < 0.01 && (b - 2.3).abs() < 0.01, "word {a}..{b}");
}

#[test]
fn the_demo_has_no_blocking_picture_caption_or_edit_finding() {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let v = s.execute("bashcut.review.run", json!({"writeMarkers": false, "samplesPerSecond": 2})).unwrap();
    let blocking: Vec<&Value> = v["findings"].as_array().unwrap().iter().filter(|f| f["severity"] == "blocking" && f["category"] != "sound").collect();
    assert!(blocking.is_empty(), "{blocking:?}");
    for c in ["sound", "picture", "captions", "edit"] {
        assert!(v["scores"][c].is_u64(), "{c}");
    }
}

#[test]
fn markers_replace_the_previous_review_and_undo_in_one_step() {
    let mut s = session();
    let matte = s.execute("file.newColorMatte", json!({"color": "#909090", "seconds": 4.0})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 0.0, "audio": false})).unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 5.0, "audio": false})).unwrap();
    s.execute("markers.add", json!({"seconds": 0.5, "name": "mine"})).unwrap();
    let review_markers = |s: &Session| s.active_sequence().unwrap().markers.iter().filter(|m| m.name.starts_with(super::review::MARKER_PREFIX)).count();
    let v = s.execute("bashcut.review.run", json!({"checks": ["edit"]})).unwrap();
    let n = v["markers"].as_u64().unwrap() as usize;
    assert!(n >= 1, "the 1 s gap is a finding: {v}");
    assert_eq!(review_markers(&s), n);
    s.execute("bashcut.review.run", json!({"checks": ["edit"]})).unwrap();
    assert_eq!(review_markers(&s), n, "a second review replaces the first's markers");
    assert_eq!(s.active_sequence().unwrap().markers.iter().filter(|m| m.name == "mine").count(), 1);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(review_markers(&s), n);
    s.execute("edit.undo", json!({})).unwrap();
    assert_eq!(review_markers(&s), 0);
}

#[test]
fn hostile_params_are_typed_errors() {
    let mut empty = Session::default();
    assert!(empty.execute("bashcut.review.run", json!({})).is_err());
    let mut s = session();
    let matte = s.execute("file.newColorMatte", json!({"color": "#909090", "seconds": 2.0})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 0.0, "audio": false})).unwrap();
    for p in [
        json!({"checks": "sound"}),
        json!({"checks": ["loud"]}),
        json!({"checks": [5]}),
        json!({"profile": 5}),
        json!({"profile": "no-such-profile"}),
        json!({"writeMarkers": "yes"}),
        json!({"samplesPerSecond": 0}),
        json!({"range": {"start": 10.0}}),
        json!({"sequence": 987_654}),
        json!({"sequence": "x"}),
    ] {
        let e = s.execute("bashcut.review.run", p.clone()).unwrap_err().to_string();
        assert!(super::errors::tagged(&e).is_some(), "{p}: {e}");
    }
}

/// A 7 s colour matte (a long still) on V1 of a `w`×`h` sequence, and the review's longStill findings.
fn long_still(w: u32, h: u32, profile: Option<&str>, words: bool) -> Vec<Value> {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "still", "video": 2, "audio": 2, "width": w, "height": h})).unwrap();
    let matte = s.execute("file.newColorMatte", json!({"color": "#808080", "seconds": 7.0})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 0.0, "audio": false})).unwrap();
    if words {
        // someone talks over the still (a locked-off interview)
        let item = import_wav(&mut s, "voice", &tone(-20.0, 7.0));
        let t = |x: f64| Tick::from_seconds_f64(x).0;
        let words: Vec<Value> = (0..12).map(|i| json!({"text": "word", "start": t(0.5 * i as f64 + 0.2), "end": t(0.5 * i as f64 + 0.6)})).collect();
        s.execute("transcript.set", json!({"item": item, "transcript": {"language": "en", "words": words}})).unwrap();
        s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "video": false})).unwrap();
    }
    let mut p = json!({"checks": ["picture"], "samplesPerSecond": 4, "writeMarkers": false});
    if let Some(pr) = profile {
        p["profile"] = json!(pr);
    }
    let v = s.execute("bashcut.review.run", p).unwrap();
    findings(&v, "longStill").into_iter().cloned().collect()
}

#[test]
fn a_silent_long_still_warns_in_short_form_only() {
    // #185: the vlog's last 6 s were a near-still shot under an end card; a reels review called it info
    let vertical = long_still(180, 320, Some("reels"), false);
    assert_eq!(vertical.len(), 1, "{vertical:?}");
    assert_eq!(vertical[0]["severity"], "warning");
    // no profile: the sequence's shape decides
    assert_eq!(long_still(180, 320, None, false)[0]["severity"], "warning");
    // landscape, or someone talking over it (#178): still info
    assert_eq!(long_still(320, 180, Some("youtube"), false)[0]["severity"], "info");
    assert_eq!(long_still(320, 180, None, false)[0]["severity"], "info");
    assert_eq!(long_still(180, 320, Some("reels"), true)[0]["severity"], "info");
}

/// Two 3 s stills (frozen stretches) then a 7 s still (a long still) on V1 of a `w`×`h` sequence; `speech`: a
/// narration transcript over all 13 s; `captions`: subtitle cues over it instead. Returns the picture findings.
fn stills(w: u32, h: u32, speech: bool, captions: bool) -> Vec<Value> {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "stills", "video": 2, "audio": 2, "width": w, "height": h})).unwrap();
    for (color, at, len) in [("#808080", 0.0, 3.0), ("#305070", 3.0, 3.0), ("#a07040", 6.0, 7.0)] {
        let m = s.execute("file.newColorMatte", json!({"color": color, "seconds": len})).unwrap()["item"].as_u64().unwrap();
        s.execute("timeline.place", json!({"item": m, "track": "V1", "seconds": at, "audio": false})).unwrap();
    }
    if speech {
        // narration with normal pauses: 0.4 s words, 0.2-0.9 s apart
        let item = import_wav(&mut s, "voice", &tone(-20.0, 13.0));
        let t = |x: f64| Tick::from_seconds_f64(x).0;
        let mut words = Vec::new();
        let mut at = 0.1;
        let mut k = 0;
        while at + 0.4 < 13.0 {
            words.push(json!({"text": "word", "start": t(at), "end": t(at + 0.4)}));
            at += 0.6 + if k % 5 == 4 { 0.7 } else { 0.0 };
            k += 1;
        }
        s.execute("transcript.set", json!({"item": item, "transcript": {"language": "en", "words": words}})).unwrap();
        s.execute("timeline.place", json!({"item": item, "audioTrack": "A1", "seconds": 0.0, "video": false})).unwrap();
    }
    if captions {
        for i in 0..6 {
            let text = format!("Xin chào các bạn, phần {i}");
            s.execute("captions.add", json!({"seconds": i as f64 * 2.2, "durationSeconds": 2.0, "text": text})).unwrap();
        }
    }
    let v = s.execute("bashcut.review.run", json!({"checks": ["picture"], "samplesPerSecond": 4, "writeMarkers": false})).unwrap();
    v["findings"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|f| f["check"] == "frozen" || f["check"] == "stillUnderSpeech" || f["check"] == "longStill")
        .cloned()
        .collect()
}

fn severities(f: &[Value]) -> Vec<(String, String)> {
    f.iter().map(|f| (f["check"].as_str().unwrap().to_string(), f["severity"].as_str().unwrap().to_string())).collect()
}

#[test]
fn a_still_screen_under_narration_is_info() {
    // #193: a screen-recording tutorial got one "frozen" warning per still menu or dialog under the narration
    let f = stills(320, 180, true, false);
    assert!(!f.is_empty());
    assert!(f.iter().all(|f| f["severity"] == "info"), "{:?}", severities(&f));
    assert!(f.iter().any(|f| f["check"] == "stillUnderSpeech"), "{:?}", severities(&f));
    // the same stills with nothing said: frozen stays a warning
    let silent = stills(320, 180, false, false);
    assert!(silent.iter().any(|f| f["check"] == "frozen" && f["severity"] == "warning"), "{:?}", severities(&silent));
    // and in a vertical edit the silent long still warns too (#185), but not under narration
    let vertical = stills(180, 320, false, false);
    assert!(vertical.iter().any(|f| f["check"] == "longStill" && f["severity"] == "warning"), "{:?}", severities(&vertical));
    let narrated = stills(180, 320, true, false);
    assert!(narrated.iter().all(|f| f["severity"] == "info"), "{:?}", severities(&narrated));
}

#[test]
fn a_subtitled_still_interview_is_info() {
    // #196: a locked-off interview with Vietnamese subtitles and no transcript got "frozen" warnings
    let f = stills(320, 180, false, true);
    assert!(!f.is_empty());
    assert!(f.iter().all(|f| f["severity"] == "info"), "{:?}", severities(&f));
}
