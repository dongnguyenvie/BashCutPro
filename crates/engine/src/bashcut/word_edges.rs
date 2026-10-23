//! BashCutPro: word edges moved out of the pauses Whisper hides (#203). `crates/speech` stays as upstream ships it.
//!
//! Whisper starts the word after a pause where the previous word ended ("panel" 1.40–1.90, "Now" 1.90–5.34 over a
//! silence from 2.0 to 5.2 s). With no gaps between words, `transcript.removePauses` finds nothing, the review
//! flags cuts made in the silence as `cutInWord`, and captions show seconds before the word is said.
//!
//! `bashcut.transcript.refineEdges` measures voice activity on each transcribed item's audio (short-term energy in
//! 10 ms frames, 16 kHz mono, read in one-minute chunks). A frame is quiet below a threshold 30 % of the way from
//! the noise floor (10th percentile) to the speech level (90th percentile). A run of quiet frames at least
//! `minSilenceSeconds` (0.3) long is a pause. A word holding a pause keeps the side with more voice: its start moves
//! to the end of the pause, or its end to the start of it. Words only shrink, so they never overlap. An item whose
//! loudness has no clear speech/noise split (under 10 dB apart) is left alone. `bashcut.transcript.generate` runs it
//! after recognition.

use std::sync::Arc;

use filmcraft_project::{ItemId, ItemKind, Transcript, Word};
use filmcraft_time::Tick;
use serde_json::{Value, json};

use super::analysis::secs;
use crate::commands::{CommandSpec, always, bad, bool_p, f64_p};
use crate::{Result, Session};

const CMD: &str = "bashcut.transcript.refineEdges";
const UNDO_LABEL: &str = "Refine Word Edges";
/// Analysis sample rate and frame length.
const RATE: u32 = 16_000;
const FRAME: usize = 160;
/// Samples read at a time (one minute).
const CHUNK: usize = RATE as usize * 60;
/// Longest audio analysed per item (6 h of 10 ms frames).
const MAX_FRAMES: usize = 6 * 3600 * 100;
/// Quietest frame level counted, dBFS (digital silence).
const FLOOR_DB: f32 = -100.0;
/// The speech level must be this far above the noise floor to tell pauses from words, dB.
const MIN_SPREAD_DB: f32 = 10.0;
/// Where between the noise floor and the speech level a frame counts as quiet.
const QUIET_AT: f32 = 0.3;
/// At most this many moves are listed per item.
const MAX_LISTED: usize = 20;

pub(crate) fn commands() -> Vec<CommandSpec> {
    vec![spec! {
        id: CMD,
        label: "Refine Word Edges from Audio",
        params: r#"{"items":[id]? (default every transcript a recogniser made),"minSilenceSeconds":f=0.3 (0.1..5),"dryRun":bool=false}  (moves word edges out of pauses measured on the audio; one undo step)"#,
        enabled: always,
        run: refine_cmd,
        journal: true,
    }]
}

/// Level of each 10 ms frame, dBFS (at most [`MAX_FRAMES`]).
pub fn frame_levels(mono: &[f32]) -> Vec<f32> {
    mono.chunks(FRAME).take(MAX_FRAMES).map(level_db).collect()
}

fn level_db(f: &[f32]) -> f32 {
    if f.is_empty() {
        return FLOOR_DB;
    }
    let ms = f.iter().map(|v| if v.is_finite() { v * v } else { 0.0 }).sum::<f32>() / f.len() as f32;
    if ms > 0.0 { (10.0 * ms.log10()).max(FLOOR_DB) } else { FLOOR_DB }
}

/// Pauses `(start, end)` in frames: runs of quiet frames at least `min_frames` long. Empty when the levels show no
/// clear split between speech and noise.
pub fn pauses(levels: &[f32], min_frames: usize) -> Vec<(usize, usize)> {
    let mut sorted: Vec<f32> = levels.to_vec();
    sorted.sort_by(f32::total_cmp);
    let pct = |p: usize| sorted.get(sorted.len().saturating_sub(1) * p / 100).copied().unwrap_or(FLOOR_DB);
    let (noise, speech) = (pct(10), pct(90));
    if speech - noise < MIN_SPREAD_DB {
        return Vec::new();
    }
    let quiet = noise + (speech - noise) * QUIET_AT;
    let mut out = Vec::new();
    let mut run: Option<usize> = None;
    for (i, l) in levels.iter().chain(std::iter::once(&f32::MAX)).enumerate() {
        match (run, *l < quiet) {
            (None, true) => run = Some(i),
            (Some(a), false) => {
                if i - a >= min_frames.max(1) {
                    out.push((a, i));
                }
                run = None;
            }
            _ => {}
        }
    }
    out
}

