//! Mesh Builder: meshes modelled in the editor, stored on an object's Editable Mesh component.
//!
//! An [`EditableMesh`] is a face list: shared positions plus polygons (any vertex count, one
//! material index, a smoothing group and auto-UV settings each). It is baked into ordinary
//! render meshes on demand and registered in the runtime mesh cache under a key made from its
//! content, so the Mesh Filter, the renderer, picking, Mesh Colliders and static batching all
//! read it like any other mesh (see [`sync`]).
//!
//! Faces wind like engine triangles: `cross(b - a, c - a)` points out of the surface (clockwise
//! seen from outside in our left-handed world).

mod bake;
pub mod convert;
pub mod export;
pub mod ops;
pub mod shapes;
mod triangulate;

use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::sync::{LazyLock, Mutex};

use glam::{Mat4, Vec2, Vec3};
use serde::{Deserialize, Serialize};

use crate::scene::{MeshFilter, MeshRef, Scene};

pub use bake::bake;
pub use ops::Selection;
pub use triangulate::triangulate;

/// Per-face automatic UVs: the face is projected onto the axis plane closest to it (1 unit per
/// metre), then rotated, tiled and offset.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct FaceUv {
    /// Project world-space positions, so neighbouring faces and objects line up and textures
    /// tile evenly whatever the object's transform. Off: object space, anchored at the face's
    /// lower corner.
    pub world: bool,
    pub offset: Vec2,
    /// Tiling: texture repeats per metre.
    pub scale: Vec2,
    /// Degrees, counter-clockwise.
    pub rotation: f32,
}

impl Default for FaceUv {
    fn default() -> Self {
        Self {
            world: false,
            offset: Vec2::ZERO,
            scale: Vec2::ONE,
            rotation: 0.0,
        }
    }
}

impl FaceUv {
    fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// One polygon of an Editable Mesh.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Face {
    /// Indices into [`EditableMesh::positions`], in winding order (at least three).
    pub verts: Vec<u32>,
    /// Materials element of the Mesh Renderer drawing this face.
    #[serde(skip_serializing_if = "is_zero")]
    pub material: u32,
    /// Faces sharing a non-zero group are shaded smoothly across their shared vertices;
    /// 0 = flat (hard edges).
    #[serde(skip_serializing_if = "is_zero")]
    pub smoothing: u32,
    #[serde(skip_serializing_if = "FaceUv::is_default")]
    pub uv: FaceUv,
}

impl Face {
    pub fn new(verts: Vec<u32>) -> Self {
        Self {
            verts,
            ..Default::default()
        }
    }

    /// Copy of this face's settings with other vertices.
    pub fn with_verts(&self, verts: Vec<u32>) -> Self {
        Self {
            verts,
            ..self.clone()
        }
    }

    /// Directed edges (a → b) in winding order.
    pub fn edges(&self) -> impl Iterator<Item = (u32, u32)> + '_ {
        let n = self.verts.len();
        (0..n).map(move |i| (self.verts[i], self.verts[(i + 1) % n]))
    }
}

/// Undirected edge key (smaller index first).
pub fn edge_key(a: u32, b: u32) -> [u32; 2] {
    if a < b { [a, b] } else { [b, a] }
}

/// The Editable Mesh component: a polygon mesh modelled with the Mesh Builder tools, baked to
/// the object's Mesh Filter (see the module docs).
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EditableMesh {
    pub positions: Vec<Vec3>,
    pub faces: Vec<Face>,
}

impl EditableMesh {
    pub fn new(positions: Vec<Vec3>, faces: Vec<Face>) -> Self {
        Self { positions, faces }
    }

    pub fn position(&self, v: u32) -> Vec3 {
        self.positions[v as usize]
    }

    /// Area-weighted normal of a face (Newell's method), normalised.
    pub fn face_normal(&self, f: usize) -> Vec3 {
        polygon_normal(self.faces[f].verts.iter().map(|v| self.position(*v)))
    }

    /// Average of a face's corners.
    pub fn face_center(&self, f: usize) -> Vec3 {
        let v = &self.faces[f].verts;
        v.iter().map(|i| self.position(*i)).sum::<Vec3>() / v.len().max(1) as f32
    }

    /// Unique undirected edges, sorted.
    pub fn edges(&self) -> Vec<[u32; 2]> {
        let mut out: Vec<[u32; 2]> = self
            .faces
            .iter()
            .flat_map(|f| f.edges().map(|(a, b)| edge_key(a, b)))
            .collect();
        out.sort_unstable();
        out.dedup();
        out
    }

