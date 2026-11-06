//! The study's blade field: the chunks the live renderer (terrain_grass.rs, main a91ac1e)
//! would draw for one camera, with the same scatter levels, frustum test and per-chunk draw
//! prefixes, concatenated into one buffer (so a blade has one index for the visibility
//! buffer), plus CPU statistics per distance band for the calculations.

use glam::{Mat4, Vec2, Vec3};
use rayon::prelude::*;

use crate::render::culling::{Aabb, Frustum};
use crate::render::terrain_grass::keep_at;
use crate::terrain::TerrainData;
use crate::terrain::details::{self, Blade};

const CHUNK: f32 = 16.0;
const MIN_KEEP: f32 = 0.012;
const MAX_WIDEN: f32 = 3.0;
const COARSE_FROM: f32 = 1.5;
pub const BLADE_VERTICES: u32 = 10;
pub const COARSE_VERTICES: u32 = 8;

/// What decides which blades a frame draws.
#[derive(Clone, Copy, Debug)]
pub struct FieldSettings {
    /// The quality level's share of the blades.
    pub density: f32,
    /// Variant D: extra thinning (and widening) past ~2 LOD distances.
    pub clump: f32,
    /// Blades end here (the terrain's draw distance, or variant E's far-field start).
    pub draw_distance: f32,
}

/// One chunk's draw: instances `first..first + count`, coarse cards or full ones.
#[derive(Clone, Copy, Debug)]
pub struct ChunkDraw {
    pub first: u32,
    pub count: u32,
    pub coarse: bool,
    pub dist: f32,
}

pub struct Field {
    pub blades: Vec<Blade>,
    pub draws: Vec<ChunkDraw>,
    /// Indices of every drawn blade (the software rasteriser's work list).
    pub drawn: Vec<u32>,
}

fn level_for(keep: f32) -> u32 {
    let mut l = 0;
    while l < details::MAX_LEVEL && details::level_keep(l + 1) >= keep {
        l += 1;
    }
    l
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Variant D's thinning at `dist` (study_blade.wgsl `clump_at`).
pub fn clump_at(dist: f32, lod: f32, clump: f32) -> f32 {
    1.0 + (clump - 1.0) * smoothstep(lod * 1.5, lod * 3.0, dist)
}

impl Field {
    /// The chunks of `data` (corner at `origin`) a camera at `eye` with `view_proj` draws.
    pub fn build(
        data: &TerrainData,
        origin: Vec3,
        eye: Vec3,
        view_proj: &Mat4,
        s: FieldSettings,
    ) -> Self {
        let cam = eye - origin;
        let lod = data.details.lod_distance;
        let range = s.draw_distance.max(1.0);
        let density = s.density.clamp(MIN_KEEP, 1.0);
        let per_x = (data.size.x / CHUNK).ceil() as i64;
        let per_z = (data.size.z / CHUNK).ceil() as i64;
        let (cx, cz) = ((cam.x / CHUNK) as i64, (cam.z / CHUNK) as i64);
        let reach = (range / CHUNK).ceil() as i64 + 1;
        let near = |x: i64, z: i64| {
            let c = (Vec2::new(x as f32, z as f32) + 0.5) * CHUNK;
            c.distance(Vec2::new(cam.x, cam.z)) < range + CHUNK
        };
        let nearest = |x: i64, z: i64| {
            let lo = Vec2::new(x as f32, z as f32) * CHUNK;
            let p = Vec2::new(cam.x, cam.z).clamp(lo, lo + CHUNK);
            Vec3::new(p.x - cam.x, 0.0, p.y - cam.z).length()
        };
        let mut keys = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                if near(x, z) {
                    keys.push((x, z));
                }
            }
        }
        let frustum = Frustum::from_matrix(view_proj);
        let keep_of = |dist: f32| keep_at(dist, lod) * density / clump_at(dist, lod, s.clump);
        let mut chunks: Vec<(f32, u32, Vec<Blade>)> = keys
            .par_iter()
            .filter_map(|&(x, z)| {
                let level = level_for(keep_of(nearest(x, z)));
                let lo = Vec2::new(x as f32, z as f32) * CHUNK;
                let blades = details::scatter_level(data, lo, lo + CHUNK, level);
                if blades.is_empty() {
                    return None;
                }
                let top = blades
                    .iter()
                    .map(|b| b.position[1] + b.position[3])
                    .fold(f32::MIN, f32::max);
                let bottom = blades
                    .iter()
                    .map(|b| b.position[1])
                    .fold(f32::MAX, f32::min);
                let world = Aabb::new(
                    Vec3::new(lo.x - 1.0, bottom - 1.0, lo.y - 1.0) + origin,
                    Vec3::new(lo.x + CHUNK + 1.0, top + 1.0, lo.y + CHUNK + 1.0) + origin,
                );
                if !frustum.intersects(&world) {
                    return None;
                }
                let dist = eye.clamp(world.min, world.max).distance(eye);
                Some((dist, level, blades))
            })
            .collect();
        chunks.sort_by(|a, b| a.0.total_cmp(&b.0));
        let mut field = Field {
            blades: Vec::new(),
            draws: Vec::new(),
            drawn: Vec::new(),
        };
        for (dist, level, blades) in chunks {
            let keep = keep_of(dist) / details::level_keep(level);
            let count = ((blades.len() as f32 * keep.min(1.0)).ceil() as u32).min(blades.len() as u32);
            if count == 0 {
                continue;
            }
            let first = field.blades.len() as u32;
            // Only the drawn prefix is kept (the rest is never drawn for this camera).
            field.blades.extend_from_slice(&blades[..count as usize]);
            field.drawn.extend(first..first + count);
            field.draws.push(ChunkDraw {
                first,
                count,
                coarse: dist > lod * COARSE_FROM,
                dist,
            });
        }
        field
    }

    /// Triangles of one full draw of the field.
    pub fn triangles(&self) -> u64 {
        self.draws
            .iter()
            .map(|d| {
                let v = if d.coarse {
                    COARSE_VERTICES
                } else {
                    BLADE_VERTICES
                };
                d.count as u64 * (v - 2) as u64
            })
            .sum()
    }
}

