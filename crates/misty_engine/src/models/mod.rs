//! Imported 3D models (`.gltf`, `.glb`, `.fbx`, `.obj`) and the process-wide cache that the
//! renderer, physics and editor read meshes from.
//!
//! Files are right-handed (+Z towards the viewer); the engine is left-handed, so import mirrors X
//! (positions, normals, node translations and rotations) and flips triangle winding. FBX files
//! are read with ufbx, converted to glTF's space (right-handed, Y up, metres) and then go
//! through the same steps, so FBX and glTF models behave alike. Import settings
//! ([`ModelImporter`]) are applied afterwards ([`process`]).

mod extract;
mod fbx_import;
mod gltf_anim;
mod gltf_import;
mod gltf_materials;
mod obj_import;
pub mod preview;
pub mod process;
pub mod runtime;
pub mod settings;
pub mod skin;
pub mod virtual_build;

#[cfg(test)]
mod fbx_sample;
#[cfg(test)]
mod fbx_tests;
#[cfg(test)]
pub(crate) mod fbx_write;

use std::collections::HashMap;
use std::path::Path;
use std::sync::{Arc, LazyLock, Mutex, OnceLock};
use std::time::{Instant, SystemTime};

use glam::{Quat, Vec3};

use crate::assets::{self, Material};
use crate::render::mesh::{self, MeshData};
use crate::scene::MeshRef;

pub use extract::{
    animation_asset_path, extract_animations, extract_materials, material_asset_path,
    refresh_animations,
};
pub use gltf_anim::SAMPLE_RATE;
pub use settings::{AxisConversion, ClipSettings, LodSetting, ModelImporter, VectorSource};
pub use skin::{BlendShape, MeshSkin, ModelAnimation, ModelSkin};

/// Model file extensions.
pub const EXTENSIONS: [&str; 4] = ["gltf", "glb", "fbx", "obj"];

pub fn is_model(path: &str) -> bool {
    assets::has_extension(path, &EXTENSIONS)
}

/// Whether the model is an FBX file (the import settings show Convert Units for it).
pub fn is_fbx(path: &str) -> bool {
    assets::has_extension(path, &["fbx"])
}

/// One mesh of a model (a glTF primitive, an FBX mesh's material part or an OBJ object),
/// already in engine space.
pub struct ModelMesh {
    pub name: String,
    pub data: MeshData,
    /// Index into [`Model::materials`].
    pub material: Option<usize>,
    pub bounds: (Vec3, Vec3),
    edges: OnceLock<Vec<[Vec3; 2]>>,
    /// Bone influences (skinned meshes).
    pub skin: Option<MeshSkin>,
    /// Morph targets; the file's default weights (0–100) alongside.
    pub blend_shapes: Vec<BlendShape>,
    pub default_weights: Vec<f32>,
    /// One primitive of a glTF mesh with several (one per material): the index of that mesh's
    /// first primitive in [`Model::meshes`] and this one's slot. Virtual Geometry builds the
    /// primitives as one hierarchy, held by the first ([`virtual_mesh`]).
    pub primitive: Option<(usize, u32)>,
    /// The normals came from the file (not calculated on import).
    pub file_normals: bool,
    /// The tangents came from the file (import setting Tangents: Import keeps them).
    pub file_tangents: bool,
}

impl ModelMesh {
    fn new(name: String, data: MeshData, material: Option<usize>) -> Self {
        Self::with_vectors(name, data, material, false)
    }

    /// A mesh whose vertices hold the file's tangents when `tangents` (else they are calculated).
    fn with_vectors(
        name: String,
        mut data: MeshData,
        material: Option<usize>,
        tangents: bool,
    ) -> Self {
        let file_normals = !data.vertices.iter().all(|v| v.normal == [0.0; 3]);
        if !file_normals {
            data.compute_normals();
        }
        if !tangents {
            data.compute_tangents();
        }
        Self {
            name,
            bounds: data.bounds(),
            data,
            material,
            edges: OnceLock::new(),
            skin: None,
            blend_shapes: Vec::new(),
            default_weights: Vec::new(),
            primitive: None,
            file_normals,
            file_tangents: tangents,
        }
    }

