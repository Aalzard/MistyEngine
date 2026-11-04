//! Erosion on a heightmap in metres.
//!
//! Hydraulic: particle droplets run downhill, pick up sediment where they speed up and drop it
//! where they slow down, carving gullies and filling valley floors. Droplets run in batches on
//! a fixed number of chunks; each chunk sees the heightmap at the start of the batch plus its own
//! changes, and the chunks' changes are summed in a fixed order. The result is the same on every
//! machine and thread count, and mass is conserved (sediment still carried at the end of a
//! droplet's life is dropped where it stops).
//!
//! Thermal: material on slopes steeper than the talus angle slides to lower neighbours
//! (gather form, so every cell updates independently and in parallel).

use serde::{Deserialize, Serialize};

use super::Progress;
use super::noise::Rng;
use super::par_rows;

/// Chunks per batch (fixed so results don't depend on the CPU).
const CHUNKS: usize = 16;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ErosionSettings {
    pub hydraulic: bool,
    /// Droplet passes; one pass drops one droplet per 8 heightmap samples.
    pub hydraulic_iterations: u32,
    /// Erosion and deposition speed (0..1).
    pub hydraulic_strength: f32,
    pub thermal: bool,
    pub thermal_iterations: u32,
    /// Material slides off slopes steeper than this (degrees).
    pub talus_angle: f32,
    /// Fraction of the excess moved per iteration (0..1).
    pub thermal_strength: f32,
}

impl Default for ErosionSettings {
    fn default() -> Self {
        Self {
            hydraulic: true,
            hydraulic_iterations: 4,
            hydraulic_strength: 0.5,
            thermal: true,
            thermal_iterations: 20,
            talus_angle: 40.0,
            thermal_strength: 0.4,
        }
    }
}

/// Droplet constants (in heightmap cell units).
struct Droplet {
    inertia: f32,
    capacity: f32,
    min_capacity: f32,
    erode: f32,
    deposit: f32,
    evaporate: f32,
    gravity: f32,
    lifetime: u32,
}

impl Droplet {
    fn new(strength: f32) -> Self {
        let s = strength.clamp(0.0, 1.0);
        Self {
            inertia: 0.05,
            capacity: 4.0,
            min_capacity: 0.01,
            erode: 0.1 + 0.5 * s,
            deposit: 0.1 + 0.3 * s,
            evaporate: 0.015,
            gravity: 4.0,
            lifetime: 40,
        }
    }
}

/// Erosion brush: cell offsets and normalised weights.
fn brush(radius: i32) -> Vec<(i32, i32, f32)> {
    let mut out = Vec::new();
    for dz in -radius..=radius {
        for dx in -radius..=radius {
            let d = ((dx * dx + dz * dz) as f32).sqrt();
            if d <= radius as f32 {
                out.push((dx, dz, 1.0 - d / (radius as f32 + 1.0)));
            }
        }
    }
    let sum: f32 = out.iter().map(|w| w.2).sum();
    out.iter_mut().for_each(|w| w.2 /= sum);
    out
}

/// One chunk's view of the map: the batch snapshot plus its own changes.
struct Local<'a> {
    base: &'a [f32],
    delta: &'a mut [f32],
    res: usize,
}

