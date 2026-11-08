//! Pitch Shifter: a phase vocoder. Overlapping Hann-windowed frames (FFT size, Overlap) are
//! transformed; each bin's true frequency is estimated from its phase advance since the previous
//! frame; bins are moved to `pitch` times their frequency, phases re-accumulated, and frames
//! transformed back and overlap-added. A steady tone comes out at exactly `pitch` times its
//! frequency (unlike delay-line shifters, which smear it into grain-rate sidebands). Latency:
//! FFT size minus one hop.

use std::f32::consts::TAU;

/// Largest FFT size (buffers are allocated for it once).
pub const MAX_FFT: usize = 4096;

/// In-place radix-2 FFT of `re`/`im` (length a power of two). `inverse` skips the 1/N scale.
pub fn fft(re: &mut [f32], im: &mut [f32], inverse: bool) {
    let n = re.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            re.swap(i, j);
            im.swap(i, j);
        }
    }
    let sign = if inverse { 1.0 } else { -1.0 };
    let mut len = 2;
    while len <= n {
        let ang = sign * TAU / len as f32;
        let (ws, wc) = ang.sin_cos();
        for start in (0..n).step_by(len) {
            let (mut cr, mut ci) = (1.0f32, 0.0f32);
            for k in 0..len / 2 {
                let (a, b) = (start + k, start + k + len / 2);
                let tr = re[b] * cr - im[b] * ci;
                let ti = re[b] * ci + im[b] * cr;
                re[b] = re[a] - tr;
                im[b] = im[a] - ti;
                re[a] += tr;
                im[a] += ti;
                let nr = cr * wc - ci * ws;
                ci = cr * ws + ci * wc;
                cr = nr;
            }
        }
        len <<= 1;
    }
}

#[derive(Clone)]
struct Channel {
    in_fifo: Vec<f32>,
    out_fifo: Vec<f32>,
    acc: Vec<f32>,
    last_phase: Vec<f32>,
    sum_phase: Vec<f32>,
}

impl Channel {
    fn new() -> Self {
        Self {
            in_fifo: vec![0.0; MAX_FFT],
            out_fifo: vec![0.0; MAX_FFT],
            acc: vec![0.0; 2 * MAX_FFT],
            last_phase: vec![0.0; MAX_FFT / 2 + 1],
            sum_phase: vec![0.0; MAX_FFT / 2 + 1],
        }
    }

    fn clear(&mut self) {
        for v in [
            &mut self.in_fifo,
            &mut self.out_fifo,
            &mut self.acc,
            &mut self.last_phase,
            &mut self.sum_phase,
        ] {
            v.fill(0.0);
        }
    }
}

/// Shared scratch for one frame.
#[derive(Clone)]
struct Work {
    re: Vec<f32>,
    im: Vec<f32>,
    ana_mag: Vec<f32>,
    ana_freq: Vec<f32>,
    syn_mag: Vec<f32>,
    syn_freq: Vec<f32>,
    window: Vec<f32>,
}

#[derive(Clone)]
pub struct PitchShifter {
    ch: [Channel; 2],
    w: Work,
    n: usize,
    osamp: usize,
    rover: usize,
}

impl Default for PitchShifter {
    fn default() -> Self {
        Self::new()
    }
}

impl PitchShifter {
    pub fn new() -> Self {
        let half = MAX_FFT / 2 + 1;
        Self {
            ch: [Channel::new(), Channel::new()],
            w: Work {
                re: vec![0.0; MAX_FFT],
                im: vec![0.0; MAX_FFT],
                ana_mag: vec![0.0; half],
                ana_freq: vec![0.0; half],
                syn_mag: vec![0.0; half],
                syn_freq: vec![0.0; half],
                window: vec![0.0; MAX_FFT],
            },
            n: 0,
            osamp: 0,
            rover: 0,
        }
    }

