//! Effect processors: one per [`EffectKind`], each turning a block of stereo frames into the
//! effect's output in place. Parameter values arrive in [`EffectKind::params`] order every block;
//! changes that would click (delay times, filter corners) are smoothed inside the processor.
//!
//! The algorithms are textbook ones: RBJ-cookbook biquads for the filters and the parametric EQ,
//! a damped feedback delay for Echo, a Schroeder–Moorer network (eight lowpass-feedback combs and
//! four allpasses per side) for Reverb, a feed-forward compressor with a soft knee for Compressor
//! and Duck Volume, modulated delay taps for Chorus and Flange, and a phase vocoder for the Pitch
//! Shifter ([`super::pitch`]).

use std::f32::consts::{LN_2, PI, TAU};

use super::pitch::PitchShifter;
use super::{Frame, one_pole};
use crate::audio::effects::{EffectKind, amplitude_to_db, db_to_amplitude};

/// Value `i` of `values`, or the parameter's default when missing.
pub fn value(kind: EffectKind, values: &[f32], i: usize) -> f32 {
    values
        .get(i)
        .copied()
        .unwrap_or_else(|| kind.params().get(i).map_or(0.0, |d| d.default))
}

// ---- building blocks -------------------------------------------------------------------------

/// Stereo delay line with linearly interpolated reads.
#[derive(Clone)]
pub struct DelayLine {
    buf: Vec<Frame>,
    /// Next write position.
    pos: usize,
}

impl DelayLine {
    pub fn new(len: usize) -> Self {
        Self {
            buf: vec![Frame::ZERO; len.max(4)],
            pos: 0,
        }
    }

    pub fn len(&self) -> usize {
        self.buf.len()
    }

    pub fn is_empty(&self) -> bool {
        self.buf.is_empty()
    }

    #[inline]
    pub fn push(&mut self, f: Frame) {
        self.buf[self.pos] = f;
        self.pos += 1;
        if self.pos == self.buf.len() {
            self.pos = 0;
        }
    }

    /// The frame `delay` samples ago (1 = the newest), fractional delays interpolated.
    #[inline]
    pub fn read(&self, delay: f32) -> Frame {
        let len = self.buf.len();
        let d = delay.clamp(1.0, (len - 2) as f32);
        let x = self.pos as f32 + len as f32 - d;
        let i0 = x as usize;
        let frac = x - i0 as f32;
        let a = self.buf[i0 % len];
        let b = self.buf[(i0 + 1) % len];
        a + (b - a) * frac
    }

    pub fn clear(&mut self) {
        self.buf.fill(Frame::ZERO);
    }
}

/// Biquad filter coefficients (normalised, `a0` = 1).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Coeffs {
    pub b0: f32,
    pub b1: f32,
    pub b2: f32,
    pub a1: f32,
    pub a2: f32,
}

impl Coeffs {
    fn from_raw(b0: f32, b1: f32, b2: f32, a0: f32, a1: f32, a2: f32) -> Self {
        Self {
            b0: b0 / a0,
            b1: b1 / a0,
            b2: b2 / a0,
            a1: a1 / a0,
            a2: a2 / a0,
        }
    }

    fn w0(freq: f32, sample_rate: f32) -> f32 {
        TAU * freq.clamp(10.0, sample_rate * 0.49) / sample_rate
    }

