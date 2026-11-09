//! HLOD proxies: one cheap merged mesh per cell, drawn while the cell is not loaded.
//!
//! "Build HLODs" reads every cell sub-scene and merges what it shows into one mesh with vertex
//! colours (drawn with one vertex-colour material, so a whole cell is one draw):
//!
//! - meshes (the coarsest level of a LOD Group), welded and simplified to a few dozen
//!   triangles each, coloured with their material's colour times its albedo texture's mean;
//!   objects smaller than [`MIN_SIZE`] are left out (they are a pixel at proxy distances);
//! - terrains as a coarse grid (one vertex every [`TERRAIN_STEP`] metres) coloured with the
//!   splat layers' mean texture colours, darker where trees stand;
//! - terrain trees as a small diamond around each crown (the tree impostors' job at mid range,
//!   done by one merged mesh here).
//!
//! Proxies are written next to the cells (`<folder>/HLOD/Cell_x_z.hlod`: JSON with quantised
//! positions, octahedral normals and colours in base64, a few kB each). At run time
//! [`draws`] adds one draw per unloaded cell; the file is read and its mesh registered on the
//! background pool the first time the cell's proxy is wanted.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};

use glam::{Mat4, Vec2, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::{CellCoord, WorldPartition};
use crate::render::mesh::{MeshData, Vertex};
use crate::render::{MaterialParams, MeshDraw};
use crate::render_scene::MaterialSource;
use crate::scene::{LodGroup, MeshFilter, MeshRef, MeshRenderer, Scene, Terrain};
use crate::terrain::TerrainData;

/// Objects whose bounds are smaller than this (metres, longest side) stay out of proxies.
pub const MIN_SIZE: f32 = 1.5;
/// Spacing of a terrain proxy's grid (metres).
pub const TERRAIN_STEP: f32 = 32.0;
/// Triangles a mesh keeps in a proxy (at most).
pub const MESH_TRIANGLES: usize = 48;
/// The terrain proxy sits this far below the real surface, so a cell whose objects are still
/// arriving (proxy and tile both drawn) shows the tile.
const TERRAIN_SINK: f32 = 0.6;

/// A tree crown in a proxy: a small diamond, made when the proxy is loaded (so the file holds
/// ten bytes per tree, not a mesh).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Crown {
    pub center: Vec3,
    /// Horizontal and vertical radius (metres, up to 25.5).
    pub radius: f32,
    pub height: f32,
    /// Turn about the vertical axis (radians).
    pub yaw: f32,
    /// Colour variation 0..1.
    pub tint: f32,
}

/// Triangles of a crown's diamond.
const CROWN_TRIANGLES: usize = 8;

/// A proxy in world space: a merged mesh and the tree crowns standing on it.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct HlodMesh {
    pub positions: Vec<Vec3>,
    pub normals: Vec<Vec3>,
    /// Linear RGB.
    pub colors: Vec<[u8; 3]>,
    pub indices: Vec<u32>,
    pub crowns: Vec<Crown>,
}

impl HlodMesh {
    /// Triangles drawn (crowns included).
    pub fn triangles(&self) -> usize {
        self.indices.len() / 3 + self.crowns.len() * CROWN_TRIANGLES
    }

    pub fn is_empty(&self) -> bool {
        self.indices.is_empty() && self.crowns.is_empty()
    }

    fn push(&mut self, p: Vec3, n: Vec3, c: [u8; 3]) -> u32 {
        self.positions.push(p);
        self.normals.push(n);
        self.colors.push(c);
        self.positions.len() as u32 - 1
    }

