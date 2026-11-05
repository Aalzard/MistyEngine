//! Camera noise: layered 1D gradient (Perlin) noise per position and rotation channel.
//! A pure function of time and seed, so the same seed always shakes the same way.

use glam::Vec3;

use super::components::{Noise, NoiseProfile};

/// One noise layer: frequency (Hz) and amplitude (metres or degrees).
#[derive(Clone, Copy)]
struct Layer {
    freq: f32,
    amp: f32,
}

const fn l(freq: f32, amp: f32) -> Layer {
    Layer { freq, amp }
}

/// Two layers for each of x, y, z position then pitch, yaw, roll rotation.
type Channels = [[Layer; 2]; 6];

fn channels(profile: NoiseProfile) -> &'static Channels {
    const HANDHELD: Channels = [
        [l(0.25, 0.015), l(0.9, 0.004)],
        [l(0.3, 0.02), l(1.1, 0.005)],
        [l(0.2, 0.01), l(0.8, 0.003)],
        [l(0.3, 0.6), l(1.2, 0.15)],
        [l(0.22, 0.5), l(0.95, 0.12)],
        [l(0.35, 0.25), l(1.3, 0.06)],
    ];
    const SHAKE: Channels = [
        [l(6.0, 0.05), l(13.0, 0.02)],
        [l(7.0, 0.06), l(15.0, 0.025)],
        [l(5.0, 0.03), l(11.0, 0.01)],
        [l(7.5, 1.5), l(16.0, 0.6)],
        [l(6.5, 1.3), l(14.0, 0.5)],
        [l(8.0, 0.8), l(17.0, 0.3)],
    ];
    match profile {
        NoiseProfile::Handheld => &HANDHELD,
        NoiseProfile::Shake => &SHAKE,
    }
}

fn hash(i: i32, seed: u32) -> u32 {
    let mut h = (i as u32).wrapping_mul(0x27d4_eb2d) ^ seed.wrapping_mul(0x9e37_79b9);
    h ^= h >> 15;
    h = h.wrapping_mul(0x85eb_ca6b);
    h ^= h >> 13;
    h = h.wrapping_mul(0xc2b2_ae35);
    h ^ (h >> 16)
}

/// Gradient noise in about −1..1, 0 at every integer.
pub fn perlin(x: f32, seed: u32) -> f32 {
    let i = x.floor();
    let f = x - i;
    let i = i as i32;
    let grad = |i: i32| hash(i, seed) as f32 / u32::MAX as f32 * 2.0 - 1.0;
    let (g0, g1) = (grad(i), grad(i.wrapping_add(1)));
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let (a, b) = (g0 * f, g1 * (f - 1.0));
    (a + (b - a) * u) * 2.0
}

/// Position (metres, camera-local) and rotation (degrees: pitch, yaw, roll) offsets at time `t`.
pub fn sample(noise: &Noise, t: f32) -> (Vec3, Vec3) {
    let gain = noise.amplitude_gain;
    let t = t * noise.frequency_gain;
    let mut out = [0.0f32; 6];
    for (c, layers) in channels(noise.profile).iter().enumerate() {
        for (k, layer) in layers.iter().enumerate() {
            let seed = noise.seed ^ ((c as u32 * 2 + k as u32 + 1).wrapping_mul(0x632b_e5ab));
            // Offset each channel so they don't all cross zero together.
            let phase = (c * 7 + k * 3) as f32 * 0.37;
            out[c] += perlin(t * layer.freq + phase, seed) * layer.amp * gain;
        }
    }
    (
        Vec3::new(out[0], out[1], out[2]),
        Vec3::new(out[3], out[4], out[5]),
    )
}
