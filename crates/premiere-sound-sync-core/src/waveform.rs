//! Matches video to production audio by sound, for clips with no usable timecode.
//!
//! Each file is decoded with ffmpeg to mono PCM at a low sample rate and fingerprinted as both
//! the raw samples and a coarse amplitude envelope. Finding an offset between two fingerprints
//! happens in two passes:
//!
//! 1. A coarse pass cross-correlates the envelopes with phase-transform (GCC-PHAT) weighting,
//!    which is cheap even for long clips and copes with the two recordings having different mic
//!    tone, gain or EQ.
//! 2. A fine pass cross-correlates the raw samples in a small window around the coarse estimate,
//!    giving an offset accurate to a fraction of a video frame.

use std::path::Path;
use std::process::{Command, Stdio};

use anyhow::{anyhow, bail, Context, Result};
use rustfft::num_complex::Complex;
use rustfft::FftPlanner;

use crate::probe::ffmpeg_path;
use crate::time::TICKS_PER_SECOND;

/// Sample rate audio is decoded to for fingerprinting.
const SAMPLE_RATE: u32 = 8_000;
/// Envelope bins per second; coarse enough to cross-correlate quickly over long takes.
const ENVELOPE_RATE: f64 = 500.0;
/// A match is rejected unless its best peak beats the next-best peak by at least this ratio.
const CONFIDENCE_THRESHOLD: f64 = 4.0;
/// How far, in seconds, the fine pass searches around the coarse estimate.
const REFINE_WINDOW_SECS: f64 = 0.02;

/// A file's audio, decoded and reduced to what's needed to correlate it against another file.
#[derive(Debug, Clone)]
pub struct Fingerprint {
    samples: Vec<f32>,
    envelope: Vec<f32>,
}

impl Fingerprint {
    /// Builds a fingerprint directly from mono PCM samples at [`SAMPLE_RATE`]. Exposed mainly so
    /// tests (and any future caller with already-decoded audio) can skip invoking ffmpeg.
    pub fn from_samples(samples: Vec<f32>) -> Self {
        let envelope = downsample_envelope(&samples);
        Self { samples, envelope }
    }
}

fn downsample_envelope(samples: &[f32]) -> Vec<f32> {
    let bin = (SAMPLE_RATE as f64 / ENVELOPE_RATE).round().max(1.0) as usize;
    samples
        .chunks(bin)
        .map(|chunk| {
            let energy: f64 = chunk.iter().map(|&s| (s as f64) * (s as f64)).sum();
            (energy / chunk.len().max(1) as f64).sqrt() as f32
        })
        .collect()
}