    pub fn edges(&self) -> &[[Vec3; 2]] {
        self.edges.get_or_init(|| self.data.edges())
    }

    /// Recompute the bounds (and forget cached edges) after the vertices changed.
    pub(crate) fn refresh(&mut self) {
        self.bounds = self.data.bounds();
        self.edges = OnceLock::new();
    }
}

/// A node of the model's hierarchy (becomes one GameObject).
pub struct ModelNode {
    pub name: String,
    pub parent: Option<usize>,
    pub position: Vec3,
    pub rotation: Quat,
    pub scale: Vec3,
    /// Meshes drawn by this node (indices into [`Model::meshes`]).
    pub meshes: Vec<usize>,
    /// Skin deforming this node's meshes (index into [`Model::skins`]).
    pub skin: Option<usize>,
    /// Generated LODs (import setting Generate LODs): per level from LOD 1, the simplified
    /// copies of [`Self::meshes`], in the same order.
    pub lods: Vec<Vec<usize>>,
}

/// Where a material's texture comes from.
#[derive(Clone, Debug)]
pub enum TextureSource {
    /// An image file in the project (absolute path).
    File(std::path::PathBuf),
    /// Image bytes stored inside the model (GLB buffer views, FBX embedded media) or read
    /// from a file outside the project; extraction writes them next to the materials.
    Embedded(Vec<u8>),
}

pub struct ModelMaterial {
    pub name: String,
    pub material: Material,
    pub albedo: Option<TextureSource>,
    pub normal: Option<TextureSource>,
    /// Maps of the optional Standard features (glTF extensions), written on extraction.
    pub extra_maps: Vec<(ExtraMap, TextureSource)>,
}

/// A feature map a model's material brings (see [`ModelMaterial::extra_maps`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExtraMap {
    /// `KHR_materials_clearcoat` normal texture → Clear Coat ▸ Normal Map.
    ClearCoatNormal,
}

impl ExtraMap {
    /// Suffix of the extracted texture file ("Model_Paint_ClearCoatNormal.png").
    pub fn label(self) -> &'static str {
        match self {
            Self::ClearCoatNormal => "ClearCoatNormal",
        }
    }

    /// The material field the map goes into.
    pub fn slot(self, m: &mut Material) -> &mut Option<String> {
        match self {
            Self::ClearCoatNormal => &mut m.features.clear_coat.normal_map,
        }
    }
}

/// One animation of the file before clip splitting (a take): its name and length (seconds).
#[derive(Clone, Debug, PartialEq)]
pub struct Take {
    pub name: String,
    pub length: f32,
}

#[derive(Default)]
pub struct Model {
    pub meshes: Vec<ModelMesh>,
    /// Parents come before their children.
    pub nodes: Vec<ModelNode>,
    pub materials: Vec<ModelMaterial>,
    pub skins: Vec<ModelSkin>,
    pub animations: Vec<ModelAnimation>,
    /// The file's animations as read, before the import settings split them into clips.
    pub takes: Vec<Take>,
    /// Generated LODs: each level's LOD Group screen height, LOD 0 first (empty: none).
    pub lod_heights: Vec<f32>,
    /// Import setting Read/Write Enabled.
    pub read_write: bool,
    /// Import setting: project folder the materials are extracted to ("" = next to the model).
    pub material_folder: String,
    /// Import setting Virtual Geometry: its static meshes draw as clusters with per-cluster
    /// level of detail when the Quality level has Virtual Geometry on (render/vgeo).
    pub virtual_geometry: bool,
    /// The meshes' cluster builds (made on a worker, cached in `Library/VirtualGeometry`).
    pub virtual_builds: Arc<virtual_build::VirtualBuilds>,
}

impl Model {
    /// Curve path of node `i` below the model's root object ("Bone0/Bone1").
    pub fn node_path(&self, i: usize) -> String {
        let mut names = Vec::new();
        let mut cur = Some(i);
        while let Some(n) = cur.and_then(|k| self.nodes.get(k)) {
            names.push(n.name.as_str());
            cur = n.parent;
        }
        names.reverse();
        names.join("/")
    }

