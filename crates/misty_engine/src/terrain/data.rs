//! Terrain data asset: a square heightmap (normalised 0..1 heights scaled by `size.y`), a splat
//! (alpha) map with one weight per terrain layer, and up to four layers.
//!
//! Local space: the origin is the terrain's corner; x runs over the width, z over the length.
//! Heightmap sample `(ix, iz)` sits at `(ix * dx, h, iz * dz)` and is stored at `iz * res + ix`.
//! Cells are split into two triangles along the diagonal from `(x0, z1)` to `(x1, z0)`, the same
//! split as rapier's heightfield, so sampled heights, the mesh and the collider agree exactly.

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

/// Most layers a terrain blends (one splat-map channel each).
pub const MAX_LAYERS: usize = 4;
/// Heightmap resolutions offered (2^n + 1 samples per side).
pub const HEIGHTMAP_RESOLUTIONS: [usize; 6] = [33, 65, 129, 257, 513, 1025];
/// Splat-map resolutions offered (texels per side).
pub const ALPHAMAP_RESOLUTIONS: [usize; 6] = [16, 32, 64, 128, 256, 512];

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
    pub layers: Vec<TerrainLayer>,
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
            layers: Vec::new(),
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
    pub fn weights_at(&self, x: f32, z: f32) -> [f32; 4] {
        let n = self.alphamap_resolution;
        let tx = ((x / self.size.x) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        let tz = ((z / self.size.z) * n as f32).clamp(0.0, n as f32 - 1.0) as usize;
        self.alphamap[tz * n + tx].map(|w| w as f32 / 255.0)
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
        self.heightmap_resolution = res;
    }

    /// Resample the splat map to another resolution (nearest texel).
    pub fn set_alphamap_resolution(&mut self, resolution: usize) {
        let res = closest(&ALPHAMAP_RESOLUTIONS, resolution);
        if res == self.alphamap_resolution {
            return;
        }
        let old = self.alphamap_resolution;
        let mut map = Vec::with_capacity(res * res);
        for tz in 0..res {
            for tx in 0..res {
                let sx = (tx * old + old / 2) / res;
                let sz = (tz * old + old / 2) / res;
                map.push(self.alphamap[sz.min(old - 1) * old + sx.min(old - 1)]);
            }
        }
        self.alphamap = map;
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
        self.size = sanitize_size(self.size);
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
