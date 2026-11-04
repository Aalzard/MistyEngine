//! Wave model shared by the GPU and the CPU: a sum of Gerstner waves (swell and long wind waves)
//! plus an FFT ocean (Phillips or JONSWAP spectrum). The GPU runs the full 256² FFT; the CPU
//! evaluates the same spectrum's 32² lowest modes and the same Gerstner waves, so buoyancy and
//! `Water.GetHeight` follow what is drawn (minus the smallest ripples).

use std::sync::{Arc, Mutex, OnceLock};

use glam::{Vec2, Vec3};

use super::settings::{SpectrumKind, WaterSettings};

pub const GRAVITY: f32 = 9.81;
pub const GERSTNER_COUNT: usize = 8;
/// GPU FFT resolution.
pub const FFT_SIZE: usize = 256;
/// CPU low-mode grid resolution.
pub const CPU_SIZE: usize = 32;

/// One Gerstner wave: `dir` = unit travel direction (world XZ), `k` = 2π / wavelength.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Gerstner {
    pub dir: Vec2,
    pub k: f32,
    pub amplitude: f32,
    pub omega: f32,
    pub phase: f32,
    /// Sideways (Q) factor: crests sharpen as it grows.
    pub steepness: f32,
}

/// Unit XZ direction for degrees clockwise from +Z.
pub fn direction(degrees: f32) -> Vec2 {
    let r = degrees.to_radians();
    Vec2::new(r.sin(), r.cos())
}

/// Significant wave height (m) of a fully developed sea for a wind speed.
pub fn developed_height(wind_speed: f32) -> f32 {
    0.21 * wind_speed.max(0.5).powi(2) / GRAVITY
}

/// The Gerstner set: four swell waves and four long wind waves. `flow` (rivers) turns every wave
/// along the flow and drops the swell.
pub fn gerstner_waves(s: &WaterSettings, flow: Option<Vec2>) -> [Gerstner; GERSTNER_COUNT] {
    const PHASES: [f32; GERSTNER_COUNT] = [0.0, 1.3, 2.9, 4.1, 0.7, 5.2, 3.3, 2.2];
    let mut waves = [Gerstner::default(); GERSTNER_COUNT];
    let swell_height = if flow.is_some() { 0.0 } else { s.swell_height };
    let wind_height = developed_height(s.wind_speed) * s.wave_height * 0.35;
    // Peak wavelength of the wind sea.
    let peak = 8.6 * s.wind_speed.max(0.5).powi(2) / GRAVITY;
    let flow_angle = flow.map(|f| f.x.atan2(f.y).to_degrees());
    let specs = [
        // (wavelength, angle offset, share of height, swell?)
        (s.swell_length, 0.0, 0.42, true),
        (s.swell_length * 0.78, 13.0, 0.26, true),
        (s.swell_length * 0.63, -9.0, 0.18, true),
        (s.swell_length * 1.27, -21.0, 0.14, true),
        (peak * 1.6, -18.0, 0.4, false),
        (peak * 1.1, 25.0, 0.3, false),
        (peak * 0.8, 8.0, 0.2, false),
        (peak * 0.55, -35.0, 0.1, false),
    ];
    let q = (s.choppiness * 0.6).clamp(0.0, 1.0);
    for (i, (length, offset, share, swell)) in specs.into_iter().enumerate() {
        let length = length.max(0.5);
        let k = std::f32::consts::TAU / length;
        let base = match (flow_angle, swell) {
            (Some(f), _) => f,
            (None, true) => s.swell_direction,
            (None, false) => s.wind_direction,
        };
        let spread = if flow_angle.is_some() { 0.3 } else { 1.0 };
        let amplitude = 0.5 * share * if swell { swell_height } else { wind_height };
        waves[i] = Gerstner {
            dir: direction(base + offset * spread),
            k,
            amplitude,
            omega: (GRAVITY * k).sqrt(),
            phase: PHASES[i],
            steepness: if amplitude > 0.0 {
                q / (k * amplitude * GERSTNER_COUNT as f32)
            } else {
                0.0
            },
        };
    }
    waves
}

/// Displacement (x, height, z) of the Gerstner sum at a rest position.
pub fn gerstner_displacement(waves: &[Gerstner], p: Vec2, t: f32) -> Vec3 {
    let mut d = Vec3::ZERO;
    for w in waves {
        let theta = w.k * w.dir.dot(p) - w.omega * t + w.phase;
        let (s, c) = theta.sin_cos();
        let side = w.steepness * w.amplitude * c;
        d += Vec3::new(side * w.dir.x, w.amplitude * s, side * w.dir.y);
    }
    d
}