    /// Give each skinned mesh its skin's bind poses (like a mesh asset's bindposes) and joint
    /// bounds, for meshes the importer has not bound yet.
    pub(crate) fn bind_skins(&mut self) {
        for node in &self.nodes {
            let Some(skin) = node.skin.and_then(|s| self.skins.get(s)) else {
                continue;
            };
            for &m in &node.meshes {
                let mesh = &mut self.meshes[m];
                let pad = mesh
                    .blend_shapes
                    .iter()
                    .map(|s| s.max_offset())
                    .sum::<f32>();
                let positions: Vec<[f32; 3]> =
                    mesh.data.vertices.iter().map(|v| v.position).collect();
                if let Some(s) = mesh.skin.as_mut().filter(|s| s.bind_poses.is_empty()) {
                    s.bind(&skin.inverse_bind, &positions, pad);
                }
            }
        }
    }
}

/// Read a model file with default import settings.
pub fn load(path: &Path) -> Result<Model, String> {
    load_with(path, &ModelImporter::default())
}

/// Read a model file. FBX files use the settings that change how the file is read (Convert
/// Units, Axis Conversion); the rest are applied by [`process::apply`].
pub fn load_with(path: &Path, settings: &ModelImporter) -> Result<Model, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    let mut model = match ext.as_str() {
        "gltf" | "glb" => gltf_import::load(path),
        "fbx" => fbx_import::load(path, settings),
        "obj" => obj_import::load(path),
        _ => Err(format!("{}: not a model file", path.display())),
    }?;
    model.takes = model
        .animations
        .iter()
        .map(|a| Take {
            name: a.name.clone(),
            length: a.clip.length,
        })
        .collect();
    Ok(model)
}

/// Right-handed file space → engine space: mirror X and keep triangles facing outward.
fn mirror_x(data: &mut MeshData) {
    for v in &mut data.vertices {
        v.position[0] = -v.position[0];
        v.normal[0] = -v.normal[0];
        // A mirrored frame flips the bitangent's side.
        v.tangent[0] = -v.tangent[0];
        v.tangent[3] = -v.tangent[3];
    }
    for t in data.indices.as_chunks_mut::<3>().0 {
        t.swap(1, 2);
    }
}

fn mirror_rotation(q: Quat) -> Quat {
    Quat::from_xyzw(q.x, -q.y, -q.z, q.w)
}

// ---- cache ---------------------------------------------------------------------------------

struct Entry {
    modified: Option<SystemTime>,
    model: Result<Arc<Model>, String>,
}

#[derive(Default)]
struct Library {
    entries: HashMap<String, Entry>,
    generation: u64,
    last_poll: Option<Instant>,
}

static LIBRARY: LazyLock<Mutex<Library>> = LazyLock::new(Default::default);

fn library() -> std::sync::MutexGuard<'static, Library> {
    LIBRARY.lock().unwrap_or_else(|e| e.into_inner())
}

/// Newest of the model file and its `.meta` (import settings re-import the model too).
fn mtime(path: &Path) -> Option<SystemTime> {
    mtime_with(path, crate::vfs::modified)
}

/// [`mtime`] through `modified` (the disk, or the file watch's cached stamps).
fn mtime_with(path: &Path, modified: fn(&Path) -> Option<SystemTime>) -> Option<SystemTime> {
    let meta = modified(crate::asset_db::meta::meta_path(path).as_path());
    modified(path).max(meta)
}

/// The model at a project-relative path (read once, then cached until the file changes).
pub fn model(rel: &str) -> Result<Arc<Model>, String> {
    if let Some(e) = library().entries.get(rel) {
        return e.model.clone();
    }
    crate::profile_scope!(format!("Import.Model: {rel}"));
    let abs = assets::absolute(rel);
    let settings = ModelImporter::load(&abs);
    let entry = Entry {
        modified: mtime(&abs),
        model: load_with(&abs, &settings)
            .map(|m| process::apply(m, &settings, rel))
            .map(Arc::new),
    };
    let result = entry.model.clone();
    library().entries.insert(rel.to_owned(), entry);
    result
}

