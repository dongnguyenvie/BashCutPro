//! BashCutPro: one-knob voice polish (#48).
//!
//! `bashcut.audio.polish` appends one ordered chain of FilmCraft's own audio effects to each dialogue clip
//! (Highpass → DeNoise → DeEsser → Single-band Compressor → Hard Limiter), every stage scaled by one `amount`
//! (0–1), then matches the clip's loudness to a target with `essentialSound.autoMatch`: `target` (LUFS), else the
//! output profile's loudness (`profile`, #43), else the dialogue target from the preferences. Everything is one undo
//! step. `amount` 0 changes nothing. With `savePreset`, the chain is also saved as an effect preset
//! (`BashCut Voice Polish NN%`) through `presets.save`, so the user can apply it by hand.

use filmcraft_project::{ClipId, TrackKind};
use serde_json::{Value, json};

use super::profiles;
use crate::commands::{CommandSpec, audio_effect_targets, bad, bool_p, clips_p, f64_p, has_seq, str_p};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.audio.polish";
const UNDO_LABEL: &str = "Voice Polish";
const MAX_CLIPS: usize = 1000;

pub fn commands() -> Vec<CommandSpec> {
    vec![CommandSpec {
        id: CMD,
        label: "Polish Voice",
        menu: &[],
        shortcut: None,
        params: r#"{"clips":[id]? (default the selection),"amount":0..1=0.5,"target":lufs?,"profile":str? (its loudness when no target),"loudness":bool=true,"savePreset":bool=false,"dir":str?}"#,
        enabled: has_seq,
        run: polish,
        journal: true,
    }]
}

/// One stage of the chain: an audio effect id and its parameters for `amount` (0–1).
pub fn chain(amount: f64) -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
    let a = amount.clamp(0.0, 1.0);
    vec![
        // rumble and handling noise below the voice
        ("highpass", vec![("cutoff", 40.0 + 60.0 * a)]),
        // steady background noise
        ("denoise", vec![("amount", 60.0 * a)]),
        // harsh sibilance
        ("deesser", vec![("frequency", 6500.0), ("threshold", -12.0 - 12.0 * a), ("reduction", 10.0 * a)]),
        // even level between words
        ("single_band_compressor", vec![("threshold", -10.0 - 14.0 * a), ("ratio", 1.0 + 3.0 * a), ("attack", 10.0), ("release", 120.0)]),
        // catch what the compressor lets through
        ("hard_limiter", vec![("max", -1.0), ("boost", 0.0)]),
    ]
}

/// Number of effects on an audio clip of the active sequence.
fn effect_count(s: &Session, clip: ClipId) -> Option<usize> {
    let seq = s.active_sequence()?;
    let (t, it) = seq.find_item(clip)?;
    seq.track(t).filter(|t| t.kind == TrackKind::Audio)?;
    Some(it.effects.len())
}

fn polish(s: &mut Session, p: &Value) -> Result<Value> {
    let amount = match p.get("amount") {
        None | Some(Value::Null) => 0.5,
        Some(_) => f64_p(p, "amount").filter(|a| a.is_finite()).ok_or_else(|| bad(CMD, "`amount` must be a number 0–1"))?.clamp(0.0, 1.0),
    };
    let target = match f64_p(p, "target") {
        Some(t) if t.is_finite() && (-60.0..=0.0).contains(&t) => Some(t),
        Some(_) => return Err(bad(CMD, "`target` must be -60 to 0 LUFS")),
        None => match str_p(p, "profile") {
            Some(key) => Some(profiles::find(p, key).ok_or_else(|| bad(CMD, format!("no output profile `{key}`")))?.loudness_lufs),
            None => None,
        },
    };
    let mut clips = audio_effect_targets(s, &clips_p(s, p));
    clips.sort_by_key(|c| c.0);
    clips.dedup();
    clips.retain(|c| effect_count(s, *c).is_some());
    if clips.is_empty() {
        return Err(bad(CMD, "select audio clips (or video clips with linked audio)"));
    }
    if clips.len() > MAX_CLIPS {
        return Err(bad(CMD, format!("at most {MAX_CLIPS} clips at once")));
    }
    if amount <= 0.0 {
        return Ok(json!({"amount": 0.0, "clips": clips.iter().map(|c| c.0).collect::<Vec<_>>(), "changed": false}));
    }

    let undo_before = s.history.undo.len();
    let result = apply(s, p, &clips, amount, target);
    // one undo step for the whole polish, also when a later stage failed part-way
    let steps = s.history.undo.len().saturating_sub(undo_before);
    fold_history(s, steps);
    result
}

fn apply(s: &mut Session, p: &Value, clips: &[ClipId], amount: f64, target: Option<f64>) -> Result<Value> {
    let ids: Vec<u64> = clips.iter().map(|c| c.0).collect();
    let stages = chain(amount);
    let first_index = effect_count(s, clips[0]).unwrap_or(0);
    for (effect, params) in &stages {
        let before: Vec<Option<usize>> = clips.iter().map(|c| effect_count(s, *c)).collect();
        s.execute("effects.apply", json!({"clips": ids, "effect": effect}))?;
        for (clip, n) in clips.iter().zip(before) {
            // audio effects are appended, so the new instance sits at the old count
            let Some(index) = n else { continue };
            if effect_count(s, *clip) != Some(index + 1) {
                return Err(EngineError::Other(format!("{CMD}: {effect} was not added to clip {}", clip.0)));
            }
            for (param, value) in params {
                s.execute("effects.setParam", json!({"clip": clip.0, "effect": index, "param": param, "value": value}))?;
            }
        }
    }

    let mut loudness = Value::Null;
    if bool_p(p, "loudness").unwrap_or(true) {
        // Auto-Match needs an audio type; untyped clips become dialogue
        let untyped: Vec<u64> = {
            let seq = s.active_sequence().ok_or(EngineError::NoSequence)?;
            clips.iter().filter(|c| seq.find_item(**c).is_some_and(|(_, it)| it.essential.is_none())).map(|c| c.0).collect()
        };
        if !untyped.is_empty() {
            s.execute("essentialSound.setType", json!({"clips": untyped, "type": "dialogue"}))?;
        }
        let mut q = json!({"clips": ids});
        if let Some(t) = target {
            q["target"] = json!(t);
        }
        loudness = s.execute("essentialSound.autoMatch", q)?;
    }

    let mut preset = Value::Null;
    if bool_p(p, "savePreset").unwrap_or(false) {
        let name = format!("BashCut Voice Polish {:.0}%", amount * 100.0);
        let effects: Vec<usize> = (first_index..first_index + stages.len()).collect();
        let description = "Highpass, DeNoise, DeEsser, compressor and limiter scaled by one amount (bashcut.audio.polish)";
        s.execute("presets.save", json!({"clip": ids[0], "effects": effects, "name": name, "description": description, "keyframes": "none"}))?;
        preset = json!(name);
    }
    Ok(
        json!({"amount": amount, "clips": ids, "chain": stages.iter().map(|(e, _)| *e).collect::<Vec<_>>(), "loudness": loudness, "preset": preset, "changed": true}),
    )
}

/// Fold the last `steps` undo entries into one labelled [`UNDO_LABEL`].
fn fold_history(s: &mut Session, steps: usize) {
    let len = s.history.undo.len();
    let steps = steps.min(len);
    if steps == 0 {
        return;
    }
    let first = len - steps;
    s.history.undo.truncate(first + 1);
    if let Some(step) = s.history.undo.get_mut(first) {
        step.0 = UNDO_LABEL.to_string();
    }
}