    /// Faces using each undirected edge.
    pub fn edge_faces(&self) -> HashMap<[u32; 2], Vec<usize>> {
        let mut map: HashMap<[u32; 2], Vec<usize>> = HashMap::new();
        for (i, f) in self.faces.iter().enumerate() {
            for (a, b) in f.edges() {
                map.entry(edge_key(a, b)).or_default().push(i);
            }
        }
        map
    }

    /// Faces around each vertex.
    pub fn vertex_faces(&self) -> Vec<Vec<usize>> {
        let mut out = vec![Vec::new(); self.positions.len()];
        for (i, f) in self.faces.iter().enumerate() {
            for v in &f.verts {
                if let Some(list) = out.get_mut(*v as usize)
                    && !list.contains(&i)
                {
                    list.push(i);
                }
            }
        }
        out
    }

    /// Every edge has exactly two faces (a watertight surface).
    pub fn is_closed(&self) -> bool {
        !self.faces.is_empty() && self.edge_faces().values().all(|f| f.len() == 2)
    }

    /// Every edge has one or two faces, and two faces always use it in opposite directions
    /// (consistent winding, so normals agree).
    pub fn is_manifold(&self) -> bool {
        let mut directed = std::collections::HashSet::new();
        for f in &self.faces {
            for e in f.edges() {
                if !directed.insert(e) {
                    return false;
                }
            }
        }
        self.edge_faces().values().all(|f| f.len() <= 2)
    }

    /// Faces are valid: three or more distinct, existing vertices each.
    pub fn is_valid(&self) -> bool {
        let n = self.positions.len() as u32;
        self.faces.iter().all(|f| {
            let mut v = f.verts.clone();
            v.sort_unstable();
            v.dedup();
            v.len() == f.verts.len() && v.len() >= 3 && v.iter().all(|i| *i < n)
        })
    }

    /// Signed volume (positive when faces point outward).
    pub fn volume(&self) -> f32 {
        let mut vol = 0.0;
        for f in 0..self.faces.len() {
            for [a, b, c] in self.triangles_of(f) {
                let (a, b, c) = (self.position(a), self.position(b), self.position(c));
                // cross(b - a, c - a) points outward, so outward faces add volume.
                vol += a.dot(b.cross(c)) / 6.0;
            }
        }
        vol
    }

    /// Triangles (vertex indices) of face `f`, winding kept.
    pub fn triangles_of(&self, f: usize) -> Vec<[u32; 3]> {
        let face = &self.faces[f];
        let pts: Vec<Vec3> = face.verts.iter().map(|v| self.position(*v)).collect();
        triangulate(&pts)
            .into_iter()
            .map(|t| t.map(|k| face.verts[k]))
            .collect()
    }

    /// Local-space bounds (min, max); zero box when empty.
    pub fn bounds(&self) -> (Vec3, Vec3) {
        if self.positions.is_empty() {
            return (Vec3::ZERO, Vec3::ZERO);
        }
        self.positions.iter().fold(
            (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN)),
            |(lo, hi), p| (lo.min(*p), hi.max(*p)),
        )
    }

    /// Highest material index used (+1): the Materials elements the mesh needs.
    pub fn material_count(&self) -> usize {
        self.faces
            .iter()
            .map(|f| f.material as usize + 1)
            .max()
            .unwrap_or(1)
    }

    /// Some face projects world-space UVs (the bake then depends on the object's transform).
    pub fn uses_world_uv(&self) -> bool {
        self.faces.iter().any(|f| f.uv.world)
    }

    /// Drop repeated corners and faces left with fewer than three, then unused vertices.
    /// Returns the old → new vertex index map (`u32::MAX` = removed).
    pub fn compact(&mut self) -> Vec<u32> {
        for f in &mut self.faces {
            f.verts.dedup();
            while f.verts.len() > 1 && f.verts.first() == f.verts.last() {
                f.verts.pop();
            }
        }
        self.faces.retain(|f| {
            let mut v = f.verts.clone();
            v.sort_unstable();
            v.dedup();
            v.len() >= 3 && v.len() == f.verts.len()
        });
        let mut used = vec![false; self.positions.len()];
        for f in &self.faces {
            for v in &f.verts {
                used[*v as usize] = true;
            }
        }
        let mut map = vec![u32::MAX; self.positions.len()];
        let mut kept = Vec::new();
        for (i, p) in self.positions.iter().enumerate() {
            if used[i] {
                map[i] = kept.len() as u32;
                kept.push(*p);
            }
        }
        self.positions = kept;
        for f in &mut self.faces {
            for v in &mut f.verts {
                *v = map[*v as usize];
            }
        }
        map
    }

    /// Hash of everything the bake reads (plus the object's matrix when UVs are world-space).
    pub fn content_hash(&self, world: Option<&Mat4>) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        self.positions.len().hash(&mut h);
        for p in &self.positions {
            p.to_array().map(f32::to_bits).hash(&mut h);
        }
        for f in &self.faces {
            f.verts.hash(&mut h);
            (f.material, f.smoothing, f.uv.world).hash(&mut h);
            [
                f.uv.offset.x,
                f.uv.offset.y,
                f.uv.scale.x,
                f.uv.scale.y,
                f.uv.rotation,
            ]
            .map(f32::to_bits)
            .hash(&mut h);
        }
        if let Some(m) = world {
            m.to_cols_array().map(f32::to_bits).hash(&mut h);
        }
        h.finish()
    }
}

