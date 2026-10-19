//! bashcut-analysis on generated signals: a −23 LUFS reference tone, a 120 BPM click track, and
//! generated black / frozen / flashing frames; plus hostile inputs.

use bashcut_analysis::beats::{self, click_track};
use bashcut_analysis::loudness::{self, loud_stretches};
use bashcut_analysis::picture::{self, Kind, Options, Sample, Thumb};

const SR: u32 = 48_000;

fn sine(amp: f32, freq: f64, seconds: f64) -> Vec<f32> {
    let n = (SR as f64 * seconds) as usize;
    (0..n).map(|i| amp * (2.0 * std::f64::consts::PI * freq * i as f64 / SR as f64).sin() as f32).collect()
}

#[test]
fn reference_tone_measures_minus_23_lufs() {
    // EBU Tech 3341 case 1: stereo 1 kHz sine at −23 dBFS on both channels reads −23.0 LUFS
    let amp = 10f32.powf(-23.0 / 20.0);
    let x = sine(amp, 1000.0, 20.0);
    let l = loudness::measure(&[&x, &x], SR);
    let i = l.integrated_lufs.expect("tone is measurable");
    assert!((i + 23.0).abs() < 0.2, "integrated {i}");
    let tp = l.true_peak_dbtp.expect("tone has a peak");
    assert!((tp + 23.0).abs() < 0.5, "true peak {tp}");
    assert_eq!(l.clipped_samples, 0);
    assert_eq!(l.short_term.len(), 20);
    assert!(l.short_term.iter().skip(3).all(|v| v.is_some_and(|v| (v + 23.0).abs() < 0.3)), "{:?}", l.short_term);
}

#[test]
fn streaming_matches_one_call_and_counts_clipping() {
    let x = sine(1.0, 440.0, 4.0);
    let whole = loudness::measure(&[&x], SR);
    let mut m = loudness::Meter::new(SR, 1);
    for chunk in x.chunks(7_919) {
        m.push(&[chunk]);
    }
    let parts = m.finish();
    assert_eq!(whole.integrated_lufs.map(|v| (v * 1000.0).round()), parts.integrated_lufs.map(|v| (v * 1000.0).round()));
    assert_eq!(whole.short_term.len(), parts.short_term.len());
    assert!(parts.clipped_samples > 0, "a full-scale sine touches the clip level");
}

#[test]
fn loud_stretches_are_found() {
    let series = vec![Some(-30.0), Some(-12.0), Some(-11.0), None, Some(-25.0), Some(-10.0)];
    assert_eq!(loud_stretches(&series, -14.0), vec![(1, 3, -11.0), (5, 6, -10.0)]);
}

#[test]
fn click_track_at_120_bpm() {
    let (x, truth) = click_track(SR, 120.0, 20.0);
    let b = beats::detect(&[&x], SR).expect("a click track has beats");
    assert!((b.bpm - 120.0).abs() < 1.5, "bpm {}", b.bpm);
    assert!(b.beats.len() >= truth.len() - 3, "{} beats of {}", b.beats.len(), truth.len());
    let tol = (SR as f64 * 0.025) as i64;
    for t in &b.beats {
        assert!(truth.iter().any(|g| (g - t).abs() <= tol), "beat at {t} is not near a click");
    }
    assert!(b.downbeats.len() >= 4);
    for w in b.downbeats.windows(2) {
        let gap = w[1] - w[0];
        assert!((gap - 2 * SR as i64).abs() <= tol, "downbeats {gap} samples apart");
    }
    // the accented clicks (first of each bar) are the downbeats
    for d in &b.downbeats {
        let k = truth.iter().position(|g| (g - d).abs() <= tol).expect("downbeat on a click");
        assert_eq!(k % 4, 0, "downbeat on click {k}");
    }
}

#[test]
fn no_beats_in_silence_tone_or_hostile_input() {
    assert!(beats::detect(&[&vec![0.0; SR as usize * 5]], SR).is_none());
    let _ = beats::detect(&[&sine(0.5, 440.0, 5.0)], SR);
    assert!(beats::detect(&[], SR).is_none());
    assert!(beats::detect(&[&[0.5; 100]], 0).is_none());
    assert!(beats::detect(&[&[f32::NAN; 48_000]], SR).is_none());
    let (x, _) = click_track(100, 120.0, 10.0);
    let _ = beats::detect(&[&x], 100);
    let l = loudness::measure(&[], 0);
    assert_eq!(l.integrated_lufs, None);
    let silent = loudness::measure(&[&vec![0.0; 48_000]], SR);
    assert_eq!(silent.integrated_lufs, None);
    assert_eq!(silent.true_peak_dbtp, None);
}

