//! Detail layers (the Paint Details tab): more grass species and detail meshes on a terrain,
//! each with its own painted density map, like the reference engine's detail prototypes.
//!
//! - **Grass** species ride along with the terrain's grass (`details`): their blades are
//!   scattered into the same chunks and drawn by the same passes, so a species costs only its
//!   blades. Each has its own density, height range, blade width, bend, seed heads and colour
//!   (between the terrain grass's healthy and dry colours, lighter or darker).
//! - **Mesh** details (pebbles, ferns, small rocks…) are mesh instances scattered from their
//!   map, with random scale and turn, optionally tilted with the ground, drawn as ordinary mesh
//!   draws that share one material (the renderer's automatic instancing batches them) and
//!   culled by their size on screen past their draw distance.
//!
//! Layer 0 of the tab is the terrain grass itself (`TerrainData::detail_map`); the layers here
//! follow it. Maps have one byte per splat texel (0..255 of the layer's density).

use std::collections::HashMap;
use std::sync::{Arc, LazyLock, Mutex};

use glam::{Mat4, Vec2, Vec3};
use hecs::Entity;
use serde::{Deserialize, Deserializer, Serialize, Serializer};

use super::TerrainData;
use super::brush::Brush;
use super::details::{self, Blade, MAX_DENSITY, canopy, canopy_slope, patches, swathe};
use super::noise;
use crate::render::MeshDraw;
use crate::render::lod::LodRange;
use crate::render_scene::MaterialSource;
use crate::scene::{MeshRef, PrimitiveMesh, Scene};

/// Most detail layers a terrain holds (besides its grass).
pub const MAX_DETAIL_LAYERS: usize = 16;
/// Most mesh instances one mesh layer places (denser maps are thinned evenly).
pub const MAX_MESH_INSTANCES: usize = 20_000;
/// Most mesh details per square metre.
pub const MAX_MESH_DENSITY: f32 = 4.0;

/// What a detail layer places.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum DetailKind {
    /// Grass blades drawn with the terrain grass.
    #[default]
    Grass,
    /// Mesh instances.
    Mesh,
}

/// One grass species or detail mesh with its painted density.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct DetailLayer {
    pub name: String,
    pub kind: DetailKind,
    /// Instances per square metre at full painted density (grass: blades).
    pub density: f32,
    /// Grass: blade height range in metres.
    pub height: [f32; 2],
    /// Grass: blade width in metres.
    pub width: f32,
    /// Grass: how far blades arch over (0..1.5).
    pub bend: f32,
    /// Grass: 0 = the terrain grass's healthy colour, 1 = its dry colour.
    pub dryness: f32,
    /// Grass: darker (-1) or lighter (+1).
    pub tint: f32,
    /// Grass: share of stems with a seed head (0..1).
    pub seed_heads: f32,
    /// Mesh: what to place and with which material.
    pub mesh: MeshRef,
    pub material: Option<String>,
    /// Mesh: uniform scale range.
    pub scale: [f32; 2],
    /// Mesh: random turn about the up axis.
    pub random_rotation: bool,
    /// Mesh: tilt with the ground instead of standing upright.
    pub align_to_normal: bool,
    pub cast_shadows: bool,
    /// Mesh: drawn while nearer than about this (metres; culled by size on screen).
    pub draw_distance: f32,
    /// Painted density per splat texel (0..255 of `density`); empty = nothing painted.
    #[serde(serialize_with = "save_map", deserialize_with = "load_map")]
    pub map: Vec<u8>,
}

impl Default for DetailLayer {
    fn default() -> Self {
        Self {
            name: "Grass".into(),
            kind: DetailKind::Grass,
            density: 40.0,
            height: [0.25, 0.5],
            width: 0.04,
            bend: 0.35,
            dryness: 0.3,
            tint: 0.0,
            seed_heads: 0.0,
            mesh: MeshRef::Primitive(PrimitiveMesh::Sphere),
            material: None,
            scale: [0.8, 1.2],
            random_rotation: true,
            align_to_normal: true,
            cast_shadows: true,
            draw_distance: 60.0,
            map: Vec::new(),
        }
    }
}

