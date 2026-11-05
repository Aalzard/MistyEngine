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

/// Erosion brush: offsets with weights, its radius and the weight sum when it lies fully
/// inside the map (summed in the same order as the edge case, so results match exactly).
struct Brush {
    cells: Vec<(i32, i32, f32)>,
    radius: i32,
    interior_sum: f32,
}

impl Brush {
    fn new(radius: i32) -> Self {
        let cells = brush(radius);
        let interior_sum = cells.iter().map(|w| w.2).sum();
        Self {
            cells,
            radius,
            interior_sum,
        }
    }
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
    fn erode(&mut self, px: f32, pz: f32, amount: f32, brush: &Brush) {
        let (cx, cz) = (px.round() as i32, pz.round() as i32);
        let r = self.res as i32;
        let b = brush.radius;
        if cx >= b && cz >= b && cx + b < r && cz + b < r {
            // Fully inside: no per-cell bounds checks.
            let sum = brush.interior_sum;
            let centre = (cz * r + cx) as isize;
            for (dx, dz, w) in &brush.cells {
                let i = (centre + (*dz * r + *dx) as isize) as usize;
                self.delta[i] -= amount * w / sum;
            }
            return;
        }
        let inside = |x: i32, z: i32| x >= 0 && z >= 0 && x < r && z < r;
        let sum: f32 = brush
            .cells
            .iter()
            .filter(|(dx, dz, _)| inside(cx + dx, cz + dz))
            .map(|w| w.2)
            .sum();
        if sum <= 0.0 {
            return;
        }
        for (dx, dz, w) in &brush.cells {
            let (x, z) = (cx + dx, cz + dz);
            if inside(x, z) {
                self.delta[(z * r + x) as usize] -= amount * w / sum;
            }
        }
    }
}

/// Simulate `count` droplets on one chunk.
fn run_droplets(map: &mut Local, rng: &mut Rng, count: usize, p: &Droplet, brush: &Brush) {
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
    let brush = Brush::new(radius);
    let mut deltas: Vec<Vec<f32>> = (0..CHUNKS).map(|_| vec![0.0; res * res]).collect();
    let mut finished = true;
    for batch in 0..batches {
        if progress.cancelled() {
            finished = false;
            break;
        }
        let base: &[f32] = heights;
        {
            use rayon::prelude::*;
            deltas.par_iter_mut().enumerate().for_each(|(c, delta)| {
                let mut rng =
                    Rng::new((seed as u64) << 32 ^ (batch as u64) << 8 ^ c as u64 ^ 0x6a09_e667);
                let mut local = Local { base, delta, res };
                run_droplets(&mut local, &mut rng, per_chunk, &params, &brush);
            });
        }
        merge(heights, &mut deltas, res);
        progress.set_fraction((batch + 1) as f32 / batches as f32);
    }
    heights.iter_mut().for_each(|h| *h *= cell);
    finished
}

/// Shared pointers to the chunk deltas: rows are merged in parallel, each row by one thread.
struct Deltas(Vec<*mut f32>);
// SAFETY: every thread touches only its own rows of each delta (see `merge`).
unsafe impl Sync for Deltas {}

/// Add every chunk's changes to the map (fixed order, so the sum is the same on every machine)
/// and clear them, in one pass.
fn merge(heights: &mut [f32], deltas: &mut [Vec<f32>], res: usize) {
    let n = heights.len();
    assert!(deltas.iter().all(|d| d.len() == n));
    let ptrs = Deltas(deltas.iter_mut().map(|d| d.as_mut_ptr()).collect());
    let ptrs = &ptrs;
    par_rows(heights, res, |z, row| {
        let start = z * res;
        for p in &ptrs.0 {
            // SAFETY: rows [start, start + row.len()) of each delta belong to this call only,
            // and every delta is as long as `heights` (checked above).
            let d = unsafe { std::slice::from_raw_parts_mut(p.add(start), row.len()) };
            for (h, v) in row.iter_mut().zip(d.iter_mut()) {
                *h += *v;
                *v = 0.0;
            }
        }
    });
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
    const SQRT2: f32 = std::f32::consts::SQRT_2;
    const N: [(isize, isize, f32); 8] = [
        (-1, 0, 1.0),
        (1, 0, 1.0),
        (0, -1, 1.0),
        (0, 1, 1.0),
        (-1, -1, SQRT2),
        (1, -1, SQRT2),
        (-1, 1, SQRT2),
        (1, 1, SQRT2),
    ];
    // Height drop allowed towards each neighbour.
    let limit = N.map(|n| talus * n.2);
    let r = res as isize;
    // Neighbour `j` of cell (x, z) at index `i`, if inside: interior cells just add an offset.
    let offset = N.map(|(dx, dz, _)| dz * r + dx);
    let neighbour = |i: isize, x: isize, z: isize, j: usize| -> Option<usize> {
        if x > 0 && z > 0 && x < r - 1 && z < r - 1 {
            return Some((i + offset[j]) as usize);
        }
        let (nx, nz) = (x + N[j].0, z + N[j].1);
        (nx >= 0 && nz >= 0 && nx < r && nz < r).then_some((nz * r + nx) as usize)
    };
    // Per cell: the share of each unit of excess it sheds, and the total it sheds.
    let mut shed = vec![(0.0f32, 0.0f32); res * res];
    let mut next = vec![0.0f32; res * res];
    for it in 0..s.thermal_iterations {
        if progress.cancelled() {
            return false;
        }
        let h: &[f32] = heights;
        // Pass 1: how much each cell sheds.
        par_rows(&mut shed, res, |z, row| {
            let z = z as isize;
            for (x, out) in row.iter_mut().enumerate() {
                let x = x as isize;
                let i = z * r + x;
                let hc = h[i as usize];
                let (mut total, mut most) = (0.0f32, 0.0f32);
                for (j, lim) in limit.iter().enumerate() {
                    if let Some(n) = neighbour(i, x, z, j) {
                        let e = (hc - h[n] - lim).max(0.0);
                        total += e;
                        most = most.max(e);
                    }
                }
                *out = if total > 0.0 {
                    (k * most / total, k * most)
                } else {
                    (0.0, 0.0)
                };
            }
        });
        // Pass 2: each cell loses what it sheds and gathers its share of its neighbours'
        // (a neighbour's excess towards this cell: same distance both ways).
        let sh: &[(f32, f32)] = &shed;
        par_rows(&mut next, res, |z, row| {
            let z = z as isize;
            for (x, out) in row.iter_mut().enumerate() {
                let x = x as isize;
                let i = z * r + x;
                let hc = h[i as usize];
                let mut v = hc - sh[i as usize].1;
                for (j, lim) in limit.iter().enumerate() {
                    if let Some(n) = neighbour(i, x, z, j) {
                        v += sh[n].0 * (h[n] - hc - lim).max(0.0);
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
