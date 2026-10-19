//! Loudness of a programme: BS.1770-4 / EBU R128 values from `filmcraft-audio-dsp`'s meter, plus a
//! short-term series (one value per second) and a count of clipped samples.

use filmcraft_audio_dsp::LoudnessMeter;

/// A sample at or above this magnitude counts as clipped.
pub const CLIP_LEVEL: f32 = 0.999;

/// At most this many short-term values are kept (one per second: a day).
const MAX_POINTS: usize = 24 * 3600;

/// What [`Meter::finish`] and [`measure`] report. `None` = silent or unmeasurable (−∞, NaN).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Loudness {
    /// Gated integrated loudness, LUFS.
    pub integrated_lufs: Option<f64>,
    /// Loudness range, LU (0 when too short).
    pub loudness_range_lu: f64,
    pub max_momentary_lufs: Option<f64>,
    pub max_short_term_lufs: Option<f64>,
    /// True peak, dBTP.
    pub true_peak_dbtp: Option<f64>,
    /// Sample peak, dBFS.
    pub sample_peak_dbfs: Option<f64>,
    /// Short-term loudness at the end of each full second, LUFS: entry `i` is the 3 s window ending
    /// at second `i + 1`.
    pub short_term: Vec<Option<f64>>,
    /// Sample frames measured.
    pub frames: u64,
    /// Samples (over all channels) at or above [`CLIP_LEVEL`].
    pub clipped_samples: u64,
}

fn finite(v: f64) -> Option<f64> {
    v.is_finite().then_some(v)
}

/// A streaming meter: [`Meter::push`] blocks of planar audio, then [`Meter::finish`].
#[derive(Clone, Debug)]
pub struct Meter {
    meter: LoudnessMeter,
    step: usize,
    since: usize,
    frames: u64,
    clipped: u64,
    short_term: Vec<Option<f64>>,
}

impl Meter {
    /// A meter for `channels` channels (at least 1) at `sample_rate` Hz (at least 1).
    pub fn new(sample_rate: u32, channels: usize) -> Self {
        let rate = sample_rate.max(1);
        Meter { meter: LoudnessMeter::new(rate as f64, channels.clamp(1, 8)), step: rate as usize, since: 0, frames: 0, clipped: 0, short_term: Vec::new() }
    }

    /// Feed planar audio (one slice per channel; the shortest slice sets the block length).
    pub fn push(&mut self, channels: &[&[f32]]) {
        let n = channels.iter().map(|c| c.len()).min().unwrap_or(0);
        let mut off = 0usize;
        while off < n {
            let take = (n - off).min(self.step.saturating_sub(self.since).max(1));
            let end = off.saturating_add(take);
            let parts: Vec<&[f32]> = channels.iter().map(|c| c.get(off..end).unwrap_or(&[])).collect();
            self.meter.process(&parts);
            let clipped = parts.iter().flat_map(|c| c.iter()).filter(|x| x.abs() >= CLIP_LEVEL).count();
            self.clipped = self.clipped.saturating_add(clipped as u64);
            self.frames = self.frames.saturating_add(take as u64);
            self.since += take;
            off = end;
            if self.since >= self.step {
                self.since = 0;
                if self.short_term.len() < MAX_POINTS {
                    self.short_term.push(finite(self.meter.short_term()));
                }
            }
        }
    }

    /// The measurements so far.
    pub fn finish(&self) -> Loudness {
        let s = self.meter.summary();
        Loudness {
            integrated_lufs: finite(s.integrated_lufs),
            loudness_range_lu: if s.loudness_range_lu.is_finite() { s.loudness_range_lu } else { 0.0 },
            max_momentary_lufs: finite(s.max_momentary_lufs),
            max_short_term_lufs: finite(s.max_short_term_lufs),
            true_peak_dbtp: finite(s.true_peak_dbtp),
            sample_peak_dbfs: finite(s.sample_peak_dbfs),
            short_term: self.short_term.clone(),
            frames: self.frames,
            clipped_samples: self.clipped,
        }
    }
}

/// Measure a whole planar programme in one call.
pub fn measure(channels: &[&[f32]], sample_rate: u32) -> Loudness {
    let mut m = Meter::new(sample_rate, channels.len());
    m.push(channels);
    m.finish()
}

/// Index stretches `[start, end)` of a [`Loudness::short_term`] series, with their maximum, where the short-term loudness is above
/// `max_lufs`: loud segments for a review.
pub fn loud_stretches(short_term: &[Option<f64>], max_lufs: f64) -> Vec<(usize, usize, f64)> {
    let mut out = Vec::new();
    let mut run: Option<(usize, f64)> = None;
    for (i, v) in short_term.iter().enumerate() {
        match (v.filter(|v| *v > max_lufs), run) {
            (Some(v), None) => run = Some((i, v)),
            (Some(v), Some((s, m))) => run = Some((s, m.max(v))),
            (None, Some((s, m))) => {
                out.push((s, i, m));
                run = None;
            }
            (None, None) => {}
        }
    }
    if let Some((s, m)) = run {
        out.push((s, short_term.len(), m));
    }
    out
}
