//! Rule-based auto painting: each rule-driven layer covers a slope band (and optionally a
//! height band), widened by a soft Spread falloff and made irregular by fBm Breakup noise.
//! Layers stack by priority (higher covers lower); the lowest rule-driven layer fills what is
//! left. Generate repaints only rule-driven layers: weight on manual layers is kept.

use glam::Vec2;
use serde::{Deserialize, Serialize};

use super::brush::{Brush, quantize_weights};
use super::data::{MAX_LAYERS, TerrainData};
use super::noise;

/// Slope falloff at Spread 1 (degrees) and height falloff at Spread 1 (fraction of the height).
const SLOPE_SPREAD: f32 = 30.0;
const HEIGHT_SPREAD: f32 = 0.15;
/// Breakup amplitude at strength 1, on top of the falloff width.
const SLOPE_BREAKUP: f32 = 10.0;
const HEIGHT_BREAKUP: f32 = 0.06;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AutoPaintRules {
    /// Rule-driven: Generate paints this layer from the rules below.
    pub enabled: bool,
    /// Slope band in degrees (0 = flat, 90 = vertical).
    pub slope: [f32; 2],
    pub use_height: bool,
    /// Height band in metres above the terrain's base.
    pub height: [f32; 2],
    /// 0..1: how far the layer fades out past its band edges.
    pub spread: f32,
    /// 0..1: strength of the noise that makes the edges irregular.
    pub breakup: f32,
    /// Feature size of the breakup noise in metres.
    pub breakup_scale: f32,
    /// Higher priorities paint over lower ones.
    pub priority: i32,
}