impl DetailLayer {
    /// A grass species.
    pub fn grass(name: &str) -> Self {
        Self {
            name: name.into(),
            ..Default::default()
        }
    }

    /// A detail mesh.
    pub fn mesh(name: &str, mesh: MeshRef, material: Option<String>) -> Self {
        Self {
            name: name.into(),
            kind: DetailKind::Mesh,
            density: 0.3,
            mesh,
            material,
            ..Default::default()
        }
    }

    /// The layer without its painted map (what decides where its blades go).
    pub fn settings(&self) -> Self {
        Self {
            map: Vec::new(),
            ..self.clone()
        }
    }
}

fn save_map<S: Serializer>(v: &[u8], s: S) -> Result<S::Ok, S::Error> {
    s.serialize_str(&super::file::encode(v))
}

fn load_map<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<u8>, D::Error> {
    let text = String::deserialize(d)?;
    super::file::decode(&text).map_err(serde::de::Error::custom)
}

/// Fix the layers after loading (maps of the wrong size are dropped).
pub fn sanitize(d: &mut TerrainData) {
    let n = d.alphamap_resolution * d.alphamap_resolution;
    d.detail_layers.truncate(MAX_DETAIL_LAYERS);
    for l in &mut d.detail_layers {
        if l.map.len() != n {
            l.map.clear();
        }
    }
}

/// Some grass species has painted blades.
pub fn has_grass_species(d: &TerrainData) -> bool {
    let n = d.alphamap_resolution * d.alphamap_resolution;
    d.detail_layers
        .iter()
        .any(|l| l.kind == DetailKind::Grass && l.map.len() == n && l.density > 0.0)
}

/// The terrain draws grass: its own (Grass & Details on and generated) or a painted species.
pub fn draws_grass(d: &TerrainData) -> bool {
    let n = d.alphamap_resolution * d.alphamap_resolution;
    d.details.enabled && (d.detail_map.len() == n || has_grass_species(d))
}

/// Which map the Paint Details brush paints.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DetailTarget {
    /// The terrain grass (`detail_map`).
    TerrainGrass,
    /// `detail_layers[i]`.
    Layer(usize),
}

/// Paint Details: move the target's density under the brush towards `target` (0..1; `erase`
/// = towards 0). Returns true when something changed.
pub fn paint(
    d: &mut TerrainData,
    b: &Brush,
    k: f32,
    which: DetailTarget,
    target: f32,
    erase: bool,
) -> bool {
    let n = d.alphamap_resolution;
    let map = match which {
        DetailTarget::TerrainGrass => &mut d.detail_map,
        DetailTarget::Layer(i) => match d.detail_layers.get_mut(i) {
            Some(l) => &mut l.map,
            None => return false,
        },
    };
    if map.len() != n * n {
        if erase {
            return false;
        }
        *map = vec![0; n * n];
    }
    let goal = if erase {
        0.0
    } else {
        target.clamp(0.0, 1.0) * 255.0
    };
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let lo = ((b.center - b.radius()) / texel).floor().max(Vec2::ZERO);
    let hi = ((b.center + b.radius()) / texel)
        .ceil()
        .min(Vec2::splat(n as f32));
    let mut changed = false;
    for tz in lo.y as usize..hi.y as usize {
        for tx in lo.x as usize..hi.x as usize {
            let a = (k * b.weight((Vec2::new(tx as f32, tz as f32) + 0.5) * texel)).min(1.0);
            if a <= 0.0 {
                continue;
            }
            let v = &mut map[tz * n + tx];
            let cur = *v as f32;
            // At least one step, so a slow brush still gets there.
            let step = (goal - cur) * a;
            let step = if step.abs() < 1.0 && cur != goal {
                step.signum()
            } else {
                step
            };
            let next = (cur + step).round().clamp(0.0, 255.0) as u8;
            changed |= next != *v;
            *v = next;
        }
    }
    changed
}

/// Fill (or clear) a whole map: Paint Details ▸ Fill / Clear.
pub fn fill(d: &mut TerrainData, which: DetailTarget, value: f32) {
    let n = d.alphamap_resolution;
    let v = (value.clamp(0.0, 1.0) * 255.0).round() as u8;
    let map = match which {
        DetailTarget::TerrainGrass => &mut d.detail_map,
        DetailTarget::Layer(i) => match d.detail_layers.get_mut(i) {
            Some(l) => &mut l.map,
            None => return,
        },
    };
    if v == 0 && which != DetailTarget::TerrainGrass {
        map.clear();
    } else {
        *map = vec![v; n * n];
    }
}

