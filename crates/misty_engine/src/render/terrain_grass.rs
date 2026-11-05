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
/// Blades scattered per frame at most (after the first chunk).
const BLADES_PER_FRAME: usize = 400_000;
/// Vertices of one blade (grass.wgsl: six segments and a tip).
pub(super) const BLADE_VERTICES: u32 = 14;
/// Vertices of a coarse far blade (three segments), drawn from vertex 100 on.
const COARSE_VERTICES: u32 = 8;
/// Chunks further than this many LOD distances draw coarse blades.
const COARSE_FROM: f32 = 1.5;
/// Fewest blades a far chunk keeps (share of full density).
const MIN_KEEP: f32 = 0.02;
/// Far blades grow at most this much wider to cover the ground of the ones left out.
const MAX_WIDEN: f32 = 4.0;

/// Share of blades drawn at `dist` metres: all up to the LOD distance, then falling with the
/// square of the distance (blades grow wider to cover the same ground, see grass.wgsl).
pub(super) fn keep_at(dist: f32, lod_distance: f32) -> f32 {
    let r = lod_distance.max(1.0) / dist.max(1e-3);
    (r * r).clamp(MIN_KEEP, 1.0)
}

/// Coarsest density level that still has `keep` of the blades.
fn level_for(keep: f32) -> u32 {
    let mut l = 0;
    while l < details::MAX_LEVEL && details::level_keep(l + 1) >= keep {
        l += 1;
    }
    l
}

struct Chunk {
    buffer: Option<wgpu::Buffer>,
    count: u32,
    /// Density level the blades were scattered at.
    level: u32,
    /// Terrain-local bounds of the blades.
    bounds: Aabb,
}