impl Default for AutoPaintRules {
    fn default() -> Self {
        Self {
            enabled: false,
            slope: [0.0, 90.0],
            use_height: false,
            height: [0.0, 1000.0],
            spread: 0.25,
            breakup: 0.5,
            breakup_scale: 12.0,
            priority: 0,
        }
    }
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Coverage 0..1 of a band `[lo, hi]` at `v`: 1 inside, fading over `feather` outside.
/// `shift` moves the edges (breakup); `open_lo` / `open_hi` drop a side (no limit there).
fn band(v: f32, lo: f32, hi: f32, feather: f32, shift: f32, open: (bool, bool)) -> f32 {
    let below = if open.0 { f32::MAX } else { v - lo + shift };
    let above = if open.1 { f32::MAX } else { hi - v + shift };
    smoothstep(-feather, 0.0, below.min(above))
}

/// Terrain surface under a local point: (slope in degrees, height in metres).
pub fn surface_at(d: &TerrainData, x: f32, z: f32) -> (f32, f32) {
    let s = d.spacing();
    let dx = (d.sample_height(x + s.x, z) - d.sample_height(x - s.x, z)) / (2.0 * s.x);
    let dz = (d.sample_height(x, z + s.y) - d.sample_height(x, z - s.y)) / (2.0 * s.y);
    let slope = (dx * dx + dz * dz).sqrt().atan().to_degrees();
    (slope, d.sample_height(x, z))
}

/// How much one layer's rules want the point (0..1), before priorities.
pub fn rule_mask(
    r: &AutoPaintRules,
    d: &TerrainData,
    at: Vec2,
    surface: (f32, f32),
    seed: u32,
) -> f32 {
    let (slope, height) = surface;
    let noise = if r.breakup > 0.0 {
        noise::fbm(at.x, at.y, r.breakup_scale.max(0.1), 4, seed) * r.breakup.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let spread = r.spread.clamp(0.0, 1.0);
    let feather = 0.5 + spread * SLOPE_SPREAD;
    let (lo, hi) = (r.slope[0].min(r.slope[1]), r.slope[0].max(r.slope[1]));
    let mut m = band(
        slope,
        lo,
        hi,
        feather,
        noise * (feather + SLOPE_BREAKUP),
        (lo <= 0.0, hi >= 90.0),
    );
    if r.use_height {
        let top = d.size.y;
        let feather = 0.01 * top + spread * HEIGHT_SPREAD * top;
        let (lo, hi) = (r.height[0].min(r.height[1]), r.height[0].max(r.height[1]));
        m *= band(
            height,
            lo,
            hi,
            feather,
            noise * (feather + HEIGHT_BREAKUP * top),
            (lo <= 0.0, hi >= top),
        );
    }
    m
}

/// Rule-driven layers in painting order (lowest priority first).
fn rule_layers(d: &TerrainData) -> Vec<usize> {
    let mut v: Vec<usize> = (0..d.layers.len().min(MAX_LAYERS))
        .filter(|i| d.layers[*i].rules.enabled)
        .collect();
    v.sort_by_key(|i| (d.layers[*i].rules.priority, *i));
    v
}

/// Breakup seed of a layer (each layer gets its own noise field).
fn layer_seed(d: &TerrainData, layer: usize) -> u32 {
    d.rule_seed
        .wrapping_mul(31)
        .wrapping_add(layer as u32 * 7919)
}

/// The generated weights of one texel; manual layers keep their weight from `current`.
fn texel_weights(
    d: &TerrainData,
    order: &[usize],
    tx: usize,
    tz: usize,
    current: [u8; MAX_LAYERS],
) -> [u8; MAX_LAYERS] {
    let n = d.alphamap_resolution as f32;
    let at = Vec2::new(
        (tx as f32 + 0.5) / n * d.size.x,
        (tz as f32 + 0.5) / n * d.size.z,
    );
    let surface = surface_at(d, at.x, at.y);
    let mut w = [0.0f32; MAX_LAYERS];
    for (k, &i) in order.iter().enumerate() {
        let m = rule_mask(&d.layers[i].rules, d, at, surface, layer_seed(d, i));
        // The lowest layer is the base: it fills whatever the others leave.
        let m = if k == 0 { 1.0 } else { m };
        for &j in &order[..k] {
            w[j] *= 1.0 - m;
        }
        w[i] = m;
    }
    let mut manual = 0.0;
    for (i, c) in current.iter().enumerate() {
        if !order.contains(&i) && i < d.layers.len() {
            w[i] = *c as f32 / 255.0;
            manual += w[i];
        }
    }
    let budget = (1.0 - manual).max(0.0);
    for &i in order {
        w[i] *= budget;
    }
    quantize_weights(w, order[0])
}

/// Run `f(row index, row)` over the rows of `out` on all cores.
pub(crate) fn par_rows<T: Send>(out: &mut [T], row_len: usize, f: impl Fn(usize, &mut [T]) + Sync) {
    let rows = out.len() / row_len.max(1);
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(4)
        .min(rows.max(1));
    let per = rows.div_ceil(threads).max(1);
    std::thread::scope(|s| {
        for (c, chunk) in out.chunks_mut(per * row_len).enumerate() {
            let f = &f;
            s.spawn(move || {
                for (r, row) in chunk.chunks_mut(row_len).enumerate() {
                    f(c * per + r, row);
                }
            });
        }
    });
}

/// Repaint the whole splat map from the rules (and the detail map, see `details`).
/// Returns false when no layer is rule-driven.
pub fn generate(d: &mut TerrainData) -> bool {
    crate::profile_scope!("Terrain.AutoPaint");
    let order = rule_layers(d);
    if order.is_empty() {
        return false;
    }
    let n = d.alphamap_resolution;
    d.fit_splat_maps();
    if d.alphamap_extra.is_empty() {
        // Four layers or fewer: one map.
        let mut out = d.alphamap.clone();
        par_rows(&mut out, n, |tz, row| {
            for (tx, px) in row.iter_mut().enumerate() {
                let mut all = [0u8; MAX_LAYERS];
                all[..4].copy_from_slice(px);
                let w = texel_weights(d, &order, tx, tz, all);
                *px = [w[0], w[1], w[2], w[3]];
            }
        });
        d.alphamap = out;
        return true;
    }
    let mut out = vec![[0u8; MAX_LAYERS]; n * n];
    par_rows(&mut out, n, |tz, row| {
        for (tx, px) in row.iter_mut().enumerate() {
            *px = texel_weights(d, &order, tx, tz, d.layer_weights(tz * n + tx));
        }
    });
    for (t, w) in out.iter().enumerate() {
        d.set_layer_weights(t, w);
    }
    true
}

/// One frame of "Generate in brush": texels under the brush move towards the generated
/// weights at `rate` per second (like painting a layer).
pub fn generate_in_brush(d: &mut TerrainData, b: &Brush, dt: f32) {
    let order = rule_layers(d);
    if order.is_empty() {
        return;
    }
    let n = d.alphamap_resolution;
    d.fit_splat_maps();
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let k = (super::brush::PAINT_RATE * b.strength * dt).min(1.0);
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    let mut changes = Vec::new();
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let a = k * b.weight((Vec2::new(tx as f32, tz as f32) + 0.5) * texel);
            if a <= 0.0 {
                continue;
            }
            let cur = d.layer_weights(tz * n + tx);
            let target = texel_weights(d, &order, tx, tz, cur);
            let mixed: [f32; MAX_LAYERS] = std::array::from_fn(|c| {
                cur[c] as f32 / 255.0 + (target[c] as f32 - cur[c] as f32) / 255.0 * a
            });
            changes.push((tz * n + tx, quantize_weights(mixed, order[0])));
        }
    }
    for (i, px) in changes {
        d.set_layer_weights(i, &px);
    }
    super::details::generate_in_brush(d, b, k);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::terrain::TerrainLayer;
    use glam::Vec3;

