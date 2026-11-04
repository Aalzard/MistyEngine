//! Grass of one terrain: blades are scattered per 16 m chunk around the camera (only within
//! the draw distance, a few chunks per frame), kept in small instance buffers and dropped
//! again when far away or when the terrain under them changes.

use std::collections::HashMap;

use glam::{Vec2, Vec3};
use wgpu::util::DeviceExt;

use super::culling::{Aabb, Frustum};
use crate::terrain::TerrainData;
use crate::terrain::details::{self, DetailSettings};
use crate::terrain::store::Region;

/// Chunk side in metres.
const CHUNK: f32 = 16.0;
/// Chunks scattered per frame at most (spreads the work while flying around).
const BUILDS_PER_FRAME: usize = 24;

struct Chunk {
    buffer: Option<wgpu::Buffer>,
    count: u32,
    /// Terrain-local bounds of the blades.
    bounds: Aabb,
}

#[derive(Default)]
pub(super) struct Grass {
    chunks: HashMap<(u32, u32), Chunk>,
    /// Settings the blades were scattered with (look-only fields cleared).
    key: Option<(DetailSettings, u32)>,
}

/// Settings that change where blades are and how big (colours and wind are uniforms).
fn scatter_key(d: &TerrainData) -> (DetailSettings, u32) {
    let mut s = d.details.clone();
    s.healthy_color = [0.0; 3];
    s.dry_color = [0.0; 3];
    s.wind_strength = 0.0;
    s.wind_speed = 0.0;
    s.draw_distance = 0.0;
    (s, d.rule_seed)
}

impl Grass {
    /// Forget blades of a changed region (`None` = everything).
    pub fn invalidate(&mut self, d: &TerrainData, region: Option<Region>) {
        let Some(r) = region else {
            self.chunks.clear();
            return;
        };
        if r.is_empty() {
            return;
        }
        let size = Vec2::new(d.size.x, d.size.z);
        let lo = Vec2::from(r.min) * size;
        let hi = Vec2::from(r.max) * size;
        self.chunks.retain(|(x, z), _| {
            let c0 = Vec2::new(*x as f32, *z as f32) * CHUNK;
            let c1 = c0 + CHUNK;
            c1.x < lo.x || c1.y < lo.y || c0.x > hi.x || c0.y > hi.y
        });
    }

    /// Scatter missing chunks near the camera (terrain-local) and drop far ones.
    pub fn update(&mut self, device: &wgpu::Device, d: &TerrainData, camera: Vec3) {
        let key = scatter_key(d);
        if self.key.as_ref() != Some(&key) {
            self.chunks.clear();
            self.key = Some(key);
        }
        if !d.details.enabled || d.detail_map.is_empty() {
            self.chunks.clear();
            return;
        }
        let range = d.details.draw_distance.max(1.0);
        let near = |x: u32, z: u32, r: f32| {
            let c = (Vec2::new(x as f32, z as f32) + 0.5) * CHUNK;
            c.distance(Vec2::new(camera.x, camera.z)) < r + CHUNK
        };
        self.chunks.retain(|(x, z), _| near(*x, *z, range * 1.5));
        let per_x = (d.size.x / CHUNK).ceil() as i64;
        let per_z = (d.size.z / CHUNK).ceil() as i64;
        let (cx, cz) = ((camera.x / CHUNK) as i64, (camera.z / CHUNK) as i64);
        let reach = (range / CHUNK).ceil() as i64 + 1;
        let mut todo: Vec<(u32, u32)> = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                let k = (x as u32, z as u32);
                if !self.chunks.contains_key(&k) && near(k.0, k.1, range) {
                    todo.push(k);
                }
            }
        }
        // Nearest first.
        let dist = |k: &(u32, u32)| (k.0 as i64 - cx).pow(2) + (k.1 as i64 - cz).pow(2);
        todo.sort_by_key(dist);
        for k in todo.into_iter().take(BUILDS_PER_FRAME) {
            crate::profile_scope!("Terrain.GrassScatter");
            let lo = Vec2::new(k.0 as f32, k.1 as f32) * CHUNK;
            let blades = details::scatter(d, lo, lo + CHUNK);
            let top = blades
                .iter()
                .map(|b| b.position[1] + b.position[3])
                .fold(f32::MIN, f32::max);
            let bottom = blades
                .iter()
                .map(|b| b.position[1])
                .fold(f32::MAX, f32::min);
            let buffer = (!blades.is_empty()).then(|| {
                device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                    label: Some("misty grass blades"),
                    contents: bytemuck::cast_slice(&blades),
                    usage: wgpu::BufferUsages::VERTEX,
                })
            });
            let bounds = Aabb::new(
                Vec3::new(lo.x - 1.0, bottom - 1.0, lo.y - 1.0),
                Vec3::new(lo.x + CHUNK + 1.0, top + 1.0, lo.y + CHUNK + 1.0),
            );
            let count = blades.len() as u32;
            self.chunks.insert(
                k,
                Chunk {
                    buffer,
                    count,
                    bounds,
                },
            );
        }
    }

    /// Draw visible chunks (pipeline and groups 0–2 already bound).
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        origin: Vec3,
        frustum: &Frustum,
        stats: &mut super::RenderStats,
    ) {
        for c in self.chunks.values() {
            let Some(buffer) = &c.buffer else {
                continue;
            };
            let world = Aabb::new(c.bounds.min + origin, c.bounds.max + origin);
            if !frustum.intersects(&world) {
                continue;
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            pass.draw(0..7, 0..c.count);
            stats.draw(c.count, 15, 7);
        }
    }

    pub fn bytes(&self) -> u64 {
        self.chunks
            .values()
            .map(|c| c.count as u64 * std::mem::size_of::<details::Blade>() as u64)
            .sum()
    }
}

/// Grass uniform (see grass.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GrassParams {
    origin: [f32; 4],
    healthy: [f32; 4],
    dry: [f32; 4],
    wind: [f32; 4],
    /// x = the terrain's draw slot (its reflection probes), yzw unused.
    draw: [u32; 4],
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

impl GrassParams {
    pub fn new(s: &DetailSettings, origin: Vec3, time: f32, draw: usize) -> Self {
        let far = s.draw_distance.max(1.0);
        let [hr, hg, hb] = s.healthy_color.map(lin);
        let [dr, dg, db] = s.dry_color.map(lin);
        Self {
            origin: [origin.x, origin.y, origin.z, time],
            healthy: [hr, hg, hb, far],
            dry: [dr, dg, db, far * 0.6],
            // A fixed diagonal breeze.
            wind: [s.wind_strength, s.wind_speed, 0.8, 0.6],
            draw: [draw as u32, 0, 0, 0],
        }
    }
}
