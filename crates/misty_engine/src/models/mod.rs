//! Imported 3D models (`.gltf`, `.glb`, `.obj`) and the process-wide cache that the renderer,
//! physics and editor read meshes from.
//!
//! Files are right-handed (+Z towards the viewer); the engine is left-handed, so import mirrors X
//! (positions, normals, node translations and rotations) and flips triangle winding.

mod extract;
mod gltf_anim;
mod gltf_import;
mod obj_import;
pub mod runtime;
pub mod skin;

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
};
pub use skin::{BlendShape, MeshSkin, ModelAnimation, ModelSkin};

/// Model file extensions.
pub const EXTENSIONS: [&str; 3] = ["gltf", "glb", "obj"];

pub fn is_model(path: &str) -> bool {
    assets::has_extension(path, &EXTENSIONS)
}

/// One mesh of a model (a glTF primitive or an OBJ object), already in engine space.
pub struct ModelMesh {
    pub name: String,
    pub data: MeshData,
    /// Index into [`Model::materials`].
    pub material: Option<usize>,
    pub bounds: (Vec3, Vec3),
    edges: OnceLock<Vec<[Vec3; 2]>>,
    /// Bone influences (skinned glTF meshes).
    pub skin: Option<MeshSkin>,
    /// Morph targets; the file's default weights (0–100) alongside.
    pub blend_shapes: Vec<BlendShape>,
    pub default_weights: Vec<f32>,
}

impl ModelMesh {
    fn new(name: String, mut data: MeshData, material: Option<usize>) -> Self {
        if data.vertices.iter().all(|v| v.normal == [0.0; 3]) {
            data.compute_normals();
        }
        data.compute_tangents();
        Self {
            name,
            bounds: data.bounds(),
            data,
            material,
            edges: OnceLock::new(),
            skin: None,
            blend_shapes: Vec::new(),
            default_weights: Vec::new(),
        }
    }

    pub fn edges(&self) -> &[[Vec3; 2]] {
        self.edges.get_or_init(|| self.data.edges())
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
}

/// Where a material's texture comes from.
#[derive(Clone, Debug)]
pub enum TextureSource {
    /// An image file next to the model (absolute path).
    File(std::path::PathBuf),
    /// Image bytes stored inside the model (GLB buffer views).
    Embedded(Vec<u8>),
}

pub struct ModelMaterial {
    pub name: String,
    pub material: Material,
    pub albedo: Option<TextureSource>,
    pub normal: Option<TextureSource>,
}

#[derive(Default)]
pub struct Model {
    pub meshes: Vec<ModelMesh>,
    /// Parents come before their children.
    pub nodes: Vec<ModelNode>,
    pub materials: Vec<ModelMaterial>,
    pub skins: Vec<ModelSkin>,
    pub animations: Vec<ModelAnimation>,
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
}

/// Read a model file.
pub fn load(path: &Path) -> Result<Model, String> {
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_ascii_lowercase();
    match ext.as_str() {
        "gltf" | "glb" => gltf_import::load(path),
        "obj" => obj_import::load(path),
        _ => Err(format!("{}: not a model file", path.display())),
    }
}

/// Right-handed file space → engine space: mirror X and keep triangles facing outward.
fn mirror_x(data: &mut MeshData) {
    for v in &mut data.vertices {
        v.position[0] = -v.position[0];
        v.normal[0] = -v.normal[0];
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

fn mtime(path: &Path) -> Option<SystemTime> {
    crate::vfs::modified(path)
}

/// The model at a project-relative path (read once, then cached until the file changes).
pub fn model(rel: &str) -> Result<Arc<Model>, String> {
    if let Some(e) = library().entries.get(rel) {
        return e.model.clone();
    }
    let abs = assets::absolute(rel);
    let entry = Entry {
        modified: mtime(&abs),
        model: load(&abs).map(Arc::new),
    };
    let result = entry.model.clone();
    library().entries.insert(rel.to_owned(), entry);
    result
}

/// Forget models whose files changed (checked at most twice a second). Returns true if any did.
pub fn poll_changes() -> bool {
    let mut lib = library();
    let now = Instant::now();
    if lib
        .last_poll
        .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
    {
        return false;
    }
    lib.last_poll = Some(now);
    let before = lib.entries.len();
    lib.entries
        .retain(|rel, e| e.modified == mtime(&assets::absolute(rel)));
    let changed = lib.entries.len() != before;
    if changed {
        lib.generation += 1;
    }
    changed
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
