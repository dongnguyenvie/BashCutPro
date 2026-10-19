//! BashCutPro: one-knob voice polish (#48).

use std::path::Path;
use std::sync::Arc;

use filmcraft_project::{ClipId, ItemId, SequenceSettings, TrackKind};
use filmcraft_time::{FrameRate, Tick, TimeRange};
use serde_json::json;

use crate::Session;
use crate::media_test_util::tmp_dir;

const SR: u32 = 48_000;

/// A 16-bit mono WAV.
fn write_wav(path: &Path, samples: &[f32]) {
    let data_len = (samples.len() * 2) as u32;
    let mut b = Vec::with_capacity(44 + samples.len() * 2);
    b.extend_from_slice(b"RIFF");
    b.extend_from_slice(&(36 + data_len).to_le_bytes());
    b.extend_from_slice(b"WAVEfmt ");
    b.extend_from_slice(&16u32.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&1u16.to_le_bytes());
    b.extend_from_slice(&SR.to_le_bytes());
    b.extend_from_slice(&(SR * 2).to_le_bytes());
    b.extend_from_slice(&2u16.to_le_bytes());
    b.extend_from_slice(&16u16.to_le_bytes());
    b.extend_from_slice(b"data");
    b.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0) as i16).to_le_bytes());
    }
    std::fs::write(path, b).unwrap();
}

/// Deterministic white noise at `level` plus, when `voice`, a speech-like syllable pattern (a 180 Hz voice with
/// harmonics, gated at 4 Hz) and some sibilance.
fn fixture(seconds: f32, level: f32, voice: bool) -> Vec<f32> {
    let n = (seconds * SR as f32) as usize;
    let mut seed = 0x2545_f491_u32;
    (0..n)
        .map(|i| {
            seed ^= seed << 13;
            seed ^= seed >> 17;
            seed ^= seed << 5;
            let noise = (seed as f32 / u32::MAX as f32 * 2.0 - 1.0) * level;
            if !voice {
                return noise;
            }
            let t = i as f32 / SR as f32;
            let gate = if (t * 4.0).fract() < 0.6 { 1.0 } else { 0.0 };
            let f0 = 180.0 * std::f32::consts::TAU * t;
            let v = 0.3 * f0.sin() + 0.15 * (2.0 * f0).sin() + 0.08 * (3.0 * f0).sin() + 0.02 * (40.0 * f0).sin();
            v * gate + noise
        })
        .collect()
}

/// A session with one audio clip per file on A1, one after another; returns the clip ids.
fn session(files: &[&Path]) -> (Session, Vec<ClipId>) {
    let mut s = Session::default();
    let paths: Vec<String> = files.iter().map(|p| p.to_string_lossy().into_owned()).collect();
    let r = s.execute("file.import", json!({"paths": paths})).unwrap();
    let items: Vec<ItemId> = r["items"].as_array().unwrap().iter().map(|v| ItemId(v.as_u64().unwrap())).collect();
    let mut p = (*s.project).clone();
    let rate = FrameRate::FPS_24;
    let seq = p.new_sequence("Seq", SequenceSettings { frame_rate: rate, ..Default::default() }, 1, 1, None);
    let mut at = Tick::ZERO;
    let mut clips = Vec::new();
    for &it in &items {
        let d = p.item(it).unwrap().duration();
        let ai = p.make_track_item(it, TrackKind::Audio, at, TimeRange::new(Tick::ZERO, d), rate).unwrap();
        clips.push(ai.id);
        p.sequence_mut(seq).unwrap().audio_tracks[0].items.push(ai);
        at += d;
    }
    s.project = Arc::new(p);
    s.state.active_sequence = Some(seq);
    s.state.open_sequences = vec![seq];
    (s, clips)
}

fn loudness(s: &Session, clip: ClipId) -> f64 {
    let seq = s.active_sequence().unwrap();
    let (_, it) = seq.find_item(clip).unwrap();
    let provider = s.media.provider(s.project.clone(), s.services.clone());
    crate::essential_sound::clip_loudness(it, seq.settings.sample_rate.max(1), &provider).unwrap()
}

