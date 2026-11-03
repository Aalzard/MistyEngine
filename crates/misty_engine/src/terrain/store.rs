//! Process-wide store of loaded terrain data, keyed by asset path. The renderer, physics, scripts
//! and the editor's brushes all read (and the editor writes) the same copy; each change bumps a
//! version so the renderer knows when to rebuild its mesh.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock, Mutex};

use super::data::TerrainData;
use crate::assets;

struct Entry {
    data: Result<Arc<TerrainData>, String>,
    version: u64,
}

#[derive(Default)]
struct Store {
    entries: HashMap<PathBuf, Entry>,
    next_version: u64,
}

impl Store {
    fn bump(&mut self) -> u64 {
        self.next_version += 1;
        self.next_version
    }

    /// The entry for `path`, loading the file on first use.
    fn entry(&mut self, path: &str) -> &mut Entry {
        let key = assets::absolute(path);
        if !self.entries.contains_key(&key) {
            let version = self.bump();
            let data = TerrainData::load(&key).map(Arc::new);
            self.entries.insert(key.clone(), Entry { data, version });
        }
        self.entries.get_mut(&key).expect("just inserted")
    }
}

static STORE: LazyLock<Mutex<Store>> = LazyLock::new(Default::default);

fn store() -> std::sync::MutexGuard<'static, Store> {
    STORE.lock().unwrap_or_else(|e| e.into_inner())
}

/// The terrain data at `path` and its version (`None` when the file can't be loaded).
pub fn get(path: &str) -> Option<(Arc<TerrainData>, u64)> {
    if path.is_empty() {
        return None;
    }
    let mut s = store();
    let e = s.entry(path);
    e.data.as_ref().ok().map(|d| (d.clone(), e.version))
}

/// Why `path` could not be loaded.
pub fn error(path: &str) -> Option<String> {
    let mut s = store();
    s.entry(path).data.as_ref().err().cloned()
}

/// Replace the data at `path` (a new asset, an undo); not written to disk.
pub fn insert(path: &str, data: TerrainData) {
    let mut s = store();
    let version = s.bump();
    s.entries.insert(
        assets::absolute(path),
        Entry {
            data: Ok(Arc::new(data)),
            version,
        },
    );
}

/// Change the data at `path` in place; `None` when it isn't loaded.
pub fn modify<R>(path: &str, f: impl FnOnce(&mut TerrainData) -> R) -> Option<R> {
    let mut s = store();
    let version = s.bump();
    let e = s.entry(path);
    let data = e.data.as_mut().ok()?;
    e.version = version;
    Some(f(Arc::make_mut(data)))
}

/// Write the data at `path` to its file.
pub fn save(path: &str) -> Result<(), String> {
    let (data, _) = get(path).ok_or_else(|| format!("{path} is not loaded"))?;
    data.save(&assets::absolute(path))
}

/// Forget `path` so the next use reads the file again.
pub fn unload(path: &str) {
    store().entries.remove(&assets::absolute(path));
}