/// What the FFT spectrum depends on.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectrumParams {
    pub wind_speed: f32,
    pub wind_direction: f32,
    pub wave_height: f32,
    pub kind: SpectrumKind,
}

impl SpectrumParams {
    pub fn of(s: &WaterSettings) -> Self {
        Self {
            wind_speed: s.wind_speed.max(0.5),
            wind_direction: s.wind_direction,
            wave_height: s.wave_height.max(0.0),
            kind: s.spectrum,
        }
    }

    /// Tile size in metres: about two peak wavelengths.
    pub fn tile(&self) -> f32 {
        let peak = 8.6 * self.wind_speed.powi(2) / GRAVITY;
        (peak * 2.0).clamp(48.0, 900.0)
    }
}

/// Initial spectrum: per FFT cell (h0(k), conj(h0(-k))).
pub struct Spectrum {
    pub params: SpectrumParams,
    pub tile: f32,
    /// Row-major (z index × N + x index); cell i holds wave number n = i - N/2.
    pub h0: Vec<[f32; 4]>,
}

/// Deterministic Gaussian pair per cell (Box-Muller on a hash).
fn gaussian(ix: usize, iy: usize) -> (f32, f32) {
    let hash = |a: u32| {
        let mut x = a.wrapping_mul(0x9E37_79B9) ^ 0x85EB_CA6B;
        x ^= x >> 16;
        x = x.wrapping_mul(0x7FEB_352D);
        x ^= x >> 15;
        x = x.wrapping_mul(0x846C_A68B);
        x ^= x >> 16;
        (x >> 8) as f32 / (1u32 << 24) as f32
    };
    let seed = (iy * FFT_SIZE + ix) as u32;
    let u1 = hash(seed.wrapping_mul(2)).max(1e-7);
    let u2 = hash(seed.wrapping_mul(2) + 1);
    let r = (-2.0 * u1.ln()).sqrt();
    let a = std::f32::consts::TAU * u2;
    (r * a.cos(), r * a.sin())
}

impl Spectrum {
    pub fn new(params: SpectrumParams) -> Self {
        let n = FFT_SIZE;
        let tile = params.tile();
        let wind = direction(params.wind_direction);
        let v = params.wind_speed;
        let lw = v * v / GRAVITY;
        let small = tile / n as f32 * 0.5;
        let omega_p = 0.855 * GRAVITY / v;
        let density = |kv: Vec2| -> f32 {
            let k = kv.length();
            if k < 1e-6 {
                return 0.0;
            }
            let cos = kv.dot(wind) / k;
            // Waves running against the wind are much weaker.
            let along = if cos < 0.0 { 0.07 } else { 1.0 };
            let cutoff = (-k * k * small * small).exp();
            match params.kind {
                SpectrumKind::Phillips => {
                    (-1.0 / (k * lw).powi(2)).exp() / k.powi(4) * cos * cos * along * cutoff
                }
                SpectrumKind::Jonswap => {
                    let w = (GRAVITY * k).sqrt();
                    let sigma = if w <= omega_p { 0.07 } else { 0.09 };
                    let r =
                        (-(w - omega_p).powi(2) / (2.0 * sigma * sigma * omega_p * omega_p)).exp();
                    let s = w.powi(-5) * (-1.25 * (omega_p / w).powi(4)).exp() * 3.3f32.powf(r);
                    let spread = cos.max(0.0).powi(2) + 0.02;
                    s * spread * (GRAVITY / (2.0 * w)) / k * along * cutoff
                }
            }
        };
        let mut base = vec![(0.0f32, 0.0f32); n * n];
        let mut variance = 0.0f64;
        for iy in 0..n {
            for ix in 0..n {
                let kv = Vec2::new(ix as f32 - (n / 2) as f32, iy as f32 - (n / 2) as f32)
                    * (std::f32::consts::TAU / tile);
                let amp = (density(kv) * 0.5).sqrt();
                let (gr, gi) = gaussian(ix, iy);
                let h = (gr * amp, gi * amp);
                variance += (h.0 as f64).powi(2) + (h.1 as f64).powi(2);
                base[iy * n + ix] = h;
            }
        }
        // Scale to the significant wave height Hs = 4 σ (σ² = 2 Σ |h0|²).
        let hs = 4.0 * (2.0 * variance).sqrt() as f32;
        let target = developed_height(v) * params.wave_height;
        let scale = if hs > 0.0 { target / hs } else { 0.0 };
        let h0 = (0..n * n)
            .map(|i| {
                let (ix, iy) = (i % n, i / n);
                let mirror = ((n - iy) % n) * n + (n - ix) % n;
                let (a, b) = (base[i], base[mirror]);
                [a.0 * scale, a.1 * scale, b.0 * scale, -b.1 * scale]
            })
            .collect();
        Self { params, tile, h0 }
    }

