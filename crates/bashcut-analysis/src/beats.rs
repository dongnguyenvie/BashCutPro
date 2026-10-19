//! Tempo, beats and downbeats. The onset envelope (spectral flux) comes from
//! `filmcraft_audio_dsp::remix::onset_envelope`; tempo is the autocorrelation peak of that envelope
//! (with a mild preference for tempos near 120 BPM), beats follow the tempo grid snapped to local onset
//! peaks, and downbeats are the strongest of the four beat phases.

use filmcraft_audio_dsp::remix::onset_envelope;

/// Slowest and fastest tempo looked for.
pub const MIN_BPM: f64 = 60.0;
pub const MAX_BPM: f64 = 200.0;
/// At most this much audio is analysed (20 minutes at 48 kHz); the rest is ignored.
pub const MAX_SAMPLES: usize = 48_000 * 60 * 20;
/// Below this normalised autocorrelation there is no usable pulse.
const MIN_CONFIDENCE: f64 = 0.1;
/// Beats per bar assumed for downbeats.
const BAR: usize = 4;

/// Beats found in a signal. Positions are sample offsets from the start of the analysed audio.
#[derive(Clone, Debug, PartialEq)]
pub struct Beats {
    pub bpm: f64,
    pub beats: Vec<i64>,
    /// Every fourth beat, on the strongest phase.
    pub downbeats: Vec<i64>,
    /// Normalised autocorrelation at the beat period, 0…1.
    pub confidence: f64,
}

/// Find the beats of planar audio (channels summed to mono). `None` for silence, noise, speech or
/// too little audio (fewer than four beats).
pub fn detect(channels: &[&[f32]], sample_rate: u32) -> Option<Beats> {
    if sample_rate == 0 || channels.is_empty() {
        return None;
    }
    let len = channels.iter().map(|c| c.len()).max().unwrap_or(0).min(MAX_SAMPLES);
    let k = 1.0 / channels.len() as f32;
    let mono: Vec<f32> = (0..len).map(|i| channels.iter().map(|c| c.get(i).copied().filter(|v| v.is_finite()).unwrap_or(0.0)).sum::<f32>() * k).collect();
    let (env, n, hop) = onset_envelope(&mono, sample_rate);
    if hop == 0 || env.len() < 16 {
        return None;
    }
    let frame_rate = sample_rate as f64 / hop as f64;
    let (period, confidence) = tempo(&env, frame_rate)?;
    let strength = |t: usize| local_max(&env, t, 2);
    let mut frames = track(&env, period);
    let mut strengths: Vec<f32> = frames.iter().map(|&t| strength(t)).collect();
    strengths.sort_by(f32::total_cmp);
    let median = strengths.get(strengths.len() / 2).copied().unwrap_or(0.0);
    // weak beats at the edges (silence before / after the music) are not beats
    while frames.first().is_some_and(|&t| strength(t) < 0.3 * median) {
        frames.remove(0);
    }
    while frames.last().is_some_and(|&t| strength(t) < 0.3 * median) {
        frames.pop();
    }
    if frames.len() < BAR {
        return None;
    }
    // the flux of frame t peaks once an attack enters the newer window: about t·hop + n − ¾·hop
    let last = (len.max(1) - 1) as i64;
    let to_sample = |t: usize| (t.saturating_mul(hop).saturating_add(n) as i64).saturating_sub(3 * hop as i64 / 4).clamp(0, last);
    let mut beats: Vec<i64> = frames.iter().map(|&t| to_sample(t)).collect();
    beats.dedup();
    let phase = (0..BAR)
        .map(|ph| (ph, frames.iter().skip(ph).step_by(BAR).map(|&t| strength(t) as f64).sum::<f64>()))
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(ph, _)| ph)
        .unwrap_or(0);
    let downbeats = beats.iter().skip(phase).step_by(BAR).copied().collect();
    Some(Beats { bpm: 60.0 * frame_rate / period, beats, downbeats, confidence })
}

