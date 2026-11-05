//! The occlusion bake: view cells, sample points, depth cubes and potentially visible sets.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

use glam::{UVec3, Vec3};
use hecs::Entity;
use rayon::prelude::*;

use super::OcclusionSettings;
use super::data::{INVALID, OUTSIDE, OcclusionData, words_for};
use crate::navigation::geometry::{self, BakeGeometry};
use crate::render::culling::Aabb;
use crate::render::probes::bvh::Bvh;
use crate::scene::{MeshFilter, MeshRenderer, OcclusionArea, Scene, StaticFlags};

/// Grids above this many cells get larger cells.
pub const MAX_CELLS: usize = 1 << 18;

/// Everything a bake needs, copied out of the scene (it runs on a worker thread).
#[derive(Clone, Debug, Default)]
pub struct BakeInput {
    /// Triangles of the Occluder Static renderers and terrains.
    pub occluders: BakeGeometry,
    /// World boxes of the Occludee Static renderers, in Hierarchy order.
    pub occludees: Vec<Aabb>,
    /// Enabled View Volume Occlusion Areas (empty = the static geometry's bounds).
    pub volumes: Vec<Aabb>,
    pub settings: OcclusionSettings,
}

/// Bake progress shared with the window: sample points done of `total`, and a cancel flag.
#[derive(Debug, Default)]
pub struct Progress {
    pub done: AtomicUsize,
    pub total: AtomicUsize,
    pub cancel: AtomicBool,
}

impl Progress {
    pub fn fraction(&self) -> f32 {
        let total = self.total.load(Ordering::Relaxed);
        match total {
            0 => 0.0,
            t => self.done.load(Ordering::Relaxed) as f32 / t as f32,
        }
    }
}

/// World box of a renderer's mesh (`None` without a loaded mesh).
fn renderer_bounds(scene: &Scene, e: Entity) -> Option<Aabb> {
    if !scene.world.get::<&MeshRenderer>(e).is_ok_and(|r| r.enabled) {
        return None;
    }
    let mesh = scene.world.get::<&MeshFilter>(e).ok()?.mesh.clone()?;
    let (lo, hi) = crate::models::bounds(&mesh)?;
    Some(Aabb::new(lo, hi).transformed(&scene.world_matrix(e)))
}

/// Active Occludee Static renderers and their world boxes, in Hierarchy order (the bake and the
/// runtime both number occludees this way).
pub fn occludees(scene: &Scene) -> Vec<(Entity, Aabb)> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|&e| scene.active_in_hierarchy(e))
        .filter(|&e| scene.game_object(e).has_static(StaticFlags::OCCLUDEE))
        .filter_map(|e| renderer_bounds(scene, e).map(|b| (e, b)))
        .collect()
}

/// Is `e` an active Occluder Static object at least `smallest` across?
fn is_occluder(scene: &Scene, e: Entity, smallest: f32) -> bool {
    if !scene.active_in_hierarchy(e) || !scene.game_object(e).has_static(StaticFlags::OCCLUDER) {
        return false;
    }
    match renderer_bounds(scene, e) {
        Some(b) => (b.max - b.min).max_element() >= smallest,
        // Terrains (no Mesh Renderer) always occlude.
        None => scene.world.get::<&crate::scene::Terrain>(e).is_ok(),
    }
}

/// Enabled View Volume Occlusion Areas.
pub fn view_volumes(scene: &Scene) -> Vec<Aabb> {
    scene
        .active_with::<OcclusionArea>()
        .into_iter()
        .filter_map(|e| {
            let a = scene.world.get::<&OcclusionArea>(e).ok()?;
            (a.enabled && a.is_view_volume).then(|| {
                let (lo, hi) = a.world_bounds(&scene.world_matrix(e));
                Aabb::new(lo, hi)
            })
        })
        .collect()
}

