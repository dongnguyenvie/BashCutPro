//! BashCutPro: readable captions from the transcript (#200).

use filmcraft_time::{FrameRate, Tick};
use serde_json::json;

use super::captions_fit::{Cue, FitRules, FitWord, clean, cps, fit, group, trim, wrap};
use crate::Session;

fn sec(x: f64) -> Tick {
    Tick::from_seconds_f64(x)
}

/// `text start end` per word, seconds.
fn words(spec: &str) -> Vec<FitWord> {
    spec.lines()
        .filter_map(|l| {
            let mut it = l.split_whitespace();
            let (t, a, b) = (it.next()?, it.next()?.parse().ok()?, it.next()?.parse().ok()?);
            Some(FitWord { text: t.into(), start: sec(a), end: sec(b), speaker: None })
        })
        .collect()
}

const SPEECH: &str = "
So, 0.00 0.37
here 0.54 0.66
is 0.66 0.80
the 0.80 0.90
thing 0.90 1.04
about 1.04 1.24
editing 1.24 1.50
podcasts. 1.50 2.35
Um, 2.52 2.78
most 2.93 3.12
people 3.12 3.38
cut 3.38 3.58
the 3.58 3.70
the 3.70 3.76
clip 3.76 3.96
short. 3.96 4.30
";

fn rate() -> FrameRate {
    FrameRate::new(30, 1)
}

fn rules() -> FitRules {
    FitRules { max_chars: 32, end: Some(sec(5.0)), ..FitRules::default() }
}

#[test]
fn fillers_and_stutters_go_meaning_stays() {
    assert_eq!(clean("Um, most people cut the the clip short."), "Most people cut the clip short.");
    assert_eq!(clean("I mean it, uh."), "I mean it.");
    assert_eq!(clean("um"), "um", "a cue of only a filler is left for the editor");
}

#[test]
fn wrap_prefers_punctuation_and_never_ends_on_the() {
    assert_eq!(wrap("then you trim the fillers, keep the breaths,", 32, 2).as_deref(), Some("then you trim the fillers,\nkeep the breaths,"));
    assert_eq!(wrap("a line far too long to fit on one line of twenty", 20, 1), None);
}

#[test]
fn trim_drops_hedges_only() {
    assert_eq!(trim("You just need to be ruthless, you know"), "You need to be ruthless");
}

#[test]
fn breaks_at_sentence_ends_and_reads_under_the_limit() {
    let r = rules();
    let cues = group(&words(SPEECH), &r, rate());
    let texts: Vec<&str> = cues.iter().map(|c| c.text.as_str()).collect();
    assert_eq!(texts, ["So, here is the thing about editing podcasts.", "Um, most people cut the the clip short."]);
    let work = fit(&cues, &r, rate());
    assert_eq!(work[1].text, "Most people cut the clip short.");
    for c in &work {
        assert!(cps(c) <= 20.0, "{c:?} reads at {}", cps(c));
        assert!(c.text.lines().count() <= 2 && c.text.lines().all(|l| l.chars().count() <= 32), "{c:?}");
    }
    let gap = Tick(rate().frame_duration().0 * 2);
    assert!(work[0].end + gap <= work[1].start, "no overlap, a 2-frame gap: {work:?}");
    assert!(work[1].end <= sec(5.0));
}

#[test]
fn breaks_at_a_comma_once_half_full_on_a_word_edge() {
    // one breath, no pause: the comma ends the first cue, not the character limit mid-phrase
    let ws = words(
        "
Then 0.0 0.2
you 0.2 0.3
trim 0.3 0.5
the 0.5 0.6
fillers, 0.6 1.0
keep 1.0 1.2
the 1.2 1.3
breaths 1.3 1.6
and 1.6 1.7
export 1.7 2.0
it. 2.0 2.2
",
    );
    let r = FitRules { max_chars: 20, lines: 2, ..FitRules::default() };
    let cues = group(&ws, &r, rate());
    assert_eq!(cues[0].text, "Then you trim the fillers,", "{cues:?}");
    assert!(cues.iter().all(|c| !c.text.ends_with(" the") && !c.text.ends_with(" and")), "{cues:?}");
}