/// Newell normal of a polygon, normalised (+Y when degenerate).
pub fn polygon_normal(points: impl Iterator<Item = Vec3>) -> Vec3 {
    let pts: Vec<Vec3> = points.collect();
    let mut n = Vec3::ZERO;
    for i in 0..pts.len() {
        let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
        n += a.cross(b);
    }
    n.normalize_or(Vec3::Y)
}

// ---- baking into the runtime mesh cache ------------------------------------------------------

/// Runtime mesh name prefix of baked Editable Meshes.
pub const KEY_PREFIX: &str = "editmesh/";

/// Most baked meshes kept in the cache; older ones are dropped (and re-baked if still used).
const CACHE_LIMIT: usize = 4096;

static BAKED: LazyLock<Mutex<VecDeque<String>>> = LazyLock::new(Default::default);

/// Mesh key of an Editable Mesh with this content: "runtime://editmesh/<hash>#0". Mesh 0 holds
/// every face; mesh `k + 1` the faces of Materials element `k`.
pub fn mesh_key(mesh: &EditableMesh, world: Option<&Mat4>) -> String {
    format!(
        "{}{KEY_PREFIX}{:016x}#0",
        crate::models::runtime::PREFIX,
        mesh.content_hash(world)
    )
}

/// The mesh a Materials element draws: element `i` of a baked Editable Mesh draws its faces
/// with material `i`; any other mesh is drawn whole by every element.
pub fn submesh(mesh: &MeshRef, i: usize) -> MeshRef {
    if let MeshRef::Asset(key) = mesh
        && let Some(rest) = key.strip_prefix(crate::models::runtime::PREFIX)
        && rest.starts_with(KEY_PREFIX)
        && let Some((path, _)) = key.rsplit_once('#')
    {
        return MeshRef::Asset(format!("{path}#{}", i + 1));
    }
    mesh.clone()
}

/// Is this a baked Editable Mesh key?
pub fn is_baked_key(mesh: &MeshRef) -> bool {
    matches!(mesh, MeshRef::Asset(k) if k.starts_with(crate::models::runtime::PREFIX)
        && k[crate::models::runtime::PREFIX.len()..].starts_with(KEY_PREFIX))
}

/// Bake (if needed) and register a mesh; returns its key.
pub fn ensure_baked(mesh: &EditableMesh, world: Option<&Mat4>) -> String {
    let key = mesh_key(mesh, world);
    let (path, _) = key.rsplit_once('#').unwrap_or((&key, ""));
    let name = &path[crate::models::runtime::PREFIX.len()..];
    if !crate::models::runtime::contains(name) {
        let meshes = bake(mesh, world);
        crate::models::runtime::register_meshes(name, meshes);
        let mut baked = BAKED.lock().unwrap_or_else(|e| e.into_inner());
        baked.push_back(name.to_owned());
        while baked.len() > CACHE_LIMIT {
            if let Some(old) = baked.pop_front() {
                crate::models::runtime::unregister_exact(&old);
            }
        }
    }
    key
}

/// Bake every Editable Mesh whose content changed and point its Mesh Filter at the result.
/// Cheap when nothing changed (one hash per mesh). Called before drawing and after loading.
pub fn sync(scene: &Scene) {
    let mut q = scene
        .world
        .query::<(hecs::Entity, &EditableMesh, &mut MeshFilter)>();
    for (e, m, f) in q.iter() {
        let world = m.uses_world_uv().then(|| scene.world_matrix(e));
        let key = ensure_baked(m, world.as_ref());
        if !matches!(&f.mesh, Some(MeshRef::Asset(k)) if *k == key) {
            f.mesh = Some(MeshRef::Asset(key));
        }
    }
}

#[cfg(test)]
mod tests;
