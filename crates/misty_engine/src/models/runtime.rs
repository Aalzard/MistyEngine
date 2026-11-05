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
            skin: None,
            blend_shapes: Vec::new(),
            default_weights: Vec::new(),
        }],
        ..Default::default()
    };
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