    /// Bounds of the mesh and the crowns.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        let start = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        let mesh = self
            .positions
            .iter()
            .fold(start, |(lo, hi), p| (lo.min(*p), hi.max(*p)));
        self.crowns.iter().fold(mesh, |(lo, hi), c| {
            let r = Vec3::new(c.radius, c.height, c.radius);
            (lo.min(c.center - r), hi.max(c.center + r))
        })
    }

    /// The crowns' diamonds added to the mesh.
    pub fn merged(&self) -> HlodMesh {
        let mut out = HlodMesh {
            crowns: Vec::new(),
            ..self.clone()
        };
        for c in &self.crowns {
            let col = to_u8(Vec3::new(0.05, 0.11, 0.035) * (0.8 + 0.4 * c.tint));
            let top = out.push(c.center + Vec3::Y * c.height, Vec3::Y, col);
            let bottom = out.push(c.center - Vec3::Y * c.height, -Vec3::Y, col);
            let ring: Vec<u32> = (0..4)
                .map(|k| {
                    let a = c.yaw + k as f32 * std::f32::consts::FRAC_PI_2;
                    let d = Vec3::new(a.cos(), 0.0, a.sin());
                    out.push(c.center + d * c.radius, d, col)
                })
                .collect();
            for k in 0..4 {
                let (a, b) = (ring[k], ring[(k + 1) % 4]);
                out.indices.extend([top, b, a, bottom, a, b]);
            }
        }
        out
    }

    /// Renderer mesh (vertex colours, no UVs), crowns included.
    pub fn to_mesh_data(&self) -> MeshData {
        let all = self.merged();
        let mut m = MeshData {
            vertices: all
                .positions
                .iter()
                .zip(&all.normals)
                .map(|(p, n)| Vertex {
                    position: p.to_array(),
                    normal: n.to_array(),
                    uv: [0.0; 2],
                    tangent: [1.0, 0.0, 0.0, 1.0],
                    uv2: [0.0; 2],
                })
                .collect(),
            indices: all.indices,
            ..Default::default()
        };
        m.colors = all.colors.iter().map(|c| [c[0], c[1], c[2], 255]).collect();
        m
    }

    // ---- file -------------------------------------------------------------------------

    pub fn to_file_string(&self, cell: CellCoord) -> String {
        let (lo, hi) = self.bounds();
        let ext = (hi - lo).max(Vec3::splat(1e-3));
        let quant = |p: Vec3, out: &mut Vec<u8>| {
            let q = (p - lo) / ext;
            for v in q.to_array() {
                out.extend(((v.clamp(0.0, 1.0) * 65535.0).round() as u16).to_le_bytes());
            }
        };
        let mut pos = Vec::with_capacity(self.positions.len() * 6);
        for p in &self.positions {
            quant(*p, &mut pos);
        }
        let mut nor = Vec::with_capacity(self.normals.len() * 2);
        for n in &self.normals {
            let o = oct_encode(*n);
            nor.extend([
                (o.x * 127.0).round() as i8 as u8,
                (o.y * 127.0).round() as i8 as u8,
            ]);
        }
        let col: Vec<u8> = self.colors.iter().flatten().copied().collect();
        let wide = self.positions.len() > u16::MAX as usize;
        let idx: Vec<u8> = if wide {
            self.indices.iter().flat_map(|i| i.to_le_bytes()).collect()
        } else {
            self.indices
                .iter()
                .flat_map(|i| (*i as u16).to_le_bytes())
                .collect()
        };
        let mut crowns = Vec::with_capacity(self.crowns.len() * 10);
        for c in &self.crowns {
            quant(c.center, &mut crowns);
            let byte = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
            crowns.extend([
                byte(c.radius / 25.5),
                byte(c.height / 25.5),
                byte(c.yaw.rem_euclid(std::f32::consts::TAU) / std::f32::consts::TAU),
                byte(c.tint),
            ]);
        }
        let file = HlodFile {
            version: 1,
            cell: [cell.0, cell.1],
            min: lo.to_array(),
            max: hi.to_array(),
            vertices: self.positions.len(),
            wide,
            positions: crate::terrain::base64_encode(&pos),
            normals: crate::terrain::base64_encode(&nor),
            colors: crate::terrain::base64_encode(&col),
            indices: crate::terrain::base64_encode(&idx),
            crowns: crate::terrain::base64_encode(&crowns),
        };
        serde_json::to_string(&file).expect("hlod serialises")
    }

    pub fn from_file_str(text: &str) -> Result<Self, String> {
        let f: HlodFile = serde_json::from_str(text).map_err(|e| format!("invalid HLOD: {e}"))?;
        let lo = Vec3::from(f.min);
        let ext = (Vec3::from(f.max) - lo).max(Vec3::splat(1e-3));
        let pos = crate::terrain::base64_decode(&f.positions)?;
        let nor = crate::terrain::base64_decode(&f.normals)?;
        let col = crate::terrain::base64_decode(&f.colors)?;
        let idx = crate::terrain::base64_decode(&f.indices)?;
        let crowns = crate::terrain::base64_decode(&f.crowns)?;
        let n = f.vertices;
        if pos.len() != n * 6 || nor.len() != n * 2 || col.len() != n * 3 {
            return Err("HLOD arrays don't match the vertex count".into());
        }
        let unquant = |b: &[u8]| {
            let q = |i: usize| u16::from_le_bytes([b[i], b[i + 1]]) as f32 / 65535.0;
            lo + Vec3::new(q(0), q(2), q(4)) * ext
        };
        let positions = pos.as_chunks::<6>().0.iter().map(|b| unquant(b)).collect();
        let normals = nor
            .as_chunks::<2>()
            .0
            .iter()
            .map(|b| {
                oct_decode(Vec2::new(
                    b[0] as i8 as f32 / 127.0,
                    b[1] as i8 as f32 / 127.0,
                ))
            })
            .collect();
        let colors = col.as_chunks::<3>().0.to_vec();
        let indices: Vec<u32> = if f.wide {
            idx.as_chunks::<4>()
                .0
                .iter()
                .map(|b| u32::from_le_bytes(*b))
                .collect()
        } else {
            idx.as_chunks::<2>()
                .0
                .iter()
                .map(|b| u16::from_le_bytes(*b) as u32)
                .collect()
        };
        if indices.iter().any(|i| *i as usize >= n) {
            return Err("HLOD index out of range".into());
        }
        let crowns = crowns
            .as_chunks::<10>()
            .0
            .iter()
            .map(|b| Crown {
                center: unquant(&b[..6]),
                radius: b[6] as f32 / 255.0 * 25.5,
                height: b[7] as f32 / 255.0 * 25.5,
                yaw: b[8] as f32 / 255.0 * std::f32::consts::TAU,
                tint: b[9] as f32 / 255.0,
            })
            .collect();
        Ok(Self {
            positions,
            normals,
            colors,
            indices,
            crowns,
        })
    }
}

