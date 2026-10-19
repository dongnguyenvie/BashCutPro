//! Picture checks over sampled frames: black stretches, frozen pictures, long stills and flashes.
//!
//! Each frame is reduced to a [`Thumb`] (a 32×18 grid of Rec. 709 luma, 0…1); the checks compare
//! thumbs only, so the caller can render frames as small as it likes. Times are the caller's integer
//! unit (the engine passes ticks); a sample covers the time from its own to the next sample's.

/// Thumbnail grid size.
pub const GRID_W: usize = 32;
pub const GRID_H: usize = 18;

/// A frame reduced to a grid of mean luma values (0…1), row-major.
#[derive(Clone, Debug, PartialEq)]
pub struct Thumb {
    pub luma: Vec<f32>,
}

impl Thumb {
    /// Average an RGBA8 frame (`w`×`h`, row-major, 4 bytes a pixel) into the grid. `None` when the
    /// frame is empty or `px` is shorter than `w·h·4`.
    pub fn from_rgba8(w: usize, h: usize, px: &[u8]) -> Option<Thumb> {
        let need = w.checked_mul(h)?.checked_mul(4)?;
        if w == 0 || h == 0 || px.len() < need {
            return None;
        }
        let mut luma = vec![0f32; GRID_W * GRID_H];
        for (cy, row) in luma.chunks_mut(GRID_W).enumerate() {
            let y0 = cy * h / GRID_H;
            let y1 = ((cy + 1) * h / GRID_H).max(y0 + 1).min(h);
            for (cx, cell) in row.iter_mut().enumerate() {
                let x0 = cx * w / GRID_W;
                let x1 = ((cx + 1) * w / GRID_W).max(x0 + 1).min(w);
                let (mut sum, mut count) = (0f64, 0u32);
                for y in y0..y1 {
                    for x in x0..x1 {
                        let i = (y * w + x) * 4;
                        if let Some([r, g, b]) = px.get(i..i + 3) {
                            sum += 0.2126 * *r as f64 + 0.7152 * *g as f64 + 0.0722 * *b as f64;
                            count += 1;
                        }
                    }
                }
                *cell = if count > 0 { (sum / count as f64 / 255.0) as f32 } else { 0.0 };
            }
        }
        Some(Thumb { luma })
    }

    /// A thumb of one flat luma value (tests).
    pub fn flat(v: f32) -> Thumb {
        Thumb { luma: vec![v.clamp(0.0, 1.0); GRID_W * GRID_H] }
    }

    /// Mean luma, 0…1.
    pub fn mean(&self) -> f32 {
        if self.luma.is_empty() { 0.0 } else { self.luma.iter().sum::<f32>() / self.luma.len() as f32 }
    }

    /// Brightest cell, 0…1.
    pub fn max(&self) -> f32 {
        self.luma.iter().copied().fold(0.0, f32::max)
    }

    /// Mean absolute difference to `o`, 0…1 (1 when the grids differ in size).
    pub fn diff(&self, o: &Thumb) -> f32 {
        if self.luma.len() != o.luma.len() || self.luma.is_empty() {
            return 1.0;
        }
        self.luma.iter().zip(&o.luma).map(|(a, b)| (a - b).abs()).sum::<f32>() / self.luma.len() as f32
    }
}

/// One sampled frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Sample {
    pub time: i64,
    pub thumb: Thumb,
}

/// Thresholds. Durations are in the caller's time unit.
#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    /// A frame is black when its mean luma is below this…
    pub black_mean: f32,
    /// …and no cell is brighter than this.
    pub black_max: f32,
    /// Shortest black stretch reported.
    pub min_black: i64,
    /// Two frames are the same picture below this mean difference.
    pub still_diff: f32,
    /// Shortest unchanged stretch reported as frozen.
    pub min_frozen: i64,
    /// Unchanged stretches at least this long are reported as long stills instead.
    pub min_still: i64,
    /// A flash: mean luma jumps up by at least this and comes back down by half of it.
    pub flash_jump: f32,
}