    /// Wave number vector of a cell.
    fn k(&self, ix: usize, iy: usize) -> Vec2 {
        let half = (FFT_SIZE / 2) as f32;
        Vec2::new(ix as f32 - half, iy as f32 - half) * (std::f32::consts::TAU / self.tile)
    }

    /// The low-mode displacement grid at time `t` (x, height, z per cell, choppiness applied).
    pub fn cpu_field(&self, t: f32, choppiness: f32) -> CpuField {
        let m = CPU_SIZE;
        let half = (FFT_SIZE / 2) as isize;
        let tw: Vec<(f32, f32)> = (0..m)
            .map(|i| {
                let a = std::f32::consts::TAU * i as f32 / m as f32;
                (a.cos(), a.sin())
            })
            .collect();
        // Spectrum of the three fields over modes -m/2..m/2.
        let mut spec = vec![[(0.0f32, 0.0f32); 3]; m * m];
        for j in 0..m {
            for i in 0..m {
                let ix = (i as isize - (m / 2) as isize + half) as usize;
                let iy = (j as isize - (m / 2) as isize + half) as usize;
                let kv = self.k(ix, iy);
                let k = kv.length();
                let w = (GRAVITY * k).sqrt();
                let [a, b, c, d] = self.h0[iy * FFT_SIZE + ix];
                let (s, co) = (w * t).sin_cos();
                // h0 e^{iwt} + conj(h0(-k)) e^{-iwt}
                let h = (
                    a * co - b * s + c * co + d * s,
                    a * s + b * co - c * s + d * co,
                );
                let (ux, uz) = if k > 1e-6 {
                    (kv.x / k * choppiness, kv.y / k * choppiness)
                } else {
                    (0.0, 0.0)
                };
                // -i u h = (u h.im, -u h.re)
                spec[j * m + i] = [(ux * h.1, -ux * h.0), h, (uz * h.1, -uz * h.0)];
            }
        }
        // Rows (over n for each m), then columns.
        let mut rows = vec![[(0.0f32, 0.0f32); 3]; m * m];
        for j in 0..m {
            for x in 0..m {
                let mut acc = [(0.0f32, 0.0f32); 3];
                for i in 0..m {
                    let n = i as isize - (m / 2) as isize;
                    let (c, s) = tw[(n * x as isize).rem_euclid(m as isize) as usize];
                    for ch in 0..3 {
                        let v = spec[j * m + i][ch];
                        acc[ch].0 += v.0 * c - v.1 * s;
                        acc[ch].1 += v.0 * s + v.1 * c;
                    }
                }
                rows[j * m + x] = acc;
            }
        }
        let mut cells = vec![Vec3::ZERO; m * m];
        for y in 0..m {
            for x in 0..m {
                let mut out = [0.0f32; 3];
                for j in 0..m {
                    let n = j as isize - (m / 2) as isize;
                    let (c, s) = tw[(n * y as isize).rem_euclid(m as isize) as usize];
                    for (ch, o) in out.iter_mut().enumerate() {
                        let v = rows[j * m + x][ch];
                        *o += v.0 * c - v.1 * s;
                    }
                }
                cells[y * m + x] = Vec3::from(out);
            }
        }
        CpuField {
            tile: self.tile,
            cells,
        }
    }
}

/// The low modes of the FFT ocean on a periodic CPU_SIZE² grid over one tile.
pub struct CpuField {
    pub tile: f32,
    pub cells: Vec<Vec3>,
}

impl CpuField {
    /// Bilinear, wrapping displacement at a world XZ position.
    pub fn sample(&self, p: Vec2) -> Vec3 {
        let m = CPU_SIZE as f32;
        let g = p / self.tile * m;
        let (x0, y0) = (g.x.floor(), g.y.floor());
        let (fx, fy) = (g.x - x0, g.y - y0);
        let cell = |x: f32, y: f32| {
            let xi = (x as i64).rem_euclid(CPU_SIZE as i64) as usize;
            let yi = (y as i64).rem_euclid(CPU_SIZE as i64) as usize;
            self.cells[yi * CPU_SIZE + xi]
        };
        let a = cell(x0, y0).lerp(cell(x0 + 1.0, y0), fx);
        let b = cell(x0, y0 + 1.0).lerp(cell(x0 + 1.0, y0 + 1.0), fx);
        a.lerp(b, fy)
    }
}