/// Chunks within `range` metres only, `share` of their blades, coarse blades only.
#[derive(Clone, Copy)]
pub(super) struct Limit {
    pub range: f32,
    pub share: f32,
    pub coarse: bool,
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
    s.seed_color = [0.0; 3];
    s.translucency = 0.0;
    s.tip_dryness = 0.0;
    s.lod_distance = 0.0;
    s.lush_color = [0.0; 3];
    s.lush_amount = 0.0;
    s.dry_amount = 0.0;
    s.color_variation = 0.0;
    s.ground_tint = 0.0;
    s.cast_shadows = false;
    s.wind_direction = 0.0;
    s.gust_strength = 0.0;
    s.gust_scale = 0.0;
    s.turbulence = 0.0;
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
        let lod = d.details.lod_distance;
        let near = |x: u32, z: u32, r: f32| {
            let c = (Vec2::new(x as f32, z as f32) + 0.5) * CHUNK;
            c.distance(Vec2::new(camera.x, camera.z)) < r + CHUNK
        };
        // Level a chunk needs: from its nearest point to the camera.
        let wanted = |x: u32, z: u32| {
            let lo = Vec2::new(x as f32, z as f32) * CHUNK;
            let p = Vec2::new(camera.x, camera.z).clamp(lo, lo + CHUNK);
            let dist = Vec3::new(p.x - camera.x, 0.0, p.y - camera.z).length();
            level_for(keep_at(dist, lod))
        };
        self.chunks
            .retain(|(x, z), c| near(*x, *z, range * 1.5) && c.level <= wanted(*x, *z) + 1);
        let per_x = (d.size.x / CHUNK).ceil() as i64;
        let per_z = (d.size.z / CHUNK).ceil() as i64;
        let (cx, cz) = ((camera.x / CHUNK) as i64, (camera.z / CHUNK) as i64);
        let reach = (range / CHUNK).ceil() as i64 + 1;
        let mut todo: Vec<(u32, u32)> = Vec::new();
        for z in (cz - reach).max(0)..(cz + reach + 1).min(per_z) {
            for x in (cx - reach).max(0)..(cx + reach + 1).min(per_x) {
                let k = (x as u32, z as u32);
                let stale = self
                    .chunks
                    .get(&k)
                    .is_none_or(|c| c.level > wanted(k.0, k.1));
                if stale && near(k.0, k.1, range) {
                    todo.push(k);
                }
            }
        }
        // Nearest first.
        let dist = |k: &(u32, u32)| (k.0 as i64 - cx).pow(2) + (k.1 as i64 - cz).pow(2);
        todo.sort_by_key(dist);
        let mut budget = BLADES_PER_FRAME;
        for k in todo.into_iter().take(BUILDS_PER_FRAME) {
            if budget == 0 {
                break;
            }
            crate::profile_scope!("Terrain.GrassScatter");
            let lo = Vec2::new(k.0 as f32, k.1 as f32) * CHUNK;
            let level = wanted(k.0, k.1);
            let blades = details::scatter_level(d, lo, lo + CHUNK, level);
            budget = budget.saturating_sub(blades.len());
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
                    level,
                    bounds,
                },
            );
        }
    }

    /// Draw visible chunks (pipeline and groups 0–2 already bound), nearest first: far
    /// chunks draw only the share of their key-sorted blades their nearest point keeps (the
    /// vertex shader shrinks away the rest blade by blade). `limit` draws only the chunks
    /// within a range, a share of their blades (shadow casters, the prepass).
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &self,
        pass: &mut wgpu::RenderPass,
        origin: Vec3,
        camera: Vec3,
        lod_distance: f32,
        frustum: &Frustum,
        limit: Option<Limit>,
        stats: &mut super::RenderStats,
    ) {
        // Nearest chunks first: their blades fill the depth buffer, so the many blades
        // behind them fail the depth test before shading.
        let mut order: Vec<(f32, &Chunk)> = self
            .chunks
            .values()
            .filter_map(|c| {
                c.buffer.as_ref()?;
                let world = Aabb::new(c.bounds.min + origin, c.bounds.max + origin);
                let dist = camera.clamp(world.min, world.max).distance(camera);
                let near = limit.is_none_or(|l| dist < l.range);
                (near && frustum.intersects(&world)).then_some((dist, c))
            })
            .collect();
        order.sort_by(|a, b| a.0.total_cmp(&b.0));
        let share = limit.map_or(1.0, |l| l.share);
        let coarse_only = limit.is_some_and(|l| l.coarse);
        for (dist, c) in order {
            let Some(buffer) = &c.buffer else {
                continue;
            };
            let keep = keep_at(dist, lod_distance) * share / details::level_keep(c.level);
            let count = ((c.count as f32 * keep.min(1.0)).ceil() as u32).min(c.count);
            if count == 0 {
                continue;
            }
            pass.set_vertex_buffer(0, buffer.slice(..));
            let verts = if coarse_only || dist > lod_distance * COARSE_FROM {
                pass.draw(100..100 + COARSE_VERTICES, 0..count);
                COARSE_VERTICES
            } else {
                pass.draw(0..BLADE_VERTICES, 0..count);
                BLADE_VERTICES
            };
            stats.draw(count, (verts - 2) * 3, verts);
        }
    }

    pub fn bytes(&self) -> u64 {
        self.chunks
            .values()
            .map(|c| c.count as u64 * std::mem::size_of::<details::Blade>() as u64)
            .sum()
    }
}

/// Grass uniform (see grass_blade.wgsl).
#[repr(C)]
#[derive(Clone, Copy, bytemuck::Pod, bytemuck::Zeroable)]
pub(super) struct GrassParams {
    origin: [f32; 4],
    healthy: [f32; 4],
    dry: [f32; 4],
    wind: [f32; 4],
    /// rgb = linear seed-head colour, w = translucency.
    seed: [f32; 4],
    /// x = full-density distance, y = tip dryness, z = most widening, w = colour variation.
    look: [f32; 4],
    /// x = the terrain's draw slot (its reflection probes), yzw unused.
    draw: [u32; 4],
    /// rgb = linear lush colour, w = patch size.
    lush: [f32; 4],
    /// x = lush amount, y = dry amount, z = gust strength, w = turbulence.
    amounts: [f32; 4],
    /// xyz = camera position, w = gust wave length.
    camera: [f32; 4],
    /// x = caster range, y = caster share, z = 1 in shadow passes, w = ground tint.
    shadow: [f32; 4],
}