/// Words with their edges moved out of `pauses` (in ticks, sorted). Returns the words and how many moved.
pub fn refine(words: &[Word], pauses: &[(Tick, Tick)]) -> (Vec<Word>, Vec<(usize, Tick, Tick)>) {
    let mut out = words.to_vec();
    let mut moved = Vec::new();
    for (k, w) in out.iter_mut().enumerate() {
        let (s0, e0) = (w.start, w.end);
        // a word can hold several pauses: drop one at a time, the longest first
        for _ in 0..8 {
            let inside =
                pauses.iter().filter(|(a, b)| *a < w.end && *b > w.start).map(|(a, b)| ((*a).max(w.start), (*b).min(w.end))).max_by_key(|(a, b)| *b - *a);
            let Some((a, b)) = inside else { break };
            let (before, after) = (a - w.start, w.end - b);
            if before <= Tick::ZERO && after <= Tick::ZERO {
                // the pause covers the whole word: keep it as the recogniser placed it
                break;
            }
            if after >= before {
                w.start = b;
            } else {
                w.end = a;
            }
        }
        if (w.start, w.end) != (s0, e0) {
            moved.push((k, w.start, w.end));
        }
    }
    (out, moved)
}

/// Pauses in an item's audio, media ticks; `None` when it has no audio.
fn item_pauses(s: &Session, item: ItemId, min_silence: f64) -> Option<Vec<(Tick, Tick)>> {
    let dur = match &s.project.item(item)?.kind {
        ItemKind::Media(m) => m.duration(),
        _ => return None,
    };
    let src = s.source(item)?;
    if !src.info().has_audio() {
        return None;
    }
    let total = dur.to_units_floor(i64::from(RATE)).clamp(0, (MAX_FRAMES * FRAME) as i64) as usize;
    let mut levels = Vec::with_capacity(total / FRAME + 1);
    let mut at = 0usize;
    while at < total {
        let len = CHUNK.min(total - at);
        let buf = src.audio(at as i64, len, RATE).ok()?;
        levels.extend(frame_levels(&filmcraft_speech::downmix(&buf.channels)));
        at += len;
    }
    let min_frames = (min_silence * 100.0).round().max(1.0) as usize;
    let frame = |f: usize| Tick::from_units((f * FRAME) as i64, i64::from(RATE));
    Some(pauses(&levels, min_frames).into_iter().map(|(a, b)| (frame(a), frame(b))).collect())
}

/// Refine `items`' transcripts; one undo step when any word moved. Returns the per-item report.
pub(crate) fn refine_items(s: &mut Session, items: &[ItemId], min_silence: f64, dry_run: bool) -> Result<Value> {
    let mut report = Vec::new();
    let mut changed: Vec<(ItemId, Transcript)> = Vec::new();
    for &item in items {
        let Some(t) = s.project.transcripts.get(&item).cloned() else { continue };
        let Some(found) = item_pauses(s, item, min_silence) else {
            report.push(json!({"item": item.0, "skipped": "no audio"}));
            continue;
        };
        let (words, moved) = refine(&t.words, &found);
        let listed: Vec<Value> = moved
            .iter()
            .take(MAX_LISTED)
            .filter_map(|(k, a, b)| {
                let w = t.words.get(*k)?;
                Some(json!({"word": w.text, "was": [secs(w.start), secs(w.end)], "now": [secs(*a), secs(*b)]}))
            })
            .collect();
        report.push(json!({"item": item.0, "words": t.words.len(), "pauses": found.len(), "moved": moved.len(), "moves": listed}));
        if !moved.is_empty() {
            let mut t2 = (*t).clone();
            t2.words = words;
            changed.push((item, t2));
        }
    }
    if !changed.is_empty() && !dry_run {
        s.edit(UNDO_LABEL, move |pr, _| {
            for (i, t) in changed {
                pr.transcripts.insert(i, Arc::new(t));
            }
            Ok(())
        })?;
    }
    Ok(json!({"items": report, "dryRun": dry_run, "units": {"time": "media seconds"}}))
}

/// `minSilenceSeconds`, checked.
pub(crate) fn min_silence_p(p: &Value, cmd: &str) -> Result<f64> {
    match f64_p(p, "minSilenceSeconds") {
        None => Ok(0.3),
        Some(v) if v.is_finite() && (0.1..=5.0).contains(&v) => Ok(v),
        Some(_) => Err(bad(cmd, "`minSilenceSeconds` must be 0.1 … 5")),
    }
}

fn refine_cmd(s: &mut Session, p: &Value) -> Result<Value> {
    let min_silence = min_silence_p(p, CMD)?;
    let items = super::transcript_clean::targets(s, p);
    refine_items(s, &items, min_silence, bool_p(p, "dryRun").unwrap_or(false))
}