#[derive(Serialize, Deserialize)]
struct HlodFile {
    version: u32,
    cell: [i32; 2],
    min: [f32; 3],
    max: [f32; 3],
    vertices: usize,
    wide: bool,
    positions: String,
    normals: String,
    colors: String,
    indices: String,
    /// Tree crowns: 10 bytes each (centre quantised like the positions, radius, height, yaw,
    /// tint).
    #[serde(default)]
    crowns: String,
}

fn oct_encode(n: Vec3) -> Vec2 {
    let n = n / (n.x.abs() + n.y.abs() + n.z.abs()).max(1e-6);
    if n.y >= 0.0 {
        Vec2::new(n.x, n.z)
    } else {
        let s = |v: f32| if v >= 0.0 { 1.0 } else { -1.0 };
        Vec2::new((1.0 - n.z.abs()) * s(n.x), (1.0 - n.x.abs()) * s(n.z))
    }
}

fn oct_decode(e: Vec2) -> Vec3 {
    let y = 1.0 - e.x.abs() - e.y.abs();
    let mut n = Vec3::new(e.x, y, e.y);
    if y < 0.0 {
        let s = |v: f32| if v >= 0.0 { 1.0 } else { -1.0 };
        n.x = (1.0 - e.y.abs()) * s(e.x);
        n.z = (1.0 - e.x.abs()) * s(e.y);
    }
    n.normalize_or_zero()
}

// ---- building ------------------------------------------------------------------------------

fn to_u8(c: Vec3) -> [u8; 3] {
    (c.clamp(Vec3::ZERO, Vec3::ONE) * 255.0)
        .round()
        .to_array()
        .map(|v| v as u8)
}