    /// 128 m square, 40 m tall: flat for x < 48 m, then steeper and steeper towards +x.
    fn valley() -> TerrainData {
        let mut d = TerrainData::new(129, Vec3::new(128.0, 40.0, 128.0), 128);
        for iz in 0..129 {
            for ix in 0..129 {
                let x = ix as f32;
                let t = ((x - 48.0) / 80.0).max(0.0);
                d.set_height(ix, iz, t * t);
            }
        }
        let rule = |slope: [f32; 2], priority| AutoPaintRules {
            enabled: true,
            slope,
            breakup: 0.0,
            spread: 0.1,
            priority,
            ..Default::default()
        };
        d.layers = vec![
            TerrainLayer {
                name: "Grass".into(),
                rules: rule([0.0, 25.0], 0),
                ..Default::default()
            },
            TerrainLayer {
                name: "Rock".into(),
                rules: rule([30.0, 90.0], 1),
                ..Default::default()
            },
        ];
        d
    }

    fn weight(d: &TerrainData, layer: usize, x: f32, z: f32) -> f32 {
        d.weights_at(x, z)[layer]
    }

    /// x (metres) where rock first passes 50 % along the row at `tz`.
    fn edge(d: &TerrainData, tz: usize) -> f32 {
        let n = d.alphamap_resolution;
        (0..n)
            .find(|tx| d.alphamap[tz * n + tx][1] > 127)
            .map(|tx| tx as f32)
            .unwrap_or(f32::NAN)
    }

    #[test]
    fn rock_on_steep_ground_grass_on_flat() {
        let mut d = valley();
        assert!(generate(&mut d));
        for z in [10.0, 64.0, 120.0] {
            assert!(weight(&d, 0, 20.0, z) > 0.95, "grass on the flat");
            assert!(weight(&d, 1, 125.0, z) > 0.95, "rock on the steep end");
        }
        // Every texel sums to 255.
        assert!(
            d.alphamap
                .iter()
                .all(|p| p.iter().map(|v| *v as u32).sum::<u32>() == 255)
        );
    }

