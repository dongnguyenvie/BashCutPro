//! Read-only measurements (#36) through `bashcut-analysis`:
//!
//! | command | what it does |
//! |---|---|
//! | `bashcut.audio.loudness` | integrated / short-term loudness, true peak and clipping of a clip, a track's input or the sequence mix |
//! | `bashcut.audio.beats` | tempo, beats and downbeats of an audio clip, in sequence seconds; `writeMarkers` adds them as sequence markers (one undo step) |
//! | `bashcut.review.pictureChecks` | black, frozen, long-still and flash stretches of the sequence picture |
//!
//! Times in params and results are seconds; loudness in LUFS / LU, peaks in dBTP / dBFS.

use bashcut_analysis::{beats, loudness, picture};
use filmcraft_project::{ClipId, Label, Marker, MarkerId, MarkerKind, TrackId, TrackKind};
use filmcraft_render::audio::{clip_is_mono, clip_signal, mix_sequence, track_input};
use filmcraft_time::{TICKS_PER_SECOND, Tick};
use serde_json::{Value, json};

use crate::commands::{CommandSpec, bad, bool_p, has_seq, track_p};
use crate::{EngineError, Result, Session};

/// Longest stretch one call measures (4 hours); the rest is left out and the result says so.
const MAX_SPAN_SECONDS: i64 = 4 * 3600;
/// At most this many beat markers are written.
const MAX_MARKERS: usize = 5_000;
/// At most this many frames are rendered for picture checks (the sampling step grows to fit).
const MAX_FRAMES: i64 = 20_000;
/// Frames are rendered about this wide for picture checks.
const CHECK_WIDTH: f32 = 160.0;

/// Seconds, rounded to the millisecond, for results.
pub(crate) fn secs(t: Tick) -> f64 {
    (t.seconds() * 1000.0).round() / 1000.0
}

fn round2(v: Option<f64>) -> Option<f64> {
    v.map(|v| (v * 100.0).round() / 100.0)
}

/// `range: {"start": seconds?, "end": seconds?}` clipped to `[lo, hi)`; absent = `[lo, hi)`.
pub(crate) fn range_p(p: &Value, cmd: &str, lo: Tick, hi: Tick) -> Result<(Tick, Tick)> {
    let r = match p.get("range") {
        None | Some(Value::Null) => return non_empty(cmd, lo, hi),
        Some(r) if r.is_object() => r,
        Some(_) => return Err(bad(cmd, "`range` must be {\"start\": seconds, \"end\": seconds}")),
    };
    let sec = |k: &str| -> Result<Option<Tick>> {
        match r.get(k) {
            None | Some(Value::Null) => Ok(None),
            Some(v) => v
                .as_f64()
                .filter(|f| f.is_finite() && (0.0..1e7).contains(f))
                .map(|f| Some(Tick::from_seconds_f64(f)))
                .ok_or_else(|| bad(cmd, format!("`range.{k}` must be a number of seconds ≥ 0"))),
        }
    };
    let a = sec("start")?.unwrap_or(lo).max(lo);
    let b = sec("end")?.unwrap_or(hi).min(hi);
    non_empty(cmd, a, b)
}

fn non_empty(cmd: &str, a: Tick, b: Tick) -> Result<(Tick, Tick)> {
    if b <= a { Err(bad(cmd, "nothing to measure: the range is empty (end must be after start, inside the clip or sequence)")) } else { Ok((a, b)) }
}

/// What a loudness measurement covers.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum Target {
    Clip(ClipId),
    Track(TrackId),
    Mix,
}

impl Target {
    fn name(self) -> &'static str {
        match self {
            Target::Clip(_) => "clip",
            Target::Track(_) => "track",
            Target::Mix => "mix",
        }
    }
}

/// The sample frames `[a, b)` of the active sequence (sequence rate), capped at
/// [`MAX_SPAN_SECONDS`]: (first, end, truncated).
fn sample_span(a: Tick, b: Tick, sr: u32) -> (i64, i64, bool) {
    let a0 = a.to_units_floor(sr as i64);
    let a1 = b.to_units_floor(sr as i64);
    let cap = a0.saturating_add(MAX_SPAN_SECONDS.saturating_mul(sr as i64));
    (a0, a1.min(cap), a1 > cap)
}