#[test]
fn no_lone_word_inside_a_sentence() {
    let ws = words(
        "
alpha 0.0 0.3
beta 0.3 0.6
gamma 0.6 0.9
delta 0.9 1.2
eps. 1.2 1.5
",
    );
    let r = FitRules { max_chars: 17, lines: 1, ..FitRules::default() };
    let cues = group(&ws, &r, rate());
    assert!(cues.iter().all(|c| c.text.split_whitespace().count() > 1), "{cues:?}");
}

#[test]
fn a_fast_cue_holds_into_the_silence_and_never_over_the_next() {
    let fd = rate().frame_duration();
    let cue = |a: f64, b: f64, t: &str| Cue { start: sec(a), end: sec(b), text: t.into(), speaker: None, word_end: sec(b) };
    let cues = [cue(0.0, 0.6, "So here is the thing about"), cue(1.6, 2.4, "editing"), cue(2.4, 4.0, "podcasts.")];
    let r = FitRules { end: Some(sec(4.0)), ..FitRules::default() };
    let work = fit(&cues, &r, rate());
    assert!(work.iter().all(|c| cps(c) <= 20.0), "{work:?}");
    assert!(work[0].end <= sec(1.6) - Tick(fd.0 * 2) + Tick(1), "held into the silence, not over the next: {work:?}");
    assert!(work.windows(2).all(|w| w[0].end <= w[1].start), "{work:?}");
}

#[test]
fn hostile_params_are_errors_or_clamped_never_a_panic() {
    let mut s = Session::default();
    s.execute("file.newSequence", json!({"width": 320, "height": 180, "fps": 24})).unwrap();
    assert!(s.execute("bashcut.captions.create", json!({})).is_err(), "no transcript: an error");
    let r = FitRules::default();
    assert!(fit(&[], &r, rate()).is_empty());
    assert!(group(&[], &r, rate()).is_empty());
    let one = [FitWord { text: "word".into(), start: Tick(i64::MAX / 8), end: Tick(i64::MAX / 8), speaker: None }];
    let cues = fit(&group(&one, &r, rate()), &r, rate());
    assert_eq!(cues.len(), 1);
}

/// The demo project with `SPEECH` as the first A1 clip's transcript.
fn session() -> Session {
    let mut s = Session::default();
    s.execute("file.openDemoProject", json!({})).unwrap();
    let q = s.active_sequence().unwrap();
    let a = &q.audio_tracks[0].items[0];
    let (item, sin) = (a.item, a.source_in);
    let ws: Vec<_> = words(SPEECH).iter().map(|w| json!({"text": w.text, "start": (sin + w.start).0, "end": (sin + w.end).0})).collect();
    s.execute("transcript.set", json!({"item": item.0, "transcript": {"language": "en", "words": ws}})).unwrap();
    s
}

#[test]
fn the_command_makes_a_readable_caption_track_in_one_undo_step() {
    let mut s = session();
    let undo = s.history.undo.len();
    let dry = s.execute("bashcut.captions.create", json!({"maxChars": 32, "dryRun": true})).unwrap();
    assert_eq!(s.history.undo.len(), undo, "a dry run changes nothing");
    assert_eq!(dry["cues"].as_array().map(Vec::len), Some(2), "{dry}");
    let r = s.execute("bashcut.captions.create", json!({"maxChars": 32})).unwrap();
    assert_eq!(s.history.undo.len(), undo + 1);
    assert_eq!(r["after"]["atOrUnder"], 100.0, "{r}");
    assert_eq!(r["left"], json!([]));
    let q = s.active_sequence().unwrap();
    let t = &q.caption_tracks[0];
    assert_eq!(
        t.captions.iter().map(|c| c.text.as_str()).collect::<Vec<_>>(),
        ["So, here is the thing\nabout editing podcasts.", "Most people cut the clip short."]
    );
    q.check().unwrap();
    // the review's reading-speed check agrees
    let rev = s.execute("bashcut.review.run", json!({})).unwrap();
    let fast: Vec<_> = rev["findings"].as_array().into_iter().flatten().filter(|f| f["check"] == "readingSpeed").collect();
    assert!(fast.is_empty(), "{fast:?}");
}
