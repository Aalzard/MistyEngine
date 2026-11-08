//! Binaural rendering for 3D sources (the Audio Source's Spatialize option): a spherical-head
//! model instead of a measured HRTF set. Per ear:
//!
//! - **ITD**: the sound reaches the far ear later, by the path around a sphere of radius
//!   [`HEAD_RADIUS`] (up to ~0.66 ms for a source at the side): `a/c · (1 − cos θ)` while the ear
//!   sees the source, `a/c · (1 + θ − π/2)` once it wraps round the head (θ = angle between the
//!   source and the ear's axis).
//! - **ILD / head shadow**: a one-pole, one-zero shelf whose high-frequency gain goes from +6 dB
//!   (source facing the ear) down to −20 dB (source behind the head from that ear's view), with
//!   its corner at `c / a` (~620 Hz): low frequencies bend round the head, high ones are shadowed.
//! - **Front / back**: sources behind the listener lose some top end (the outer ear's shadow).
//!
//! The model is the well-known structural one from the spatial-audio literature (Brown & Duda),
//! implemented from its equations; no measured data is shipped.

use std::f32::consts::{FRAC_PI_2, PI};

use glam::Vec3;

use super::{Frame, one_pole};

/// Head radius (m) of the model.
pub const HEAD_RADIUS: f32 = 0.0875;
const SPEED_OF_SOUND: f32 = 343.0;
/// Head-shadow gain at the shadowed extreme (α_min) and the angle it is reached at.
const ALPHA_MIN: f32 = 0.1;
const THETA_MIN: f32 = 150.0 * PI / 180.0;

/// Delay (seconds) of the sound at an ear whose axis makes angle `theta` with the source.
pub fn ear_delay(theta: f32) -> f32 {
    let a = HEAD_RADIUS / SPEED_OF_SOUND;
    if theta < FRAC_PI_2 {
        a * (1.0 - theta.cos())
    } else {
        a * (1.0 + theta - FRAC_PI_2)
    }
}

/// Interaural time difference (seconds, positive = right ear first) for a direction in
/// listener space (x right, y up, z forward).
pub fn itd(dir: Vec3) -> f32 {
    let d = dir.normalize_or_zero();
    ear_delay(angle_to_ear(d, -1.0)) - ear_delay(angle_to_ear(d, 1.0))
}

/// Angle between direction `d` and the left (`side` = -1) or right (+1) ear axis.
fn angle_to_ear(d: Vec3, side: f32) -> f32 {
    if d == Vec3::ZERO {
        return FRAC_PI_2;
    }
    (d.x * side).clamp(-1.0, 1.0).acos()
}

/// High-frequency gain α of the head-shadow shelf at angle `theta` from the ear.
fn alpha(theta: f32) -> f32 {
    (1.0 + ALPHA_MIN * 0.5) + (1.0 - ALPHA_MIN * 0.5) * (theta / THETA_MIN * PI).min(PI).cos()
}

/// One ear: fractional delay, head-shadow shelf, rear shelf.
#[derive(Clone)]
struct Ear {
    line: Vec<f32>,
    pos: usize,
    delay: f32,
    // Shelf: y = b0 x + b1 x1 - a1 y1.
    x1: f32,
    y1: f32,
    rear: f32,
}

impl Ear {
    fn new(sample_rate: f32) -> Self {
        // Longest delay ~0.67 ms plus room for interpolation.
        let len = (0.002 * sample_rate) as usize + 8;
        Self {
            line: vec![0.0; len],
            pos: 0,
            delay: -1.0,
            x1: 0.0,
            y1: 0.0,
            rear: 0.0,
        }
    }

    #[inline]
    fn read(&self, delay: f32) -> f32 {
        let len = self.line.len();
        let x = self.pos as f32 + len as f32 - 1.0 - delay.clamp(0.0, (len - 3) as f32);
        let i0 = x as usize;
        let frac = x - i0 as f32;
        let a = self.line[i0 % len];
        let b = self.line[(i0 + 1) % len];
        a + (b - a) * frac
    }
}

/// Binaural state of one voice.
#[derive(Clone)]
pub struct Binaural {
    ears: [Ear; 2],
    sample_rate: f32,
    /// Per-sample output of both ears (kept to avoid allocating while rendering).
    scratch: Vec<[f32; 2]>,
}

impl Binaural {
    pub fn new(sample_rate: f32) -> Self {
        Self {
            ears: [Ear::new(sample_rate), Ear::new(sample_rate)],
            sample_rate,
            scratch: vec![[0.0; 2]; super::BLOCK],
        }
    }