/// Decodes `path`'s first audio stream to mono [`SAMPLE_RATE`] and fingerprints it.
pub fn fingerprint(path: &Path) -> Result<Fingerprint> {
    let ffmpeg = ffmpeg_path().ok_or_else(|| anyhow!("ffmpeg not found (install FFmpeg)"))?;
    let output = Command::new(ffmpeg)
        .arg("-v")
        .arg("error")
        .arg("-i")
        .arg(path)
        .args(["-map", "0:a:0", "-ac", "1", "-ar", &SAMPLE_RATE.to_string(), "-f", "f32le", "-"])
        .stdin(Stdio::null())
        .output()
        .context("running ffmpeg")?;
    if !output.status.success() {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let samples = bytes_to_f32(&output.stdout);
    if samples.is_empty() {
        bail!("no audio decoded");
    }
    Ok(Fingerprint::from_samples(samples))
}

fn bytes_to_f32(bytes: &[u8]) -> Vec<f32> {
    bytes.chunks_exact(4).map(|c| f32::from_le_bytes([c[0], c[1], c[2], c[3]])).collect()
}

/// An alignment found between two fingerprints.
#[derive(Debug, Clone, Copy)]
pub struct Match {
    /// `video.position = audio.position + offset_ticks`.
    pub offset_ticks: i64,
    /// Ratio of the best correlation peak to the next-best, unrelated peak. Higher is safer.
    pub confidence: f64,
}

/// Finds how `a` (typically a camera's scratch track) aligns onto `b` (production audio).
/// Returns `None` when there isn't enough signal to be confident the two actually match.
pub fn correlate(a: &Fingerprint, b: &Fingerprint) -> Option<Match> {
    let (coarse_lag_bins, confidence) = best_lag(&a.envelope, &b.envelope)?;
    if !confidence.is_finite() || confidence < CONFIDENCE_THRESHOLD {
        return None;
    }
    let coarse_lag_samples = (coarse_lag_bins as f64 * SAMPLE_RATE as f64 / ENVELOPE_RATE).round() as i64;
    let window = (REFINE_WINDOW_SECS * SAMPLE_RATE as f64).round() as i64;
    let lag_samples = refine(&a.samples, &b.samples, coarse_lag_samples, window);
    let tick_per_sample = TICKS_PER_SECOND / SAMPLE_RATE as i64;
    Some(Match {
        offset_ticks: -lag_samples * tick_per_sample,
        confidence,
    })
}

/// GCC-PHAT cross-correlation. Returns `(lag, confidence)` where `a[i]` best lines up with
/// `b[i - lag]`.
fn best_lag(a: &[f32], b: &[f32]) -> Option<(i64, f64)> {
    if a.is_empty() || b.is_empty() {
        return None;
    }
    let n = (a.len() + b.len()).next_power_of_two();
    let mut planner = FftPlanner::<f64>::new();
    let fft = planner.plan_fft_forward(n);
    let ifft = planner.plan_fft_inverse(n);

    let mut fa = pad(a, n);
    let mut fb = pad(b, n);
    fft.process(&mut fa);
    fft.process(&mut fb);

    let mut cross: Vec<Complex<f64>> = fa
        .iter()
        .zip(fb.iter())
        .map(|(&x, &y)| {
            let c = x * y.conj();
            let mag = c.norm();
            if mag > 1e-12 {
                c / mag
            } else {
                Complex::new(0.0, 0.0)
            }
        })
        .collect();
    ifft.process(&mut cross);

    let max_lag = a.len().max(b.len()) as i64;
    let mut best_idx = 0usize;
    let mut best_val = f64::MIN;
    for (idx, c) in cross.iter().enumerate() {
        if idx_to_lag(idx, n).abs() > max_lag {
            continue;
        }
        if c.re > best_val {
            best_val = c.re;
            best_idx = idx;
        }
    }

    let guard = (n / 64).max(4) as i64;
    let mut second_val = f64::MIN;
    for (idx, c) in cross.iter().enumerate() {
        if idx_to_lag(idx, n).abs() > max_lag {
            continue;
        }
        let dist = (idx as i64 - best_idx as i64).rem_euclid(n as i64);
        let dist = dist.min(n as i64 - dist);
        if dist < guard {
            continue;
        }
        second_val = second_val.max(c.re);
    }

    let confidence = if second_val > 1e-9 { best_val / second_val } else { f64::INFINITY };
    Some((idx_to_lag(best_idx, n), confidence.max(0.0)))
}

fn idx_to_lag(idx: usize, n: usize) -> i64 {
    if idx <= n / 2 {
        idx as i64
    } else {
        idx as i64 - n as i64
    }
}

fn pad(data: &[f32], n: usize) -> Vec<Complex<f64>> {
    let mut v: Vec<Complex<f64>> = data.iter().map(|&x| Complex::new(x as f64, 0.0)).collect();
    v.resize(n, Complex::new(0.0, 0.0));
    v
}

/// Time-domain refinement around the coarse lag, at full sample resolution.
fn refine(a: &[f32], b: &[f32], coarse_lag: i64, window: i64) -> i64 {
    let mut best_lag = coarse_lag;
    let mut best_score = f64::MIN;
    for d in -window..=window {
        let lag = coarse_lag + d;
        let score = normalized_corr_at_lag(a, b, lag);
        if score > best_score {
            best_score = score;
            best_lag = lag;
        }
    }
    best_lag
}

/// Normalized cross-correlation of `a` and `b` at `lag`, where `a[i]` is paired with `b[i - lag]`.
fn normalized_corr_at_lag(a: &[f32], b: &[f32], lag: i64) -> f64 {
    let (mut sab, mut saa, mut sbb) = (0.0f64, 0.0f64, 0.0f64);
    let mut n = 0usize;
    for (i, &av) in a.iter().enumerate() {
        let j = i as i64 - lag;
        if j < 0 || j as usize >= b.len() {
            continue;
        }
        let (av, bv) = (av as f64, b[j as usize] as f64);
        sab += av * bv;
        saa += av * av;
        sbb += bv * bv;
        n += 1;
    }
    if n == 0 || saa <= 0.0 || sbb <= 0.0 {
        return f64::MIN;
    }
    sab / (saa.sqrt() * sbb.sqrt())
}

/// Deterministic irregular bursty noise, used by tests here and in [`crate::sync`] to stand in
/// for two different recordings of the same real-world sound.
#[cfg(test)]
pub(crate) fn synthetic_signal(len: usize, seed: u64) -> Vec<f32> {
    let mut state = (seed.wrapping_mul(2_685_821_657_736_338_717) | 1).max(1);
    let mut next = move || {
        state ^= state << 13;
        state ^= state >> 7;
        state ^= state << 17;
        state
    };
    let mut out = vec![0.0f32; len];
    let mut i = 0usize;
    while i < len {
        i += 50 + (next() % 300) as usize;
        if i >= len {
            break;
        }
        let burst_len = 20 + (next() % 60) as usize;
        for j in 0..burst_len {
            if i + j >= len {
                break;
            }
            out[i + j] = ((next() as f64 / u64::MAX as f64) * 2.0 - 1.0) as f32;
        }
        i += burst_len;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recovers_a_known_offset() {
        let base = synthetic_signal(8_000, 1);
        let shift = 237i64;
        let mut shifted = vec![0.0f32; base.len() + shift as usize + 50];
        for (i, &s) in base.iter().enumerate() {
            shifted[i + shift as usize] = s * 0.6;
        }
        let noise = synthetic_signal(shifted.len(), 2);
        for (s, n) in shifted.iter_mut().zip(noise) {
            *s += n * 0.02;
        }

        let a = Fingerprint::from_samples(base);
        let b = Fingerprint::from_samples(shifted);
        let m = correlate(&a, &b).expect("a shifted copy of the same signal should match");

        let tick_per_sample = TICKS_PER_SECOND / SAMPLE_RATE as i64;
        let expected = shift * tick_per_sample;
        assert!(
            (m.offset_ticks - expected).abs() <= tick_per_sample,
            "offset {} expected close to {}",
            m.offset_ticks,
            expected
        );
        assert!(m.confidence >= CONFIDENCE_THRESHOLD);
    }

    #[test]
    fn unrelated_signals_are_rejected() {
        let a = Fingerprint::from_samples(synthetic_signal(8_000, 10));
        let b = Fingerprint::from_samples(synthetic_signal(8_000, 20));
        assert!(correlate(&a, &b).is_none());
    }
}