/// The beat period in envelope frames (fractional) and its normalised autocorrelation.
fn tempo(env: &[f32], frame_rate: f64) -> Option<(f64, f64)> {
    let mean = env.iter().map(|v| *v as f64).sum::<f64>() / env.len().max(1) as f64;
    let x: Vec<f64> = env.iter().map(|v| *v as f64 - mean).collect();
    let r0 = x.iter().map(|v| v * v).sum::<f64>() / x.len().max(1) as f64;
    if r0 <= 0.0 || !r0.is_finite() {
        return None;
    }
    let ac = |lag: usize| {
        let m = x.len().saturating_sub(lag);
        if m == 0 {
            return 0.0;
        }
        x.iter().zip(x.iter().skip(lag)).map(|(a, b)| a * b).sum::<f64>() / m as f64 / r0
    };
    let lo = (frame_rate * 60.0 / MAX_BPM).floor().max(1.0) as usize;
    let hi = ((frame_rate * 60.0 / MIN_BPM).ceil() as usize).min(x.len() / 2);
    if lo + 1 >= hi {
        return None;
    }
    // acs[i] is the autocorrelation at lag lo − 1 + i
    let acs: Vec<f64> = (lo - 1..=hi + 1).map(ac).collect();
    let at = |lag: usize| acs.get(lag + 1 - lo).copied().unwrap_or(0.0);
    let mut best: Option<(usize, f64)> = None;
    for lag in lo..=hi {
        let bpm = 60.0 * frame_rate / lag as f64;
        let prior = (-0.5 * (bpm / 120.0).log2().powi(2)).exp();
        let score = at(lag) * prior;
        if best.is_none_or(|(_, s)| score > s) {
            best = Some((lag, score));
        }
    }
    let (lag, _) = best?;
    let (a, b, c) = (at(lag - 1), at(lag), at(lag + 1));
    if b < MIN_CONFIDENCE {
        return None;
    }
    // parabolic interpolation of the peak
    let den = a - 2.0 * b + c;
    let shift = if den.abs() > 1e-12 { (0.5 * (a - c) / den).clamp(-0.5, 0.5) } else { 0.0 };
    Some((lag as f64 + shift, b.min(1.0)))
}

/// Beat frames: the best phase of the tempo grid, then each beat snapped to the strongest onset
/// within a tenth of a period of where the previous beat predicts it.
fn track(env: &[f32], period: f64) -> Vec<usize> {
    let len = env.len();
    if period.is_nan() || period < 1.0 || len == 0 {
        return Vec::new();
    }
    let p = period.round() as usize;
    let phase = (0..p.min(len))
        .map(|ph| {
            let mut s = 0.0f64;
            let mut t = ph as f64;
            while t.round() < len as f64 {
                s += env.get(t.round() as usize).copied().unwrap_or(0.0) as f64;
                t += period;
            }
            (ph, s)
        })
        .max_by(|a, b| a.1.total_cmp(&b.1).then(b.0.cmp(&a.0)))
        .map(|(ph, _)| ph)
        .unwrap_or(0);
    let r = ((period * 0.1).round() as usize).max(1);
    let value = |i: usize| env.get(i).copied().unwrap_or(0.0);
    let mut out: Vec<usize> = Vec::new();
    let mut t = phase as f64;
    while t.round() < len as f64 {
        let c = t.round() as usize;
        let lo = c.saturating_sub(r);
        let hi = (c + r + 1).min(len);
        let snapped = (lo..hi).max_by(|a, b| value(*a).total_cmp(&value(*b)).then(b.cmp(a))).unwrap_or(c);
        if out.last().is_none_or(|&l| snapped > l) {
            out.push(snapped);
        }
        // always move forward, even when a snap pulled the beat back
        t = (snapped as f64 + period).max(t + 1.0);
    }
    out
}

fn local_max(env: &[f32], t: usize, r: usize) -> f32 {
    env.get(t.saturating_sub(r)..(t + r + 1).min(env.len())).unwrap_or(&[]).iter().copied().fold(0.0, f32::max)
}

/// A click track for tests: a short decaying 1 kHz burst on every beat, louder on the first beat of
/// each bar of four. Returns mono audio and the true beat positions (samples).
pub fn click_track(sample_rate: u32, bpm: f64, seconds: f64) -> (Vec<f32>, Vec<i64>) {
    let sr = sample_rate.max(1) as f64;
    let n = (sr * seconds.clamp(0.0, 600.0)) as usize;
    let beat = sr * 60.0 / bpm.clamp(1.0, 1000.0);
    let mut x = vec![0f32; n];
    let mut beats = Vec::new();
    let burst = (sr * 0.03) as usize;
    let mut k = 0usize;
    loop {
        let start = (k as f64 * beat).round() as usize;
        if start >= n {
            break;
        }
        beats.push(start as i64);
        let amp = if k % BAR == 0 { 0.9 } else { 0.45 };
        for i in 0..burst {
            if let Some(s) = x.get_mut(start + i) {
                let t = i as f64 / sr;
                *s += (amp * (-t * 150.0).exp() * (2.0 * std::f64::consts::PI * 1000.0 * t).sin()) as f32;
            }
        }
        k += 1;
    }
    (x, beats)
}