    /// Render the mono sum of `buf` for a source in direction `dir` (listener space), in place.
    /// Delays glide across the block, so a moving source doesn't click.
    pub fn process(&mut self, dir: Vec3, buf: &mut [Frame]) {
        let sr = self.sample_rate;
        let d = dir.normalize_or_zero();
        let w0 = SPEED_OF_SOUND / HEAD_RADIUS;
        // Behind the listener (z < 0): up to a 4 kHz one-pole on top of the shadow.
        let rear = (-d.z).max(0.0);
        let rear_k = one_pole(1.0 / (std::f32::consts::TAU * 4000.0), sr);
        let n = buf.len().max(1) as f32;
        if self.scratch.len() < buf.len() {
            self.scratch.resize(buf.len(), [0.0; 2]);
        }
        let out = &mut self.scratch;
        for (e, ear) in self.ears.iter_mut().enumerate() {
            let side = if e == 0 { -1.0 } else { 1.0 };
            let theta = angle_to_ear(d, side);
            let target = ear_delay(theta) * sr;
            if ear.delay < 0.0 {
                ear.delay = target;
            }
            let (start, step) = (ear.delay, (target - ear.delay) / n);
            // Bilinear-transformed shelf (α s + 2ω0) / (s + 2ω0) at this sample rate.
            let a = alpha(theta);
            let t = 2.0 * sr;
            let (b0, b1, a0, a1) = (
                a * t + 2.0 * w0,
                -a * t + 2.0 * w0,
                t + 2.0 * w0,
                -t + 2.0 * w0,
            );
            let (b0, b1, a1) = (b0 / a0, b1 / a0, a1 / a0);
            for (i, f) in buf.iter().enumerate() {
                let x = (f.left + f.right) * 0.5;
                let len = ear.line.len();
                ear.line[ear.pos] = x;
                ear.pos = (ear.pos + 1) % len;
                let delayed = ear.read(start + step * (i as f32 + 1.0));
                let y = b0 * delayed + b1 * ear.x1 - a1 * ear.y1;
                ear.x1 = delayed;
                ear.y1 = y;
                ear.rear += (y - ear.rear) * rear_k;
                out[i][e] = y + (ear.rear - y) * rear;
            }
            ear.delay = target;
            if ear.y1.abs() < 1e-20 {
                ear.y1 = 0.0;
            }
        }
        for (f, o) in buf.iter_mut().zip(out.iter()) {
            f.left = o[0];
            f.right = o[1];
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::analysis::{delay_between, noise, rms, side};
    use super::*;

    const SR: f32 = 48000.0;

    fn render(dir: Vec3) -> Vec<Frame> {
        let mut b = Binaural::new(SR);
        let mut buf = noise(7, 0.5, 0.5, SR);
        for block in buf.chunks_mut(256) {
            b.process(dir, block);
        }
        buf
    }

    #[test]
    fn side_sources_reach_the_near_ear_first_and_louder() {
        let right = render(Vec3::X);
        let (l, r) = (side(&right, false), side(&right, true));
        let ild = 20.0 * (rms(&r) / rms(&l)).log10();
        assert!(ild > 6.0, "right ear louder by {ild} dB");
        // The left ear trails by the ITD of a source at 90°: (a/c)(1 + π/2) ≈ 0.656 ms.
        let lag = delay_between(&r, &l, 64);
        let expected = itd(Vec3::X) * SR;
        assert!(
            (lag as f32 - expected).abs() <= 1.5,
            "lag {lag}, expected {expected}"
        );
        assert!((itd(Vec3::X) * 1000.0 - 0.656).abs() < 0.01);
        // Mirrored on the left.
        let left = render(-Vec3::X);
        let (l2, r2) = (side(&left, false), side(&left, true));
        assert!(rms(&l2) > rms(&r2) * 2.0);
        assert_eq!(delay_between(&l2, &r2, 64), lag);
    }

    #[test]
    fn front_is_centred_and_back_is_duller() {
        let front = render(Vec3::Z);
        let (l, r) = (side(&front, false), side(&front, true));
        assert!((rms(&l) / rms(&r) - 1.0).abs() < 1e-3);
        assert_eq!(delay_between(&l, &r, 32), 0);
        assert_eq!(itd(Vec3::Z), 0.0);
        let back = render(-Vec3::Z);
        let hf = |x: &[Frame]| {
            let s = side(x, false);
            let diff: Vec<f32> = s.windows(2).map(|w| w[1] - w[0]).collect();
            rms(&diff)
        };
        assert!(hf(&back) < hf(&front) * 0.8, "less top end behind");
    }
}
