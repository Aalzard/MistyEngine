//! Grass and small details: placement rules (the same slope / height / spread / breakup rules
//! as layers, plus an optional layer to grow on), a density map filled by Generate, and the
//! deterministic scatter of grass blades per chunk for the renderer.

use bytemuck::{Pod, Zeroable};
use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::auto_paint::{AutoPaintRules, par_rows, rule_mask, surface_at};
use super::brush::Brush;
use super::data::TerrainData;
use super::noise;

/// Most blades per square metre.
pub const MAX_DENSITY: f32 = 64.0;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailSettings {
    pub enabled: bool,
    /// Where grass grows (slope 0..`rules.slope[1]` keeps it off cliffs).
    pub rules: AutoPaintRules,
    /// Only where this layer is painted (weighted by it); None = any layer.
    pub on_layer: Option<usize>,
    /// Blades per square metre at full density.
    pub density: f32,
    /// Blades fade out (shrink) towards this distance.
    pub draw_distance: f32,
    /// Blade height range in metres.
    pub height: [f32; 2],
    pub width: f32,
    /// sRGB colours of healthy and dry blades (mixed by a noise field).
    pub healthy_color: [f32; 3],
    pub dry_color: [f32; 3],
    pub wind_strength: f32,
    pub wind_speed: f32,
}

impl Default for DetailSettings {
    fn default() -> Self {
        Self {
            enabled: false,
            rules: AutoPaintRules {
                enabled: true,
                slope: [0.0, 28.0],
                spread: 0.2,
                breakup: 0.6,
                breakup_scale: 8.0,
                ..Default::default()
            },
            on_layer: None,
            density: 6.0,
            draw_distance: 80.0,
            height: [0.35, 0.7],
            width: 0.05,
            healthy_color: [0.36, 0.52, 0.18],
            dry_color: [0.66, 0.62, 0.32],
            wind_strength: 0.35,
            wind_speed: 1.2,
        }
    }
}

/// Density 0..1 of the detail rules at one splat texel.
fn texel_density(d: &TerrainData, tx: usize, tz: usize) -> f32 {
    let s = &d.details;
    let n = d.alphamap_resolution;
    let at = Vec2::new(
        (tx as f32 + 0.5) / n as f32 * d.size.x,
        (tz as f32 + 0.5) / n as f32 * d.size.z,
    );
    let surface = surface_at(d, at.x, at.y);
    // Hard limit: breakup may thin grass but never pushes it past the slope limit + spread.
    let limit =
        s.rules.slope[0].max(s.rules.slope[1]) + 0.5 + s.rules.spread.clamp(0.0, 1.0) * 30.0;
    if surface.0 > limit {
        return 0.0;
    }
    let seed = d.rule_seed.wrapping_add(0xde7a);
    let mut m = rule_mask(&s.rules, d, at, surface, seed);
    if let Some(l) = s.on_layer.filter(|l| *l < d.layers.len()) {
        m *= d.alphamap[tz * n + tx][l] as f32 / 255.0;
    }
    m
}

/// Fill the detail map from the rules (cleared when details are off).
pub fn generate(d: &mut TerrainData) {
    crate::profile_scope!("Terrain.Details");
    if !d.details.enabled {
        d.detail_map.clear();
        return;
    }
    let n = d.alphamap_resolution;
    let mut out = vec![0u8; n * n];
    par_rows(&mut out, n, |tz, row| {
        for (tx, v) in row.iter_mut().enumerate() {
            *v = (texel_density(d, tx, tz) * 255.0).round() as u8;
        }
    });
    d.detail_map = out;
}

/// Move the detail map under the brush towards the rules by `k` (Generate in brush).
pub fn generate_in_brush(d: &mut TerrainData, b: &Brush, k: f32) {
    if !d.details.enabled {
        return;
    }
    let n = d.alphamap_resolution;
    if d.detail_map.len() != n * n {
        d.detail_map = vec![0; n * n];
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let a = k * b.weight((Vec2::new(tx as f32, tz as f32) + 0.5) * texel);
            if a > 0.0 {
                let cur = d.detail_map[tz * n + tx] as f32;
                let target = texel_density(d, tx, tz) * 255.0;
                d.detail_map[tz * n + tx] = (cur + (target - cur) * a).round() as u8;
            }
        }
    }
}

/// One grass blade for the renderer (terrain-local position).
#[repr(C)]
#[derive(Clone, Copy, Debug, Pod, Zeroable, PartialEq)]
pub struct Blade {
    /// xyz = root position, w = height.
    pub position: [f32; 4],
    /// x = facing angle (radians), y = width, z = dry mix 0..1, w = random phase 0..1.
    pub shape: [f32; 4],
}