impl Local<'_> {
    fn at(&self, x: usize, z: usize) -> f32 {
        let i = z * self.res + x;
        self.base[i] + self.delta[i]
    }

    /// Bilinear height and gradient at a point inside [0, res-1).
    fn sample(&self, px: f32, pz: f32) -> (f32, f32, f32) {
        let (x, z) = (px as usize, pz as usize);
        let (u, v) = (px - x as f32, pz - z as f32);
        let h00 = self.at(x, z);
        let h10 = self.at(x + 1, z);
        let h01 = self.at(x, z + 1);
        let h11 = self.at(x + 1, z + 1);
        let gx = (h10 - h00) * (1.0 - v) + (h11 - h01) * v;
        let gz = (h01 - h00) * (1.0 - u) + (h11 - h10) * u;
        let h =
            h00 * (1.0 - u) * (1.0 - v) + h10 * u * (1.0 - v) + h01 * (1.0 - u) * v + h11 * u * v;
        (h, gx, gz)
    }

    /// Add `amount` spread bilinearly over the 4 cells around a point.
    fn deposit(&mut self, px: f32, pz: f32, amount: f32) {
        let (x, z) = (px as usize, pz as usize);
        let (u, v) = (px - x as f32, pz - z as f32);
        let r = self.res;
        self.delta[z * r + x] += amount * (1.0 - u) * (1.0 - v);
        self.delta[z * r + x + 1] += amount * u * (1.0 - v);
        self.delta[(z + 1) * r + x] += amount * (1.0 - u) * v;
        self.delta[(z + 1) * r + x + 1] += amount * u * v;
    }

    /// Remove `amount` with the brush around the cell nearest a point.
    fn erode(&mut self, px: f32, pz: f32, amount: f32, brush: &[(i32, i32, f32)]) {
        let (cx, cz) = (px.round() as i32, pz.round() as i32);
        let r = self.res as i32;
        let inside = |x: i32, z: i32| x >= 0 && z >= 0 && x < r && z < r;
        let sum: f32 = brush
            .iter()
            .filter(|(dx, dz, _)| inside(cx + dx, cz + dz))
            .map(|w| w.2)
            .sum();
        if sum <= 0.0 {
            return;
        }
        for (dx, dz, w) in brush {
            let (x, z) = (cx + dx, cz + dz);
            if inside(x, z) {
                self.delta[(z * r + x) as usize] -= amount * w / sum;
            }
        }
    }
}

/// Simulate `count` droplets on one chunk.
fn run_droplets(
    map: &mut Local,
    rng: &mut Rng,
    count: usize,
    p: &Droplet,
    brush: &[(i32, i32, f32)],
) {
    let max = (map.res - 1) as f32 - 1e-3;
    for _ in 0..count {
        let (mut x, mut z) = (rng.range(0.0, max), rng.range(0.0, max));
        let (mut dx, mut dz) = (0.0f32, 0.0f32);
        let (mut speed, mut water, mut sediment) = (1.0f32, 1.0f32, 0.0f32);
        for _ in 0..p.lifetime {
            let (h, gx, gz) = map.sample(x, z);
            dx = dx * p.inertia - gx * (1.0 - p.inertia);
            dz = dz * p.inertia - gz * (1.0 - p.inertia);
            let len = (dx * dx + dz * dz).sqrt();
            if len < 1e-6 {
                // Flat: pick a random direction.
                let a = rng.range(0.0, std::f32::consts::TAU);
                (dx, dz) = (a.cos(), a.sin());
            } else {
                (dx, dz) = (dx / len, dz / len);
            }
            let (nx, nz) = (x + dx, z + dz);
            if !(0.0..max).contains(&nx) || !(0.0..max).contains(&nz) {
                break;
            }
            let dh = map.sample(nx, nz).0 - h;
            let capacity = (-dh * speed * water * p.capacity).max(p.min_capacity);
            if sediment > capacity || dh > 0.0 {
                // Uphill: fill the pit behind; else drop the excess.
                let amount = if dh > 0.0 {
                    dh.min(sediment)
                } else {
                    (sediment - capacity) * p.deposit
                };
                sediment -= amount;
                map.deposit(x, z, amount);
            } else {
                let amount = ((capacity - sediment) * p.erode).min(-dh);
                map.erode(x, z, amount, brush);
                sediment += amount;
            }
            speed = (speed * speed - dh * p.gravity).max(0.0).sqrt();
            water *= 1.0 - p.evaporate;
            (x, z) = (nx, nz);
        }
        // What it still carries stays where it stopped (mass is conserved).
        if sediment > 0.0 {
            map.deposit(x, z, sediment);
        }
    }
}