/// Spectra are costly to build: keep the last few.
pub fn spectrum(params: SpectrumParams) -> Arc<Spectrum> {
    static CACHE: OnceLock<Mutex<Vec<Arc<Spectrum>>>> = OnceLock::new();
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some(s) = cache.iter().find(|s| s.params == params) {
        return s.clone();
    }
    let s = Arc::new(Spectrum::new(params));
    cache.insert(0, s.clone());
    cache.truncate(4);
    s
}

/// The CPU field for a spectrum at a time (the last few are kept: many queries per step).
pub fn cpu_field(spectrum: &Arc<Spectrum>, t: f32, choppiness: f32) -> Arc<CpuField> {
    type Key = (usize, u32, u32);
    type Cache = Mutex<Vec<(Key, Arc<CpuField>)>>;
    static CACHE: OnceLock<Cache> = OnceLock::new();
    let key = (
        Arc::as_ptr(spectrum) as usize,
        t.to_bits(),
        choppiness.to_bits(),
    );
    let mut cache = CACHE
        .get_or_init(Default::default)
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    if let Some((_, f)) = cache.iter().find(|(k, _)| *k == key) {
        return f.clone();
    }
    let f = Arc::new(spectrum.cpu_field(t, choppiness));
    cache.insert(0, (key, f.clone()));
    cache.truncate(4);
    f
}

/// The full wave model of one surface at one time.
pub struct WaveState {
    pub gerstner: [Gerstner; GERSTNER_COUNT],
    pub field: Arc<CpuField>,
    pub time: f32,
}

impl WaveState {
    pub fn new(s: &WaterSettings, flow: Option<Vec2>, time: f32) -> Self {
        let spec = spectrum(SpectrumParams::of(s));
        Self {
            gerstner: gerstner_waves(s, flow),
            field: cpu_field(&spec, time, s.choppiness),
            time,
        }
    }

    /// Displacement of the surface point that rests at `p` (flow scrolls the FFT part).
    pub fn displacement(&self, p: Vec2, flow: Vec2) -> Vec3 {
        gerstner_displacement(&self.gerstner, p, self.time)
            + self.field.sample(p - flow * self.time)
    }

    /// Height offset of the surface above a world XZ point: solves for the rest position whose
    /// displaced point lands there (a few fixed-point steps).
    pub fn height(&self, p: Vec2, flow: Vec2) -> f32 {
        let mut rest = p;
        for _ in 0..4 {
            let d = self.displacement(rest, flow);
            rest = p - Vec2::new(d.x, d.z);
        }
        self.displacement(rest, flow).y
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::water::WaterPreset;

    #[test]
    fn spectrum_matches_its_height_and_is_hermitian() {
        let s = WaterPreset::Ocean.settings();
        let spec = Spectrum::new(SpectrumParams::of(&s));
        // conj(h0(-k)) of a cell is the conjugate of the mirrored cell's h0.
        let n = FFT_SIZE;
        let (ix, iy) = (n / 2 + 5, n / 2 - 3);
        let a = spec.h0[iy * n + ix];
        let b = spec.h0[(n - iy) * n + (n - ix)];
        assert_eq!((a[2], a[3]), (b[0], -b[1]));
        // Sample the CPU field: its spread matches the requested significant height.
        let f = spec.cpu_field(3.0, 0.0);
        let mean = f.cells.iter().map(|c| c.y).sum::<f32>() / f.cells.len() as f32;
        let var = f.cells.iter().map(|c| (c.y - mean).powi(2)).sum::<f32>() / f.cells.len() as f32;
        let hs = 4.0 * var.sqrt();
        let target = developed_height(s.wind_speed) * s.wave_height;
        assert!(
            hs > target * 0.5 && hs < target * 1.3,
            "Hs {hs} vs {target}"
        );
        assert!(mean.abs() < 0.2, "mean height {mean}");
    }

    #[test]
    fn height_query_inverts_choppy_displacement() {
        let s = WaterPreset::Stormy.settings();
        let state = WaveState::new(&s, None, 12.5);
        let rest = Vec2::new(13.0, -7.0);
        let d = state.displacement(rest, Vec2::ZERO);
        let surface = rest + Vec2::new(d.x, d.z);
        let h = state.height(surface, Vec2::ZERO);
        assert!((h - d.y).abs() < 0.05, "{h} vs {}", d.y);
        // Calm water is nearly flat; waves move over time.
        let calm = WaveState::new(&WaterPreset::Lake.settings(), None, 1.0);
        assert!(calm.height(Vec2::ZERO, Vec2::ZERO).abs() < 0.6);
        let later = WaveState::new(&s, None, 14.0);
        assert_ne!(
            state.height(surface, Vec2::ZERO),
            later.height(surface, Vec2::ZERO)
        );
    }
}
