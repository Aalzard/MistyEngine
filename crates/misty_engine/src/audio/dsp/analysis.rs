//! Measurements on rendered buffers: levels, the amplitude of one frequency, the dominant
//! frequency, and the delay between two signals. Used by the DSP tests and benchmarks.

use std::f32::consts::TAU;

use super::Frame;

/// Samples of one side (`right` = false: left).
pub fn side(frames: &[Frame], right: bool) -> Vec<f32> {
    frames
        .iter()
        .map(|f| if right { f.right } else { f.left })
        .collect()
}

/// Root mean square of `x`.
pub fn rms(x: &[f32]) -> f32 {
    if x.is_empty() {
        return 0.0;
    }
    (x.iter().map(|v| v * v).sum::<f32>() / x.len() as f32).sqrt()
}

/// Largest absolute sample.
pub fn peak(x: &[f32]) -> f32 {
    x.iter().fold(0.0, |m, v| m.max(v.abs()))
}

/// Amplitude of the `freq` component of `x` (a sine of amplitude A reads A), by correlating with
/// a sine and a cosine over a Hann window.
pub fn amplitude_at(x: &[f32], freq: f32, sample_rate: f32) -> f32 {
    let n = x.len();
    if n == 0 {
        return 0.0;
    }
    let (mut re, mut im, mut wsum) = (0.0f64, 0.0f64, 0.0f64);
    let w = TAU * freq / sample_rate;
    for (i, v) in x.iter().enumerate() {
        let win = 0.5 - 0.5 * (TAU * i as f32 / n as f32).cos();
        let ph = w * i as f32;
        re += (v * win * ph.cos()) as f64;
        im += (v * win * ph.sin()) as f64;
        wsum += win as f64;
    }
    (2.0 * (re * re + im * im).sqrt() / wsum) as f32
}

/// Frequency (Hz, 1 Hz steps refined to 0.1 Hz) with the most energy between `lo` and `hi`.
pub fn dominant_frequency(x: &[f32], sample_rate: f32, lo: f32, hi: f32) -> f32 {
    let scan = |from: f32, to: f32, step: f32| {
        let mut best = (from, 0.0);
        let mut f = from;
        while f <= to {
            let a = amplitude_at(x, f, sample_rate);
            if a > best.1 {
                best = (f, a);
            }
            f += step;
        }
        best.0
    };
    let coarse = scan(lo, hi, 1.0);
    scan(coarse - 1.0, coarse + 1.0, 0.1)
}

/// Lag (samples) by which `b` trails `a`, searched within ±`max_lag` by cross-correlation.
pub fn delay_between(a: &[f32], b: &[f32], max_lag: usize) -> isize {
    let n = a.len().min(b.len());
    let mut best = (0isize, f32::MIN);
    for lag in -(max_lag as isize)..=max_lag as isize {
        let end = n.saturating_sub(max_lag).max(max_lag);
        let sum: f32 = a[max_lag..end]
            .iter()
            .enumerate()
            .map(|(k, x)| x * b[((max_lag + k) as isize + lag) as usize])
            .sum();
        if sum > best.1 {
            best = (lag, sum);
        }
    }
    best.0
}

/// A mono sine of `seconds` at `freq` and amplitude `amp`, as stereo frames.
pub fn sine(freq: f32, amp: f32, seconds: f32, sample_rate: f32) -> Vec<Frame> {
    let n = (seconds * sample_rate) as usize;
    (0..n)
        .map(|i| Frame::from_mono(amp * (TAU * freq * i as f32 / sample_rate).sin()))
        .collect()
}

/// Deterministic white noise (xorshift), amplitude `amp`, as mono-in-stereo frames.
pub fn noise(seed: u32, amp: f32, seconds: f32, sample_rate: f32) -> Vec<Frame> {
    let n = (seconds * sample_rate) as usize;
    let mut s = seed.max(1);
    (0..n)
        .map(|_| {
            s ^= s << 13;
            s ^= s >> 17;
            s ^= s << 5;
            Frame::from_mono(amp * (s as f32 / u32::MAX as f32 * 2.0 - 1.0))
        })
        .collect()
}

/// Largest sample-to-sample jump (a click detector: smooth signals stay small).
pub fn max_step(x: &[f32]) -> f32 {
    x.windows(2).fold(0.0, |m, w| m.max((w[1] - w[0]).abs()))
}
