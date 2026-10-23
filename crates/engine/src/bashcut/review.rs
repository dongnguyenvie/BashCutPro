//! `bashcut.review.run` (#57): a measured quality gate. Runs the checks below over the active sequence and
//! returns findings with a severity, a time range in seconds, the measurement, the limit and a suggested fix;
//! writes them as coloured range markers (one undo step, earlier review markers replaced) so they show on the
//! timeline; scores each category.
//!
//! | category | checks |
//! |---|---|
//! | sound | integrated loudness and true peak of the mix against the profile, clipped samples, loud seconds, long silences |
//! | picture | black, frozen, long-still and flashing stretches (`bashcut.review.pictureChecks`), clips and gaps shorter than 3 frames |
//! | captions | reading speed (characters a second), line length, lines drawn after wrapping, overlaps; with a profile, caption and title boxes against its safe area |
//! | edit | cuts inside a word (transcript word edges), gaps on the main video track, offline media |
//!
//! Not measured here (listed under `skipped` in the result): music over speech (needs stems), text contrast, and
//! the safe area when no profile is given. A batch item whose review has blocking findings ends "needs fix"
//! (`verdict`).

use filmcraft_project::{ItemId, Label, Marker, MarkerId, MarkerKind, Sequence};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::analysis::{Target, measure, picture_findings, range_p, secs};
use super::errors::{Code, fail};
use super::safe_area::{Insets, caption_box, text_boxes};
use crate::commands::{CommandSpec, bad, has_seq};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.review.run";
/// Name prefix of the markers a review writes (replaced on the next review).
pub const MARKER_PREFIX: &str = "Review: ";
/// At most this many findings are written as markers.
const MAX_MARKERS: usize = 500;

/// Limits; a profile sets the loudness ones.
#[derive(Clone, Debug)]
struct Limits {
    lufs: f64,
    lufs_tolerance: f64,
    true_peak: f64,
    /// Short-term loudness this far above the target is a loud second.
    loud_over: f64,
    /// Silence: short-term below this…
    silence_lufs: f64,
    /// …for at least this many seconds.
    silence_seconds: usize,
    cps_warn: f64,
    cps_block: f64,
    line_chars: usize,
    lines: usize,
    /// Clips and gaps shorter than this many frames.
    short_frames: i64,
    /// Margin inside a word that a cut must not fall into (seconds).
    word_margin: f64,
    /// A vertical (short-form) output: a long still with nothing said over it is a warning, not info (#185).
    short_form: bool,
    /// The profile's safe area (frame fractions platform UI covers) and its name (#205).
    safe_area: Option<(super::profiles::SafeArea, String)>,
}

