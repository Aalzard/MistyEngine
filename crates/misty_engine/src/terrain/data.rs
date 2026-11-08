//! Terrain data asset: a square heightmap (normalised 0..1 heights scaled by `size.y`), splat
//! (alpha / control) maps with one weight per terrain layer, four layers to a map, and up to
//! sixteen layers. The first map is `alphamap`; maps for layers 5.. follow in
//! `alphamap_extra`, which stays empty while a terrain has four layers or fewer (so such
//! terrains, their files and their drawing are exactly as before more layers existed).
//!
//! Local space: the origin is the terrain's corner; x runs over the width, z over the length.
//! Heightmap sample `(ix, iz)` sits at `(ix * dx, h, iz * dz)` and is stored at `iz * res + ix`.
//! Cells are split into two triangles along the diagonal from `(x0, z1)` to `(x1, z0)`, the same
//! split as rapier's heightfield, so sampled heights, the mesh and the collider agree exactly.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::auto_paint::AutoPaintRules;
use super::cliffs::CliffSettings;
use super::details::DetailSettings;
use super::flowers::FlowerSettings;
use super::harmony::HarmonySettings;
use super::trees::TreeSettings;

/// Most layers a terrain holds (one splat-map channel each, four channels to a map).
pub const MAX_LAYERS: usize = 16;
/// Layers per splat (control) map.
pub const LAYERS_PER_MAP: usize = 4;
/// Heightmap resolutions offered (2^n + 1 samples per side).
pub const HEIGHTMAP_RESOLUTIONS: [usize; 8] = [33, 65, 129, 257, 513, 1025, 2049, 4097];
/// Splat-map resolutions offered (texels per side).
pub const ALPHAMAP_RESOLUTIONS: [usize; 8] = [16, 32, 64, 128, 256, 512, 1024, 2048];

/// One paintable surface: a tiled texture with an optional normal map.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TerrainLayer {
    pub name: String,
    /// Project-relative albedo texture ("Assets/Textures/Grass.png").
    pub diffuse: Option<String>,
    /// Tangent-space normal map (OpenGL convention).
    pub normal_map: Option<String>,
    pub normal_scale: f32,
    /// World size of one texture repeat (metres along x and z).
    pub tile_size: [f32; 2],
    /// Offset of the tiling in metres.
    pub tile_offset: [f32; 2],
    pub metallic: f32,
    pub smoothness: f32,
    /// Greyscale height texture for height-based blending (None = the albedo's luminance).
    pub height_map: Option<String>,
    /// Auto Paint rules; `rules.enabled` marks the layer rule-driven (Generate repaints it).
    pub rules: AutoPaintRules,
}