/// Per distance band: blades drawn, standing (not shrunk to a flat fan), triangles, vertices,
/// the mean projected width / height of the standing ones in pixels and the share of them
/// under 1 / 2 pixels wide.
#[derive(Clone, Copy, Debug, Default)]
pub struct Band {
    pub lo: f32,
    pub hi: f32,
    pub blades: u64,
    pub standing: u64,
    pub triangles: u64,
    pub vertices: u64,
    pub width_px: f64,
    pub height_px: f64,
    pub under_1px: u64,
    pub under_2px: u64,
}

pub const BANDS: [f32; 7] = [0.0, 5.0, 10.0, 20.0, 40.0, 80.0, 1000.0];

/// CPU twin of the blade stage's drawn height and width (study_blade.wgsl, no wind).
pub fn band_stats(
    field: &Field,
    data: &TerrainData,
    origin: Vec3,
    eye: Vec3,
    pixel_angle: f32,
    s: FieldSettings,
) -> Vec<Band> {
    let d = &data.details;
    let lod = d.lod_distance.max(1.0);
    let far = s.draw_distance.max(1.0);
    let mut bands: Vec<Band> = BANDS
        .windows(2)
        .map(|w| Band {
            lo: w[0],
            hi: w[1],
            ..Default::default()
        })
        .collect();
    for draw in &field.draws {
        let verts = if draw.coarse {
            COARSE_VERTICES
        } else {
            BLADE_VERTICES
        };
        for b in &field.blades[draw.first as usize..(draw.first + draw.count) as usize] {
            let root = origin + Vec3::new(b.position[0], b.position[1], b.position[2]);
            let dist = root.distance(eye);
            let Some(band) = bands.iter_mut().find(|x| dist >= x.lo && dist < x.hi) else {
                continue;
            };
            band.blades += 1;
            band.triangles += (verts - 2) as u64;
            band.vertices += verts as u64;
            let clump = clump_at(dist, lod, s.clump);
            let keep = ((lod / dist.max(1e-3)).powi(2)).clamp(MIN_KEEP, 1.0) * s.density / clump;
            let fade = 1.0 - smoothstep(far * 0.8, far, dist);
            let height = b.position[3] * fade * (1.0 - smoothstep(keep * 0.7, keep, b.key()));
            if height <= 0.0 {
                continue;
            }
            band.standing += 1;
            let widen = (1.0 / keep).min(MAX_WIDEN * clump);
            let mut width = if b.seed_head() {
                b.position[3] * 0.1
            } else {
                b.shape[1]
            } * widen;
            let down = (eye.y - root.y) / dist.max(1e-3);
            if !b.seed_head() {
                width *= 1.0 + 0.3 * smoothstep(0.2, 0.9, down);
            }
            let px = width / (dist * pixel_angle).max(1e-6);
            band.width_px += px.max(0.8) as f64;
            band.height_px += (height / (dist * pixel_angle).max(1e-6)) as f64;
            band.under_1px += u64::from(px < 1.0);
            band.under_2px += u64::from(px < 2.0);
        }
    }
    for b in &mut bands {
        if b.standing > 0 {
            b.width_px /= b.standing as f64;
            b.height_px /= b.standing as f64;
        }
    }
    bands
}