    /// FFT size (rounded to a power of two, 256..4096) and overlap (2..32, a power of two).
    fn configure(&mut self, fft_size: f32, overlap: f32) {
        let n = (fft_size.clamp(256.0, MAX_FFT as f32) as usize)
            .next_power_of_two()
            .min(MAX_FFT);
        let osamp = (overlap.clamp(2.0, 32.0) as usize)
            .next_power_of_two()
            .min(32);
        if n == self.n && osamp == self.osamp {
            return;
        }
        self.n = n;
        self.osamp = osamp;
        for c in &mut self.ch {
            c.clear();
        }
        for (k, w) in self.w.window[..n].iter_mut().enumerate() {
            *w = 0.5 - 0.5 * (TAU * k as f32 / n as f32).cos();
        }
        self.rover = n - n / osamp;
    }

    /// Shift `buf` (stereo) by `pitch` (0.5..2).
    pub fn process(&mut self, pitch: f32, fft_size: f32, overlap: f32, buf: &mut [super::Frame]) {
        self.configure(fft_size, overlap);
        let (n, hop) = (self.n, self.n / self.osamp);
        let latency = n - hop;
        for f in buf.iter_mut() {
            let r = self.rover;
            self.ch[0].in_fifo[r] = f.left;
            self.ch[1].in_fifo[r] = f.right;
            f.left = self.ch[0].out_fifo[r - latency];
            f.right = self.ch[1].out_fifo[r - latency];
            self.rover += 1;
            if self.rover >= n {
                self.rover = latency;
                for c in 0..2 {
                    frame(&mut self.ch[c], &mut self.w, n, self.osamp, pitch);
                }
            }
        }
    }
}

/// Analyse, shift and resynthesise one frame of one channel.
fn frame(c: &mut Channel, w: &mut Work, n: usize, osamp: usize, pitch: f32) {
    let half = n / 2;
    let hop = n / osamp;
    let expected = TAU * hop as f32 / n as f32;
    for k in 0..n {
        w.re[k] = c.in_fifo[k] * w.window[k];
        w.im[k] = 0.0;
    }
    fft(&mut w.re[..n], &mut w.im[..n], false);
    for k in 0..=half {
        let (re, im) = (w.re[k], w.im[k]);
        let phase = im.atan2(re);
        let mut d = phase - c.last_phase[k] - k as f32 * expected;
        c.last_phase[k] = phase;
        d -= TAU * (d / TAU).round();
        w.ana_mag[k] = 2.0 * (re * re + im * im).sqrt();
        // True frequency in bins.
        w.ana_freq[k] = k as f32 + d * osamp as f32 / TAU;
    }
    w.syn_mag[..=half].fill(0.0);
    w.syn_freq[..=half].fill(0.0);
    for k in 0..=half {
        let to = (k as f32 * pitch).round() as usize;
        if to <= half {
            w.syn_mag[to] += w.ana_mag[k];
            w.syn_freq[to] = w.ana_freq[k] * pitch;
        }
    }
    for k in 0..n {
        if k <= half {
            let d = (w.syn_freq[k] - k as f32) * TAU / osamp as f32 + k as f32 * expected;
            c.sum_phase[k] = (c.sum_phase[k] + d) % (TAU * 64.0);
            let (s, co) = c.sum_phase[k].sin_cos();
            w.re[k] = w.syn_mag[k] * co;
            w.im[k] = w.syn_mag[k] * s;
        } else {
            w.re[k] = 0.0;
            w.im[k] = 0.0;
        }
    }
    fft(&mut w.re[..n], &mut w.im[..n], true);
    let scale = 2.0 / (half as f32 * osamp as f32);
    for k in 0..n {
        c.acc[k] += w.window[k] * w.re[k] * scale;
    }
    c.out_fifo[..hop].copy_from_slice(&c.acc[..hop]);
    c.acc.copy_within(hop..hop + n, 0);
    c.acc[n..n + hop].fill(0.0);
    c.in_fifo.copy_within(hop..n, 0);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fft_round_trips_and_finds_a_bin() {
        let n = 64;
        let mut re: Vec<f32> = (0..n)
            .map(|i| (TAU * 5.0 * i as f32 / n as f32).cos())
            .collect();
        let mut im = vec![0.0; n];
        let orig = re.clone();
        fft(&mut re, &mut im, false);
        assert!((re[5] - n as f32 / 2.0).abs() < 1e-3 && re[6].abs() < 1e-3);
        fft(&mut re, &mut im, true);
        for (a, b) in re.iter().zip(&orig) {
            assert!((a / n as f32 - b).abs() < 1e-5);
        }
    }
}
