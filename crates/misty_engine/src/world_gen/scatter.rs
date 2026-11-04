//! Scatter placement: Poisson-disk-like sampling per biome scatter item (one jittered candidate
//! per spacing-sized cell, kept by the item's rules and density, rejected when closer than the
//! minimum spacing to an accepted neighbour). Deterministic per seed: cells are visited in order
//! and each draws its random numbers from its own coordinates.

use std::collections::HashMap;

use glam::{Vec2, Vec3};

use super::biome::{Biome, ScatterShape};
use super::classify::Fields;
use super::noise::Rng;
use crate::terrain::TerrainData;

/// One placed object (terrain-local).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScatterInstance {
    pub position: Vec3,
    /// Radians about the up axis.
    pub yaw: f32,
    pub scale: f32,
    /// Up direction (ground normal when aligned, else +Y).
    pub up: Vec3,
}

/// The instances of one biome scatter item.
#[derive(Clone, Debug, PartialEq)]
pub struct ScatterBatch {
    pub biome: usize,
    pub item: usize,
    pub instances: Vec<ScatterInstance>,
}

/// Spatial hash of accepted points (cell = `cell` metres).
struct Hash {
    cell: f32,
    map: HashMap<(i32, i32), Vec<(Vec2, f32)>>,
}

impl Hash {
    fn new(cell: f32) -> Self {
        Self {
            cell: cell.max(0.1),
            map: HashMap::new(),
        }
    }

    fn key(&self, p: Vec2) -> (i32, i32) {
        (
            (p.x / self.cell).floor() as i32,
            (p.y / self.cell).floor() as i32,
        )
    }

    /// Any point within `max(r, its radius)` of `p`? (`r` ≤ the hash cell.)
    fn near(&self, p: Vec2, r: f32) -> bool {
        let (kx, kz) = self.key(p);
        for dz in -1..=1 {
            for dx in -1..=1 {
                if let Some(list) = self.map.get(&(kx + dx, kz + dz))
                    && list.iter().any(|(q, qr)| q.distance(p) < r.max(*qr))
                {
                    return true;
                }
            }
        }
        false
    }

    fn insert(&mut self, p: Vec2, r: f32) {
        let k = self.key(p);
        self.map.entry(k).or_default().push((p, r));
    }
}

/// Scatter every biome's items over the terrain.
pub fn scatter(d: &TerrainData, fields: &Fields, biomes: &[Biome], seed: u32) -> Vec<ScatterBatch> {
    let mut out = Vec::new();
    // Large objects (not grass) keep clear of each other across items.
    let mut solid = Hash::new(16.0);
    for (bi, biome) in biomes.iter().enumerate().take(fields.biome_count) {
        for (ii, item) in biome.scatter.iter().enumerate() {
            let spacing = item.min_spacing.max(0.25);
            let grass = item.shape == ScatterShape::Grass;
            let mut own = Hash::new(spacing);
            let mut instances = Vec::new();
            let nx = (d.size.x / spacing).ceil() as i32;
            let nz = (d.size.z / spacing).ceil() as i32;
            let item_seed = (seed as u64) << 32 ^ (bi as u64) << 16 ^ ii as u64;
            // Chance per cell at full density (density is per 1000 m²).
            let base = item.density.max(0.0) * spacing * spacing / 1000.0;
            for cz in 0..nz {
                for cx in 0..nx {
                    let mut rng =
                        Rng::new(item_seed ^ ((cx as u64) << 40) ^ ((cz as u64) << 20) ^ 0x5ca7);
                    let p = Vec2::new(
                        (cx as f32 + rng.f32()) * spacing,
                        (cz as f32 + rng.f32()) * spacing,
                    );
                    let roll = rng.f32();
                    let (yaw, s) = (rng.range(0.0, std::f32::consts::TAU), rng.f32());
                    if p.x >= d.size.x || p.y >= d.size.z {
                        continue;
                    }
                    let t = fields.index(p);
                    let info = &fields.info[t];
                    // Hard limits: slope, height band, dry ground.
                    if info.slope > item.max_slope
                        || info.height01 < item.height_range[0]
                        || info.height01 > item.height_range[1]
                        || info.above_water < 0.3
                        || info.water_dist < 1.0
                    {
                        continue;
                    }
                    let mut chance = base * fields.biome_weight(t, bi);
                    let rock = fields.rock[t];
                    chance *= if item.shape.is_rock() {
                        1.0 + 4.0 * rock
                    } else {
                        1.0 - rock
                    };
                    if roll >= chance {
                        continue;
                    }
                    if own.near(p, spacing) || (!grass && solid.near(p, spacing * 0.5)) {
                        continue;
                    }
                    own.insert(p, spacing);
                    if !grass {
                        solid.insert(p, spacing * 0.5);
                    }
                    let h = d.sample_height(p.x, p.y);
                    let up = if item.align_to_normal {
                        normal(d, p)
                    } else {
                        Vec3::Y
                    };
                    let (lo, hi) = (item.scale[0], item.scale[1].max(item.scale[0]));
                    instances.push(ScatterInstance {
                        position: Vec3::new(p.x, h, p.y),
                        yaw: if item.random_rotation { yaw } else { 0.0 },
                        scale: lo + (hi - lo) * s,
                        up,
                    });
                }
            }
            out.push(ScatterBatch {
                biome: bi,
                item: ii,
                instances,
            });
        }
    }
    out
}

/// Ground normal at a terrain-local point.
fn normal(d: &TerrainData, p: Vec2) -> Vec3 {
    let e = d.spacing();
    let h = |dx: f32, dz: f32| d.sample_height(p.x + dx, p.y + dz);
    let gx = (h(e.x, 0.0) - h(-e.x, 0.0)) / (2.0 * e.x);
    let gz = (h(0.0, e.y) - h(0.0, -e.y)) / (2.0 * e.y);
    Vec3::new(-gx, 1.0, -gz).normalize()
}
