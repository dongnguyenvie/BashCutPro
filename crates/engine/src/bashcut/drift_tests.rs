//! BashCutPro: a slow Ken Burns drift on a still is motion, not a frozen picture (#206).

use filmcraft_time::Tick;
use serde_json::{Value, json};

use crate::Session;

/// A 1920×1080 test-pattern still: colour bars over a soft gradient, a few rings and fine stripes.
fn pattern_png() -> Vec<u8> {
    let (w, h) = (1920usize, 1080usize);
    let bars = [[192u8, 192, 192], [192, 192, 0], [0, 192, 192], [0, 192, 0], [192, 0, 192], [192, 0, 0], [0, 0, 192]];
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let i = (y * w + x) * 4;
            let mut c = if y < h * 2 / 3 {
                bars[x * bars.len() / w]
            } else {
                let g = (x * 255 / w) as u8;
                [g, g, g]
            };
            let (dx, dy) = (x as f64 - 960.0, y as f64 - 540.0);
            let r = (dx * dx + dy * dy).sqrt();
            if (r as i64 / 40) % 2 == 0 && r < 400.0 {
                c = [255 - c[0], 255 - c[1], 255 - c[2]];
            }
            if y > h * 2 / 3 && (x / 6) % 2 == 0 && x > w / 2 {
                c = [20, 20, 20];
            }
            px[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], 255]);
        }
    }
    filmcraft_export::encode_png(px, w as u32, h as u32).unwrap()
}

/// The pattern for 5 s on V1 of a 1080×1920 sequence with Motion scale keyframes `from` → `to` % (`None`: no
/// keyframes, a plain still).
fn still(scale: Option<(f64, f64)>) -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "drift", "width": 1080, "height": 1920, "fps": 30})).unwrap();
    let item = crate::commands::import_bytes(&mut s, "/pattern.png", pattern_png().into(), None).unwrap().0;
    s.execute("timeline.place", json!({"item": item, "track": "V1", "seconds": 0.0, "duration": Tick::from_seconds_f64(5.0).0, "audio": false})).unwrap();
    let clip = s.active_sequence().unwrap().video_tracks[0].items[0].id;
    if let Some((from, to)) = scale {
        for (at, value) in [(0.0, from), (5.0 - 1.0 / 30.0, to)] {
            let t = Tick::from_seconds_f64(at);
            s.execute("effects.addKeyframe", json!({"clip": clip.0, "effect": "motion", "param": "scale", "time": t.0})).unwrap();
            let it = s.active_sequence().unwrap().find_item(clip).unwrap().1;
            let mt = it.source_time_at(t);
            s.execute("effects.setKeyframe", json!({"clip": clip.0, "effect": "motion", "param": "scale", "mediaTime": mt.0, "value": value})).unwrap();
        }
    }
    s
}

fn frozen(v: &Value) -> Vec<Value> {
    v["findings"].as_array().unwrap().iter().filter(|f| f["kind"] == "frozen" || f["kind"] == "longStill").cloned().collect()
}

#[test]
fn a_slow_ken_burns_drift_is_not_frozen() {
    // 100 → 104 % over 5 s (0.8 %/s): the kit saw a 1 s "frozen" finding
    let mut s = still(Some((100.0, 104.0)));
    let v = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 5})).unwrap();
    assert!(frozen(&v).is_empty(), "{v}");
    let v = s.execute("bashcut.review.run", json!({"checks": ["picture"], "writeMarkers": false})).unwrap();
    assert!(!v["findings"].as_array().unwrap().iter().any(|f| f["check"] == "frozen" || f["check"] == "longStill"), "{v}");
    // slower still: 100 → 102 %
    let mut s = still(Some((100.0, 102.0)));
    let v = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 5})).unwrap();
    assert!(frozen(&v).is_empty(), "{v}");
}

#[test]
fn a_plain_still_is_still_frozen() {
    let mut s = still(None);
    let v = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 5})).unwrap();
    let f = frozen(&v);
    assert_eq!(f.len(), 1, "{v}");
    assert_eq!(f[0]["kind"], "longStill", "5 s unchanged: {v}");
}

#[test]
fn a_drift_that_stops_leaves_the_held_part_frozen() {
    // 100 → 101.6 % over the first 2 s (0.8 %/s), then held for 3 s: the hold is a still picture again
    let mut s = still(None);
    let clip = s.active_sequence().unwrap().video_tracks[0].items[0].id;
    for (at, value) in [(0.0, 100.0), (2.0, 101.6)] {
        let t = Tick::from_seconds_f64(at);
        s.execute("effects.addKeyframe", json!({"clip": clip.0, "effect": "motion", "param": "scale", "time": t.0})).unwrap();
        let mt = s.active_sequence().unwrap().find_item(clip).unwrap().1.source_time_at(t);
        s.execute("effects.setKeyframe", json!({"clip": clip.0, "effect": "motion", "param": "scale", "mediaTime": mt.0, "value": value})).unwrap();
    }
    let v = s.execute("bashcut.review.pictureChecks", json!({"samplesPerSecond": 5})).unwrap();
    let f = frozen(&v);
    assert_eq!(f.len(), 1, "{v}");
    let (a, b) = (f[0]["start"].as_f64().unwrap(), f[0]["end"].as_f64().unwrap());
    assert!((1.7..=2.3).contains(&a) && (b - 5.0).abs() < 0.05, "the held part (from 2 s, give or take a sample) {a}..{b}: {v}");
}