#[test]
fn polish_lowers_the_noise_floor_and_lands_at_the_target() {
    let dir = tmp_dir("bashcut-voice");
    let (speech, noise) = (dir.join("speech.wav"), dir.join("noise.wav"));
    write_wav(&speech, &fixture(4.0, 0.05, true));
    write_wav(&noise, &fixture(4.0, 0.05, false));
    let (mut s, clips) = session(&[speech.as_path(), noise.as_path()]);

    // the noise floor: the same chain on noise alone, without loudness matching
    let floor_before = loudness(&s, clips[1]);
    s.execute("bashcut.audio.polish", json!({"clips": [clips[1].0], "amount": 1.0, "loudness": false})).unwrap();
    let floor_after = loudness(&s, clips[1]);
    assert!(floor_after < floor_before - 3.0, "noise floor {floor_before:.1} → {floor_after:.1} LUFS");

    let undo = s.history.undo.len();
    let r = s.execute("bashcut.audio.polish", json!({"clips": [clips[0].0], "amount": 0.7, "target": -16.0, "savePreset": true})).unwrap();
    assert_eq!(r["chain"].as_array().unwrap().len(), 6, "{r}");
    let measured = loudness(&s, clips[0]);
    assert!((measured + 16.0).abs() <= 1.0, "speech at {measured:.2} LUFS, want -16 ±1");
    assert_eq!(s.history.undo.len(), undo + 1, "one undo step");
    assert_eq!(s.history.undo.last().map(|u| u.0.as_str()), Some("Voice Polish"));
    assert!(s.presets.find("BashCut Voice Polish 70%").is_some(), "saved as an effect preset");

    // the output profile's loudness when no target is given
    s.execute("bashcut.audio.polish", json!({"clips": [clips[0].0], "amount": 0.3, "profile": "facebook-square"})).unwrap();
    assert!((loudness(&s, clips[0]) + 16.0).abs() <= 1.0);
}

#[test]
fn amount_zero_changes_nothing() {
    let dir = tmp_dir("bashcut-voice-zero");
    let speech = dir.join("speech.wav");
    write_wav(&speech, &fixture(1.0, 0.05, true));
    let (mut s, clips) = session(&[speech.as_path()]);
    let before = s.project.to_json();
    let undo = s.history.undo.len();
    let r = s.execute("bashcut.audio.polish", json!({"clips": [clips[0].0], "amount": 0})).unwrap();
    assert_eq!(r["changed"], false);
    assert_eq!(s.project.to_json(), before);
    assert_eq!(s.history.undo.len(), undo);
}

#[test]
fn polish_refuses_hostile_params() {
    let dir = tmp_dir("bashcut-voice-hostile");
    let speech = dir.join("speech.wav");
    write_wav(&speech, &fixture(0.5, 0.05, true));
    let (mut s, clips) = session(&[speech.as_path()]);
    let c = clips[0].0;
    for p in [
        json!({"clips": []}),
        json!({"clips": [u64::MAX]}),
        json!({"clips": "x"}),
        json!({"clips": [c], "amount": "loud"}),
        json!({"clips": [c], "target": 40.0}),
        json!({"clips": [c], "target": -1e308}),
        json!({"clips": [c], "profile": "nope"}),
    ] {
        assert!(s.execute("bashcut.audio.polish", p.clone()).is_err(), "{p}");
    }
    // out-of-range amounts clamp instead of failing
    assert!(s.execute("bashcut.audio.polish", json!({"clips": [c], "amount": 1e9, "loudness": false})).is_ok());
    assert!(s.execute("bashcut.audio.polish", json!({"clips": [c], "amount": -5})).is_ok());
}

#[test]
fn a_quiet_voice_reaches_the_target_under_the_true_peak_and_polishing_again_replaces_the_chain() {
    let dir = tmp_dir("bashcut-voice-quiet");
    let speech = dir.join("quiet.wav");
    // about -36 LUFS: the old route raised it ~22 dB with clip gain after the limiter, to +3 dBTP
    let quiet: Vec<f32> = fixture(6.0, 0.05, true).iter().map(|x| x * 0.05).collect();
    write_wav(&speech, &quiet);
    let (mut s, clips) = session(&[speech.as_path()]);
    let c = clips[0].0;
    let effects = |s: &Session| s.active_sequence().unwrap().find_item(clips[0]).unwrap().1.effects.len();
    let before = effects(&s);
    // an Auto-Match gain from before (it plays after the effects) must not be stacked on the makeup gain
    s.execute("essentialSound.setType", json!({"clips": [c], "type": "dialogue"})).unwrap();
    s.execute("essentialSound.autoMatch", json!({"clips": [c], "target": -14.0})).unwrap();

    s.execute("bashcut.audio.polish", json!({"clips": [c], "amount": 0.6, "profile": "tiktok"})).unwrap();
    let m = s.execute("bashcut.audio.loudness", json!({"clip": c})).unwrap();
    let (lufs, peak) = (m["integratedLufs"].as_f64().unwrap(), m["truePeakDbtp"].as_f64().unwrap());
    assert!((lufs + 14.0).abs() <= 1.0, "at {lufs:.2} LUFS, want -14 ±1: {m}");
    assert!(peak <= -1.0 + 0.3, "true peak {peak:.2} dBTP over the profile's -1: {m}");
    assert_eq!(m["clippedSamples"], 0, "{m}");
    let polished = effects(&s);
    assert_eq!(polished, before + 6);

    // a second polish replaces the chain, it does not stack another one
    s.execute("bashcut.audio.polish", json!({"clips": [c], "amount": 0.3, "target": -16.0})).unwrap();
    assert_eq!(effects(&s), polished);
    let m = s.execute("bashcut.audio.loudness", json!({"clip": c})).unwrap();
    assert!((m["integratedLufs"].as_f64().unwrap() + 16.0).abs() <= 1.0, "{m}");
    assert!(m["truePeakDbtp"].as_f64().unwrap() <= -0.7, "{m}");
}