fn rgba(w: usize, h: usize, v: u8) -> Vec<u8> {
    (0..w * h).flat_map(|_| [v, v, v, 255]).collect()
}

/// 10 samples a second from `[(seconds, luma byte)]` runs.
fn frames(runs: &[(f64, u8)], noise: bool) -> (Vec<Sample>, i64) {
    let mut out = Vec::new();
    let mut t = 0i64;
    for (k, (secs, v)) in runs.iter().enumerate() {
        for i in 0..(secs * 10.0) as usize {
            // moving pictures: a bright square that walks across the frame
            let mut px = rgba(64, 36, *v);
            if noise && *v > 20 {
                let x0 = (i * 3 + k * 7) % 56;
                for y in 10..20 {
                    for x in x0..x0 + 8 {
                        let o = (y * 64 + x) * 4;
                        px[o..o + 3].copy_from_slice(&[255, 255, 255]);
                    }
                }
            }
            out.push(Sample { time: t, thumb: Thumb::from_rgba8(64, 36, &px).expect("valid frame") });
            t += 100;
        }
    }
    (out, t)
}

#[test]
fn black_frozen_still_and_flash() {
    let o = Options::per_second(1000);
    // moving picture 2 s, black 1 s, moving 2 s
    let (s, end) = frames(&[(2.0, 120), (1.0, 0), (2.0, 120)], true);
    let f = picture::check(&s, end, &o);
    let black: Vec<_> = f.iter().filter(|f| f.kind == Kind::Black).collect();
    assert_eq!(black.len(), 1, "{f:?}");
    assert_eq!((black[0].start, black[0].end), (2000, 3000));
    assert!(f.iter().all(|f| f.kind == Kind::Black), "{f:?}");

    // a frozen picture for 2 s and a still for 6 s
    let (s, end) = frames(&[(1.0, 120), (2.0, 90), (1.0, 120), (6.0, 60), (1.0, 120)], false);
    let mut moving: Vec<Sample> = Vec::new();
    let (m, _) = frames(&[(1.0, 120), (2.0, 90), (1.0, 120), (6.0, 60), (1.0, 120)], true);
    for (a, b) in s.iter().zip(m) {
        // only the 90 and 60 runs stay unchanged
        let mean = a.thumb.mean();
        moving.push(if (mean - 90.0 / 255.0).abs() < 0.01 || (mean - 60.0 / 255.0).abs() < 0.01 { a.clone() } else { b });
    }
    let f = picture::check(&moving, end, &o);
    let frozen: Vec<_> = f.iter().filter(|f| f.kind == Kind::Frozen).collect();
    let still: Vec<_> = f.iter().filter(|f| f.kind == Kind::LongStill).collect();
    assert_eq!(frozen.len(), 1, "{f:?}");
    assert_eq!((frozen[0].start, frozen[0].end), (1000, 3000));
    assert_eq!(still.len(), 1, "{f:?}");
    assert_eq!((still[0].start, still[0].end), (4000, 10_000));

    // a one-frame white flash in a dark scene
    let mut s: Vec<Sample> = (0..20).map(|i| Sample { time: i * 100, thumb: Thumb::flat(0.2 + (i % 2) as f32 * 0.01) }).collect();
    s[10].thumb = Thumb::flat(0.95);
    let f = picture::check(&s, 2000, &o);
    let flash: Vec<_> = f.iter().filter(|f| f.kind == Kind::Flash).collect();
    assert_eq!(flash.len(), 1, "{f:?}");
    assert_eq!((flash[0].start, flash[0].end), (1000, 1100));
}

#[test]
fn hostile_frames_do_not_crash() {
    assert!(Thumb::from_rgba8(0, 0, &[]).is_none());
    assert!(Thumb::from_rgba8(usize::MAX, 2, &[0; 16]).is_none());
    assert!(Thumb::from_rgba8(4, 4, &[0; 10]).is_none());
    let tiny = Thumb::from_rgba8(1, 1, &[255, 255, 255, 255]).expect("1×1 frame");
    assert!((tiny.mean() - 1.0).abs() < 1e-6);
    assert!(picture::check(&[], 0, &Options::per_second(0)).is_empty());
    let one = [Sample { time: i64::MAX, thumb: Thumb::flat(0.0) }];
    let _ = picture::check(&one, i64::MIN, &Options::per_second(i64::MAX));
    assert_eq!(Thumb { luma: vec![] }.diff(&Thumb::flat(0.5)), 1.0);
}