    #[test]
    fn breakup_makes_edges_irregular_and_is_seeded() {
        let mut plain = valley();
        generate(&mut plain);
        let edges: Vec<f32> = (0..128).map(|tz| edge(&plain, tz)).collect();
        // No breakup: the edge is one straight line across the terrain.
        assert!(edges.iter().all(|e| *e == edges[0]), "{edges:?}");

        let noisy = |seed| {
            let mut d = valley();
            d.rule_seed = seed;
            for l in &mut d.layers {
                l.rules.breakup = 0.8;
                l.rules.breakup_scale = 10.0;
            }
            generate(&mut d);
            d
        };
        let a = noisy(3);
        let edges: Vec<f32> = (0..128).map(|tz| edge(&a, tz)).collect();
        let mean = edges.iter().sum::<f32>() / edges.len() as f32;
        let spread = edges.iter().map(|e| (e - mean).abs()).sum::<f32>() / edges.len() as f32;
        assert!(spread > 1.5, "edge wanders ({spread} texels)");
        let distinct: std::collections::HashSet<i32> = edges.iter().map(|e| *e as i32).collect();
        assert!(distinct.len() > 5, "not a straight iso-line");
        assert_eq!(a.alphamap, noisy(3).alphamap, "same seed, same result");
        assert_ne!(a.alphamap, noisy(4).alphamap, "another seed, another edge");
    }

    #[test]
    fn spread_widens_the_transition() {
        let blended = |spread: f32| {
            let mut d = valley();
            for l in &mut d.layers {
                l.rules.spread = spread;
            }
            generate(&mut d);
            d.alphamap
                .iter()
                .filter(|p| p[1] > 12 && p[1] < 243)
                .count()
        };
        let (narrow, wide) = (blended(0.0), blended(0.8));
        assert!(wide > narrow * 2, "{narrow} -> {wide}");
    }

    #[test]
    fn manual_layers_keep_their_strokes() {
        let mut d = valley();
        d.layers.push(TerrainLayer {
            name: "Path".into(),
            ..Default::default()
        });
        let n = d.alphamap_resolution;
        // A hand-painted path at x = 10 m.
        for tz in 0..n {
            d.alphamap[tz * n + 10] = [55, 0, 200, 0];
        }
        generate(&mut d);
        for tz in 0..n {
            assert_eq!(d.alphamap[tz * n + 10][2], 200, "manual weight kept");
            assert_eq!(d.alphamap[tz * n + 10][0], 55, "grass fills the rest");
            assert_eq!(d.alphamap[tz * n + 30][2], 0);
        }
    }

    #[test]
    fn height_band_paints_snow_on_tops() {
        let mut d = valley();
        d.layers.push(TerrainLayer {
            name: "Snow".into(),
            rules: AutoPaintRules {
                enabled: true,
                use_height: true,
                height: [30.0, 40.0],
                breakup: 0.0,
                spread: 0.0,
                priority: 5,
                ..Default::default()
            },
            ..Default::default()
        });
        generate(&mut d);
        assert!(weight(&d, 2, 127.0, 64.0) > 0.9, "snow high up");
        assert!(weight(&d, 2, 60.0, 64.0) < 0.01, "no snow low down");
    }

    #[test]
    fn generate_in_brush_only_touches_the_brush() {
        let mut d = valley();
        d.alphamap.fill([0, 255, 0, 0]);
        let b = Brush {
            center: Vec2::new(20.0, 20.0),
            size: 10.0,
            strength: 1.0,
            falloff: 0.0,
        };
        for _ in 0..30 {
            generate_in_brush(&mut d, &b, 0.1);
        }
        assert!(
            weight(&d, 0, 20.0, 20.0) > 0.95,
            "grass generated in the brush"
        );
        assert_eq!(d.weights_at(60.0, 60.0)[1], 1.0, "untouched outside");
    }
}