fn srgb_to_linear(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Colours, simplified meshes and tree sizes looked up once per build.
#[derive(Default)]
pub struct BuildCache {
    /// Linear mean colour of a texture.
    textures: HashMap<String, Vec3>,
    /// Linear colour of a material (colour × albedo mean).
    materials: HashMap<Option<String>, Vec3>,
    /// Welded, simplified mesh (local space).
    meshes: HashMap<String, Option<Arc<MeshData>>>,
    /// Crown centre, radii (horizontal, vertical) of a tree asset.
    trees: HashMap<String, Option<(Vec3, Vec2)>>,
    /// Folder asset paths resolve against.
    root: PathBuf,
}

impl BuildCache {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            ..Default::default()
        }
    }

    fn abs(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    fn texture_mean(&mut self, rel: &str) -> Vec3 {
        if let Some(c) = self.textures.get(rel) {
            return *c;
        }
        let c = crate::render::texture::load_image(&self.abs(rel))
            .ok()
            .map(|img| {
                let small = image::imageops::thumbnail(&img, 32, 32);
                let mut sum = Vec3::ZERO;
                for p in small.pixels() {
                    sum += Vec3::new(
                        srgb_to_linear(p[0] as f32 / 255.0),
                        srgb_to_linear(p[1] as f32 / 255.0),
                        srgb_to_linear(p[2] as f32 / 255.0),
                    );
                }
                sum / (small.width() * small.height()).max(1) as f32
            })
            .unwrap_or(Vec3::splat(0.5));
        self.textures.insert(rel.to_owned(), c);
        c
    }

    fn material(&mut self, rel: Option<&str>) -> Vec3 {
        let key = rel.map(str::to_owned);
        if let Some(c) = self.materials.get(&key) {
            return *c;
        }
        let c = match rel.and_then(|r| crate::assets::Material::load(&self.abs(r)).ok()) {
            Some(m) => {
                let base = Vec3::new(
                    srgb_to_linear(m.color[0]),
                    srgb_to_linear(m.color[1]),
                    srgb_to_linear(m.color[2]),
                );
                match m.albedo_map.as_deref() {
                    Some(t) => base * self.texture_mean(t),
                    None => base,
                }
            }
            None => Vec3::splat(0.6),
        };
        self.materials.insert(key, c);
        c
    }

    fn mesh(&mut self, r: &MeshRef) -> Option<Arc<MeshData>> {
        let key = match r {
            MeshRef::Asset(k) => k.clone(),
            MeshRef::Primitive(p) => p.name().to_owned(),
        };
        if let Some(m) = self.meshes.get(&key) {
            return m.clone();
        }
        let resolved = match r.split_asset() {
            Some((path, i)) if path.starts_with("Assets/") => {
                MeshRef::asset(&self.abs(path).to_string_lossy().replace('\\', "/"), i)
            }
            _ => r.clone(),
        };
        let m = crate::models::mesh(&resolved).map(|h| Arc::new(simplified(&h, MESH_TRIANGLES)));
        self.meshes.insert(key, m.clone());
        m
    }

    fn tree(&mut self, rel: &str) -> Option<(Vec3, Vec2)> {
        if let Some(t) = self.trees.get(rel) {
            return *t;
        }
        let abs = self.abs(rel).to_string_lossy().replace('\\', "/");
        let t = crate::trees::store::get(&abs)
            .or_else(|| crate::trees::store::get(rel))
            .map(|b| (b.model.crown_center, b.model.crown_radii));
        self.trees.insert(rel.to_owned(), t);
        t
    }
}

/// A mesh welded by position (no UV seams), simplified to about `target` triangles.
fn simplified(m: &MeshData, target: usize) -> MeshData {
    let mut welded = MeshData::default();
    let mut at: HashMap<[u32; 3], u32> = HashMap::new();
    for i in &m.indices {
        let v = m.vertices[*i as usize];
        let key = v.position.map(f32::to_bits);
        let id = *at.entry(key).or_insert_with(|| {
            welded.vertices.push(Vertex {
                normal: [0.0; 3],
                uv: [0.0; 2],
                ..v
            });
            welded.vertices.len() as u32 - 1
        });
        welded.indices.push(id);
    }
    let tris = welded.indices.len() / 3;
    let mut out = if tris > target {
        crate::vgeo::rock::simplified(&welded, target as f32 / tris as f32)
    } else {
        welded
    };
    out.compute_normals();
    out
}

