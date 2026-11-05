//! Splat painting from biome rules: each texel's biome weights × feature weights pick the biome
//! textures, which are folded into the terrain's (at most four) layers.
//!
//! Painting sits behind [`SplatPainter`] so another painter (e.g. the rule-based terrain
//! auto-painter) can replace it without touching the rest of the pipeline.

use super::biome::{Biome, Feature};
use super::classify::{Fields, classify};
use super::par_rows;
use crate::terrain::{MAX_LAYERS, TerrainLayer};

/// Layers and splat map for a terrain.
#[derive(Clone, Debug)]
pub struct Painted {
    pub layers: Vec<TerrainLayer>,
    /// RGBA weights per texel (channel n = layer n), on the fields' grid.
    pub alphamap: Vec<[u8; 4]>,
}

pub trait SplatPainter {
    fn paint(&self, fields: &Fields, biomes: &[Biome]) -> Painted;
}

/// The World Builder's own painter: biome feature textures.
pub struct BiomeRules;

/// One distinct texture used by some biome feature.
struct Slot {
    layer: TerrainLayer,
    /// Total weight over the map (decides which survive when there are more than four).
    usage: f64,
}

/// Texture key of a feature layer (same file = same terrain layer).
fn key(b: &Biome, f: Feature) -> String {
    b.layer(f).texture.clone().unwrap_or_default()
}

impl SplatPainter for BiomeRules {
    fn paint(&self, fields: &Fields, biomes: &[Biome]) -> Painted {
        let n = fields.res;
        let bc = fields.biome_count.min(biomes.len()).max(1);
        // Distinct textures in first-use order.
        let mut slots: Vec<Slot> = Vec::new();
        let mut slot_of = vec![[0usize; 5]; bc];
        for (bi, b) in biomes.iter().take(bc).enumerate() {
            for (fi, f) in Feature::ALL.into_iter().enumerate() {
                let k = key(b, f);
                let at = slots
                    .iter()
                    .position(|s| s.layer.diffuse.clone().unwrap_or_default() == k)
                    .unwrap_or_else(|| {
                        let l = b.layer(f);
                        slots.push(Slot {
                            layer: TerrainLayer {
                                name: texture_name(&k, f),
                                diffuse: l.texture.clone(),
                                normal_map: l.normal_map.clone(),
                                tile_size: [l.tiling.max(0.1); 2],
                                smoothness: if f == Feature::Peak { 0.3 } else { 0.05 },
                                ..Default::default()
                            },
                            usage: 0.0,
                        });
                        slots.len() - 1
                    });
                slot_of[bi][fi] = at;
            }
        }
        // Weight per texel per slot.
        let sc = slots.len();
        let mut weights = vec![0.0f32; n * n * sc];
        par_rows(&mut weights, n * sc, |tz, row| {
            for tx in 0..n {
                let i = tz * n + tx;
                let out = &mut row[tx * sc..(tx + 1) * sc];
                for (bi, b) in biomes.iter().take(bc).enumerate() {
                    let wb = fields.biome_weight(i, bi);
                    if wb <= 0.0 {
                        continue;
                    }
                    let fw = classify(&fields.info[i], b, fields.rock[i]);
                    for (fi, v) in fw.iter().enumerate() {
                        out[slot_of[bi][fi]] += wb * v;
                    }
                }
            }
        });
        for (i, w) in weights.iter().enumerate() {
            slots[i % sc].usage += *w as f64;
        }
        // Keep each used biome's Flat texture (its identity), then the most used others, at most
        // four, in first-use order.
        let mut kept: Vec<usize> = Vec::new();
        for (bi, slots) in slot_of.iter().enumerate() {
            let used = (0..n * n).any(|i| fields.biome_weight(i, bi) > 0.25);
            let flat = slots[0];
            if used && !kept.contains(&flat) && kept.len() < MAX_LAYERS {
                kept.push(flat);
            }
        }
        let mut order: Vec<usize> = (0..sc).filter(|s| !kept.contains(s)).collect();
        order.sort_by(|a, b| slots[*b].usage.total_cmp(&slots[*a].usage).then(a.cmp(b)));
        kept.extend(order.into_iter().take(MAX_LAYERS - kept.len()));
        kept.sort();
        // A dropped texture's weight goes to its biome's Flat texture (else the first layer).
        let channel = |s: usize| kept.iter().position(|k| *k == s);
        let fallback: Vec<usize> = (0..sc)
            .map(|s| {
                channel(s).unwrap_or_else(|| {
                    (0..bc)
                        .find(|b| slot_of[*b].contains(&s))
                        .and_then(|b| channel(slot_of[b][0]))
                        .unwrap_or(0)
                })
            })
            .collect();
        let layers = kept.iter().map(|s| slots[*s].layer.clone()).collect();
        let mut alphamap = vec![[0u8; 4]; n * n];
        par_rows(&mut alphamap, n, |tz, row| {
            for (tx, out) in row.iter_mut().enumerate() {
                let i = tz * n + tx;
                let w = &weights[i * sc..(i + 1) * sc];
                let mut v = [0.0f32; 4];
                for (s, x) in w.iter().enumerate() {
                    v[fallback[s]] += x;
                }
                let sum: f32 = v.iter().sum();
                *out = if sum > 1e-5 {
                    quantize(v.map(|x| x / sum))
                } else {
                    [255, 0, 0, 0]
                };
            }
        });
        Painted { layers, alphamap }
    }
}

/// Weights to bytes that sum to exactly 255.
fn quantize(w: [f32; 4]) -> [u8; 4] {
    let mut q = w.map(|x| (x * 255.0).round().clamp(0.0, 255.0) as u8);
    let sum: i32 = q.iter().map(|v| *v as i32).sum();
    let big = (0..4).max_by_key(|i| q[*i]).unwrap_or(0);
    q[big] = (q[big] as i32 + 255 - sum).clamp(0, 255) as u8;
    q
}

/// Layer name from a texture path ("…/Rock.png" → "Rock"), else the feature.
fn texture_name(path: &str, f: Feature) -> String {
    std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| f.label().to_owned())
}
