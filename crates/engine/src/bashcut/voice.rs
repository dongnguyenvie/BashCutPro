//! BashCutPro: one-knob voice polish (#48).
//!
//! `bashcut.audio.polish` appends one ordered chain of FilmCraft's own audio effects to each dialogue clip
//! (Highpass → DeNoise → DeEsser → Single-band Compressor → Amplify → Hard Limiter), every stage scaled by one `amount`
//! (0–1), then brings the clip to a loudness target: `target` (LUFS), else the output profile's loudness
//! (`profile`, #43), else the dialogue target from the preferences. Everything is one undo step. `amount` 0 changes
//! nothing; polishing a clip again replaces its previous polish chain instead of stacking a second one.
//!
//! Loudness: upstream's clip gain (what `essentialSound.autoMatch` writes) plays *after* the effects, so raising a
//! quiet clip with it alone pushes peaks past the limiter. The gain goes in before the limiter instead (its Input
//! Boost up to +30 dB, then Amplify up to +24 dB; measured, set, measured again, a few passes), with its ceiling under the profile's true peak; Auto-Match then
//! only trims the last fraction down, which can't clip. With `savePreset`, the chain is also saved as an effect preset
//! (`BashCut Voice Polish NN%`) through `presets.save`, so the user can apply it by hand.

use filmcraft_project::{ClipId, TrackKind};
use serde_json::{Value, json};

use super::analysis::{Target, measure, target_range};
use super::profiles;
use crate::commands::{CommandSpec, audio_effect_targets, bad, bool_p, clips_p, f64_p, has_seq, str_p};
use crate::{EngineError, Result, Session};

const CMD: &str = "bashcut.audio.polish";
const UNDO_LABEL: &str = "Voice Polish";
const MAX_CLIPS: usize = 1000;
/// True peak allowed when no profile names one, dBTP.
const DEFAULT_TRUE_PEAK: f64 = -1.0;
/// The limiter ceiling sits this far under the true-peak limit (it limits samples, not true peaks), dB.
const TRUE_PEAK_MARGIN: f64 = 0.5;
/// Passes of measure → set the makeup gain.
const BOOST_PASSES: usize = 8;
/// The makeup gain aims this far over the target, so Auto-Match only trims down, LU.
const BOOST_HEADROOM: f64 = 0.3;

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
    chain_to(amount, DEFAULT_TRUE_PEAK)
}

/// [`chain`] with the limiter ceiling under `true_peak` (dBTP).
pub fn chain_to(amount: f64, true_peak: f64) -> Vec<(&'static str, Vec<(&'static str, f64)>)> {
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
        // makeup gain past the limiter's +30 dB boost, set by the loudness pass
        ("amplify", vec![("gain", 0.0)]),
        // catch what the compressor lets through
        ("hard_limiter", vec![("max", (true_peak - TRUE_PEAK_MARGIN).clamp(-30.0, 0.0)), ("boost", 0.0)]),
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
    let profile = match str_p(p, "profile") {
        Some(key) => Some(profiles::find(p, key).ok_or_else(|| bad(CMD, format!("no output profile `{key}` (see bashcut.profiles.list)")))?),
        None => None,
    };
    let target = match f64_p(p, "target") {
        Some(t) if t.is_finite() && (-60.0..=0.0).contains(&t) => Some(t),
        Some(_) => return Err(bad(CMD, "`target` must be -60 to 0 LUFS")),
        None => profile.as_ref().map(|x| x.loudness_lufs),
    };
    let true_peak = profile.as_ref().map(|x| x.true_peak_dbtp).filter(|t| t.is_finite()).unwrap_or(DEFAULT_TRUE_PEAK).min(0.0);
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
    let result = apply(s, p, &clips, amount, target, true_peak);
    // one undo step for the whole polish, also when a later stage failed part-way
    let steps = s.history.undo.len().saturating_sub(undo_before);
    fold_history(s, steps);
    result
}

/// Index of the polish chain at the end of a clip's effects, if the clip ends with one.
fn existing_chain(s: &Session, clip: ClipId) -> Option<usize> {
    let seq = s.active_sequence()?;
    let (_, it) = seq.find_item(clip)?;
    let ids: Vec<&str> = chain(0.0).into_iter().map(|(e, _)| e).collect();
    let first = it.effects.len().checked_sub(ids.len())?;
    let tail = it.effects.get(first..)?;
    tail.iter().zip(&ids).all(|(e, id)| e.effect == *id).then_some(first)
}