/// The renderers a proxy takes from the objects under `roots`: only the coarsest level of a
/// LOD Group, only enabled Mesh Renderers of active objects.
fn proxy_renderers(scene: &Scene, roots: &[Entity]) -> Vec<Entity> {
    let mut all = Vec::new();
    for r in roots {
        let mut stack = vec![*r];
        while let Some(e) = stack.pop() {
            all.push(e);
            stack.extend(scene.children(e));
        }
    }
    let mut skip = std::collections::HashSet::new();
    for e in &all {
        if let Ok(g) = scene.world.get::<&LodGroup>(*e)
            && g.enabled
            && g.lods.len() > 1
        {
            for level in &g.lods[..g.lods.len() - 1] {
                skip.extend(level.renderers.iter().copied());
            }
        }
    }
    all.into_iter()
        .filter(|e| !skip.contains(e) && scene.active_in_hierarchy(*e))
        .filter(|e| {
            scene
                .world
                .get::<&MeshRenderer>(*e)
                .is_ok_and(|r| r.enabled)
        })
        .collect()
}

/// Merge what the objects under `roots` show into one proxy (see the module docs).
pub fn build_proxy(scene: &Scene, roots: &[Entity], cache: &mut BuildCache) -> HlodMesh {
    let mut out = HlodMesh::default();
    for e in proxy_renderers(scene, roots) {
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let world = scene.world_matrix(e);
        if let Some((lo, hi)) = crate::models::bounds(&mesh) {
            let size = world.transform_vector3(hi - lo).abs().max_element();
            if size < MIN_SIZE {
                continue;
            }
        }
        let Some(data) = cache.mesh(&mesh) else {
            continue;
        };
        let material = scene
            .world
            .get::<&MeshRenderer>(e)
            .ok()
            .and_then(|r| r.material_at(0).map(str::to_owned));
        let color = to_u8(cache.material(material.as_deref()));
        let normal_m = world.inverse().transpose();
        let base = out.positions.len() as u32;
        for v in &data.vertices {
            let p = world.transform_point3(Vec3::from(v.position));
            let n = normal_m
                .transform_vector3(Vec3::from(v.normal))
                .normalize_or_zero();
            out.push(p, n, color);
        }
        out.indices.extend(data.indices.iter().map(|i| base + i));
    }
    for r in roots {
        let mut stack = vec![*r];
        while let Some(e) = stack.pop() {
            stack.extend(scene.children(e));
            let Ok(t) = scene.world.get::<&Terrain>(e).map(|t| (*t).clone()) else {
                continue;
            };
            if !t.enabled || !scene.active_in_hierarchy(e) {
                continue;
            }
            let abs = cache
                .abs(&t.terrain_data)
                .to_string_lossy()
                .replace('\\', "/");
            let data = crate::terrain::store::get(&abs)
                .or_else(|| crate::terrain::store::get(&t.terrain_data));
            if let Some((d, _)) = data {
                terrain_proxy(&d, scene.world_position(e), cache, &mut out);
            }
        }
    }
    out
}