impl Options {
    /// Defaults for a time unit of `per_second` units a second (ticks: `TICKS_PER_SECOND`).
    pub fn per_second(per_second: i64) -> Options {
        let s = per_second.max(1);
        Options { black_mean: 0.06, black_max: 0.12, min_black: s / 4, still_diff: 0.003, min_frozen: s, min_still: s.saturating_mul(5), flash_jump: 0.35 }
    }
}

/// What a finding is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Black,
    Frozen,
    LongStill,
    Flash,
}

impl Kind {
    pub fn name(self) -> &'static str {
        match self {
            Kind::Black => "black",
            Kind::Frozen => "frozen",
            Kind::LongStill => "longStill",
            Kind::Flash => "flash",
        }
    }
}

/// A problem found over `[start, end)`. `value`: black → the brightest mean luma in the stretch;
/// frozen / long still → the largest frame difference in it; flash → the luma jump.
#[derive(Clone, Debug, PartialEq)]
pub struct Finding {
    pub kind: Kind,
    pub start: i64,
    pub end: i64,
    pub value: f64,
}

/// Run every check over `samples` (sorted by time here); the last sample lasts until `end`.
pub fn check(samples: &[Sample], end: i64, o: &Options) -> Vec<Finding> {
    let mut s: Vec<&Sample> = samples.iter().collect();
    s.sort_by_key(|x| x.time);
    let n = s.len();
    let until = |i: usize| s.get(i + 1).map(|x| x.time).unwrap_or(end.max(s.get(i).map_or(end, |x| x.time)));
    let is_black = |x: &Sample| x.thumb.mean() < o.black_mean && x.thumb.max() < o.black_max;
    let mut out = Vec::new();

    // black stretches
    let mut i = 0;
    while i < n {
        if !s.get(i).is_some_and(|x| is_black(x)) {
            i += 1;
            continue;
        }
        let mut j = i;
        let mut worst = 0f32;
        while let Some(x) = s.get(j).filter(|x| is_black(x)) {
            worst = worst.max(x.thumb.mean());
            j += 1;
        }
        let (a, b) = (s.get(i).map_or(0, |x| x.time), until(j - 1));
        if b.saturating_sub(a) >= o.min_black {
            out.push(Finding { kind: Kind::Black, start: a, end: b, value: worst as f64 });
        }
        i = j;
    }

    // unchanged pictures (black ones are already reported as black)
    let mut i = 0;
    while i < n {
        let mut j = i;
        let mut worst = 0f32;
        while let (Some(a), Some(b)) = (s.get(j), s.get(j + 1)) {
            let d = a.thumb.diff(&b.thumb);
            if d >= o.still_diff || is_black(a) || is_black(b) {
                break;
            }
            worst = worst.max(d);
            j += 1;
        }
        if j > i {
            let (a, b) = (s.get(i).map_or(0, |x| x.time), until(j));
            let dur = b.saturating_sub(a);
            let kind = if dur >= o.min_still {
                Some(Kind::LongStill)
            } else if dur >= o.min_frozen {
                Some(Kind::Frozen)
            } else {
                None
            };
            if let Some(kind) = kind {
                out.push(Finding { kind, start: a, end: b, value: worst as f64 });
            }
        }
        i = j + 1;
    }

    // flashes: a frame much brighter than the one before, and the picture comes back down after it
    for k in 1..n {
        let (Some(prev), Some(cur)) = (s.get(k - 1), s.get(k)) else { continue };
        let jump = cur.thumb.mean() - prev.thumb.mean();
        if jump < o.flash_jump {
            continue;
        }
        let back = s.get(k + 1).map(|next| cur.thumb.mean() - next.thumb.mean());
        if back.is_some_and(|b| b >= 0.5 * o.flash_jump) {
            out.push(Finding { kind: Kind::Flash, start: cur.time, end: until(k), value: jump as f64 });
        }
    }
    out.sort_by_key(|f| (f.start, f.end));
    out
}