/// What a species blade keeps in `shape.z`: below zero, -1 - 2 × layer - jitter (0..1);
/// the terrain grass's dry mix (0..1) and the flowers' cards (2 and up) stay positive.
pub fn species_mark(layer: usize, jitter: f32) -> f32 {
    -1.0 - 2.0 * layer.min(MAX_DETAIL_LAYERS - 1) as f32 - jitter.clamp(0.0, 0.999)
}

/// A species blade's dry mix and tint from its layer's look (grass_blade.wgsl
/// `grass_dry_mix` / `grass_tint` do the same on the GPU from the species table).
pub fn species_look(b: &Blade, layers: &[DetailLayer]) -> Option<(f32, f32)> {
    if b.shape[2] >= 0.0 {
        return None;
    }
    let s = -b.shape[2] - 1.0;
    let li = (s * 0.5).floor();
    let jitter = s - 2.0 * li;
    let l = layers.get(li as usize)?;
    Some((
        (l.dryness + (jitter - 0.5) * 0.25).clamp(0.0, 1.0),
        (l.tint + b.extra[3]).clamp(-1.0, 1.0),
    ))
}

/// The species table of the grass uniform: (dryness, tint) of each layer, two per vec4.
pub fn species_table(layers: &[DetailLayer]) -> [[f32; 4]; MAX_DETAIL_LAYERS / 2] {
    let mut out = [[0.0; 4]; MAX_DETAIL_LAYERS / 2];
    for (i, l) in layers.iter().take(MAX_DETAIL_LAYERS).enumerate() {
        out[i / 2][(i % 2) * 2] = l.dryness;
        out[i / 2][(i % 2) * 2 + 1] = l.tint;
    }
    out
}

/// Blades of the grass species rooted in `[lo, hi)` (terrain-local metres), kept at the
/// density level's share `keep`, with their drop-out keys (see `details::scatter_level`).
pub fn species_blades(d: &TerrainData, lo: Vec2, hi: Vec2, keep: f32) -> Vec<(f32, Blade)> {
    species_blades_in(d, lo, hi, keep, false)
}