/// The time range a target covers by default: a clip's own range, else the whole sequence.
pub(crate) fn target_range(s: &Session, t: Target, cmd: &str) -> Result<(Tick, Tick)> {
    let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
    match t {
        Target::Clip(c) => {
            let (_, it) = seq.find_item(c).ok_or_else(|| bad(cmd, format!("no clip {} in the active sequence", c.0)))?;
            Ok((it.start, it.end()))
        }
        Target::Track(_) | Target::Mix => Ok((Tick::ZERO, seq.duration())),
    }
}

/// Loudness of `target` over `[a, b)`: (measurement, truncated). The measurement is `None` when the
/// target has no audio to read.
pub(crate) fn measure(s: &Session, target: Target, a: Tick, b: Tick, cmd: &str) -> Result<(Option<loudness::Loudness>, bool)> {
    let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let sr = seq.settings.sample_rate.max(1);
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    let (first, end, truncated) = sample_span(a, b, sr);
    let mut meter: Option<loudness::Meter> = None;
    let mut pos = first;
    while pos < end {
        let n = (end - pos).min(sr as i64) as usize;
        let block: Vec<Vec<f32>> = match target {
            Target::Clip(c) => {
                let (t, it) = seq.find_item(c).ok_or_else(|| bad(cmd, format!("no clip {} in the active sequence", c.0)))?;
                if seq.track(t).is_none_or(|t| t.kind != TrackKind::Audio) {
                    return Err(bad(cmd, format!("clip {} is not on an audio track", c.0)));
                }
                let Some(mono) = clip_is_mono(it, &provider) else { return Ok((None, truncated)) };
                let Some([l, r]) = clip_signal(it, pos, n, sr, &provider) else { return Ok((None, truncated)) };
                if mono { vec![l.iter().zip(&r).map(|(x, y)| 0.5 * (x + y)).collect()] } else { vec![l, r] }
            }
            Target::Track(id) => {
                let track = seq.track(id).ok_or_else(|| bad(cmd, format!("no track {} in the active sequence", id.0)))?;
                if track.kind != TrackKind::Audio {
                    return Err(bad(cmd, "`track` must be an audio track (A1, A2, … or its id)"));
                }
                track_input(&s.project, track, pos, n, sr, &provider).channels
            }
            Target::Mix => mix_sequence(&s.project, seq, pos, n, &provider).channels,
        };
        if block.is_empty() {
            break;
        }
        let refs: Vec<&[f32]> = block.iter().map(Vec::as_slice).collect();
        meter.get_or_insert_with(|| loudness::Meter::new(sr, refs.len())).push(&refs);
        pos = pos.saturating_add(n as i64);
    }
    Ok((meter.map(|m| m.finish()), truncated))
}

/// The target named by `clip` / `track` (clip first), else the sequence mix.
fn target_p(s: &Session, p: &Value, cmd: &str) -> Result<Target> {
    if let Some(v) = p.get("clip").filter(|v| !v.is_null()) {
        let c = v.as_u64().ok_or_else(|| bad(cmd, "`clip` must be a clip id"))?;
        return Ok(Target::Clip(ClipId(c)));
    }
    Ok(track_p(s, p, "track", cmd)?.map(Target::Track).unwrap_or(Target::Mix))
}

pub(crate) fn loudness_json(l: &loudness::Loudness) -> Value {
    json!({
        "integratedLufs": round2(l.integrated_lufs),
        "loudnessRangeLu": round2(Some(l.loudness_range_lu)),
        "maxMomentaryLufs": round2(l.max_momentary_lufs),
        "maxShortTermLufs": round2(l.max_short_term_lufs),
        "truePeakDbtp": round2(l.true_peak_dbtp),
        "samplePeakDbfs": round2(l.sample_peak_dbfs),
        "clippedSamples": l.clipped_samples,
        "silent": l.integrated_lufs.is_none(),
    })
}

fn loudness_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "bashcut.audio.loudness";
    let target = target_p(s, p, cmd)?;
    let (lo, hi) = target_range(s, target, cmd)?;
    let (a, b) = range_p(p, cmd, lo, hi)?;
    let (l, truncated) = measure(s, target, a, b, cmd)?;
    let l = l.unwrap_or_default();
    let mut v = loudness_json(&l);
    v["scope"] = json!(target.name());
    if let Target::Clip(c) = target {
        v["clip"] = json!(c.0);
    }
    if let Target::Track(t) = target {
        v["track"] = json!(t.0);
    }
    v["start"] = json!(secs(a));
    v["end"] = json!(secs(b));
    v["truncated"] = json!(truncated);
    if bool_p(p, "series").unwrap_or(false) {
        v["shortTerm"] = json!(l.short_term.iter().map(|x| round2(*x)).collect::<Vec<_>>());
        v["shortTermNote"] = json!("entry i = the 3 s window ending at start + i + 1 seconds");
    }
    v["units"] = json!({"time": "seconds", "loudness": "LUFS", "range": "LU", "peak": "dBTP (true) / dBFS (sample)"});
    Ok(v)
}