impl Default for TerrainLayer {
    fn default() -> Self {
        Self {
            name: "New Layer".into(),
            diffuse: None,
            normal_map: None,
            normal_scale: 1.0,
            tile_size: [10.0, 10.0],
            tile_offset: [0.0, 0.0],
            metallic: 0.0,
            smoothness: 0.0,
            height_map: None,
            rules: AutoPaintRules::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct TerrainData {
    /// Samples per side of the heightmap.
    pub heightmap_resolution: usize,
    /// Width (x), maximum height (y) and length (z) in metres.
    pub size: Vec3,
    /// Normalised heights (0..1), `heightmap_resolution²` values, row by row along z.
    pub heights: Vec<f32>,
    /// Texels per side of the splat map.
    pub alphamap_resolution: usize,
    /// Layer weights per texel (channel n = layer n), row by row along z.
    pub alphamap: Vec<[u8; 4]>,
    /// Weights of layers 5.. (`LAYERS_PER_MAP` to a map, the maps one after another, each
    /// `alphamap_resolution²` texels); empty with four layers or fewer. All of a texel's
    /// weights over every map sum to 255.
    pub alphamap_extra: Vec<[u8; 4]>,
    pub layers: Vec<TerrainLayer>,
    /// Seed of the Auto Paint breakup noise and the detail scatter.
    pub rule_seed: u32,
    /// Triplanar cliffs and height blending (shader settings).
    pub cliffs: CliffSettings,
    /// Color Harmony (non-destructive per-layer colour adjustment).
    pub harmony: HarmonySettings,
    /// Grass / detail placement rules and look.
    pub details: DetailSettings,
    /// Detail density per splat texel (0..255 of `details.density`); empty = no details yet.
    pub detail_map: Vec<u8>,
    /// Wildflowers growing in patches in the grass (drawn with it).
    pub flowers: FlowerSettings,
    /// Tree prototypes, placed trees and how they are drawn.
    pub trees: TreeSettings,
    /// Holes (Paint Holes): one byte per heightmap cell (`(res - 1)²`, row by row along z),
    /// non-zero = cut out of the surface and the collider; empty = no holes.
    pub holes: Vec<u8>,
    /// More grass species and detail meshes, each with its painted density (Paint Details).
    pub detail_layers: Vec<super::detail_layers::DetailLayer>,
}

impl Default for TerrainData {
    fn default() -> Self {
        Self::new(257, Vec3::new(200.0, 60.0, 200.0), 256)
    }
}

/// Closest allowed value of `list`.
fn closest(list: &[usize], n: usize) -> usize {
    *list
        .iter()
        .min_by_key(|v| v.abs_diff(n))
        .expect("non-empty list")
}

impl TerrainData {
    /// Flat terrain at height 0, painted with layer 0.
    pub fn new(resolution: usize, size: Vec3, alphamap_resolution: usize) -> Self {
        let res = closest(&HEIGHTMAP_RESOLUTIONS, resolution);
        let ares = closest(&ALPHAMAP_RESOLUTIONS, alphamap_resolution);
        Self {
            heightmap_resolution: res,
            size: sanitize_size(size),
            heights: vec![0.0; res * res],
            alphamap_resolution: ares,
            alphamap: vec![[255, 0, 0, 0]; ares * ares],
            alphamap_extra: Vec::new(),
            layers: Vec::new(),
            rule_seed: 1,
            cliffs: CliffSettings::default(),
            harmony: HarmonySettings::default(),
            details: DetailSettings::default(),
            detail_map: Vec::new(),
            flowers: FlowerSettings::default(),
            trees: TreeSettings::default(),
            holes: Vec::new(),
            detail_layers: Vec::new(),
        }
    }

    /// Distance between neighbouring heightmap samples along x and z.
    pub fn spacing(&self) -> Vec2 {
        let cells = (self.heightmap_resolution - 1).max(1) as f32;
        Vec2::new(self.size.x / cells, self.size.z / cells)
    }

    /// Normalised height of sample `(ix, iz)` (clamped to the grid).
    pub fn height(&self, ix: usize, iz: usize) -> f32 {
        let r = self.heightmap_resolution;
        self.heights[iz.min(r - 1) * r + ix.min(r - 1)]
    }

    pub fn set_height(&mut self, ix: usize, iz: usize, h: f32) {
        let r = self.heightmap_resolution;
        self.heights[iz * r + ix] = h.clamp(0.0, 1.0);
    }

    /// Height in metres at a local point (clamped to the terrain's edges).
    pub fn sample_height(&self, x: f32, z: f32) -> f32 {
        self.sample_normalized(x, z) * self.size.y
    }

    /// Normalised height at a local point, interpolated over the cell's triangle.
    pub fn sample_normalized(&self, x: f32, z: f32) -> f32 {
        let s = self.spacing();
        let cells = (self.heightmap_resolution - 1) as f32;
        let fx = (x / s.x.max(1e-6)).clamp(0.0, cells);
        let fz = (z / s.y.max(1e-6)).clamp(0.0, cells);
        let (ix, iz) = (
            (fx as usize).min(self.heightmap_resolution - 2),
            (fz as usize).min(self.heightmap_resolution - 2),
        );
        let (u, v) = (fx - ix as f32, fz - iz as f32);
        let h00 = self.height(ix, iz);
        let h10 = self.height(ix + 1, iz);
        let h01 = self.height(ix, iz + 1);
        let h11 = self.height(ix + 1, iz + 1);
        if u + v <= 1.0 {
            h00 + (h10 - h00) * u + (h01 - h00) * v
        } else {
            h11 + (h01 - h11) * (1.0 - u) + (h10 - h11) * (1.0 - v)
        }
    }

    /// Smooth surface normal at sample `(ix, iz)` from central differences.
    pub fn vertex_normal(&self, ix: usize, iz: usize) -> Vec3 {
        let r = self.heightmap_resolution;
        let s = self.spacing();
        let h = |x: usize, z: usize| self.height(x, z) * self.size.y;
        let (x0, x1) = (ix.saturating_sub(1), (ix + 1).min(r - 1));
        let (z0, z1) = (iz.saturating_sub(1), (iz + 1).min(r - 1));
        let dx = (h(x1, iz) - h(x0, iz)) / ((x1 - x0) as f32 * s.x).max(1e-6);
        let dz = (h(ix, z1) - h(ix, z0)) / ((z1 - z0) as f32 * s.y).max(1e-6);
        Vec3::new(-dx, 1.0, -dz).normalize()
    }

    /// Normalised (0..1) height range of the heightmap.
    pub fn height_range(&self) -> (f32, f32) {
        self.heights
            .iter()
            .fold((f32::MAX, f32::MIN), |(lo, hi), h| (lo.min(*h), hi.max(*h)))
    }

    /// Local bounds (min, max) in metres.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let (lo, hi) = self.height_range();
        (
            Vec3::new(0.0, lo * self.size.y, 0.0),
            Vec3::new(self.size.x, hi * self.size.y, self.size.z),
        )
    }

    /// Layer weights (0..1) at a local point, from the nearest splat texel.
    pub fn weights_at(&self, x: f32, z: f32) -> [f32; MAX_LAYERS] {
        let n = self.alphamap_resolution;
        let tx = ((x / self.size.x) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        let tz = ((z / self.size.z) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        self.layer_weights(tz * n + tx).map(|w| w as f32 / 255.0)
    }

    /// Splat maps the layers need (four layers to a map, at least one).
    pub fn splat_maps(&self) -> usize {
        self.layers
            .len()
            .clamp(1, MAX_LAYERS)
            .div_ceil(LAYERS_PER_MAP)
    }

    /// Every layer's weight at splat texel `t` (layers past the maps read 0).
    pub fn layer_weights(&self, t: usize) -> [u8; MAX_LAYERS] {
        let mut out = [0u8; MAX_LAYERS];
        out[..4].copy_from_slice(&self.alphamap[t]);
        let n = self.alphamap_resolution * self.alphamap_resolution;
        for (m, chunk) in out[4..].chunks_mut(4).enumerate() {
            if let Some(px) = self.alphamap_extra.get(m * n + t) {
                chunk.copy_from_slice(px);
            }
        }
        out
    }

    /// Set every layer's weight at splat texel `t` (the maps must fit: [`Self::fit_splat_maps`]).
    pub fn set_layer_weights(&mut self, t: usize, w: &[u8; MAX_LAYERS]) {
        self.alphamap[t] = [w[0], w[1], w[2], w[3]];
        let n = self.alphamap_resolution * self.alphamap_resolution;
        for m in 0..self.alphamap_extra.len() / n.max(1) {
            let c = &w[4 + m * 4..8 + m * 4];
            self.alphamap_extra[m * n + t] = [c[0], c[1], c[2], c[3]];
        }
    }

    /// Weight 0..255 of `layer` at splat texel `t`.
    pub fn layer_weight(&self, t: usize, layer: usize) -> u8 {
        if layer < LAYERS_PER_MAP {
            return self.alphamap[t][layer];
        }
        let n = self.alphamap_resolution * self.alphamap_resolution;
        let m = layer / LAYERS_PER_MAP - 1;
        self.alphamap_extra
            .get(m * n + t)
            .map_or(0, |px| px[layer % LAYERS_PER_MAP])
    }

    /// Size the extra splat maps for the layer count: maps are added empty, and maps no layer
    /// uses any more are dropped (their weight goes to layer 0, so texels still sum to 255).
    pub fn fit_splat_maps(&mut self) {
        let n = self.alphamap_resolution * self.alphamap_resolution;
        let want = (self.splat_maps() - 1) * n;
        if self.alphamap_extra.len() == want {
            return;
        }
        if self.alphamap_extra.len() > want {
            for t in 0..n {
                let lost: u32 = (want / n.max(1)..self.alphamap_extra.len() / n.max(1))
                    .filter_map(|m| self.alphamap_extra.get(m * n + t))
                    .flat_map(|px| px.iter().map(|v| *v as u32))
                    .sum();
                self.alphamap[t][0] = (self.alphamap[t][0] as u32 + lost).min(255) as u8;
            }
            self.alphamap_extra.truncate(want);
        } else {
            self.alphamap_extra.resize(want, [0; 4]);
        }
    }

    /// Heightmap cells per side (one less than the samples).
    pub fn cells(&self) -> usize {
        self.heightmap_resolution - 1
    }

    /// Some cell is a hole.
    pub fn has_holes(&self) -> bool {
        self.holes.iter().any(|h| *h != 0)
    }

    /// Cell `(cx, cz)` is cut out (false outside the grid).
    pub fn is_hole(&self, cx: usize, cz: usize) -> bool {
        let c = self.cells();
        cx < c && cz < c && self.holes.get(cz * c + cx).is_some_and(|h| *h != 0)
    }

    /// Cut cell `(cx, cz)` out (or fill it back in).
    pub fn set_hole(&mut self, cx: usize, cz: usize, hole: bool) {
        let c = self.cells();
        if cx >= c || cz >= c {
            return;
        }
        if self.holes.len() != c * c {
            if !hole {
                return;
            }
            self.holes = vec![0; c * c];
        }
        self.holes[cz * c + cx] = u8::from(hole);
    }

    /// Heightmap cell under a local point (clamped to the grid).
    pub fn cell_at(&self, x: f32, z: f32) -> (usize, usize) {
        let s = self.spacing();
        let last = self.cells().saturating_sub(1);
        let at = |v: f32, step: f32| ((v / step.max(1e-6)).floor().max(0.0) as usize).min(last);
        (at(x, s.x), at(z, s.y))
    }

    /// The local point is over a hole (outside the terrain: no).
    pub fn hole_at(&self, x: f32, z: f32) -> bool {
        if self.holes.is_empty() || x < 0.0 || z < 0.0 || x > self.size.x || z > self.size.z {
            return false;
        }
        let (cx, cz) = self.cell_at(x, z);
        self.is_hole(cx, cz)
    }

    /// Resample the heightmap to another resolution (bilinear), keeping its shape.
    pub fn set_heightmap_resolution(&mut self, resolution: usize) {
        let res = closest(&HEIGHTMAP_RESOLUTIONS, resolution);
        if res == self.heightmap_resolution {
            return;
        }
        let old = self.heightmap_resolution;
        let cells = (old - 1) as f32 / (res - 1) as f32;
        let mut heights = Vec::with_capacity(res * res);
        for iz in 0..res {
            for ix in 0..res {
                heights.push(bilinear(
                    &self.heights,
                    old,
                    ix as f32 * cells,
                    iz as f32 * cells,
                ));
            }
        }
        self.heights = heights;
        // Holes follow the cells they covered (nearest cell).
        if !self.holes.is_empty() {
            let (oc, nc) = (old - 1, res - 1);
            let mut holes = Vec::with_capacity(nc * nc);
            for cz in 0..nc {
                for cx in 0..nc {
                    let src = |c: usize| ((c * oc + oc / 2) / nc).min(oc - 1);
                    holes.push(self.holes[src(cz) * oc + src(cx)]);
                }
            }
            self.holes = holes;
        }
        self.heightmap_resolution = res;
    }

    /// Resample the splat map to another resolution (nearest texel).
    pub fn set_alphamap_resolution(&mut self, resolution: usize) {
        let res = closest(&ALPHAMAP_RESOLUTIONS, resolution);
        if res == self.alphamap_resolution {
            return;
        }
        let old = self.alphamap_resolution;
        let source = |tx: usize, tz: usize| {
            let sx = (tx * old + old / 2) / res;
            let sz = (tz * old + old / 2) / res;
            sz.min(old - 1) * old + sx.min(old - 1)
        };
        let mut map = Vec::with_capacity(res * res);
        let mut details = Vec::new();
        let mut flowers = Vec::new();
        let maps = self.alphamap_extra.len() / (old * old);
        let mut extra = Vec::with_capacity(maps * res * res);
        for m in 0..maps {
            for tz in 0..res {
                for tx in 0..res {
                    extra.push(self.alphamap_extra[m * old * old + source(tx, tz)]);
                }
            }
        }
        self.alphamap_extra = extra;
        for tz in 0..res {
            for tx in 0..res {
                map.push(self.alphamap[source(tx, tz)]);
                if !self.detail_map.is_empty() {
                    details.push(self.detail_map[source(tx, tz)]);
                }
                if !self.flowers.map.is_empty() {
                    flowers.push(self.flowers.map[source(tx, tz)]);
                }
            }
        }
        for l in &mut self.detail_layers {
            if l.map.len() == old * old {
                l.map = (0..res * res)
                    .map(|i| l.map[source(i % res, i / res)])
                    .collect();
            } else {
                l.map.clear();
            }
        }
        self.alphamap = map;
        self.detail_map = details;
        self.flowers.map = flowers;
        self.alphamap_resolution = res;
    }

    pub fn set_size(&mut self, size: Vec3) {
        self.size = sanitize_size(size);
    }

    /// Fix sizes and channels after loading (wrong lengths are padded or cut).
    pub(super) fn sanitize(&mut self) {
        self.heightmap_resolution = closest(&HEIGHTMAP_RESOLUTIONS, self.heightmap_resolution);
        self.alphamap_resolution = closest(&ALPHAMAP_RESOLUTIONS, self.alphamap_resolution);
        let r = self.heightmap_resolution;
        self.heights.resize(r * r, 0.0);
        let a = self.alphamap_resolution;
        self.alphamap.resize(a * a, [255, 0, 0, 0]);
        self.layers.truncate(MAX_LAYERS);
        let maps = self.splat_maps() - 1;
        if self.alphamap_extra.len() != maps * a * a {
            self.alphamap_extra.resize(maps * a * a, [0; 4]);
        }
        if self.detail_map.len() != a * a {
            self.detail_map.clear();
        }
        if self.holes.len() != (r - 1) * (r - 1) || !self.has_holes() {
            self.holes.clear();
        }
        self.layers.truncate(MAX_LAYERS);
        self.size = sanitize_size(self.size);
        self.trees.sanitize();
        self.flowers.sanitize(a);
        super::detail_layers::sanitize(self);
    }
}

fn sanitize_size(s: Vec3) -> Vec3 {
    s.max(Vec3::splat(1.0))
}

/// Bilinear sample of a square grid at fractional sample coordinates.
fn bilinear(values: &[f32], res: usize, fx: f32, fz: f32) -> f32 {
    let x0 = (fx.floor() as usize).min(res - 1);
    let z0 = (fz.floor() as usize).min(res - 1);
    let (x1, z1) = ((x0 + 1).min(res - 1), (z0 + 1).min(res - 1));
    let (u, v) = (fx - x0 as f32, fz - z0 as f32);
    let at = |x: usize, z: usize| values[z * res + x];
    let top = at(x0, z0) + (at(x1, z0) - at(x0, z0)) * u;
    let bottom = at(x0, z1) + (at(x1, z1) - at(x0, z1)) * u;
    top + (bottom - top) * v
}