/// [`species_blades`], the rows of texels in parallel when `parallel` (the same blades in the
/// same order).
pub(super) fn species_blades_in(
    d: &TerrainData,
    lo: Vec2,
    hi: Vec2,
    keep: f32,
    parallel: bool,
) -> Vec<(f32, Blade)> {
    let n = d.alphamap_resolution;
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let (t0, t1) = (
        (lo / texel).floor().max(Vec2::ZERO),
        (hi / texel).ceil().min(Vec2::splat(n as f32)),
    );
    let holes = d.has_holes();
    let patch = d.details.patch_scale;
    let mut out = Vec::new();
    for (li, l) in d.detail_layers.iter().enumerate() {
        if l.kind != DetailKind::Grass || l.map.len() != n * n {
            continue;
        }
        let per_texel = l.density.clamp(0.0, MAX_DENSITY) * texel.x * texel.y;
        let seed = d.rule_seed ^ (0x5eed_0000 + li as u32 * 7919);
        let (h0, h1) = (
            l.height[0].max(0.01),
            l.height[1].max(l.height[0]).max(0.01),
        );
        let rows = details::rows_of(t0.y as usize..t1.y as usize, parallel, |tz| {
            let mut row = Vec::new();
            for tx in t0.x as usize..t1.x as usize {
                let density = l.map[tz * n + tx];
                if density == 0 {
                    continue;
                }
                let want = per_texel * density as f32 / 255.0;
                let (ix, iz) = (tx as i32, tz as i32);
                let count = want.floor() as u32
                    + u32::from(noise::hash(ix, iz, seed ^ 0x51) < want.fract());
                for k in 0..count {
                    let r = |salt: u32| noise::hash(ix, iz, seed.wrapping_add(k * 97 + salt));
                    let key = r(11);
                    if key >= keep {
                        continue;
                    }
                    let p = Vec2::new(tx as f32 + r(1), tz as f32 + r(2)) * texel;
                    if p.x < lo.x || p.y < lo.y || p.x >= hi.x || p.y >= hi.y {
                        continue;
                    }
                    if holes && d.hole_at(p.x, p.y) {
                        continue;
                    }
                    let relief = canopy(p);
                    let mut tall = h0 + (h1 - h0) * (0.55 * r(3) + 0.45 * relief);
                    let seed_head = r(9) < l.seed_heads.clamp(0.0, 1.0);
                    let mut width = l.width.max(0.002) * (0.7 + 0.6 * r(5));
                    if seed_head {
                        tall *= 1.2 + 0.4 * r(10);
                        width = width.min(0.02) * 0.3;
                    }
                    let bend = l.bend.clamp(0.0, 1.5)
                        * (0.35 + 0.65 * r(12))
                        * if seed_head { 0.35 } else { 1.0 };
                    let (lush, dry) = patches(p, patch);
                    let (shade, slope) = canopy_slope(p);
                    let blade = Blade {
                        position: [p.x, d.sample_height(p.x, p.y), p.y, tall],
                        shape: [
                            r(4) * std::f32::consts::TAU,
                            width,
                            // The layer and the blade's own jitter: its Healthy↔Dry and
                            // Brightness come from the species table at draw time, so
                            // colour edits need no scatter (`species_look`).
                            species_mark(li, r(6)),
                            r(7),
                        ],
                        extra: [
                            key + if seed_head { 2.0 } else { 0.0 },
                            bend,
                            r(8) * std::f32::consts::TAU,
                            (r(14) - 0.5) * 0.3,
                        ],
                        field: [lush, dry, swathe(p, patch), shade],
                        slope: slope.into(),
                    };
                    row.push((key, blade));
                }
            }
            row
        });
        out.extend(rows);
    }
    out
}

/// Terrain-local matrices of a mesh layer's instances (deterministic per seed and map).
pub fn mesh_instances(d: &TerrainData, li: usize) -> Vec<Mat4> {
    let Some(l) = d.detail_layers.get(li) else {
        return Vec::new();
    };
    let n = d.alphamap_resolution;
    if l.kind != DetailKind::Mesh || l.map.len() != n * n || l.density <= 0.0 {
        return Vec::new();
    }
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    let per_texel = l.density.clamp(0.0, MAX_MESH_DENSITY) * texel.x * texel.y;
    // Thin evenly when the map would place more than the cap.
    let total: f32 = l.map.iter().map(|v| *v as f32 / 255.0).sum::<f32>() * per_texel;
    let keep = (MAX_MESH_INSTANCES as f32 / total.max(1.0)).min(1.0);
    let seed = d.rule_seed ^ (0x3e5_0000 + li as u32 * 104_729);
    let holes = d.has_holes();
    let (s0, s1) = (l.scale[0].max(0.01), l.scale[1].max(l.scale[0]).max(0.01));
    let mut out = Vec::new();
    for tz in 0..n {
        for tx in 0..n {
            let density = l.map[tz * n + tx];
            if density == 0 {
                continue;
            }
            let want = per_texel * density as f32 / 255.0;
            let (ix, iz) = (tx as i32, tz as i32);
            let count =
                want.floor() as u32 + u32::from(noise::hash(ix, iz, seed ^ 0x51) < want.fract());
            for k in 0..count {
                let r = |salt: u32| noise::hash(ix, iz, seed.wrapping_add(k * 131 + salt));
                if r(11) >= keep {
                    continue;
                }
                let p = Vec2::new(tx as f32 + r(1), tz as f32 + r(2)) * texel;
                if holes && d.hole_at(p.x, p.y) {
                    continue;
                }
                let up = if l.align_to_normal {
                    ground_normal(d, p)
                } else {
                    Vec3::Y
                };
                let yaw = if l.random_rotation {
                    r(3) * std::f32::consts::TAU
                } else {
                    0.0
                };
                let s = s0 + (s1 - s0) * r(4);
                out.push(Mat4::from_scale_rotation_translation(
                    Vec3::splat(s),
                    crate::world_gen::scatter_set::instance_rotation(yaw, up),
                    Vec3::new(p.x, d.sample_height(p.x, p.y), p.y),
                ));
                if out.len() >= MAX_MESH_INSTANCES {
                    return out;
                }
            }
        }
    }
    out
}