impl Default for Limits {
    fn default() -> Self {
        Limits {
            lufs: -14.0,
            lufs_tolerance: 2.0,
            true_peak: -1.0,
            loud_over: 6.0,
            silence_lufs: -50.0,
            silence_seconds: 3,
            cps_warn: 20.0,
            cps_block: 25.0,
            line_chars: 42,
            lines: 2,
            short_frames: 3,
            word_margin: 0.02,
            short_form: false,
            safe_area: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Severity {
    Blocking,
    Warning,
    Info,
}

impl Severity {
    fn name(self) -> &'static str {
        match self {
            Severity::Blocking => "blocking",
            Severity::Warning => "warning",
            Severity::Info => "info",
        }
    }
    fn color(self) -> Label {
        match self {
            Severity::Blocking => Label::Rose,
            Severity::Warning => Label::Mango,
            Severity::Info => Label::Cerulean,
        }
    }
}

#[derive(Clone, Debug)]
struct Finding {
    category: &'static str,
    check: &'static str,
    severity: Severity,
    start: Tick,
    end: Tick,
    measured: Value,
    limit: Value,
    unit: &'static str,
    message: String,
    fix: &'static str,
}

impl Finding {
    fn json(&self) -> Value {
        json!({
            "category": self.category,
            "check": self.check,
            "severity": self.severity.name(),
            "start": secs(self.start),
            "end": secs(self.end),
            "measured": self.measured,
            "limit": self.limit,
            "unit": self.unit,
            "message": self.message,
            "fix": self.fix,
        })
    }
}

fn round2(v: f64) -> f64 {
    super::data::round_to(v, 2)
}

fn sec_tick(base: Tick, s: usize) -> Tick {
    base + Tick::from_seconds_f64(s as f64)
}

// ------------------------------------------------------------------------------------------ sound

fn sound(s: &Session, a: Tick, b: Tick, l: &Limits, out: &mut Vec<Finding>) -> Result<Value> {
    let (m, truncated) = measure(s, Target::Mix, a, b, CMD)?;
    let Some(m) = m else { return Ok(json!({"silent": true})) };
    let mut f = |check, severity, start, end, measured: Value, limit: Value, unit, message: String, fix| {
        out.push(Finding { category: "sound", check, severity, start, end, measured, limit, unit, message, fix })
    };
    match m.integrated_lufs {
        Some(i) if (i - l.lufs).abs() > l.lufs_tolerance => {
            let sev = if (i - l.lufs).abs() > 2.0 * l.lufs_tolerance { Severity::Blocking } else { Severity::Warning };
            f(
                "loudness",
                sev,
                a,
                b,
                json!(round2(i)),
                json!(l.lufs),
                "LUFS",
                format!("integrated loudness {:.1} LUFS, target {:.1} ± {:.0}", i, l.lufs, l.lufs_tolerance),
                "export with loudnessLufs (the profile does it), or level the voice first (bcp:audio-mix)",
            );
        }
        Some(_) => {}
        None => f("loudness", Severity::Blocking, a, b, json!(null), json!(l.lufs), "LUFS", "the mix is silent".into(), "check muted tracks and the mix"),
    }
    if let Some(tp) = m.true_peak_dbtp.filter(|tp| *tp > l.true_peak) {
        let sev = if tp > 0.0 { Severity::Blocking } else { Severity::Warning };
        f(
            "truePeak",
            sev,
            a,
            b,
            json!(round2(tp)),
            json!(l.true_peak),
            "dBTP",
            format!("true peak {tp:.1} dBTP over {:.1}", l.true_peak),
            "lower the loud clip or export with the profile's limiter",
        );
    }
    if m.clipped_samples > 0 {
        let sev = if m.clipped_samples >= 48 { Severity::Blocking } else { Severity::Warning };
        f(
            "clipping",
            sev,
            a,
            b,
            json!(m.clipped_samples),
            json!(0),
            "samples",
            format!("{} clipped samples in the mix", m.clipped_samples),
            "lower clip gain where the meters hit 0 dB",
        );
    }
    for (i, j, max) in bashcut_analysis::loudness::loud_stretches(&m.short_term, l.lufs + l.loud_over) {
        // entry i is the 3 s window ending at second i + 1
        let start = sec_tick(a, i.saturating_sub(2)).min(b);
        let end = sec_tick(a, j).min(b);
        f(
            "loudSegment",
            Severity::Warning,
            start,
            end,
            json!(round2(max)),
            json!(l.lufs + l.loud_over),
            "LUFS",
            format!("short-term loudness up to {max:.1} LUFS"),
            "lower this part (volume keyframes, bcp:audio-mix)",
        );
    }
    let mut run = 0usize;
    for (i, v) in m.short_term.iter().chain(std::iter::once(&Some(0.0))).enumerate() {
        if v.is_none_or(|v| v < l.silence_lufs) {
            run += 1;
            continue;
        }
        if run >= l.silence_seconds {
            let start = sec_tick(a, i - run).min(b);
            let end = sec_tick(a, i).min(b);
            f(
                "silence",
                Severity::Warning,
                start,
                end,
                json!(run),
                json!(l.silence_seconds),
                "seconds",
                format!("{run} s of silence"),
                "close the gap, or add room tone or music under it",
            );
        }
        run = 0;
    }
    let mut v = super::analysis::loudness_json(&m);
    v["truncated"] = json!(truncated);
    Ok(v)
}

// ---------------------------------------------------------------------------------------- picture

fn picture(s: &Session, q: &Sequence, a: Tick, b: Tick, per_second: f64, l: &Limits, out: &mut Vec<Finding>) -> Result<Value> {
    let (found, frames, failed, _) = picture_findings(s, a, b, per_second)?;
    let dur = q.duration();
    let edge = Tick::from_seconds_f64(0.5);
    // video transitions (a dip to black is meant to be black)
    let transitions: Vec<(Tick, Tick)> = q.video_tracks.iter().flat_map(|t| t.transitions.iter()).map(|t| (t.start, t.end())).collect();
    // a still picture is fine under someone talking: a screen recording under narration, a locked-off interview
    // (#178, #193, #196). In a short-form edit with nothing said over it, it is dead air on screen (#185: a vlog's end
    // card over 6 s of a near-still shot)
    let speech = speech_spans(s, q);
    let spoken = |a: Tick, b: Tick| spoken_share(&speech, a, b) >= SPOKEN_SHARE;
    for x in &found {
        use bashcut_analysis::picture::Kind;
        let (start, end) = (Tick(x.start), Tick(x.end));
        let at_edge = start <= edge || end >= dur - edge;
        let in_transition = transitions.iter().any(|(a, b)| start < *b && end > *a);
        let (check, severity, message, fix) = match x.kind {
            Kind::Black if at_edge => ("black", Severity::Info, "black at the start or end", "fine for a fade; else trim it"),
            Kind::Black if in_transition => ("black", Severity::Info, "black inside a transition", "fine for a dip to black; else change the transition"),
            Kind::Black => ("black", Severity::Blocking, "black frames inside the edit", "close the gap or fill it (sequence.closeGap)"),
            Kind::Frozen if spoken(start, end) => {
                ("stillUnderSpeech", Severity::Info, "a still picture under narration", "fine for a screen recording, slide or interview; else punch in or cut")
            }
            Kind::Frozen => ("frozen", Severity::Warning, "the picture does not move", "a frame hold or a stuck source: replace or cut it"),
            Kind::LongStill if l.short_form && !spoken(start, end) => (
                "longStill",
                Severity::Warning,
                "a still picture with nothing said over it",
                "cut it to 2-4 s, punch in or move it (Motion keyframes), or cover it with another shot (bcp:beat-cut)",
            ),
            Kind::LongStill => ("longStill", Severity::Info, "a long static shot", "fine for a locked-off interview; else punch in or cut (bcp:beat-cut)"),
            Kind::Flash => ("flash", Severity::Warning, "a sudden bright flash", "soften it (a dissolve) or cut it; flashes can hurt viewers"),
        };
        out.push(Finding {
            category: "picture",
            check,
            severity,
            start,
            end,
            measured: json!(round2(x.value)),
            limit: Value::Null,
            unit: "luma 0…1",
            message: message.into(),
            fix,
        });
    }
    let fd = q.settings.frame_rate.sane().frame_duration();
    let short = Tick(fd.0.saturating_mul(l.short_frames));
    let cue_gap = Tick(fd.0.saturating_mul(CUE_GAP_FRAMES));
    for t in &q.video_tracks {
        let mut items: Vec<_> = t.items.iter().collect();
        items.sort_by_key(|i| i.start);
        for (k, it) in items.iter().enumerate() {
            if it.end() <= a || it.start >= b {
                continue;
            }
            if it.duration < short {
                out.push(Finding {
                    category: "picture",
                    check: "shortClip",
                    severity: Severity::Warning,
                    start: it.start,
                    end: it.end(),
                    measured: json!(secs(it.duration)),
                    limit: json!(secs(short)),
                    unit: "seconds",
                    message: format!("clip {} is shorter than {} frames (a flash frame)", it.id.0, l.short_frames),
                    fix: "remove it or extend it",
                });
            }
            if let Some(next) = items.get(k + 1) {
                let gap = next.start - it.end();
                // between two caption cues (text graphics) a gap of 2 frames is the subtitle standard; only a shorter
                // one flickers (#214)
                let limit = if is_caption_cue(it) && is_caption_cue(next) { cue_gap } else { short };
                if gap > Tick::ZERO && gap < limit {
                    out.push(Finding {
                        category: "picture",
                        check: "shortGap",
                        severity: Severity::Warning,
                        start: it.end(),
                        end: next.start,
                        measured: json!(secs(gap)),
                        limit: json!(secs(short)),
                        unit: "seconds",
                        message: format!("a gap of {} frames between clips {} and {}", (gap.0 / fd.0.max(1)).max(1), it.id.0, next.id.0),
                        fix: "close it (sequence.closeGap) or roll the cut",
                    });
                }
            }
        }
    }
    Ok(json!({"frames": frames, "unrendered": failed}))
}

/// A still stretch is under speech when speech covers at least this share of it.
const SPOKEN_SHARE: f64 = 0.5;
/// Pauses up to twice this long inside narration count as speech (each word is widened by it on both sides).
const SPEECH_PAD_SECONDS: f64 = 0.5;

/// Where someone talks in the sequence: transcript words (widened over short pauses) and enabled caption cues (a
/// subtitled interview may have captions but no transcript), merged and sorted.
/// Gaps between caption cues shorter than this many frames flicker; 2 frames is the usual minimum between subtitles.
const CUE_GAP_FRAMES: i64 = 2;

/// A caption cue on a video track: a graphic clip of text layers only (`graphics.upgradeCaption`,
/// `bashcut.captions.wordStyle`).
fn is_caption_cue(it: &filmcraft_project::TrackItem) -> bool {
    let mut layers = it.effects.iter().filter(|e| filmcraft_project::graphic::is_layer(e)).peekable();
    layers.peek().is_some() && layers.all(|e| e.effect == filmcraft_project::graphic::TEXT_LAYER)
}

fn speech_spans(s: &Session, q: &Sequence) -> Vec<(Tick, Tick)> {
    let pad = Tick::from_seconds_f64(SPEECH_PAD_SECONDS);
    let mut spans: Vec<(Tick, Tick)> = crate::transcript::sequence_words(s).iter().map(|w| (w.start - pad, w.end + pad)).collect();
    for t in q.caption_tracks.iter().filter(|t| t.enabled) {
        spans.extend(t.captions.iter().filter(|c| !c.text.trim().is_empty()).map(|c| (c.start, c.end())));
    }
    spans.sort();
    let mut merged: Vec<(Tick, Tick)> = Vec::with_capacity(spans.len());
    for (a, b) in spans {
        match merged.last_mut() {
            Some(last) if a <= last.1 => last.1 = last.1.max(b),
            _ => merged.push((a, b)),
        }
    }
    merged
}

/// The share of `[a, b)` that `spans` (sorted, not overlapping) cover, 0…1.
fn spoken_share(spans: &[(Tick, Tick)], a: Tick, b: Tick) -> f64 {
    let len = b.0.saturating_sub(a.0);
    if len <= 0 {
        return 0.0;
    }
    let covered: i64 = spans.iter().map(|(x, y)| y.0.min(b.0).saturating_sub(x.0.max(a.0)).max(0)).fold(0i64, i64::saturating_add);
    covered as f64 / len as f64
}

// --------------------------------------------------------------------------------------- captions

fn captions(s: &Session, q: &Sequence, a: Tick, b: Tick, l: &Limits, out: &mut Vec<Finding>) -> Value {
    let mut count = 0usize;
    let (w, h) = (q.settings.width as usize, q.settings.height as usize);
    let (wf, hf) = (w as f64, h as f64);
    let insets = l.safe_area.as_ref().map(|(a, _)| Insets::of(a, wf, hf)).filter(|i| !i.is_empty());
    let profile = l.safe_area.as_ref().map(|(_, n)| n.as_str()).unwrap_or_default();
    let mut boxes = 0usize;
    for t in q.caption_tracks.iter().filter(|t| t.enabled) {
        let mut caps: Vec<_> = t.captions.iter().filter(|c| c.end() > a && c.start < b).collect();
        caps.sort_by_key(|c| c.start);
        for (k, c) in caps.iter().enumerate() {
            count += 1;
            let lines: Vec<&str> = c.text.lines().collect();
            let drawn = caption_box(c, &t.style, w, h);
            let chars = c.text.chars().filter(|ch| !ch.is_control()).count();
            let dur = c.duration.seconds();
            let mut f = |check, severity, measured: Value, limit: Value, unit, message: String, fix| {
                out.push(Finding { category: "captions", check, severity, start: c.start, end: c.end(), measured, limit, unit, message, fix })
            };
            if dur > 0.0 {
                let cps = chars as f64 / dur;
                if cps > l.cps_warn {
                    let sev = if cps > l.cps_block { Severity::Blocking } else { Severity::Warning };
                    f(
                        "readingSpeed",
                        sev,
                        json!(round2(cps)),
                        json!(l.cps_warn),
                        "characters a second",
                        format!("caption {} reads at {cps:.0} characters a second", c.id.0),
                        "show it longer, split it, or cut words (bcp:captions-text)",
                    );
                }
            }
            if let Some(longest) = lines.iter().map(|x| x.chars().count()).max().filter(|n| *n > l.line_chars) {
                f(
                    "lineLength",
                    Severity::Warning,
                    json!(longest),
                    json!(l.line_chars),
                    "characters",
                    format!("caption {} has a {longest}-character line", c.id.0),
                    "break the line or shorten it",
                );
            }
            // the lines the renderer draws: a long line wraps at the track's size (#205)
            let drawn_lines = drawn.map_or(lines.len(), |d| d.lines);
            if drawn_lines > l.lines {
                let wrapped = drawn_lines > lines.len();
                f(
                    "lines",
                    if wrapped { Severity::Blocking } else { Severity::Warning },
                    json!(drawn_lines),
                    json!(l.lines),
                    "lines",
                    if wrapped {
                        format!("caption {} draws {drawn_lines} lines: its {} text lines wrap at this caption size", c.id.0, lines.len())
                    } else {
                        format!("caption {} has {} lines", c.id.0, lines.len())
                    },
                    if wrapped {
                        "shorter lines (bashcut.captions.create maxChars: 20 on vertical, with captions.setStyle size <= 36) or a smaller size (captions.setStyle {size}; see limit.sizeAtMost in the safeArea finding)"
                    } else {
                        "split it into two captions"
                    },
                );
            }
            if let (Some(ins), Some(d)) = (insets, drawn) {
                boxes += 1;
                let into = ins.intrusion(&d, wf, hf);
                if let Some(msg) = band_message(&into, &ins, profile) {
                    // bottom band: the margin that puts the box's bottom edge 2 % above it; side bands: the largest size
                    // whose laid-out box (lines re-wrap at each size) clears them; top band: anchor or margin
                    let need = ((f64::from(t.style.margin) * hf + into[1]) / hf + 0.02).clamp(0.0, 0.45);
                    let size = fitting_size(c, &t.style, (w, h), &ins);
                    let mut limit = json!({"bandsPx": [ins.top.round(), ins.bottom.round(), ins.left.round(), ins.right.round()]});
                    let fix = if into[1] >= 1.0 {
                        limit["marginAtLeast"] = json!(round2(need));
                        "raise the captions: captions.setStyle {margin} (fraction of the height) to at least `limit.marginAtLeast`"
                    } else if into[2] >= 1.0 || into[3] >= 1.0 {
                        limit["sizeAtMost"] = json!(size);
                        "a smaller caption size (captions.setStyle {size} at most `limit.sizeAtMost`) or shorter lines (bashcut.captions.create maxChars)"
                    } else {
                        "move the captions out of the top band: captions.setStyle {anchor, margin}"
                    };
                    f("safeArea", Severity::Blocking, json!({"box": [d.x0, d.y0, d.x1, d.y1]}), limit, "pixels", format!("caption {} {msg}", c.id.0), fix);
                }
            }
            if let Some(next) = caps.get(k + 1).filter(|n| n.start < c.end()) {
                f(
                    "overlap",
                    Severity::Blocking,
                    json!(secs(c.end() - next.start)),
                    json!(0),
                    "seconds",
                    format!("captions {} and {} overlap", c.id.0, next.id.0),
                    "set the times so one ends before the next starts (captions.setTimes)",
                );
            }
        }
    }
    // titles and other text clips on the video tracks
    if let Some(ins) = insets {
        for x in text_boxes(s, q, a, b) {
            boxes += 1;
            let Some(msg) = band_message(&ins.intrusion(&x.bounds, wf, hf), &ins, profile) else { continue };
            let text: String = x.text.chars().take(40).collect();
            out.push(Finding {
                category: "captions",
                check: "safeArea",
                severity: Severity::Blocking,
                start: x.start.max(a),
                end: x.end.min(b),
                measured: json!({"box": [x.bounds.x0.round(), x.bounds.y0.round(), x.bounds.x1.round(), x.bounds.y1.round()], "clip": x.clip.0}),
                limit: json!({"bandsPx": [ins.top.round(), ins.bottom.round(), ins.left.round(), ins.right.round()]}),
                unit: "pixels",
                message: format!("text clip {} (\"{text}\") {msg}", x.clip.0),
                fix: "move it inside the safe area (graphics.set {position}, or the clip's Motion position)",
            });
        }
    }
    json!({"captions": count, "safeAreaBoxes": boxes})
}

/// The largest caption size (1080-line pixels, whole) at which `c`'s box clears the side bands, by bisection.
fn fitting_size(c: &filmcraft_project::Caption, style: &filmcraft_project::CaptionStyle, (w, h): (usize, usize), ins: &Insets) -> f64 {
    let clears = |size: f32| {
        let st = filmcraft_project::CaptionStyle { size, ..style.clone() };
        caption_box(c, &st, w, h).is_none_or(|d| {
            let i = ins.intrusion(&d, w as f64, h as f64);
            i[2] < 1.0 && i[3] < 1.0
        })
    };
    let (mut lo, mut hi) = (4.0f32, style.size.floor().max(4.0));
    if !clears(lo) {
        return f64::from(lo);
    }
    for _ in 0..12 {
        if hi - lo <= 1.0 {
            break;
        }
        let mid = ((lo + hi) / 2.0).floor();
        if clears(mid) { lo = mid } else { hi = mid }
    }
    f64::from(lo)
}

/// "sits 120 px inside the bottom 384 px of Reels" for the deepest band a box reaches into; `None` when clear.
fn band_message(into: &[f64; 4], ins: &Insets, profile: &str) -> Option<String> {
    let names = ["top", "bottom", "left", "right"];
    let bands = [ins.top, ins.bottom, ins.left, ins.right];
    let (k, depth) = into.iter().copied().enumerate().filter(|(_, v)| *v >= 1.0).max_by(|x, y| x.1.total_cmp(&y.1))?;
    Some(format!(
        "sits {:.0} px inside the {} {:.0} px of {profile} (platform buttons and text cover it)",
        depth,
        names.get(k).copied().unwrap_or("bottom"),
        bands.get(k).copied().unwrap_or_default()
    ))
}

// ------------------------------------------------------------------------------------------- edit

fn edit(s: &Session, q: &Sequence, a: Tick, b: Tick, l: &Limits, out: &mut Vec<Finding>) -> Value {
    let margin = Tick::from_seconds_f64(l.word_margin);
    let mut checked_edges = 0usize;
    for t in &q.audio_tracks {
        for it in t.items.iter().filter(|i| i.end() > a && i.start < b) {
            // only plain clips map sequence time to media time one to one
            if (it.speed - 1.0).abs() > 1e-9 || it.reverse {
                continue;
            }
            let Some(tr) = s.project.transcripts.get(&it.item) else { continue };
            let tail = it.source_in + it.duration;
            for (edge, at) in [(it.source_in, it.start), (tail, it.end())] {
                checked_edges += 1;
                let Some(w) = tr.words.iter().find(|w| edge > w.start + margin && edge < w.end - margin) else { continue };
                let start = at - (edge - w.start);
                let end = at + (w.end - edge);
                out.push(Finding {
                    category: "edit",
                    check: "cutInWord",
                    severity: Severity::Blocking,
                    start,
                    end,
                    measured: json!(w.text),
                    limit: Value::Null,
                    unit: "word",
                    message: format!("clip {} is cut inside the word \"{}\"", it.id.0, w.text),
                    fix: "move the cut to the word's edge or a pause (timeline.trim / timeline.roll)",
                });
            }
        }
    }
    // gaps on the main video track (V1) inside the edit
    let mut gaps = 0usize;
    if let Some(v1) = q.video_tracks.first() {
        let mut items: Vec<_> = v1.items.iter().collect();
        items.sort_by_key(|i| i.start);
        let mut cursor = items.first().map(|i| i.start).unwrap_or(Tick::ZERO).max(Tick::ZERO);
        for it in &items {
            if it.start > cursor && it.start > a && cursor < b {
                gaps += 1;
                out.push(Finding {
                    category: "edit",
                    check: "gap",
                    severity: Severity::Blocking,
                    start: cursor,
                    end: it.start,
                    measured: json!(secs(it.start - cursor)),
                    limit: json!(0),
                    unit: "seconds",
                    message: "a gap on V1 (black on screen)".into(),
                    fix: "close it (sequence.closeGap) or fill it",
                });
            }
            cursor = cursor.max(it.end());
        }
    }
    // offline media: every item a clip in the range plays
    let mut items: Vec<ItemId> =
        q.video_tracks.iter().chain(&q.audio_tracks).flat_map(|t| t.items.iter()).filter(|i| i.end() > a && i.start < b).map(|i| i.item).collect();
    items.sort_by_key(|i| i.0);
    items.dedup();
    let mut offline = 0usize;
    for item in items {
        if s.project.item(item).is_some_and(|p| matches!(p.kind, filmcraft_project::ItemKind::Media(_))) && s.source(item).is_none() {
            offline += 1;
            let first = q.video_tracks.iter().chain(&q.audio_tracks).flat_map(|t| t.items.iter()).filter(|i| i.item == item).min_by_key(|i| i.start);
            let (start, end) = first.map(|i| (i.start, i.end())).unwrap_or((a, b));
            let name = s.project.item(item).map(|p| p.name.clone()).unwrap_or_default();
            out.push(Finding {
                category: "edit",
                check: "offline",
                severity: Severity::Blocking,
                start,
                end,
                measured: json!(name),
                limit: Value::Null,
                unit: "item",
                message: format!("media \"{name}\" is offline"),
                fix: "relink it (Link Media) or replace the clip",
            });
        }
    }
    json!({"cutEdgesChecked": checked_edges, "gaps": gaps, "offline": offline})
}

// -------------------------------------------------------------------------------------- the gate

fn score(found: &[Finding], category: &str) -> u32 {
    let lost: u32 = found
        .iter()
        .filter(|f| f.category == category)
        .map(|f| match f.severity {
            Severity::Blocking => 25,
            Severity::Warning => 8,
            Severity::Info => 2,
        })
        .sum();
    100u32.saturating_sub(lost)
}

const CATEGORIES: [&str; 4] = ["sound", "picture", "captions", "edit"];

fn run(s: &mut Session, p: &Value) -> Result<Value> {
    if let Some(v) = p.get("sequence").filter(|v| !v.is_null()) {
        let id = v.as_u64().ok_or_else(|| bad(CMD, "`sequence` must be a sequence id"))?;
        if s.state.active_sequence != Some(ItemId(id)) {
            return Err(fail(Code::Refused, CMD, format!("sequence {id} is not the active sequence; open it first (sequence.open)")));
        }
    }
    let mut l = Limits::default();
    let mut profile_id = Value::Null;
    if let Some(key) = p.get("profile").filter(|v| !v.is_null()) {
        let key = key.as_str().ok_or_else(|| bad(CMD, "`profile` must be a profile id or name"))?;
        let pr = super::profiles::find(s, p, key).ok_or_else(|| fail(Code::NotFound, CMD, format!("no output profile `{key}` (see bashcut.profiles.list)")))?;
        l.lufs = pr.loudness_lufs;
        l.true_peak = pr.true_peak_dbtp;
        l.short_form = pr.height > pr.width;
        l.safe_area = Some((pr.safe_area, pr.name.clone()));
        profile_id = json!(pr.id);
    }
    let checks: Vec<&str> = match p.get("checks") {
        None | Some(Value::Null) => CATEGORIES.to_vec(),
        Some(Value::Array(a)) => {
            let mut v = Vec::new();
            for c in a {
                let name = c.as_str().and_then(|n| CATEGORIES.iter().find(|k| **k == n).copied());
                v.push(name.ok_or_else(|| bad(CMD, format!("unknown check {c}; use sound, picture, captions, edit")))?);
            }
            v
        }
        Some(_) => return Err(bad(CMD, "`checks` must be a list: [\"sound\", \"picture\", \"captions\", \"edit\"]")),
    };
    let write = match p.get("writeMarkers") {
        None | Some(Value::Null) => true,
        Some(v) => v.as_bool().ok_or_else(|| bad(CMD, "`writeMarkers` must be true or false"))?,
    };
    let per_second = match p.get("samplesPerSecond") {
        None | Some(Value::Null) => 5.0,
        Some(v) => v.as_f64().filter(|f| f.is_finite() && (0.5..=60.0).contains(f)).ok_or_else(|| bad(CMD, "`samplesPerSecond` must be 0.5 … 60"))?,
    };
    let q = s.active_sequence().ok_or(EngineError::NoSequence)?.clone();
    if profile_id.is_null() {
        l.short_form = q.settings.height > q.settings.width;
    }
    let (a, b) = range_p(p, CMD, Tick::ZERO, q.duration())?;

    let mut found = Vec::new();
    let mut measured = serde_json::Map::new();
    for c in &checks {
        let v = match *c {
            "sound" => sound(s, a, b, &l, &mut found)?,
            "picture" => picture(s, &q, a, b, per_second, &l, &mut found)?,
            "captions" => captions(s, &q, a, b, &l, &mut found),
            _ => edit(s, &q, a, b, &l, &mut found),
        };
        measured.insert((*c).into(), v);
    }
    found.sort_by_key(|f| (f.start, f.severity, f.check));

    let mut markers = 0usize;
    if write {
        let list: Vec<Finding> = found.iter().take(MAX_MARKERS).cloned().collect();
        markers = list.len();
        s.edit_sequence("Review Markers", |q, ctx, _| {
            q.markers.retain(|m| !m.name.starts_with(MARKER_PREFIX));
            for f in &list {
                q.markers.push(Marker {
                    id: MarkerId(ctx.alloc()),
                    start: f.start,
                    duration: (f.end - f.start).max(Tick::ZERO),
                    name: format!("{MARKER_PREFIX}{} {}", f.severity.name(), f.check),
                    comment: format!("{} (measured {} {}). Fix: {}", f.message, f.measured, f.unit, f.fix),
                    kind: MarkerKind::Comment,
                    color: f.severity.color(),
                });
            }
            q.markers.sort_by_key(|m| m.start);
            Ok(())
        })?;
    }
    let mut skipped = vec![
        json!({"check": "sound.musicOverSpeech", "why": "needs voice and music stems: bcp:audio-mix loudness.py --speech"}),
        json!({"check": "captions.contrast", "why": "needs the rendered caption boxes: render_frame at phone size"}),
    ];
    if l.safe_area.is_none() {
        skipped.push(json!({"check": "captions.safeArea", "why": "pass a `profile` (bashcut.profiles.list): its safe area is the limit"}));
    }
    let blocking = found.iter().filter(|f| f.severity == Severity::Blocking).count();
    let warnings = found.iter().filter(|f| f.severity == Severity::Warning).count();
    let scores: serde_json::Map<String, Value> = checks.iter().map(|c| ((*c).to_string(), json!(score(&found, c)))).collect();
    Ok(json!({
        "ok": blocking == 0,
        "verdict": if blocking == 0 { "pass" } else { "needsFix" },
        "blocking": blocking,
        "warnings": warnings,
        "info": found.len() - blocking - warnings,
        "scores": scores,
        "profile": profile_id,
        "start": secs(a),
        "end": secs(b),
        "findings": found.iter().map(Finding::json).collect::<Vec<_>>(),
        "measured": measured,
        "markers": markers,
        "skipped": skipped,
        "units": {"time": "seconds", "loudness": "LUFS", "peak": "dBTP"},
    }))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Review Edit",
        menu: &[],
        shortcut: None,
        params: r#"{"profile":str? (loudness target and caption safe area from bashcut.profiles.*; default -14 LUFS / -1 dBTP, no safe area),"checks":["sound"|"picture"|"captions"|"edit"]?,"range":{"start":seconds?,"end":seconds?}?,"writeMarkers":bool=true,"samplesPerSecond":0.5..60=5,"sequence":id? (must be the active one)} (findings in seconds; markers replace the previous review's)"#,
        enabled: has_seq,
        run,
        journal: true,
    }]
}