/// Forget models whose files changed (checked at most twice a second). Returns their paths.
pub fn poll_changes() -> Vec<String> {
    let mut lib = library();
    let now = Instant::now();
    if lib
        .last_poll
        .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
    {
        return Vec::new();
    }
    lib.last_poll = Some(now);
    let mut changed = Vec::new();
    lib.entries.retain(|rel, e| {
        let abs = assets::absolute(rel);
        // The background file watch's stamps first; a difference is checked on disk.
        let same = e.modified == mtime_with(&abs, crate::vfs::cached_modified)
            || e.modified == mtime(&abs);
        if !same {
            changed.push(rel.clone());
        }
        same
    });
    if !changed.is_empty() {
        lib.generation += 1;
    }
    changed
}

/// Forget the cached models at `rels` right away (their import settings changed: the next
/// [`model`] call imports them again), unlike [`poll_changes`], which looks at most twice a
/// second.
pub fn forget(rels: &[String]) {
    let mut lib = library();
    let before = lib.entries.len();
    lib.entries.retain(|rel, _| !rels.contains(rel));
    if lib.entries.len() != before {
        lib.generation += 1;
    }
}

/// What the model cache holds on the CPU, per model: (path, mesh bytes, embedded texture bytes)
/// (Memory Profiler).
pub fn cache_memory() -> Vec<(String, u64, u64)> {
    let lib = library();
    let mut out: Vec<(String, u64, u64)> = lib
        .entries
        .iter()
        .filter_map(|(rel, e)| {
            let m = e.model.as_ref().ok()?;
            let meshes = m
                .meshes
                .iter()
                .map(|mesh| {
                    (mesh.data.vertices.capacity()
                        * std::mem::size_of::<crate::render::mesh::Vertex>()
                        + mesh.data.indices.capacity() * 4) as u64
                })
                .sum();
            let textures = m
                .materials
                .iter()
                .flat_map(|mat| {
                    let extra = mat.extra_maps.iter().map(|(_, t)| Some(t));
                    [mat.albedo.as_ref(), mat.normal.as_ref()]
                        .into_iter()
                        .chain(extra)
                })
                .map(|t| match t {
                    Some(TextureSource::Embedded(bytes)) => bytes.len() as u64,
                    _ => 0,
                })
                .sum();
            Some((rel.clone(), meshes, textures))
        })
        .collect();
    out.sort();
    out
}

/// Bumped whenever cached models are dropped (GPU caches compare it to know when to re-upload).
pub fn generation() -> u64 {
    library().generation
}

/// Mesh data of a primitive or an imported mesh.
pub enum MeshHandle {
    Builtin(&'static MeshData),
    Imported(Arc<Model>, usize),
}

impl std::ops::Deref for MeshHandle {
    type Target = MeshData;