impl BakeInput {
    pub fn from_scene(scene: &Scene) -> Self {
        let settings = scene.render_settings.occlusion.clone();
        let smallest = settings.smallest_occluder;
        let occluders = geometry::collect_where(scene, |s, e| is_occluder(s, e, smallest));
        Self {
            occluders,
            occludees: occludees(scene).into_iter().map(|(_, b)| b).collect(),
            volumes: view_volumes(scene),
            settings,
        }
    }
}

fn union(boxes: impl Iterator<Item = Aabb>) -> Option<Aabb> {
    boxes.reduce(|a, b| Aabb::new(a.min.min(b.min), a.max.max(b.max)))
}

fn overlaps(a: &Aabb, b: &Aabb) -> bool {
    a.min.cmple(b.max).all() && b.min.cmple(a.max).all()
}

/// The view cell grid: origin, cell size and cell counts.
pub fn grid(input: &BakeInput) -> Option<(Vec3, f32, UVec3)> {
    let bounds = if input.volumes.is_empty() {
        let geo = input.occluders.bounds().map(|(lo, hi)| Aabb::new(lo, hi));
        union(geo.into_iter().chain(input.occludees.iter().copied()))?
    } else {
        union(input.volumes.iter().copied())?
    };
    let extent = (bounds.max - bounds.min).max(Vec3::splat(1e-3));
    let mut size = input.settings.cell_size();
    loop {
        let dims = (extent / size).ceil().max(Vec3::ONE).as_uvec3();
        if (dims.x as usize) * (dims.y as usize) * (dims.z as usize) <= MAX_CELLS {
            // Centre the grid on the bounds.
            let origin = (bounds.min + bounds.max) * 0.5 - dims.as_vec3() * size * 0.5;
            return Some((origin, size, dims));
        }
        size *= 1.25;
    }
}

/// Depths seen from one point: per cube face, the farthest of each texel's four corner rays.
struct DepthCube {
    n: usize,
    depth: Vec<f32>,
}

/// Axis, sign and the two in-face axes of cube face `f`.
fn face_axes(f: usize) -> (usize, f32, usize, usize) {
    let a = f / 2;
    let s = if f.is_multiple_of(2) { 1.0 } else { -1.0 };
    (a, s, (a + 1) % 3, (a + 2) % 3)
}

/// Cast the corner rays of every texel from `p`. `None` when more than `threshold` percent of
/// the hits are back faces (the point is inside geometry).
fn depth_cube(bvh: &Bvh, p: Vec3, n: usize, far: f32, threshold: f32) -> Option<DepthCube> {
    let corners = n + 1;
    let mut corner = vec![f32::INFINITY; 6 * corners * corners];
    let (mut hits, mut back) = (0usize, 0usize);
    for f in 0..6 {
        let (a, s, ua, va) = face_axes(f);
        for j in 0..corners {
            for i in 0..corners {
                let mut d = Vec3::ZERO;
                d[a] = s;
                d[ua] = -1.0 + 2.0 * i as f32 / n as f32;
                d[va] = -1.0 + 2.0 * j as f32 / n as f32;
                let d = d.normalize();
                if let Some(h) = bvh.cast(p, d, far) {
                    corner[(f * corners + j) * corners + i] = h.t;
                    hits += 1;
                    back += usize::from(h.normal.dot(d) > 0.0);
                }
            }
        }
    }
    let total = 6 * corners * corners;
    if threshold < 100.0 && back as f32 * 100.0 > threshold * total as f32 {
        return None;
    }
    let _ = hits;
    let mut depth = vec![0.0; 6 * n * n];
    for f in 0..6 {
        for j in 0..n {
            for i in 0..n {
                let at = |i: usize, j: usize| corner[(f * corners + j) * corners + i];
                let d = at(i, j)
                    .max(at(i + 1, j))
                    .max(at(i, j + 1))
                    .max(at(i + 1, j + 1));
                depth[(f * n + j) * n + i] = d;
            }
        }
    }
    Some(DepthCube { n, depth })
}

/// Distance from `p` to the box (0 inside).
fn box_distance(p: Vec3, b: &Aabb) -> f32 {
    (b.min - p).max(p - b.max).max(Vec3::ZERO).length()
}

