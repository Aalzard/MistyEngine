//! Loaded `.navmesh` files, shared by the editor overlay and running games; a file is read
//! again when it changes on disk (after a bake).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, LazyLock, Mutex};
use std::time::SystemTime;

use super::NavMesh;

type Entry = (Option<SystemTime>, Arc<NavMesh>);

static CACHE: LazyLock<Mutex<HashMap<PathBuf, Entry>>> = LazyLock::new(Default::default);

/// The nav mesh at `path` (cached until the file's modification time changes).
pub fn load(path: &Path) -> Result<Arc<NavMesh>, String> {
    let modified =
        crate::vfs::modified(path).ok_or_else(|| format!("{}: file not found", path.display()))?;
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((time, mesh)) = cache.get(path)
        && *time == Some(modified)
    {
        return Ok(mesh.clone());
    }
    let mesh = Arc::new(NavMesh::load(path)?);
    cache.insert(path.to_owned(), (Some(modified), mesh.clone()));
    Ok(mesh)
}

/// Put a freshly baked mesh in the cache (saved to `path` already).
pub fn insert(path: &Path, mesh: Arc<NavMesh>) {
    let modified = std::fs::metadata(path).and_then(|m| m.modified()).ok();
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.insert(path.to_owned(), (modified, mesh));
}

/// Forget a file (Clear).
pub fn forget(path: &Path) {
    let mut cache = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    cache.remove(path);
}