    fn deref(&self) -> &MeshData {
        match self {
            Self::Builtin(m) => m,
            Self::Imported(model, i) => &model.meshes[*i].data,
        }
    }
}

/// Mesh data for a Mesh Filter reference (None if the model or index is missing).
pub fn mesh(r: &MeshRef) -> Option<MeshHandle> {
    match r {
        MeshRef::Primitive(p) => Some(MeshHandle::Builtin(mesh::primitive(*p))),
        // A posed Skinned Mesh Renderer: its source mesh (bind pose).
        MeshRef::Asset(key) if crate::skinning::is_pose_key(key) => {
            mesh(&crate::skinning::pose(key)?.mesh)
        }
        MeshRef::Asset(_) => {
            let (path, index) = r.split_asset()?;
            let model = model(path).ok()?;
            (index < model.meshes.len()).then_some(MeshHandle::Imported(model, index))
        }
    }
}

/// Does this mesh draw as Misty Virtual Geometry (its model imported with Virtual Geometry
/// on, or registered with [`runtime::register_virtual`])?
pub fn is_virtual(r: &MeshRef) -> bool {
    match r {
        MeshRef::Asset(key) if crate::skinning::is_pose_key(key) => false,
        MeshRef::Asset(_) => r
            .split_asset()
            .and_then(|(path, _)| model(path).ok())
            .is_some_and(|m| m.virtual_geometry),
        MeshRef::Primitive(_) => false,
    }
}

/// A virtual mesh's cluster build ([`virtual_mesh`]).
pub struct VirtualRef {
    pub mesh: Arc<crate::vgeo::VirtualMesh>,
    /// The mesh holding the build: `r` itself, or the first primitive of its glTF mesh when
    /// the primitives were built together (then [`virtual_vertices`] of it are theirs, joined).
    pub source: MeshRef,
    /// `r`'s part of the build ([`crate::vgeo::Cluster::material`]).
    pub slot: u32,
}

/// The cluster build of a virtual mesh, once it is ready (`None`: not a virtual mesh, or its
/// build is still running on a worker).
pub fn virtual_mesh(r: &MeshRef) -> Option<VirtualRef> {
    if !is_virtual(r) {
        return None;
    }
    let (path, index) = r.split_asset()?;
    let model = model(path).ok()?;
    let (first, slot) = model.meshes.get(index)?.primitive.unwrap_or((index, 0));
    let mesh = model.virtual_builds.get(first)?.mesh.clone();
    Some(VirtualRef {
        mesh,
        source: MeshRef::asset(path, first),
        slot,
    })
}

/// The vertices a build indexes ([`VirtualRef::source`]): the mesh's own, or every primitive's
/// of its glTF mesh, in slot order.
pub fn virtual_vertices(source: &MeshRef) -> Option<Vec<crate::render::mesh::Vertex>> {
    let (path, index) = source.split_asset()?;
    let model = model(path).ok()?;
    let parts = virtual_build::parts(&model, index);
    Some(
        parts
            .iter()
            .flat_map(|&i| model.meshes[i].data.vertices.iter().copied())
            .collect(),
    )
}

/// Local-space bounds (min, max) of a mesh.
pub fn bounds(r: &MeshRef) -> Option<(Vec3, Vec3)> {
    match r {
        MeshRef::Primitive(p) => Some((-p.half_extents(), p.half_extents())),
        MeshRef::Asset(key) if crate::skinning::is_pose_key(key) => {
            Some(crate::skinning::pose(key)?.bounds)
        }
        MeshRef::Asset(_) => match mesh(r)? {
            MeshHandle::Imported(model, i) => Some(model.meshes[i].bounds),
            MeshHandle::Builtin(m) => Some(m.bounds()),
        },
    }
}

/// Call `f` with the mesh's unique edges (wireframe and Mesh Collider gizmos).
pub fn with_edges<R>(r: &MeshRef, f: impl FnOnce(&[[Vec3; 2]]) -> R) -> Option<R> {
    match r {
        MeshRef::Primitive(p) => Some(f(mesh::primitive_edges(*p))),
        MeshRef::Asset(_) => match mesh(r)? {
            MeshHandle::Imported(model, i) => Some(f(model.meshes[i].edges())),
            MeshHandle::Builtin(m) => Some(f(&m.edges())),
        },
    }
}

/// Display name: "Cube", or "Ship.glb ▸ Hull" for imported meshes.
pub fn display_name(r: &MeshRef) -> String {
    match r {
        MeshRef::Primitive(p) => p.name().to_owned(),
        MeshRef::Asset(s) if s.starts_with(runtime::PREFIX) => {
            // Generated meshes: "spline-extrude" rather than a content hash.
            let name = s.trim_start_matches(runtime::PREFIX);
            name.split('/').next().unwrap_or(name).to_owned() + " (generated)"
        }
        MeshRef::Asset(s) => {
            let Some((path, index)) = r.split_asset() else {
                return s.clone();
            };
            let file = Path::new(path)
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
                .unwrap_or_default();
            match model(path) {
                Ok(m) if index < m.meshes.len() => format!("{file} ▸ {}", m.meshes[index].name),
                _ => format!("{file} #{index} (missing)"),
            }
        }
    }
}

#[cfg(test)]
mod tests;