/// Blades rooted in the local rectangle `[lo, hi)` (metres), deterministic per seed.
pub fn scatter(d: &TerrainData, lo: Vec2, hi: Vec2) -> Vec<Blade> {
    let s = &d.details;
    let n = d.alphamap_resolution;
    if !s.enabled || d.detail_map.len() != n * n {
        return Vec::new();
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let per_texel = s.density.clamp(0.0, MAX_DENSITY) * texel.x * texel.y;
    let (t0, t1) = (
        (lo / texel).floor().max(Vec2::ZERO),
        (hi / texel).ceil().min(Vec2::splat(n as f32)),
    );
    let mut out = Vec::new();
    let seed = d.rule_seed;
    for tz in t0.y as usize..t1.y as usize {
        for tx in t0.x as usize..t1.x as usize {
            let density = d.detail_map[tz * n + tx];
            if density == 0 {
                continue;
            }
            // Expected count; the fraction becomes one more blade with that probability.
            let want = per_texel * density as f32 / 255.0;
            let (ix, iz) = (tx as i32, tz as i32);
            let count =
                want.floor() as u32 + u32::from(noise::hash(ix, iz, seed ^ 0x51) < want.fract());
            for k in 0..count {
                let r = |salt: u32| noise::hash(ix, iz, seed.wrapping_add(k * 97 + salt));
                let p = Vec2::new(tx as f32 + r(1), tz as f32 + r(2)) * texel;
                if p.x < lo.x || p.y < lo.y || p.x >= hi.x || p.y >= hi.y {
                    continue;
                }
                let tall = s.height[0] + (s.height[1] - s.height[0]) * r(3);
                let dry = (noise::fbm(p.x, p.y, 9.0, 3, seed ^ 0x77) * 0.5 + 0.5).clamp(0.0, 1.0);
                out.push(Blade {
                    position: [p.x, d.sample_height(p.x, p.y), p.y, tall],
                    shape: [
                        r(4) * std::f32::consts::TAU,
                        s.width * (0.7 + 0.6 * r(5)),
                        (dry * 1.4 - 0.2 + 0.2 * r(6)).clamp(0.0, 1.0),
                        r(7),
                    ],
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Vec3;

    /// Flat for x < 64 m, a 60° cliff after.
    fn cliff() -> TerrainData {
        let mut d = TerrainData::new(129, Vec3::new(128.0, 120.0, 128.0), 128);
        for iz in 0..129 {
            for ix in 0..129 {
                let rise = (ix as f32 - 64.0).max(0.0) * 3f32.sqrt();
                d.set_height(ix, iz, rise / 120.0);
            }
        }
        d.details.enabled = true;
        d.details.rules.breakup = 0.0;
        d
    }

    #[test]
    fn grass_grows_on_flat_ground_only() {
        let mut d = cliff();
        generate(&mut d);
        let n = d.alphamap_resolution;
        for tz in (0..n).step_by(7) {
            assert_eq!(d.detail_map[tz * n + 20], 255, "dense on the flat");
            assert_eq!(d.detail_map[tz * n + 100], 0, "none on the cliff");
        }
        let blades = scatter(&d, Vec2::ZERO, Vec2::splat(128.0));
        assert!(blades.len() > 10_000, "{}", blades.len());
        for b in &blades {
            // The cliff starts at x = 64 m (texels there straddle its foot).
            assert!(
                b.position[0] < 65.0,
                "blade on the cliff at x = {}",
                b.position[0]
            );
            let ground = d.sample_height(b.position[0], b.position[2]);
            assert!(
                (b.position[1] - ground).abs() < 1e-4,
                "rooted on the ground"
            );
        }
        // Deterministic, and chunks add up to the whole.
        assert_eq!(blades, scatter(&d, Vec2::ZERO, Vec2::splat(128.0)));
        let halves = scatter(&d, Vec2::ZERO, Vec2::new(64.0, 128.0)).len()
            + scatter(&d, Vec2::new(64.0, 0.0), Vec2::splat(128.0)).len();
        assert_eq!(halves, blades.len());
    }

    #[test]
    fn grass_only_on_its_layer() {
        let mut d = cliff();
        d.layers = vec![Default::default(), Default::default()];
        let n = d.alphamap_resolution;
        for tz in 0..n {
            for tx in 0..n / 4 {
                d.alphamap[tz * n + tx] = [0, 255, 0, 0];
            }
        }
        d.details.on_layer = Some(0);
        generate(&mut d);
        assert_eq!(d.detail_map[5 * n + 10], 0, "layer 1 here");
        assert_eq!(d.detail_map[5 * n + 40], 255);
    }

    #[test]
    fn disabled_details_clear_the_map() {
        let mut d = cliff();
        generate(&mut d);
        d.details.enabled = false;
        generate(&mut d);
        assert!(d.detail_map.is_empty());
        assert!(scatter(&d, Vec2::ZERO, Vec2::splat(128.0)).is_empty());
    }
}