fn apply(s: &mut Session, p: &Value, clips: &[ClipId], amount: f64, target: Option<f64>, true_peak: f64) -> Result<Value> {
    let ids: Vec<u64> = clips.iter().map(|c| c.0).collect();
    let stages = chain_to(amount, true_peak);
    // polishing again replaces the previous chain (removed from its last effect back)
    for clip in clips {
        if let Some(first) = existing_chain(s, *clip) {
            for index in (first..first + stages.len()).rev() {
                s.execute("effects.remove", json!({"clip": clip.0, "index": index}))?;
            }
        }
    }
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
        let target = target.unwrap_or_else(|| s.prefs.audio.loudness_target(filmcraft_project::AudioType::Dialogue)).clamp(-60.0, 0.0);
        let mut report = Vec::new();
        let mut trim = Vec::new();
        for clip in clips {
            match boost_to(s, *clip, target)? {
                Some(l) if l >= target - 0.05 => trim.push(clip.0),
                Some(l) => report.push(json!({"clip": clip.0, "measuredLufs": l, "targetLufs": target,
                    "short": format!("{:.1} LU under the target: louder would need more limiting than the polish allows (or more than +54 dB of gain); raise `amount` or pick a lower target", target - l)})),
                None => report.push(json!({"clip": clip.0, "skipped": "silent"})),
            }
        }
        // the last fraction down with the clip gain (after the limiter, so it can't clip)
        if !trim.is_empty() {
            let matched = s.execute("essentialSound.autoMatch", json!({"clips": trim, "target": target}))?;
            report.extend(matched.get("clips").and_then(Value::as_array).cloned().unwrap_or_default());
        }
        loudness = json!({"clips": report, "targetLufs": target, "truePeakDbtp": true_peak});
    }

    let mut preset = Value::Null;
    if bool_p(p, "savePreset").unwrap_or(false) {
        let name = format!("BashCut Voice Polish {:.0}%", amount * 100.0);
        let effects: Vec<usize> = (first_index..first_index + stages.len()).collect();
        let description = "Highpass, DeNoise, DeEsser, compressor, makeup gain and limiter scaled by one amount (bashcut.audio.polish)";
        s.execute("presets.save", json!({"clip": ids[0], "effects": effects, "name": name, "description": description, "keyframes": "none"}))?;
        preset = json!(name);
    }
    Ok(
        json!({"amount": amount, "clips": ids, "chain": stages.iter().map(|(e, _)| *e).collect::<Vec<_>>(), "loudness": loudness, "preset": preset, "changed": true}),
    )
}

/// Integrated loudness of a clip as it plays (effects and clip gain), LUFS.
fn clip_lufs(s: &Session, clip: ClipId) -> Result<Option<f64>> {
    let (a, b) = target_range(s, Target::Clip(clip), CMD)?;
    Ok(measure(s, Target::Clip(clip), a, b, CMD)?.0.and_then(|l| l.integrated_lufs).filter(|v| v.is_finite()))
}

/// Most makeup gain before the limiter: its Input Boost, then Amplify, dB.
const MAX_BOOST: f64 = 30.0;
const MAX_AMPLIFY: f64 = 24.0;

/// Raise the makeup gain before the polish limiter until the clip reaches `target` (plus a little headroom);
/// returns the loudness reached, `None` when the clip is silent.
fn boost_to(s: &mut Session, clip: ClipId, target: f64) -> Result<Option<f64>> {
    let Some(first) = existing_chain(s, clip) else { return clip_lufs(s, clip) };
    let limiter = first + chain(0.0).len() - 1;
    let amplify = limiter - 1;
    // an earlier Auto-Match gain plays after the limiter: off while the makeup gain is set (step 3 sets a new one)
    let matched = s.active_sequence().and_then(|q| q.find_item(clip)).and_then(|(_, it)| it.essential.as_ref()).is_some_and(|e| e.loudness.enabled);
    if matched {
        s.execute("essentialSound.set", json!({"clips": [clip.0], "key": "loudness.enabled", "value": false}))?;
    }
    let mut gain = 0.0f64;
    let mut level = clip_lufs(s, clip)?;
    // LU gained per dB of makeup: 1 until the limiter works, less once it does (secant from the last pass)
    let mut slope = 1.0f64;
    let mut last: Option<(f64, f64)> = None;
    for _ in 0..BOOST_PASSES {
        let Some(l) = level else { return Ok(None) };
        if l >= target - 0.05 && l - target < 1.0 {
            break;
        }
        if let Some((g0, l0)) = last
            && (gain - g0).abs() > 0.01
        {
            slope = ((l - l0) / (gain - g0)).clamp(0.1, 1.0);
        }
        let want = (gain + (target + BOOST_HEADROOM - l) / slope).clamp(0.0, MAX_BOOST + MAX_AMPLIFY);
        if (want - gain).abs() < 0.05 {
            break;
        }
        last = Some((gain, l));
        gain = want;
        let boost = gain.min(MAX_BOOST);
        s.execute("effects.setParam", json!({"clip": clip.0, "effect": limiter, "param": "boost", "value": boost}))?;
        s.execute("effects.setParam", json!({"clip": clip.0, "effect": amplify, "param": "gain", "value": gain - boost}))?;
        level = clip_lufs(s, clip)?;
    }
    Ok(level)
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
