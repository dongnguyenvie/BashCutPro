//! BashCutPro: caption and title boxes against the profile's safe area, and lines drawn after wrapping (#205).

use filmcraft_project::{Caption, CaptionAlign, CaptionAnchor, CaptionStyle, ClipId};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::safe_area::caption_box;
use crate::Session;

fn cap(text: &str, settings: &str) -> Caption {
    Caption { id: ClipId(1), start: Tick(0), duration: Tick(100), text: text.into(), speaker: None, cue_id: None, settings: settings.into() }
}

#[test]
fn the_measured_box_is_the_rendered_overlay_box() {
    let wide = CaptionStyle::default();
    let top = CaptionStyle { anchor: CaptionAnchor::Top, outline: 4.0, margin: 0.1, ..CaptionStyle::default() };
    let left = CaptionStyle { align: CaptionAlign::Left, size: 80.0, line_spacing: 1.4, ..CaptionStyle::default() };
    let cases = [
        (cap("Hello world", ""), &wide, 1920, 1080),
        (cap("So, here is the thing about\nediting podcasts.", ""), &wide, 1080, 1920),
        (cap("A line far too long for a phone screen at this size, so it wraps", ""), &top, 1080, 1920),
        (cap("Left and large\nsecond line", ""), &left, 1280, 720),
        (cap("Placed by the cue", "line:10% align:end"), &wide, 1920, 1080),
    ];
    for (c, style, w, h) in cases {
        let o = filmcraft_captions::burn::render_caption(&c, style, w, h).unwrap();
        let b = caption_box(&c, style, w, h).unwrap();
        let got = [b.x0, b.y0, b.x1, b.y1];
        let want = [f64::from(o.x), f64::from(o.y), f64::from(o.x) + o.w as f64, f64::from(o.y) + o.h as f64];
        assert_eq!(got, want, "{:?} {w}x{h}", c.text);
    }
    assert!(caption_box(&cap("  ", ""), &wide, 1920, 1080).is_none());
    assert!(caption_box(&cap("x", ""), &wide, 0, 0).is_none(), "a zero frame is not a panic");
}

/// A 1080×1920 sequence with an 8 s matte on V1 and the caption track's style at its defaults.
fn vertical() -> Session {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "vertical", "width": 1080, "height": 1920, "fps": 30})).unwrap();
    let matte = s.execute("file.newColorMatte", json!({"color": "#808080", "seconds": 8.0})).unwrap()["item"].as_u64().unwrap();
    s.execute("timeline.place", json!({"item": matte, "track": "V1", "seconds": 0.0, "audio": false})).unwrap();
    s
}

fn checks<'a>(v: &'a Value, check: &str) -> Vec<&'a Value> {
    v["findings"].as_array().unwrap().iter().filter(|f| f["check"] == check).collect()
}

fn review(s: &mut Session, profile: Option<&str>) -> Value {
    let mut p = json!({"checks": ["captions"], "writeMarkers": false});
    if let Some(pr) = profile {
        p["profile"] = json!(pr);
    }
    s.execute("bashcut.review.run", p).unwrap()
}

#[test]
fn a_caption_in_the_reels_bottom_band_blocks_until_it_is_raised() {
    let mut s = vertical();
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 3.0, "text": "Short and calm."})).unwrap();
    let v = review(&mut s, Some("reels"));
    let safe = checks(&v, "safeArea");
    assert_eq!(safe.len(), 1, "{v}");
    assert_eq!(safe[0]["severity"], "blocking");
    assert!(safe[0]["message"].as_str().unwrap().contains("bottom 384 px of Reels"), "{}", safe[0]);
    assert!(!v["skipped"].as_array().unwrap().iter().any(|x| x["check"] == "captions.safeArea"), "measured, not skipped");
    // the suggested margin clears it
    let margin = safe[0]["limit"]["marginAtLeast"].as_f64().unwrap();
    s.execute("captions.setStyle", json!({"track": "C1", "margin": margin})).unwrap();
    let v = review(&mut s, Some("reels"));
    assert!(checks(&v, "safeArea").is_empty(), "{v}");
    assert_eq!(v["measured"]["captions"]["safeAreaBoxes"], 1);
}

#[test]
fn without_a_profile_the_safe_area_is_skipped() {
    let mut s = vertical();
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 3.0, "text": "Short and calm."})).unwrap();
    let v = review(&mut s, None);
    assert!(checks(&v, "safeArea").is_empty(), "{v}");
    assert!(v["skipped"].as_array().unwrap().iter().any(|x| x["check"] == "captions.safeArea"), "{v}");
}

#[test]
fn wrapped_lines_are_counted_as_drawn() {
    let mut s = vertical();
    // two 32-character lines: at the default size on a 1080-wide frame each wraps (the kit saw 4 lines)
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 4.0, "text": "Most people cut the clip short,\nkeep every breath and then wonder"}))
        .unwrap();
    let v = review(&mut s, None);
    let lines = checks(&v, "lines");
    assert_eq!(lines.len(), 1, "{v}");
    assert_eq!(lines[0]["severity"], "blocking");
    assert!(lines[0]["measured"].as_u64().unwrap() > 2, "{}", lines[0]);
    // the same text on a landscape frame fits on its two lines
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"name": "wide", "width": 1920, "height": 1080, "fps": 30})).unwrap();
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 4.0, "text": "Most people cut the clip short,\nkeep every breath and then wonder"}))
        .unwrap();
    assert!(checks(&review(&mut s, None), "lines").is_empty());
}

#[test]
fn a_title_in_the_top_band_blocks() {
    let mut s = vertical();
    // point text on its first baseline at y 240: inside Reels' top 269 px
    s.execute("graphics.newText", json!({"text": "Day one", "position": [540.0, 240.0], "size": 100, "seconds": 3.0, "track": 1})).unwrap();
    let v = review(&mut s, Some("reels"));
    let safe = checks(&v, "safeArea");
    assert_eq!(safe.len(), 1, "{v}");
    assert!(safe[0]["message"].as_str().unwrap().contains("top 269 px"), "{}", safe[0]);
    // the same title in the middle of the frame is fine
    let mut s = vertical();
    s.execute("graphics.newText", json!({"text": "Day one", "position": [400.0, 960.0], "size": 100, "seconds": 3.0, "track": 1})).unwrap();
    assert!(checks(&review(&mut s, Some("reels")), "safeArea").is_empty());
}

#[test]
fn a_caption_wider_than_the_side_bands_gets_a_smaller_size() {
    let mut s = vertical();
    s.execute("captions.add", json!({"seconds": 1.0, "durationSeconds": 4.0, "text": "So, here is the thing"})).unwrap();
    s.execute("captions.setStyle", json!({"track": "C1", "margin": 0.25})).unwrap();
    let v = review(&mut s, Some("reels"));
    let safe = checks(&v, "safeArea");
    assert_eq!(safe.len(), 1, "{v}");
    assert!(safe[0]["message"].as_str().unwrap().contains("right 108 px"), "{}", safe[0]);
    let size = safe[0]["limit"]["sizeAtMost"].as_f64().unwrap();
    assert!(size < 54.0, "{}", safe[0]);
    s.execute("captions.setStyle", json!({"track": "C1", "size": size})).unwrap();
    assert!(checks(&review(&mut s, Some("reels")), "safeArea").is_empty());
}