/// The built-in grass atlas: eight photographed specimens (leaf blades, timothy heads, open
/// panicles) in 128×1024 cells, alpha keyed (see grass.wgsl).
const ATLAS_PNG: &[u8] = include_bytes!("assets/grass_atlas.png");

/// Upload the grass atlas with mips (sRGB) and its clamped trilinear sampler.
pub(super) fn atlas(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
) -> (wgpu::TextureView, wgpu::Sampler) {
    let image = image::load_from_memory(ATLAS_PNG)
        .expect("built-in grass atlas")
        .into_rgba8();
    let view = super::texture::upload(device, queue, "misty grass atlas", image, true);
    let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
        label: Some("misty grass sampler"),
        address_mode_u: wgpu::AddressMode::ClampToEdge,
        address_mode_v: wgpu::AddressMode::ClampToEdge,
        mag_filter: wgpu::FilterMode::Linear,
        min_filter: wgpu::FilterMode::Linear,
        mipmap_filter: wgpu::MipmapFilterMode::Linear,
        anisotropy_clamp: 4,
        ..Default::default()
    });
    (view, sampler)
}

fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Wind blowing over the grass: the detail settings, scaled and turned by the scene's Wind
/// Zone when there is one.
pub(super) fn wind(s: &DetailSettings, zone: Option<&super::environment::Wind>) -> [f32; 6] {
    let a = s.wind_direction.to_radians();
    let mut dir = Vec2::new(a.sin(), a.cos());
    let (mut strength, mut speed, mut gusts, mut flutter) = (
        s.wind_strength,
        s.wind_speed * 5.0,
        s.gust_strength,
        s.turbulence,
    );
    if let Some(z) = zone {
        let d = Vec2::new(z.direction.x, z.direction.z);
        if d.length_squared() > 1e-6 {
            dir = d.normalize();
        }
        strength *= z.main;
        flutter *= z.turbulence;
        gusts *= z.pulse_magnitude;
        speed *= z.pulse_frequency;
    }
    [strength, speed, dir.x, dir.y, gusts, flutter]
}

impl GrassParams {
    /// Uniform of one terrain's grass; `shadow` = caster range and share for shadow passes.
    pub fn new(
        s: &DetailSettings,
        origin: Vec3,
        time: f32,
        draw: usize,
        camera: Vec3,
        zone: Option<&super::environment::Wind>,
        shadow: Option<(f32, f32)>,
    ) -> Self {
        let far = s.draw_distance.max(1.0);
        let [hr, hg, hb] = s.healthy_color.map(lin);
        let [dr, dg, db] = s.dry_color.map(lin);
        let [sr, sg, sb] = s.seed_color.map(lin);
        let [lr, lg, lb] = s.lush_color.map(lin);
        let [strength, speed, dx, dz, gusts, flutter] = wind(s, zone);
        let (range, share) = shadow.unwrap_or((0.0, 1.0));
        Self {
            origin: [origin.x, origin.y, origin.z, time],
            healthy: [hr, hg, hb, far],
            dry: [dr, dg, db, far * 0.6],
            wind: [strength, speed, dx, dz],
            seed: [sr, sg, sb, s.translucency.clamp(0.0, 2.0)],
            look: [
                s.lod_distance.max(1.0),
                s.tip_dryness.clamp(0.0, 1.0),
                MAX_WIDEN,
                s.color_variation.clamp(0.0, 1.0),
            ],
            draw: [draw as u32, 0, 0, 0],
            lush: [lr, lg, lb, s.patch_scale.max(0.5)],
            amounts: [
                s.lush_amount.clamp(0.0, 1.0),
                s.dry_amount.clamp(0.0, 1.0),
                gusts.clamp(0.0, 2.0),
                flutter.clamp(0.0, 2.0),
            ],
            camera: [camera.x, camera.y, camera.z, s.gust_scale.max(1.0)],
            shadow: [
                range,
                share,
                if shadow.is_some() { 1.0 } else { 0.0 },
                s.ground_tint.clamp(0.0, 1.0),
            ],
        }
    }
}

#[cfg(test)]
#[path = "grass_gpu_tests.rs"]
mod gpu_tests;

#[cfg(test)]
#[path = "grass_tests.rs"]
mod tests;