/// Ground normal at a terrain-local point.
pub fn ground_normal(d: &TerrainData, p: Vec2) -> Vec3 {
    let e = d.spacing();
    let h = |dx: f32, dz: f32| d.sample_height(p.x + dx, p.y + dz);
    let gx = (h(e.x, 0.0) - h(-e.x, 0.0)) / (2.0 * e.x);
    let gz = (h(0.0, e.y) - h(0.0, -e.y)) / (2.0 * e.y);
    Vec3::new(-gx, 1.0, -gz).normalize()
}

/// Per terrain asset: the version the instances were made from and (layer, local matrix) of
/// every mesh detail.
type Instances = Arc<Vec<(usize, Mat4)>>;
static CACHE: LazyLock<Mutex<HashMap<String, (u64, Instances)>>> = LazyLock::new(Default::default);

/// The mesh details of a terrain asset (made again when its data changes).
pub fn cached_instances(key: &str, d: &TerrainData, version: u64) -> Instances {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((v, list)) = cache.get(key)
        && *v == version
    {
        return list.clone();
    }
    let list: Vec<(usize, Mat4)> = (0..d.detail_layers.len())
        .flat_map(|li| mesh_instances(d, li).into_iter().map(move |m| (li, m)))
        .collect();
    let list = Arc::new(list);
    if cache.len() > 32 {
        cache.clear();
    }
    cache.insert(key.to_owned(), (version, list.clone()));
    list
}

/// Mesh draws of the detail meshes of every enabled terrain `keep` accepts.
pub fn draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    keep: impl Fn(Entity) -> bool,
    out: &mut Vec<MeshDraw>,
) {
    for (e, t) in super::terrains(scene) {
        if !keep(e) {
            continue;
        }
        let Some((d, version)) = super::store::get(&t.terrain_data) else {
            continue;
        };
        if !d.detail_layers.iter().any(|l| l.kind == DetailKind::Mesh) {
            continue;
        }
        let list = cached_instances(&t.terrain_data, &d, version);
        if list.is_empty() {
            continue;
        }
        let corner = Mat4::from_translation(scene.world_position(e));
        let params: Vec<_> = (d.detail_layers.iter())
            .map(|l| materials.params(l.material.as_deref()))
            .collect();
        for (li, m) in list.iter() {
            let l = &d.detail_layers[*li];
            let model = corner * *m;
            let (scale, _, pos) = model.to_scale_rotation_translation();
            let size = scale.max_element().max(0.05);
            // Hidden once smaller on screen than at its draw distance (60° view).
            let min = size / (2.0 * l.draw_distance.max(1.0) * 30f32.to_radians().tan());
            out.push(MeshDraw {
                mesh: l.mesh.clone(),
                model,
                material: params[*li].clone(),
                cast_shadows: l.cast_shadows && t.cast_shadows,
                receive_shadows: true,
                outline: false,
                sorting: Default::default(),
                lod: Some(LodRange {
                    center: pos + Vec3::Y * size * 0.5,
                    size,
                    min,
                    max: f32::INFINITY,
                    fade: 0.0,
                    virtual_lod: None,
                }),
            });
        }
    }
}

