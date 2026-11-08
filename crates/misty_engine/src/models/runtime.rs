//! Meshes made at runtime (static batches) living in the model cache next to imported files, so
//! every consumer finds them through an ordinary `MeshRef::Asset` key.

use std::sync::{Arc, OnceLock};

use super::{Entry, Model, ModelMesh, library};
use crate::render::mesh::MeshData;

/// Key prefix of runtime meshes (never a file on disk).
pub const PREFIX: &str = "runtime://";

/// Store `data` under `name` and return its mesh key ("runtime://name#0"). Replacing or adding
/// meshes bumps the cache generation so GPU copies are rebuilt.
pub fn register(name: &str, data: MeshData) -> String {
    register_meshes(name, vec![data])
}

/// Like [`register`], drawn as Misty Virtual Geometry when the Quality level has it on.
pub fn register_virtual(name: &str, data: MeshData) -> String {
    store(name, vec![data], true)
}

/// A virtual mesh of several parts (one per material, like a glTF mesh's primitives), built
/// as one cluster hierarchy: part `i` is "runtime://name#i"; a Mesh Renderer drawing part `i`
/// with element `i`'s material draws that part's clusters.
pub fn register_virtual_parts(name: &str, parts: Vec<MeshData>) -> String {
    store_parts(name, parts, true, true)
}

/// Drop runtime meshes whose name starts with `prefix`.
pub fn unregister(prefix: &str) {
    let full = format!("{PREFIX}{prefix}");
    let mut lib = library();
    let before = lib.entries.len();
    lib.entries.retain(|k, _| !k.starts_with(&full));
    if lib.entries.len() != before {
        lib.generation += 1;
    }
}

/// Store several meshes under `name` (mesh `i` is "runtime://name#i"); returns the key of mesh 0.
pub fn register_meshes(name: &str, meshes: Vec<MeshData>) -> String {
    store(name, meshes, false)
}

fn store(name: &str, meshes: Vec<MeshData>, virtual_geometry: bool) -> String {
    store_parts(name, meshes, virtual_geometry, false)
}

/// [`store`]; `parts` = the meshes are the primitives of one mesh (one build).
fn store_parts(name: &str, meshes: Vec<MeshData>, virtual_geometry: bool, parts: bool) -> String {
    let n = meshes.len();
    let path = format!("{PREFIX}{name}");
    let mut model = Model {
        meshes: meshes
            .into_iter()
            .enumerate()
            .map(|(i, data)| ModelMesh {
                name: if i == 0 {
                    name.to_owned()
                } else {
                    format!("{name}#{i}")
                },
                bounds: data.bounds(),
                data,
                material: None,
                edges: OnceLock::new(),
                skin: None,
                blend_shapes: Vec::new(),
                default_weights: Vec::new(),
                primitive: (parts && n > 1).then_some((0, i as u32)),
                file_normals: true,
                file_tangents: true,
            })
            .collect(),
        virtual_geometry,
        ..Default::default()
    };
    if virtual_geometry {
        model.virtual_builds = super::virtual_build::now(&model);
    }
    let mut lib = library();
    let replaced = lib.entries.insert(
        path.clone(),
        Entry {
            modified: None,
            model: Ok(Arc::new(model)),
        },
    );
    // A brand-new key can't be in any GPU cache yet (a lookup before registering leaves an
    // error entry, which counts as replaced), so only replacements invalidate them.
    if replaced.is_some() {
        lib.generation += 1;
    }
    format!("{path}#0")
}

/// Is a runtime mesh registered under exactly `name`?
pub fn contains(name: &str) -> bool {
    library()
        .entries
        .get(&format!("{PREFIX}{name}"))
        .is_some_and(|e| e.model.is_ok())
}

/// Drop the runtime mesh registered under exactly `name`.
pub fn unregister_exact(name: &str) {
    let mut lib = library();
    if lib.entries.remove(&format!("{PREFIX}{name}")).is_some() {
        lib.generation += 1;
    }
}

/// A runtime mesh key ("runtime://name#0") is registered and loaded.
pub fn is_registered(key: &str) -> bool {
    let path = key.rsplit_once('#').map_or(key, |(p, _)| p);
    library().entries.get(path).is_some_and(|e| e.model.is_ok())
}

/// Drop the runtime meshes with these keys ("runtime://name#0"), bumping the generation once.
pub fn unregister_keys(keys: &[String]) {
    let mut lib = library();
    let before = lib.entries.len();
    for key in keys {
        let path = key.rsplit_once('#').map_or(key.as_str(), |(p, _)| p);
        lib.entries.remove(path);
    }
    if lib.entries.len() != before {
        lib.generation += 1;
    }
}

/// [`register`] for meshes that get a new name whenever they change (tilemap chunks): a name
/// not seen before needs no re-upload of the other meshes, so the cache generation only moves
/// when an existing name is replaced.
pub fn register_new(name: &str, data: MeshData) -> String {
    let path = format!("{PREFIX}{name}");
    if library().entries.contains_key(&path) {
        return register(name, data);
    }
    let model = Model {
        meshes: vec![ModelMesh {
            name: name.to_owned(),
            bounds: data.bounds(),
            data,
            material: None,
            edges: OnceLock::new(),
            skin: None,
            blend_shapes: Vec::new(),
            default_weights: Vec::new(),
            primitive: None,
            file_normals: true,
            file_tangents: true,
        }],
        ..Default::default()
    };
    library().entries.insert(
        path.clone(),
        Entry {
            modified: None,
            model: Ok(Arc::new(model)),
        },
    );
    format!("{path}#0")
}

/// Drop meshes made by [`register_new`] (keys as it returned them). GPU copies are released in
/// batches: every so many retired meshes the cache generation moves.
pub fn retire(keys: &[String]) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    static RETIRED: AtomicUsize = AtomicUsize::new(0);
    if keys.is_empty() {
        return;
    }
    let mut lib = library();
    for k in keys {
        let path = k.rsplit_once('#').map_or(k.as_str(), |(p, _)| p);
        lib.entries.remove(path);
    }
    if RETIRED.fetch_add(keys.len(), Ordering::Relaxed) + keys.len() >= 256 {
        RETIRED.store(0, Ordering::Relaxed);
        lib.generation += 1;
    }
}
