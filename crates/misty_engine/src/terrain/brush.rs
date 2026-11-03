//! Sculpting and painting brushes. Each call applies one frame (`dt` seconds) of a stroke at the
//! brush position; rates are per second at full strength in the brush centre.

use glam::Vec2;

use super::data::{MAX_LAYERS, TerrainData};

/// Raise/Lower: fraction of the terrain height per second.
pub const RAISE_RATE: f32 = 0.15;
/// Set Height and Smooth: how fast heights approach their target (1/s).
pub const BLEND_RATE: f32 = 8.0;
/// Paint Texture: how fast a layer's weight approaches 1 (1/s).
pub const PAINT_RATE: f32 = 4.0;

/// A round brush in terrain-local metres.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Brush {
    /// Centre on the terrain (local x, z).
    pub center: Vec2,
    /// Diameter in metres.
    pub size: f32,
    /// 0..1, scales every rate.
    pub strength: f32,
    /// 0 = hard edge, 1 = fades from the centre to the rim.
    pub falloff: f32,
}

impl Brush {
    pub fn radius(&self) -> f32 {
        (self.size * 0.5).max(1e-3)
    }

    /// The touched area as a dirty region of `d` (normalised 0..1, see `store::Region`).
    pub fn region(&self, d: &TerrainData) -> super::store::Region {
        let size = Vec2::new(d.size.x, d.size.z);
        super::store::Region {
            min: ((self.center - self.radius()) / size).to_array(),
            max: ((self.center + self.radius()) / size).to_array(),
        }
    }

    /// Influence (0..1) at a local point.
    pub fn weight(&self, p: Vec2) -> f32 {
        let r = p.distance(self.center) / self.radius();
        if r >= 1.0 {
            return 0.0;
        }
        let inner = 1.0 - self.falloff.clamp(0.0, 1.0);
        if r <= inner {
            return 1.0;
        }
        let t = (r - inner) / (1.0 - inner).max(1e-6);
        1.0 - t * t * (3.0 - 2.0 * t)
    }

    /// Heightmap samples the brush can touch: (ix range, iz range).
    fn sample_rect(&self, d: &TerrainData) -> (std::ops::Range<usize>, std::ops::Range<usize>) {
        let s = d.spacing();
        let r = d.heightmap_resolution;
        let range = |c: f32, step: f32| {
            let lo = ((c - self.radius()) / step).floor().max(0.0) as usize;
            let hi = (((c + self.radius()) / step).ceil().max(0.0) as usize + 1).min(r);
            lo.min(hi)..hi
        };
        (range(self.center.x, s.x), range(self.center.y, s.y))
    }

    /// Apply `f(old normalised height, brush weight) -> new height` to the touched samples.
    fn each_sample(
        &self,
        d: &mut TerrainData,
        mut f: impl FnMut(&TerrainData, usize, usize, f32) -> f32,
    ) {
        let s = d.spacing();
        let (xs, zs) = self.sample_rect(d);
        let mut changed = Vec::new();
        for iz in zs {
            for ix in xs.clone() {
                let w = self.weight(Vec2::new(ix as f32 * s.x, iz as f32 * s.y));
                if w > 0.0 {
                    changed.push((ix, iz, f(d, ix, iz, w)));
                }
            }
        }
        // Written afterwards so smoothing reads the heights from before this step.
        for (ix, iz, h) in changed {
            d.set_height(ix, iz, h);
        }
    }
}

/// Raise (or lower) the terrain under the brush.
pub fn raise(d: &mut TerrainData, b: &Brush, dt: f32, lower: bool) {
    let sign = if lower { -1.0 } else { 1.0 };
    let step = sign * RAISE_RATE * b.strength * dt;
    b.each_sample(d, |d, ix, iz, w| d.height(ix, iz) + step * w);
}

/// Move heights towards `target` metres.
pub fn set_height(d: &mut TerrainData, b: &Brush, dt: f32, target: f32) {
    let goal = (target / d.size.y).clamp(0.0, 1.0);
    let k = (BLEND_RATE * b.strength * dt).min(1.0);
    b.each_sample(d, |d, ix, iz, w| {
        let h = d.height(ix, iz);
        h + (goal - h) * k * w
    });
}

/// Move heights towards the average of their 3×3 neighbourhood.
pub fn smooth(d: &mut TerrainData, b: &Brush, dt: f32) {
    let k = (BLEND_RATE * b.strength * dt).min(1.0);
    b.each_sample(d, |d, ix, iz, w| {
        let mut sum = 0.0;
        for (dx, dz) in (-1i32..=1).flat_map(|x| (-1i32..=1).map(move |z| (x, z))) {
            let x = (ix as i32 + dx).max(0) as usize;
            let z = (iz as i32 + dz).max(0) as usize;
            sum += d.height(x, z);
        }
        let h = d.height(ix, iz);
        h + (sum / 9.0 - h) * k * w
    });
}

/// Paint `layer` into the splat map: its weight grows, the others shrink, the sum stays 1.
pub fn paint(d: &mut TerrainData, b: &Brush, dt: f32, layer: usize) {
    if layer >= MAX_LAYERS {
        return;
    }
    let n = d.alphamap_resolution;
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let k = (PAINT_RATE * b.strength * dt).min(1.0);
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let centre = (Vec2::new(tx as f32, tz as f32) + 0.5) * texel;
            let a = k * b.weight(centre);
            if a <= 0.0 {
                continue;
            }
            let px = &mut d.alphamap[tz * n + tx];
            let mut w = px.map(|v| v as f32 / 255.0);
            for (i, v) in w.iter_mut().enumerate() {
                *v = if i == layer {
                    *v + (1.0 - *v) * a
                } else {
                    *v * (1.0 - a)
                };
            }
            *px = quantize_weights(w, layer);
        }
    }
}

/// Weights to bytes that sum to exactly 255 (rounding error goes to `keep`).
pub(super) fn quantize_weights(w: [f32; 4], keep: usize) -> [u8; 4] {
    let sum: f32 = w.iter().sum::<f32>().max(1e-6);
    let mut out = w.map(|v| (v / sum * 255.0).round().clamp(0.0, 255.0) as u8);
    let total: i32 = out.iter().map(|v| *v as i32).sum();
    out[keep] = (out[keep] as i32 + 255 - total).clamp(0, 255) as u8;
    out
}