    pub fn lowpass(freq: f32, q: f32, sample_rate: f32) -> Self {
        let w = Self::w0(freq, sample_rate);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.1));
        Self::from_raw(
            (1.0 - c) * 0.5,
            1.0 - c,
            (1.0 - c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    pub fn highpass(freq: f32, q: f32, sample_rate: f32) -> Self {
        let w = Self::w0(freq, sample_rate);
        let (s, c) = w.sin_cos();
        let alpha = s / (2.0 * q.max(0.1));
        Self::from_raw(
            (1.0 + c) * 0.5,
            -(1.0 + c),
            (1.0 + c) * 0.5,
            1.0 + alpha,
            -2.0 * c,
            1.0 - alpha,
        )
    }

    /// Peaking EQ: linear `gain` at `freq`, `octaves` wide (between the half-gain points).
    pub fn peaking(freq: f32, octaves: f32, gain: f32, sample_rate: f32) -> Self {
        let w = Self::w0(freq, sample_rate);
        let (s, c) = w.sin_cos();
        let a = gain.max(1e-4).sqrt();
        let alpha = s * (LN_2 / 2.0 * octaves.max(0.01) * w / s).sinh();
        Self::from_raw(
            1.0 + alpha * a,
            -2.0 * c,
            1.0 - alpha * a,
            1.0 + alpha / a,
            -2.0 * c,
            1.0 - alpha / a,
        )
    }

    /// Magnitude response (linear) at `freq`.
    pub fn magnitude(&self, freq: f32, sample_rate: f32) -> f32 {
        let w = TAU * freq / sample_rate;
        let (s1, c1) = w.sin_cos();
        let (s2, c2) = (2.0 * w).sin_cos();
        let num_re = self.b0 + self.b1 * c1 + self.b2 * c2;
        let num_im = -(self.b1 * s1 + self.b2 * s2);
        let den_re = 1.0 + self.a1 * c1 + self.a2 * c2;
        let den_im = -(self.a1 * s1 + self.a2 * s2);
        ((num_re * num_re + num_im * num_im) / (den_re * den_re + den_im * den_im)).sqrt()
    }
}

/// Stereo biquad (transposed direct form II).
#[derive(Clone, Default)]
pub struct Biquad {
    pub c: Coeffs,
    s: [[f32; 2]; 2],
    /// Values the coefficients were made from (recomputed only when they change).
    made_from: [f32; 3],
}

impl Biquad {
    #[inline]
    fn tick(c: &Coeffs, s: &mut [f32; 2], x: f32) -> f32 {
        let y = c.b0 * x + s[0];
        s[0] = c.b1 * x - c.a1 * y + s[1];
        s[1] = c.b2 * x - c.a2 * y;
        y
    }

    pub fn process(&mut self, buf: &mut [Frame]) {
        let c = self.c;
        let [l, r] = &mut self.s;
        for f in buf {
            f.left = Self::tick(&c, l, f.left);
            f.right = Self::tick(&c, r, f.right);
        }
        // Flush denormals.
        for s in self.s.iter_mut().flatten() {
            if s.abs() < 1e-20 {
                *s = 0.0;
            }
        }
    }

    /// Recompute the coefficients when `key` (the values they depend on) changed.
    fn update(&mut self, key: [f32; 3], make: impl FnOnce() -> Coeffs) {
        if key != self.made_from || self.c == Coeffs::default() {
            self.made_from = key;
            self.c = make();
        }
    }
}

/// The lowpass / highpass "Resonance" (1..10) as a filter Q: 1 is a flat (Butterworth) corner.
pub fn resonance_q(resonance: f32) -> f32 {
    resonance.max(0.1) * std::f32::consts::FRAC_1_SQRT_2
}

// ---- echo ------------------------------------------------------------------------------------

#[derive(Clone)]
pub struct Echo {
    line: DelayLine,
    delay: f32,
}

impl Echo {
    fn new(sample_rate: f32) -> Self {
        Self {
            line: DelayLine::new((5.1 * sample_rate) as usize),
            delay: 0.0,
        }
    }

    fn process(&mut self, v: impl Fn(usize) -> f32, buf: &mut [Frame], sr: f32) {
        let target = (v(0) * 0.001 * sr).clamp(1.0, self.line.len() as f32 - 3.0);
        if self.delay == 0.0 {
            self.delay = target;
        }
        let (decay, dry, wet) = (v(1) * 0.01, v(2) * 0.01, v(3) * 0.01);
        // Delay changes glide over ~50 ms (a short pitch bend instead of a jump).
        let k = one_pole(0.05, sr);
        for f in buf {
            self.delay += (target - self.delay) * k;
            let d = self.line.read(self.delay);
            self.line.push(*f + d * decay);
            *f = *f * dry + d * wet;
        }
    }
}

// ---- reverb ----------------------------------------------------------------------------------

#[derive(Clone)]
struct Comb {
    buf: Vec<f32>,
    i: usize,
    store: f32,
}

impl Comb {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            i: 0,
            store: 0.0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32, feedback: f32, damp: f32) -> f32 {
        let out = self.buf[self.i];
        self.store = out * (1.0 - damp) + self.store * damp;
        self.buf[self.i] = x + self.store * feedback;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        out
    }
}

#[derive(Clone)]
struct Allpass {
    buf: Vec<f32>,
    i: usize,
}

impl Allpass {
    fn new(len: usize) -> Self {
        Self {
            buf: vec![0.0; len.max(1)],
            i: 0,
        }
    }

    #[inline]
    fn tick(&mut self, x: f32) -> f32 {
        let b = self.buf[self.i];
        self.buf[self.i] = x + b * 0.5;
        self.i += 1;
        if self.i == self.buf.len() {
            self.i = 0;
        }
        b - x
    }
}