/// A coarse coloured grid of a terrain, plus a diamond per tree crown.
pub fn terrain_proxy(d: &TerrainData, corner: Vec3, cache: &mut BuildCache, out: &mut HlodMesh) {
    let steps = ((d.size.x.max(d.size.z) / TERRAIN_STEP).round() as usize).clamp(2, 64);
    let layer_colors: Vec<Vec3> = d
        .layers
        .iter()
        .map(|l| {
            l.diffuse
                .as_deref()
                .map_or(Vec3::splat(0.4), |t| cache.texture_mean(t))
        })
        .collect();
    // Tree count per grid vertex (darkens the ground under forests).
    let mut trees = vec![0u32; (steps + 1) * (steps + 1)];
    for t in &d.trees.instances {
        let gx = (t.position.x * steps as f32).round() as usize;
        let gz = (t.position.y * steps as f32).round() as usize;
        if let Some(n) = trees.get_mut(gz.min(steps) * (steps + 1) + gx.min(steps)) {
            *n += 1;
        }
    }
    let foliage = Vec3::new(0.035, 0.07, 0.025);
    let base = out.positions.len() as u32;
    for iz in 0..=steps {
        for ix in 0..=steps {
            let x = ix as f32 / steps as f32 * d.size.x;
            let z = iz as f32 / steps as f32 * d.size.z;
            let y = d.sample_height(x, z);
            // Normal from the surface around the vertex.
            let e = TERRAIN_STEP * 0.5;
            let dx = d.sample_height(x + e, z) - d.sample_height(x - e, z);
            let dz = d.sample_height(x, z + e) - d.sample_height(x, z - e);
            let n = Vec3::new(-dx, 2.0 * e, -dz).normalize();
            let w = d.weights_at(x, z);
            let mut c = Vec3::ZERO;
            let mut total = 0.0;
            for (i, lc) in layer_colors.iter().enumerate() {
                c += *lc * w[i];
                total += w[i];
            }
            let mut c = if total > 0.0 {
                c / total
            } else {
                Vec3::splat(0.4)
            };
            let forest = (trees[iz * (steps + 1) + ix] as f32 / 4.0).min(0.8);
            c = c.lerp(foliage, forest);
            out.push(corner + Vec3::new(x, y - TERRAIN_SINK, z), n, to_u8(c));
        }
    }
    let row = steps as u32 + 1;
    for iz in 0..steps as u32 {
        for ix in 0..steps as u32 {
            let i00 = base + iz * row + ix;
            let (i10, i01, i11) = (i00 + 1, i00 + row, i00 + row + 1);
            // Same winding as the terrain (left-handed, up-facing).
            out.indices.extend([i00, i01, i10, i01, i11, i10]);
        }
    }
    // Tree crowns.
    for t in &d.trees.instances {
        let Some(proto) = d.trees.prototypes.get(t.prototype as usize) else {
            continue;
        };
        let (center, radii) = cache
            .tree(&proto.tree)
            .unwrap_or((Vec3::new(0.0, 8.0, 0.0), Vec2::new(3.0, 4.0)));
        let foot = Vec3::new(t.position.x * d.size.x, 0.0, t.position.y * d.size.z);
        let foot = corner + foot + Vec3::Y * d.sample_height(foot.x, foot.z);
        let c = foot
            + Vec3::new(
                center.x * t.width_scale,
                center.y * t.height_scale,
                center.z * t.width_scale,
            );
        out.crowns.push(Crown {
            center: c,
            radius: (radii.x * t.width_scale).min(25.5),
            height: (radii.y * t.height_scale).min(25.5),
            yaw: t.rotation,
            tint: t.color,
        });
    }
}

/// The vertex-colour material proxies draw with (`<folder>/HLOD/HLOD.mat`).
pub fn material_path(wp: &WorldPartition) -> String {
    format!("{}/HLOD/HLOD.mat", wp.folder)
}

const SHADER: &str =
    "// HLOD: World Partition proxies. The merged objects' colours are in the vertex colours.
// @shader surface
// @property _Smoothness range(0, 1) = 0.08

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    o.albedo = input.vertex_color.rgb;
    o.smoothness = material._Smoothness;
    return o;
}
";

/// What [`build_all`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildReport {
    pub cells: usize,
    pub triangles: usize,
    pub bytes: usize,
    pub warnings: Vec<String>,
}

/// Build HLODs: a proxy for every cell of the partition on `partition` (cell files read from
/// `root`), written under `<folder>/HLOD`, with the vertex-colour material if it is missing.
pub fn build_all(scene: &mut Scene, partition: Entity, root: &Path) -> Result<BuildReport, String> {
    let mut wp = scene
        .world
        .get::<&WorldPartition>(partition)
        .map(|w| (*w).clone())
        .map_err(|_| "the object has no World Partition".to_string())?;
    let dir = root.join(&wp.folder).join("HLOD");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let shader = dir.join("HLOD.shader");
    if !shader.exists() {
        std::fs::write(&shader, SHADER).map_err(|e| e.to_string())?;
    }
    let mat = root.join(material_path(&wp));
    if !mat.exists() {
        let m = crate::assets::Material {
            shader: format!("{}/HLOD/HLOD.shader", wp.folder),
            ..Default::default()
        };
        m.save(&mat)?;
    }
    let mut cache = BuildCache::new(root);
    let mut report = BuildReport::default();
    for i in 0..wp.cells.len() {
        let c = wp.cells[i].coord();
        let text = match std::fs::read_to_string(root.join(wp.cell_path(c))) {
            Ok(t) => t,
            Err(e) => {
                report
                    .warnings
                    .push(format!("{}: {e}", super::cell_name(c)));
                continue;
            }
        };
        let (cell_scene, _) = Scene::from_json("Cell", &text)?;
        let roots = cell_scene.roots().to_vec();
        let proxy = build_proxy(&cell_scene, &roots, &mut cache);
        let path = root.join(wp.hlod_path(c));
        if proxy.is_empty() {
            let _ = std::fs::remove_file(&path);
            wp.cells[i].hlod = false;
            continue;
        }
        let text = proxy.to_file_string(c);
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        forget(&path);
        report.cells += 1;
        report.triangles += proxy.triangles();
        report.bytes += text.len();
        wp.cells[i].hlod = true;
    }
    if let Ok(mut w) = scene.world.get::<&mut WorldPartition>(partition) {
        *w = wp;
    }
    Ok(report)
}

