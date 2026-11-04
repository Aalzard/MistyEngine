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
    let path = format!("{PREFIX}{name}");
    let model = Model {
        meshes: vec![ModelMesh {
            name: name.to_owned(),
            bounds: data.bounds(),
            data,
            material: None,
            edges: OnceLock::new(),
        }],
        nodes: Vec::new(),
        materials: Vec::new(),
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