/// Comb and allpass lengths at 44.1 kHz (mutually prime-ish, so echoes don't line up); the
/// right side is offset by a few samples for width.
const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
const ALLPASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];
const STEREO_SPREAD: usize = 23;

#[derive(Clone)]
pub struct Reverb {
    combs: [Vec<Comb>; 2],
    allpasses: [Vec<Allpass>; 2],
}

impl Reverb {
    fn new(sample_rate: f32) -> Self {
        let scale = |n: usize| ((n as f32) * sample_rate / 44100.0) as usize;
        let side = |offset: usize| {
            (
                COMB_LENGTHS
                    .iter()
                    .map(|n| Comb::new(scale(n + offset)))
                    .collect(),
                ALLPASS_LENGTHS
                    .iter()
                    .map(|n| Allpass::new(scale(n + offset)))
                    .collect(),
            )
        };
        let (cl, al) = side(0);
        let (cr, ar) = side(STEREO_SPREAD);
        Self {
            combs: [cl, cr],
            allpasses: [al, ar],
        }
    }

    fn process(&mut self, v: impl Fn(usize) -> f32, buf: &mut [Frame]) {
        let feedback = 0.7 + 0.28 * v(0).clamp(0.0, 1.0);
        let damp = v(1).clamp(0.0, 1.0) * 0.4;
        let width = v(2).clamp(0.0, 1.0);
        let wet = (v(3) * 0.01).clamp(0.0, 1.0);
        let (wet1, wet2) = (
            wet * 3.0 * (width * 0.5 + 0.5),
            wet * 3.0 * (1.0 - width) * 0.5,
        );
        for f in buf {
            let input = (f.left + f.right) * 0.015;
            let mut out = [0.0f32; 2];
            for (side, o) in out.iter_mut().enumerate() {
                let mut acc = 0.0;
                for c in &mut self.combs[side] {
                    acc += c.tick(input, feedback, damp);
                }
                for a in &mut self.allpasses[side] {
                    acc = a.tick(acc);
                }
                *o = acc;
            }
            let dry = 1.0 - wet;
            f.left = f.left * dry + out[0] * wet1 + out[1] * wet2;
            f.right = f.right * dry + out[1] * wet1 + out[0] * wet2;
        }
    }
}

// ---- dynamics --------------------------------------------------------------------------------

/// Settings of a compressor-style gain computer.
#[derive(Clone, Copy, Debug)]
pub struct DynParams {
    pub threshold_db: f32,
    pub ratio: f32,
    pub attack: f32,
    pub release: f32,
    pub makeup_db: f32,
    pub knee_db: f32,
    /// Duck Volume: 1 = detect on the side-chain only, 0 = on the group's own signal.
    pub side_mix: f32,
}

impl DynParams {
    /// Gain change (dB, ≤ 0) for a detected level of `level_db`.
    pub fn curve(&self, level_db: f32) -> f32 {
        let slope = 1.0 / self.ratio.max(1.0) - 1.0;
        let over = level_db - self.threshold_db;
        let knee = self.knee_db.max(0.0);
        if knee > 0.0 && over.abs() <= knee * 0.5 {
            slope * (over + knee * 0.5).powi(2) / (2.0 * knee)
        } else if over > 0.0 {
            slope * over
        } else {
            0.0
        }
    }
}

/// Peak envelope follower plus static curve: Compressor and Duck Volume.
#[derive(Clone, Default)]
pub struct Dynamics {
    env: f32,
    /// Gain change of the last sample (dB, ≤ 0), for meters.
    pub reduction_db: f32,
}

impl Dynamics {
    pub fn process(&mut self, p: DynParams, buf: &mut [Frame], side: Option<&[Frame]>, sr: f32) {
        let att = one_pole(p.attack, sr);
        let rel = one_pole(p.release, sr);
        let makeup = p.makeup_db;
        for (i, f) in buf.iter_mut().enumerate() {
            let own = f.left.abs().max(f.right.abs());
            let det = match side {
                Some(s) => {
                    let sc = s.get(i).map_or(0.0, |s| s.left.abs().max(s.right.abs()));
                    sc * p.side_mix + own * (1.0 - p.side_mix)
                }
                None => own,
            };
            let k = if det > self.env { att } else { rel };
            self.env += (det - self.env) * k;
            let gr = p.curve(amplitude_to_db(self.env.max(1e-6)));
            self.reduction_db = gr;
            let g = db_to_amplitude_fast(gr + makeup);
            *f = *f * g;
        }
        if self.env < 1e-9 {
            self.env = 0.0;
        }
    }
}

/// dB to amplitude without the silence floor (gain computers go well below -80 dB).
#[inline]
fn db_to_amplitude_fast(db: f32) -> f32 {
    (db * (std::f32::consts::LN_10 / 20.0)).exp()
}

