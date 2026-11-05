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
    let path = format!("{PREFIX}{name}");
    let model = Model {
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
            })
            .collect(),
        ..Default::default()
    };
    let mut lib = library();
    lib.entries.insert(
        path.clone(),
        Entry {
            modified: None,
            model: Ok(Arc::new(model)),
        },
    );
    lib.generation += 1;
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