/// The audio clip `clip` names, else the first selected clip on an audio track.
fn audio_clip_p(s: &Session, p: &Value, cmd: &str) -> Result<ClipId> {
    let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let c = match p.get("clip").filter(|v| !v.is_null()) {
        Some(v) => ClipId(v.as_u64().ok_or_else(|| bad(cmd, "`clip` must be a clip id"))?),
        None => s
            .state
            .selection
            .iter()
            .copied()
            .find(|c| seq.find_item(*c).and_then(|(t, _)| seq.track(t)).is_some_and(|t| t.kind == TrackKind::Audio))
            .ok_or_else(|| bad(cmd, "select an audio clip or pass `clip`"))?,
    };
    let (t, _) = seq.find_item(c).ok_or_else(|| bad(cmd, format!("no clip {} in the active sequence", c.0)))?;
    if seq.track(t).is_none_or(|t| t.kind != TrackKind::Audio) {
        return Err(bad(cmd, format!("clip {} is not on an audio track", c.0)));
    }
    Ok(c)
}

fn beats_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "bashcut.audio.beats";
    let write = match p.get("writeMarkers") {
        None | Some(Value::Null) => false,
        Some(v) => v.as_bool().ok_or_else(|| bad(cmd, "`writeMarkers` must be true or false"))?,
    };
    let c = audio_clip_p(s, p, cmd)?;
    let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let sr = seq.settings.sample_rate.max(1);
    let (_, it) = seq.find_item(c).ok_or_else(|| bad(cmd, format!("no clip {} in the active sequence", c.0)))?;
    let start = it.start;
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    let (first, end, _) = sample_span(it.start, it.end(), sr);
    let cap = first.saturating_add(beats::MAX_SAMPLES as i64);
    let (end, truncated) = (end.min(cap), end > cap);
    let (mut l, mut r) = (Vec::new(), Vec::new());
    let mut pos = first;
    while pos < end {
        let n = (end - pos).min(sr as i64) as usize;
        let Some([a, b]) = clip_signal(it, pos, n, sr, &provider) else { break };
        l.extend(a);
        r.extend(b);
        pos = pos.saturating_add(n as i64);
    }
    let at = |k: i64| start + Tick::from_units(k, sr as i64);
    let Some(found) = beats::detect(&[&l, &r], sr) else {
        return Ok(json!({
            "clip": c.0, "found": false, "bpm": null, "beats": [], "downbeats": [], "markers": 0, "truncated": truncated,
            "message": "no steady beat found in this clip",
            "units": {"time": "sequence seconds", "tempo": "BPM"},
        }));
    };
    let beats_t: Vec<Tick> = found.beats.iter().map(|k| at(*k)).collect();
    let downs: Vec<Tick> = found.downbeats.iter().map(|k| at(*k)).collect();
    let mut markers = 0usize;
    let mut changed = Value::Null;
    if write {
        let before = super::summary::active(s);
        let bpm = found.bpm;
        let list: Vec<(Tick, bool)> = beats_t.iter().take(MAX_MARKERS).map(|t| (*t, downs.contains(t))).collect();
        markers = list.len();
        s.edit_sequence("Beat Markers", |q, ctx, _| {
            for (t, down) in &list {
                q.markers.push(Marker {
                    id: MarkerId(ctx.alloc()),
                    start: *t,
                    duration: Tick::ZERO,
                    name: if *down { "Downbeat".into() } else { "Beat".into() },
                    comment: format!("bashcut.audio.beats: {bpm:.1} BPM"),
                    kind: MarkerKind::Comment,
                    color: if *down { Label::Rose } else { Label::Yellow },
                });
            }
            Ok(())
        })?;
        if let (Some(b), Some(a)) = (before, super::summary::active(s)) {
            changed = super::summary::changes(&b, &a);
        }
    }
    Ok(json!({
        "clip": c.0,
        "found": true,
        "bpm": (found.bpm * 100.0).round() / 100.0,
        "confidence": (found.confidence * 1000.0).round() / 1000.0,
        "beats": beats_t.iter().map(|t| secs(*t)).collect::<Vec<_>>(),
        "downbeats": downs.iter().map(|t| secs(*t)).collect::<Vec<_>>(),
        "markers": markers,
        "changed": changed,
        "truncated": truncated,
        "units": {"time": "sequence seconds", "tempo": "BPM"},
    }))
}