// ---- modulated delays ------------------------------------------------------------------------

#[derive(Clone)]
pub struct Chorus {
    line: DelayLine,
    phase: f32,
}

impl Chorus {
    fn new(sample_rate: f32) -> Self {
        Self {
            line: DelayLine::new((0.25 * sample_rate) as usize),
            phase: 0.0,
        }
    }

    fn process(&mut self, v: impl Fn(usize) -> f32, buf: &mut [Frame], sr: f32) {
        let (dry, w1, w2, w3) = (v(0) * 0.01, v(1) * 0.01, v(2) * 0.01, v(3) * 0.01);
        let base = v(4) * 0.001 * sr;
        let depth = v(6) * 0.01;
        let step = v(5) / sr;
        for f in buf {
            self.line.push(*f);
            let tap = |k: f32| {
                let lfo = (TAU * (self.phase + k / 3.0)).sin();
                self.line.read((base * (1.0 + depth * lfo)).max(1.0))
            };
            let (t1, t2, t3) = (tap(0.0), tap(1.0), tap(2.0));
            let mid = (t3.left + t3.right) * 0.5;
            f.left = f.left * dry + t1.left * w1 + mid * w3;
            f.right = f.right * dry + t2.right * w2 + mid * w3;
            self.phase = (self.phase + step).fract();
        }
    }
}

/// Longest flange sweep (seconds).
const FLANGE_SWEEP: f32 = 0.005;

#[derive(Clone)]
pub struct Flange {
    line: DelayLine,
    phase: f32,
}

impl Flange {
    fn new(sample_rate: f32) -> Self {
        Self {
            line: DelayLine::new((0.02 * sample_rate) as usize),
            phase: 0.0,
        }
    }

    fn process(&mut self, v: impl Fn(usize) -> f32, buf: &mut [Frame], sr: f32) {
        let (dry, wet) = (v(0) * 0.01, v(1) * 0.01);
        let sweep = v(2).clamp(0.0, 1.0) * FLANGE_SWEEP * sr;
        let step = v(3) / sr;
        for f in buf {
            self.line.push(*f);
            // The right side sweeps a quarter cycle later (a wider, moving image).
            let delay = |ph: f32| 1.0 + sweep * (0.5 + 0.5 * (TAU * ph).sin());
            let l = self.line.read(delay(self.phase)).left;
            let r = self.line.read(delay(self.phase + 0.25)).right;
            f.left = f.left * dry + l * wet;
            f.right = f.right * dry + r * wet;
            self.phase = (self.phase + step).fract();
        }
    }
}

// ---- the processor ---------------------------------------------------------------------------

/// The running state of one effect.
#[derive(Clone)]
pub enum Processor {
    Lowpass(Biquad),
    Highpass(Biquad),
    Echo(Echo),
    Reverb(Box<Reverb>),
    Distortion,
    Compressor(Dynamics),
    Chorus(Chorus),
    Flange(Flange),
    Pitch(PitchShifter),
    Eq(Biquad),
    /// Send and Receive: the graph moves the signal; the chain passes it on unchanged.
    Routing,
    Duck(Dynamics),
}

impl Processor {
    pub fn new(kind: EffectKind, sample_rate: f32) -> Self {
        match kind {
            EffectKind::Lowpass => Self::Lowpass(Biquad::default()),
            EffectKind::Highpass => Self::Highpass(Biquad::default()),
            EffectKind::Echo => Self::Echo(Echo::new(sample_rate)),
            EffectKind::Reverb => Self::Reverb(Box::new(Reverb::new(sample_rate))),
            EffectKind::Distortion => Self::Distortion,
            EffectKind::Compressor => Self::Compressor(Dynamics::default()),
            EffectKind::Chorus => Self::Chorus(Chorus::new(sample_rate)),
            EffectKind::Flange => Self::Flange(Flange::new(sample_rate)),
            EffectKind::PitchShifter => Self::Pitch(PitchShifter::new()),
            EffectKind::ParamEQ => Self::Eq(Biquad::default()),
            EffectKind::Send | EffectKind::Receive => Self::Routing,
            EffectKind::DuckVolume => Self::Duck(Dynamics::default()),
        }
    }