impl DepthCube {
    /// Could any part of `b` be seen from `p` (the cube's centre)?
    fn sees(&self, p: Vec3, b: &Aabb) -> bool {
        let pad = Vec3::splat(1e-3) + (b.max - b.min) * 1e-3;
        let b = Aabb::new(b.min - pad, b.max + pad);
        let dist = box_distance(p, &b);
        if dist <= 0.0 {
            return true;
        }
        let limit = dist * (1.0 - 1e-4) - 1e-4;
        let (rel_min, rel_max) = (b.min - p, b.max - p);
        let n = self.n;
        for f in 0..6 {
            let (a, s, ua, va) = face_axes(f);
            // The part of the box in front of this face's plane (an axis-aligned cut), then the
            // rectangle around its projected corners (conservative: projections are convex).
            let (mut lo_box, mut hi_box) = (rel_min, rel_max);
            const EPS: f32 = 1e-4;
            if s > 0.0 {
                lo_box[a] = lo_box[a].max(EPS);
            } else {
                hi_box[a] = hi_box[a].min(-EPS);
            }
            if lo_box[a] > hi_box[a] {
                continue;
            }
            let mut lo = glam::Vec2::splat(f32::MAX);
            let mut hi = glam::Vec2::splat(f32::MIN);
            for k in 0..8 {
                let c = Vec3::new(
                    if k & 1 == 0 { lo_box.x } else { hi_box.x },
                    if k & 2 == 0 { lo_box.y } else { hi_box.y },
                    if k & 4 == 0 { lo_box.z } else { hi_box.z },
                );
                let z = s * c[a];
                let uv = glam::Vec2::new(c[ua] / z, c[va] / z);
                lo = lo.min(uv);
                hi = hi.max(uv);
            }
            if lo.x > 1.0 || lo.y > 1.0 || hi.x < -1.0 || hi.y < -1.0 {
                continue;
            }
            let texel = |v: f32| {
                (((v + 1.0) * 0.5 * n as f32).floor() as isize).clamp(0, n as isize - 1) as usize
            };
            let (i0, i1, j0, j1) = (texel(lo.x), texel(hi.x), texel(lo.y), texel(hi.y));
            for j in j0..=j1 {
                let row = &self.depth[(f * n + j) * n..(f * n + j + 1) * n];
                if row[i0..=i1].iter().any(|d| *d >= limit) {
                    return true;
                }
            }
        }
        false
    }
}