/// Picture findings over `[a, b)` of the active sequence: (findings, frames rendered, frames that
/// could not be rendered, sampling step).
pub(crate) fn picture_findings(s: &Session, a: Tick, b: Tick, per_second: f64) -> Result<(Vec<picture::Finding>, usize, usize, Tick)> {
    let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
    let scale = (CHECK_WIDTH / seq.settings.width.max(1) as f32).clamp(0.01, 1.0);
    let span = b.0.saturating_sub(a.0).max(1);
    let mut step = Tick::from_seconds_f64(1.0 / per_second).0.max(1);
    if span / step > MAX_FRAMES {
        step = span / MAX_FRAMES + 1;
    }
    let mut samples = Vec::new();
    let mut failed = 0usize;
    let mut t = a;
    while t < b {
        let thumb = s.render_program_at(scale, t).and_then(|img| picture::Thumb::from_rgba8(img.w, img.h, &img.over_black_rgba8()));
        match thumb {
            Some(thumb) => samples.push(picture::Sample { time: t.0, thumb }),
            None => failed += 1,
        }
        t = Tick(t.0.saturating_add(step));
    }
    let found = picture::check(&samples, b.0, &picture::Options::per_second(TICKS_PER_SECOND));
    Ok((found, samples.len(), failed, Tick(step)))
}

pub(crate) fn finding_json(f: &picture::Finding) -> Value {
    json!({
        "kind": f.kind.name(),
        "start": secs(Tick(f.start)),
        "end": secs(Tick(f.end)),
        "seconds": secs(Tick(f.end.saturating_sub(f.start))),
        "value": (f.value * 1000.0).round() / 1000.0,
    })
}

fn picture_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "bashcut.review.pictureChecks";
    let dur = s.active_sequence().ok_or(EngineError::NoSequence)?.duration();
    let (a, b) = range_p(p, cmd, Tick::ZERO, dur)?;
    let per_second = match p.get("samplesPerSecond") {
        None | Some(Value::Null) => 5.0,
        Some(v) => v.as_f64().filter(|f| f.is_finite() && (0.5..=60.0).contains(f)).ok_or_else(|| bad(cmd, "`samplesPerSecond` must be 0.5 … 60"))?,
    };
    let (found, rendered, failed, step) = picture_findings(s, a, b, per_second)?;
    let count = |k: picture::Kind| found.iter().filter(|f| f.kind == k).count();
    Ok(json!({
        "start": secs(a),
        "end": secs(b),
        "frames": rendered,
        "unrendered": failed,
        "stepSeconds": secs(step),
        "findings": found.iter().map(finding_json).collect::<Vec<_>>(),
        "counts": {
            "black": count(picture::Kind::Black),
            "frozen": count(picture::Kind::Frozen),
            "longStill": count(picture::Kind::LongStill),
            "flash": count(picture::Kind::Flash),
        },
        "units": {"time": "seconds", "value": "luma 0…1 (black: brightest mean; frozen / still: largest change; flash: jump)"},
    }))
}

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![
        CommandSpec {
            id: "bashcut.audio.loudness",
            label: "Measure Loudness",
            menu: &[],
            shortcut: None,
            params: r#"{"clip":id?,"track":id|"A1"?,"range":{"start":seconds?,"end":seconds?}?,"series":bool?} (no clip / track = the sequence mix; result in LUFS, LU, dBTP, seconds)"#,
            enabled: has_seq,
            run: loudness_cmd,
            journal: false,
        },
        CommandSpec {
            id: "bashcut.audio.beats",
            label: "Detect Beats",
            menu: &[],
            shortcut: None,
            params: r#"{"clip":id?,"writeMarkers":bool?} (default: the selected audio clip; result in sequence seconds and BPM)"#,
            enabled: has_seq,
            run: beats_cmd,
            journal: true,
        },
        CommandSpec {
            id: "bashcut.review.pictureChecks",
            label: "Picture Checks",
            menu: &[],
            shortcut: None,
            params: r#"{"range":{"start":seconds?,"end":seconds?}?,"samplesPerSecond":0.5..60=5} (black, frozen, long-still and flash stretches; result in seconds)"#,
            enabled: has_seq,
            run: picture_cmd,
            journal: false,
        },
    ]
}