    pub fn kind(&self) -> EffectKind {
        match self {
            Self::Lowpass(_) => EffectKind::Lowpass,
            Self::Highpass(_) => EffectKind::Highpass,
            Self::Echo(_) => EffectKind::Echo,
            Self::Reverb(_) => EffectKind::Reverb,
            Self::Distortion => EffectKind::Distortion,
            Self::Compressor(_) => EffectKind::Compressor,
            Self::Chorus(_) => EffectKind::Chorus,
            Self::Flange(_) => EffectKind::Flange,
            Self::Pitch(_) => EffectKind::PitchShifter,
            Self::Eq(_) => EffectKind::ParamEQ,
            Self::Routing => EffectKind::Receive,
            Self::Duck(_) => EffectKind::DuckVolume,
        }
    }

    /// Process `buf` in place with parameter `values`. `side` is the side-chain input of a
    /// Duck Volume (what Sends delivered this block).
    pub fn process(
        &mut self,
        kind: EffectKind,
        values: &[f32],
        buf: &mut [Frame],
        sr: f32,
        side: Option<&[Frame]>,
    ) {
        let v = |i: usize| value(kind, values, i);
        match self {
            Self::Lowpass(b) => {
                let (f, q) = (v(0), v(1));
                b.update([f, q, sr], || Coeffs::lowpass(f, resonance_q(q), sr));
                b.process(buf);
            }
            Self::Highpass(b) => {
                let (f, q) = (v(0), v(1));
                b.update([f, q, sr], || Coeffs::highpass(f, resonance_q(q), sr));
                b.process(buf);
            }
            Self::Eq(b) => {
                let (f, oct, g) = (v(0), v(1), v(2));
                b.update([f, oct, g], || Coeffs::peaking(f, oct, g, sr));
                b.process(buf);
            }
            Self::Echo(e) => e.process(v, buf, sr),
            Self::Reverb(r) => r.process(v, buf),
            Self::Distortion => {
                let drive = db_to_amplitude(v(0) * 36.0).max(1.0);
                for f in buf {
                    f.left = (f.left * drive).tanh();
                    f.right = (f.right * drive).tanh();
                }
            }
            Self::Compressor(d) => {
                let p = DynParams {
                    threshold_db: v(0),
                    ratio: v(1),
                    attack: v(2) * 0.001,
                    release: v(3) * 0.001,
                    makeup_db: v(4),
                    knee_db: 0.0,
                    side_mix: 0.0,
                };
                d.process(p, buf, None, sr);
            }
            Self::Duck(d) => {
                let p = DynParams {
                    threshold_db: v(0),
                    ratio: v(1),
                    attack: v(2) * 0.001,
                    release: v(3) * 0.001,
                    makeup_db: v(4),
                    knee_db: v(5),
                    side_mix: (v(6) * 0.01).clamp(0.0, 1.0),
                };
                // No Send feeding it: an empty side-chain (it only reacts to its own signal
                // by the share Sidechain mix leaves to it).
                d.process(p, buf, Some(side.unwrap_or(&[])), sr);
            }
            Self::Chorus(c) => c.process(v, buf, sr),
            Self::Flange(fl) => fl.process(v, buf, sr),
            Self::Pitch(p) => p.process(v(0).clamp(0.5, 2.0), v(1), v(2), buf),
            Self::Routing => {}
        }
    }

    /// Gain change (dB, ≤ 0) a Compressor or Duck Volume applied last.
    pub fn reduction_db(&self) -> f32 {
        match self {
            Self::Compressor(d) | Self::Duck(d) => d.reduction_db,
            _ => 0.0,
        }
    }

    /// Whether the effect rings on after its input stops (keep running it while it fades).
    pub fn has_tail(&self) -> bool {
        matches!(
            self,
            Self::Echo(_) | Self::Reverb(_) | Self::Chorus(_) | Self::Flange(_) | Self::Pitch(_)
        )
    }
}

/// Magnitude (linear) of a filter-type effect at `freq` (for the Inspector's response curves).
pub fn response(kind: EffectKind, values: &[f32], freq: f32, sample_rate: f32) -> f32 {
    let v = |i: usize| value(kind, values, i);
    let c = match kind {
        EffectKind::Lowpass => Coeffs::lowpass(v(0), resonance_q(v(1)), sample_rate),
        EffectKind::Highpass => Coeffs::highpass(v(0), resonance_q(v(1)), sample_rate),
        EffectKind::ParamEQ => Coeffs::peaking(v(0), v(1), v(2), sample_rate),
        _ => return 1.0,
    };
    c.magnitude(freq, sample_rate)
}

/// Half a cycle of a raised cosine from 0 to 1 (for fades that sound smoother than linear).
pub fn smoothstep(x: f32) -> f32 {
    0.5 - 0.5 * (PI * x.clamp(0.0, 1.0)).cos()
}

#[cfg(test)]
#[path = "fx_tests.rs"]
mod tests;