/// Instances of every detail layer (blades scattered at full density, mesh instances), for
/// the Inspector's counts.
pub fn counts(d: &TerrainData) -> Vec<usize> {
    let n = d.alphamap_resolution;
    let texel = Vec2::new(d.size.x, d.size.z) / n as f32;
    d.detail_layers
        .iter()
        .enumerate()
        .map(|(i, l)| match l.kind {
            DetailKind::Mesh => mesh_instances(d, i).len(),
            DetailKind::Grass => {
                let per = l.density.clamp(0.0, details::MAX_DENSITY) * texel.x * texel.y;
                (l.map.iter().map(|v| *v as f32 / 255.0).sum::<f32>() * per) as usize
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flat() -> TerrainData {
        let mut d = TerrainData::new(65, Vec3::new(64.0, 10.0, 64.0), 64);
        d.details.enabled = true;
        d
    }

    fn brush(x: f32) -> Brush {
        Brush {
            center: Vec2::new(x, 32.0),
            size: 16.0,
            strength: 1.0,
            falloff: 0.0,
        }
    }

    #[test]
    fn species_paint_their_own_maps() {
        let mut d = flat();
        d.detail_layers = vec![DetailLayer::grass("Tall"), DetailLayer::grass("Short")];
        d.detail_layers[0].height = [0.9, 1.2];
        d.detail_layers[1].height = [0.1, 0.15];
        assert!(!draws_grass(&d), "nothing painted yet");
        for _ in 0..20 {
            paint(
                &mut d,
                &brush(16.0),
                0.5,
                DetailTarget::Layer(0),
                1.0,
                false,
            );
            paint(
                &mut d,
                &brush(48.0),
                0.5,
                DetailTarget::Layer(1),
                1.0,
                false,
            );
        }
        assert!(
            draws_grass(&d),
            "a species draws grass without the terrain grass map"
        );
        let blades = species_blades(&d, Vec2::ZERO, Vec2::splat(64.0), 1.0);
        let tall: Vec<_> = blades.iter().filter(|b| b.1.position[3] > 0.8).collect();
        let short: Vec<_> = blades.iter().filter(|b| b.1.position[3] < 0.2).collect();
        assert!(
            tall.len() > 500 && short.len() > 500,
            "{} / {}",
            tall.len(),
            short.len()
        );
        assert!(
            tall.iter().all(|b| (b.1.position[0] - 16.0).abs() < 9.0),
            "tall on the left"
        );
        assert!(
            short.iter().all(|b| (b.1.position[0] - 48.0).abs() < 9.0),
            "short on the right"
        );
        // Deterministic, chunks add up, and the whole scatter carries them.
        let halves = species_blades(&d, Vec2::ZERO, Vec2::new(32.0, 64.0), 1.0).len()
            + species_blades(&d, Vec2::new(32.0, 0.0), Vec2::splat(64.0), 1.0).len();
        assert_eq!(halves, blades.len());
        let all = details::scatter(&d, Vec2::ZERO, Vec2::splat(64.0));
        assert_eq!(
            all.len(),
            blades.len(),
            "no terrain grass map: only the species"
        );
        // Erasing clears the left species.
        for _ in 0..20 {
            paint(&mut d, &brush(16.0), 0.5, DetailTarget::Layer(0), 1.0, true);
        }
        let left = species_blades(&d, Vec2::ZERO, Vec2::new(32.0, 64.0), 1.0);
        assert!(left.is_empty());
    }

    #[test]
    fn species_colour_is_looked_up_not_baked() {
        let mut d = flat();
        d.detail_layers = vec![DetailLayer::grass("A"), DetailLayer::grass("B")];
        d.detail_layers[1].dryness = 0.8;
        d.detail_layers[1].tint = -0.4;
        fill(&mut d, DetailTarget::Layer(1), 1.0);
        let before = species_blades(&d, Vec2::ZERO, Vec2::splat(16.0), 1.0);
        assert!(before.len() > 100);
        for (_, b) in &before {
            let (dry, tint) = species_look(b, &d.detail_layers).expect("a species blade");
            assert!((dry - 0.8).abs() <= 0.125 + 1e-4, "{dry}");
            assert!((tint + 0.4).abs() <= 0.15 + 1e-4, "{tint}");
        }
        // A colour edit changes no blade: the table carries it.
        d.detail_layers[1].dryness = 0.1;
        d.detail_layers[1].tint = 0.5;
        let after = species_blades(&d, Vec2::ZERO, Vec2::splat(16.0), 1.0);
        assert_eq!(before, after);
        let (dry, _) = species_look(&after[0].1, &d.detail_layers).unwrap();
        assert!(dry <= 0.1 + 0.125 + 1e-4);
        let table = species_table(&d.detail_layers);
        assert_eq!(table[0][2..], [0.1, 0.5]);
        // The marks keep the layer and the jitter apart.
        for li in [0, 3, 15] {
            for j in [0.0, 0.37, 0.999] {
                let b = Blade {
                    shape: [0.0, 0.0, species_mark(li, j), 0.0],
                    ..bytemuck::Zeroable::zeroed()
                };
                let s = -b.shape[2] - 1.0;
                assert_eq!((s * 0.5).floor() as usize, li);
                assert!((s - 2.0 * (s * 0.5).floor() - j).abs() < 1e-4);
            }
        }
    }

    #[test]
    fn detail_layers_round_trip_through_the_file() {
        let mut d = flat();
        let mut l = DetailLayer::mesh(
            "Pebbles",
            MeshRef::Primitive(PrimitiveMesh::Sphere),
            Some("Assets/Materials/Stone.mat".into()),
        );
        l.map = vec![7; 64 * 64];
        d.detail_layers.push(l);
        assert!(
            !flat().to_file_string().contains("detail_layers"),
            "left out when empty"
        );
        let back = TerrainData::from_file_str(&d.to_file_string()).unwrap();
        assert_eq!(back.detail_layers, d.detail_layers);
    }

    #[test]
    fn mesh_details_follow_their_map_and_the_ground() {
        let mut d = flat();
        // A 45° ramp along x.
        d.size.y = 100.0;
        for iz in 0..65 {
            for ix in 0..65 {
                d.set_height(ix, iz, ix as f32 / 64.0 * 0.64);
            }
        }
        let mut l = DetailLayer::mesh("Rocks", MeshRef::Primitive(PrimitiveMesh::Cube), None);
        l.density = 1.0;
        l.scale = [0.5, 2.0];
        d.detail_layers.push(l);
        assert!(mesh_instances(&d, 0).is_empty(), "nothing painted");
        fill(&mut d, DetailTarget::Layer(0), 1.0);
        let list = mesh_instances(&d, 0);
        // About one per square metre.
        assert!((3000..5200).contains(&list.len()), "{}", list.len());
        for m in &list {
            let (s, r, t) = m.to_scale_rotation_translation();
            assert!(s.x >= 0.5 - 1e-3 && s.x <= 2.0 + 1e-3);
            assert!(
                (t.y - d.sample_height(t.x, t.z)).abs() < 1e-3,
                "on the ground"
            );
            // Tilted with the slope (aligned to the normal).
            let up = r * Vec3::Y;
            let n = ground_normal(&d, Vec2::new(t.x, t.z));
            assert!(up.dot(n) > 0.999, "{up} vs {n}");
        }
        assert_eq!(list, mesh_instances(&d, 0), "deterministic");
        fill(&mut d, DetailTarget::Layer(0), 0.0);
        assert!(mesh_instances(&d, 0).is_empty());
    }

    #[test]
    fn mesh_details_become_instanced_mesh_draws() {
        let mut d = flat();
        let mut l = DetailLayer::mesh("Stones", MeshRef::Primitive(PrimitiveMesh::Sphere), None);
        l.density = 0.5;
        l.draw_distance = 40.0;
        d.detail_layers.push(l);
        fill(&mut d, DetailTarget::Layer(0), 1.0);
        let path = std::env::temp_dir()
            .join(format!(
                "misty_detail_meshes_{}.terrain",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned();
        super::super::store::insert(&path, d);
        let mut scene = Scene::new("t");
        let e = scene.spawn("Terrain", None);
        scene.set_transform(
            e,
            crate::scene::Transform::from_position(Vec3::new(10.0, 2.0, 0.0)),
        );
        scene.add(
            e,
            crate::scene::Terrain {
                terrain_data: path.clone(),
                ..Default::default()
            },
        );
        struct NoMaterials;
        impl MaterialSource for NoMaterials {
            fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
                Default::default()
            }
        }
        let mut out = Vec::new();
        draws(&scene, &NoMaterials, |_| true, &mut out);
        assert!(out.len() > 1500, "{}", out.len());
        let min_x = out
            .iter()
            .map(|m| m.model.w_axis.x)
            .fold(f32::MAX, f32::min);
        assert!(min_x >= 10.0, "placed from the terrain's corner");
        assert!(
            out.iter().all(|m| m.lod.is_some_and(|l| l.min > 0.0)),
            "culled far away"
        );
        super::super::store::unload(&path);
    }
}