/// Bake the potentially visible sets. `None` when cancelled.
pub fn bake(input: &BakeInput, progress: &Progress) -> Option<OcclusionData> {
    let s = &input.settings;
    let params = [s.smallest_occluder, s.smallest_hole, s.backface_threshold];
    let n_occ = input.occludees.len();
    let words = words_for(n_occ);
    let Some((origin, size, dims)) = grid(input) else {
        return Some(OcclusionData {
            params,
            words,
            ..Default::default()
        });
    };
    let mut data = OcclusionData {
        params,
        origin,
        cell_size: size,
        dims,
        occludees: input.occludees.clone(),
        words,
        sets: Vec::new(),
        cells: vec![OUTSIDE; (dims.x * dims.y * dims.z) as usize],
    };
    // Cells inside a view volume.
    let inside: Vec<bool> = (0..data.cells.len())
        .map(|i| {
            let b = data.cell_bounds(i);
            input.volumes.is_empty() || input.volumes.iter().any(|v| overlaps(&b, v))
        })
        .collect();

    // Lattice points at half-cell spacing; each cell uses the 27 of its corners, edges, faces
    // and centre (shared with its neighbours).
    let lattice = dims * 2 + UVec3::ONE;
    let point_index = |c: UVec3| (c.x + lattice.x * (c.z + lattice.z * c.y)) as usize;
    let coords = |i: usize| {
        let i = i as u32;
        UVec3::new(i % dims.x, i / (dims.x * dims.z), (i / dims.x) % dims.z)
    };
    let cell_points = |cell: usize| -> [usize; 27] {
        let base = coords(cell) * 2;
        std::array::from_fn(|k| {
            let k = k as u32;
            point_index(base + UVec3::new(k % 3, k / 9, (k / 3) % 3))
        })
    };
    let mut needed: Vec<usize> = (0..data.cells.len())
        .filter(|i| inside[*i])
        .flat_map(cell_points)
        .collect();
    needed.sort_unstable();
    needed.dedup();
    progress.total.store(needed.len(), Ordering::Relaxed);

    let bvh = Bvh::new(&input.occluders);
    let all = {
        let geo = input.occluders.bounds().map(|(lo, hi)| Aabb::new(lo, hi));
        union(geo.into_iter().chain(input.occludees.iter().copied()))
    };
    let far = all.map_or(1.0, |b| (b.max - b.min).length() + data.cell_size * 4.0);
    let rays = s.rays_per_side() as usize;
    let threshold = s.backface_threshold;
    let half = size * 0.5;
    let nudge = Vec3::new(0.0031, 0.0047, 0.0023) * size;
    let point_sets: Vec<Option<Vec<u64>>> = needed
        .par_iter()
        .map(|&pi| {
            if progress.cancel.load(Ordering::Relaxed) {
                return None;
            }
            let pi32 = pi as u32;
            let c = UVec3::new(
                pi32 % lattice.x,
                pi32 / (lattice.x * lattice.z),
                (pi32 / lattice.x) % lattice.z,
            );
            // Nudged off the exact cell planes, which floors and wall faces often lie on (rays
            // grazing along a surface can slip through the seams between its triangles).
            let p = origin + c.as_vec3() * half + nudge;
            let cube = depth_cube(&bvh, p, rays, far, threshold);
            progress.done.fetch_add(1, Ordering::Relaxed);
            let cube = cube?;
            let mut set = vec![0u64; words as usize];
            for (i, b) in input.occludees.iter().enumerate() {
                if cube.sees(p, b) {
                    set[i / 64] |= 1 << (i % 64);
                }
            }
            Some(set)
        })
        .collect();
    if progress.cancel.load(Ordering::Relaxed) {
        return None;
    }
    let by_point: HashMap<usize, &Option<Vec<u64>>> =
        needed.iter().copied().zip(point_sets.iter()).collect();

    // Each cell: the union of its valid points (invalid when none is valid).
    let raw: Vec<Option<Vec<u64>>> = (0..data.cells.len())
        .map(|cell| {
            if !inside[cell] {
                return None;
            }
            let mut out: Option<Vec<u64>> = None;
            for p in cell_points(cell) {
                if let Some(Some(set)) = by_point.get(&p) {
                    let o = out.get_or_insert_with(|| vec![0; words as usize]);
                    o.iter_mut().zip(set).for_each(|(a, b)| *a |= b);
                }
            }
            out
        })
        .collect();

    // Widen by the neighbours (conservative between samples), then share equal sets.
    let mut table: HashMap<Vec<u64>, u32> = HashMap::new();
    for cell in 0..data.cells.len() {
        if !inside[cell] {
            continue;
        }
        let Some(own) = &raw[cell] else {
            data.cells[cell] = INVALID;
            continue;
        };
        let mut set = own.clone();
        let c = coords(cell).as_ivec3();
        for dy in -1..=1 {
            for dz in -1..=1 {
                for dx in -1..=1 {
                    let q = c + glam::IVec3::new(dx, dy, dz);
                    if q.min_element() < 0 || q.cmpge(dims.as_ivec3()).any() {
                        continue;
                    }
                    let q = q.as_uvec3();
                    if let Some(other) = &raw[(q.x + dims.x * (q.z + dims.z * q.y)) as usize] {
                        set.iter_mut().zip(other).for_each(|(a, b)| *a |= b);
                    }
                }
            }
        }
        let next = table.len() as u32;
        let id = *table.entry(set.clone()).or_insert_with(|| {
            data.sets.extend_from_slice(&set);
            next
        });
        data.cells[cell] = id;
    }
    Some(data)
}