/// Hydraulic erosion of `heights` (metres, `res²`), `cell` metres apart. False when cancelled.
pub fn hydraulic(
    heights: &mut [f32],
    res: usize,
    cell: f32,
    s: &ErosionSettings,
    seed: u32,
    progress: &Progress,
) -> bool {
    if res < 4 || s.hydraulic_iterations == 0 {
        return true;
    }
    let cell = cell.max(1e-3);
    // Work in cell units so the droplet constants don't depend on the terrain's scale.
    heights.iter_mut().for_each(|h| *h /= cell);
    let total = s.hydraulic_iterations as usize * res * res / 8;
    let batches = (total / (CHUNKS * 2048)).clamp(1, 64);
    let per_chunk = total.div_ceil(batches * CHUNKS);
    let params = Droplet::new(s.hydraulic_strength);
    let radius = if res >= 1000 {
        3
    } else if res >= 400 {
        2
    } else {
        1
    };
    let brush = brush(radius);
    let mut deltas: Vec<Vec<f32>> = (0..CHUNKS).map(|_| vec![0.0; res * res]).collect();
    let mut finished = true;
    for batch in 0..batches {
        if progress.cancelled() {
            finished = false;
            break;
        }
        let base: &[f32] = heights;
        std::thread::scope(|scope| {
            for (c, delta) in deltas.iter_mut().enumerate() {
                let (params, brush) = (&params, &brush);
                scope.spawn(move || {
                    let mut rng = Rng::new(
                        (seed as u64) << 32 ^ (batch as u64) << 8 ^ c as u64 ^ 0x6a09_e667,
                    );
                    let mut local = Local { base, delta, res };
                    run_droplets(&mut local, &mut rng, per_chunk, params, brush);
                });
            }
        });
        merge(heights, &mut deltas, res);
        progress.set_fraction((batch + 1) as f32 / batches as f32);
    }
    heights.iter_mut().for_each(|h| *h *= cell);
    finished
}

/// Add every chunk's changes to the map (fixed order) and clear them.
fn merge(heights: &mut [f32], deltas: &mut [Vec<f32>], res: usize) {
    let deltas_ref: &[Vec<f32>] = deltas;
    par_rows(heights, res, |z, row| {
        for (x, h) in row.iter_mut().enumerate() {
            let i = z * res + x;
            for d in deltas_ref {
                *h += d[i];
            }
        }
    });
    for d in deltas.iter_mut() {
        d.fill(0.0);
    }
}

/// Thermal erosion of `heights` (metres). False when cancelled.
pub fn thermal(
    heights: &mut [f32],
    res: usize,
    cell: f32,
    s: &ErosionSettings,
    progress: &Progress,
) -> bool {
    if res < 3 {
        return true;
    }
    let talus = s.talus_angle.clamp(1.0, 89.0).to_radians().tan() * cell;
    let k = s.thermal_strength.clamp(0.0, 1.0) * 0.5;
    const N: [(i32, i32, f32); 8] = [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, std::f32::consts::SQRT_2),
        (1, -1, std::f32::consts::SQRT_2),
        (-1, 1, std::f32::consts::SQRT_2),
        (1, 1, std::f32::consts::SQRT_2),
    ];
    let r = res as i32;
    let mut scale = vec![0.0f32; res * res];
    let mut next = vec![0.0f32; res * res];
    for it in 0..s.thermal_iterations {
        if progress.cancelled() {
            return false;
        }
        let h: &[f32] = heights;
        let excess = |x: i32, z: i32, nx: i32, nz: i32, dist: f32| {
            if nx < 0 || nz < 0 || nx >= r || nz >= r {
                return 0.0;
            }
            (h[(z * r + x) as usize] - h[(nz * r + nx) as usize] - talus * dist).max(0.0)
        };
        // Pass 1: how much each cell sheds, per unit of excess.
        par_rows(&mut scale, res, |z, row| {
            for (x, out) in row.iter_mut().enumerate() {
                let (x, z) = (x as i32, z as i32);
                let (mut total, mut most) = (0.0f32, 0.0f32);
                for (dx, dz, d) in N {
                    let e = excess(x, z, x + dx, z + dz, d);
                    total += e;
                    most = most.max(e);
                }
                *out = if total > 0.0 { k * most / total } else { 0.0 };
            }
        });
        // Pass 2: gather what neighbours shed onto each cell.
        let sc: &[f32] = &scale;
        par_rows(&mut next, res, |z, row| {
            for (x, out) in row.iter_mut().enumerate() {
                let (xi, zi) = (x as i32, z as i32);
                let i = z * res + x;
                let mut v = h[i];
                for (dx, dz, d) in N {
                    let (nx, nz) = (xi + dx, zi + dz);
                    // Outflow to the neighbour, inflow from it.
                    v -= sc[i] * excess(xi, zi, nx, nz, d);
                    if nx >= 0 && nz >= 0 && nx < r && nz < r {
                        v += sc[(nz * r + nx) as usize] * excess(nx, nz, xi, zi, d);
                    }
                }
                *out = v;
            }
        });
        heights.copy_from_slice(&next);
        progress.set_fraction((it + 1) as f32 / s.thermal_iterations.max(1) as f32);
    }
    true
}