// ---- run time --------------------------------------------------------------------------

enum Slot {
    Loading,
    Ready(String),
    Failed,
}

static PROXIES: LazyLock<Mutex<HashMap<PathBuf, Slot>>> = LazyLock::new(Default::default);

fn proxies() -> std::sync::MutexGuard<'static, HashMap<PathBuf, Slot>> {
    PROXIES.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runtime mesh key of the proxy in `file`, once it is loaded (the first call starts reading it
/// on the background pool).
pub fn proxy(file: &Path) -> Option<String> {
    let mut map = proxies();
    match map.get(file) {
        Some(Slot::Ready(key)) => return Some(key.clone()),
        Some(_) => return None,
        None => {}
    }
    map.insert(file.to_path_buf(), Slot::Loading);
    drop(map);
    let file = file.to_path_buf();
    crate::background::pool().spawn(move || {
        let loaded = crate::vfs::read_to_string(&file)
            .map_err(|e| e.to_string())
            .and_then(|t| HlodMesh::from_file_str(&t));
        let slot = match loaded {
            Ok(mesh) => {
                let name = format!("hlod/{}", file.to_string_lossy().replace('\\', "/"));
                // A new key: no GPU cache is invalidated.
                Slot::Ready(crate::models::runtime::register(&name, mesh.to_mesh_data()))
            }
            Err(_) => Slot::Failed,
        };
        proxies().insert(file, slot);
    });
    None
}

/// Forget a proxy (rebuilt): the next draw reads the file again.
pub fn forget(file: &Path) {
    if let Some(Slot::Ready(key)) = proxies().remove(file) {
        crate::models::runtime::unregister_keys(&[key]);
    }
}

/// Is the proxy of `file` loaded (tests, loading screens)?
pub fn is_ready(file: &Path) -> bool {
    matches!(proxies().get(file), Some(Slot::Ready(_)))
}

/// Cells whose proxy is drawn now: they have one and are not fully loaded (within the
/// partition's HLOD distance of `eye`, when it has one).
pub fn shown_cells(scene: &Scene, wp: &WorldPartition, eye: Option<Vec3>) -> Vec<CellCoord> {
    wp.cells
        .iter()
        .filter(|c| c.hlod)
        .map(|c| c.coord())
        .filter(|c| !super::cell_loaded(scene, wp, *c))
        .filter(|c| match (eye, wp.hlod_distance > 0.0) {
            (Some(p), true) => wp.distance_to_cell(p, *c) <= wp.hlod_distance,
            _ => true,
        })
        .collect()
}

/// Proxy draws of the scene's unloaded cells (added to every view's mesh list).
pub fn draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    keep: impl Fn(Entity) -> bool,
    out: &mut Vec<MeshDraw>,
) {
    let Some((e, wp)) = super::find(scene) else {
        return;
    };
    if !keep(e) || !wp.cells.iter().any(|c| c.hlod) {
        return;
    }
    let eye = super::main_camera(scene).map(|c| scene.world_position(c));
    let material: MaterialParams = materials.params(Some(&material_path(&wp)));
    for c in shown_cells(scene, &wp, eye) {
        let file = crate::assets::absolute(&wp.hlod_path(c));
        let Some(key) = proxy(&file) else {
            continue;
        };
        out.push(MeshDraw {
            mesh: MeshRef::Asset(key),
            model: Mat4::IDENTITY,
            material: material.clone(),
            cast_shadows: true,
            receive_shadows: true,
            outline: false,
            sorting: Default::default(),
            lod: None,
        });
    }
}
