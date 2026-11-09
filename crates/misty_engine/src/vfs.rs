//! File access for assets: a mounted `.pak` (built games) or the project folder on disk.
//!
//! Asset loaders read through these functions instead of `std::fs`. With nothing mounted they
//! are plain file reads, so the editor and loose-folder development builds work as before.
//! When a pak is mounted, paths inside its project root are looked up in the archive first.
//! Asset bundles loaded at runtime (see [`crate::asset_groups`]) are extra archives mounted on
//! top with [`mount_bundle`]; they are looked up after the main pak, before the disk.

use std::io;
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::SystemTime;

use crate::pak::Pak;

struct Mount {
    root: PathBuf,
    pak: Arc<Pak>,
    /// Modification time reported for every entry (the pak never changes while mounted).
    stamp: SystemTime,
}

static MOUNT: RwLock<Option<Mount>> = RwLock::new(None);

/// Loaded asset bundles: (id, mount).
static BUNDLES: RwLock<Vec<(u64, Mount)>> = RwLock::new(Vec::new());
static NEXT_BUNDLE: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// Serve the files of an asset bundle under `root` until [`unmount_bundle`] with the returned id.
pub fn mount_bundle(root: &Path, pak: Pak) -> u64 {
    let id = NEXT_BUNDLE.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    BUNDLES.write().unwrap_or_else(|e| e.into_inner()).push((
        id,
        Mount {
            root: root.to_path_buf(),
            pak: Arc::new(pak),
            stamp: SystemTime::UNIX_EPOCH,
        },
    ));
    id
}

/// Stop serving a bundle's files.
pub fn unmount_bundle(id: u64) {
    BUNDLES
        .write()
        .unwrap_or_else(|e| e.into_inner())
        .retain(|(i, _)| *i != id);
}

/// Whether a mounted bundle serves `path`.
pub fn in_bundle(path: &Path) -> bool {
    bundle_lookup(path).is_some()
}

fn bundle_lookup(path: &Path) -> Option<(Arc<Pak>, String, SystemTime)> {
    let bundles = BUNDLES.read().unwrap_or_else(|e| e.into_inner());
    bundles.iter().rev().find_map(|(_, m)| {
        let k = key(&m.root, path)?;
        m.pak.contains(&k).then(|| (m.pak.clone(), k, m.stamp))
    })
}

fn mounted() -> std::sync::RwLockReadGuard<'static, Option<Mount>> {
    MOUNT.read().unwrap_or_else(|e| e.into_inner())
}

/// Serve the files under `root` from `pak` (its entries are paths relative to `root`).
pub fn mount(root: &Path, pak: Pak, stamp: SystemTime) {
    *MOUNT.write().unwrap_or_else(|e| e.into_inner()) = Some(Mount {
        root: root.to_path_buf(),
        pak: Arc::new(pak),
        stamp,
    });
}

/// Back to plain file access.
pub fn unmount() {
    *MOUNT.write().unwrap_or_else(|e| e.into_inner()) = None;
}

pub fn is_mounted() -> bool {
    mounted().is_some()
}

/// Archive key of `path`: relative to the mount root, `/`-separated, without `.` parts.
pub fn key(root: &Path, path: &Path) -> Option<String> {
    let rel = if path.is_absolute() || path.has_root() {
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let mut parts: Vec<String> = Vec::new();
    for c in rel.components() {
        match c {
            Component::Normal(s) => parts.push(s.to_string_lossy().into_owned()),
            Component::ParentDir => {
                parts.pop()?;
            }
            Component::CurDir => {}
            _ => return None,
        }
    }
    Some(parts.join("/"))
}

/// The pak and key for `path`, when a mounted pak has it.
fn lookup(path: &Path) -> Option<(Arc<Pak>, String, SystemTime)> {
    let main = {
        let m = mounted();
        m.as_ref().and_then(|m| {
            let k = key(&m.root, path)?;
            m.pak.contains(&k).then(|| (m.pak.clone(), k, m.stamp))
        })
    };
    main.or_else(|| bundle_lookup(path))
}

pub fn read(path: impl AsRef<Path>) -> io::Result<Vec<u8>> {
    let path = path.as_ref();
    match lookup(path) {
        Some((pak, k, _)) => pak.read(&k),
        None => std::fs::read(path),
    }
}

pub fn read_to_string(path: impl AsRef<Path>) -> io::Result<String> {
    let path = path.as_ref();
    match lookup(path) {
        Some((pak, k, _)) => String::from_utf8(pak.read(&k)?)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e)),
        None => std::fs::read_to_string(path),
    }
}

pub fn is_file(path: &Path) -> bool {
    lookup(path).is_some() || path.is_file()
}

/// Modification time (the pak's own for packed files; `None` when missing).
pub fn modified(path: &Path) -> Option<SystemTime> {
    match lookup(path) {
        Some((_, _, stamp)) => Some(stamp),
        None => std::fs::metadata(path).and_then(|m| m.modified()).ok(),
    }
}

/// [`modified`] for change polling: a loose file's time as the background file watch last saw
/// it (see [`crate::file_watch`]; confirm a difference with [`modified`]).
pub fn cached_modified(path: &Path) -> Option<SystemTime> {
    match lookup(path) {
        Some((_, _, stamp)) => Some(stamp),
        None => crate::file_watch::cached_modified(path),
    }
}

/// Every file below `dir` (recursive, sorted), from the pak and the disk.
pub fn files_under(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Some(m) = mounted().as_ref()
        && let Some(prefix) = key(&m.root, dir)
    {
        let prefix = format!("{}/", prefix.trim_end_matches('/'));
        out.extend(
            m.pak
                .paths()
                .filter(|p| p.starts_with(&prefix))
                .map(|p| m.root.join(p)),
        );
    }
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for entry in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                stack.push(p);
            } else {
                out.push(p);
            }
        }
    }
    out.sort();
    out.dedup();
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn keys_are_root_relative_with_forward_slashes() {
        let root = std::env::temp_dir().join("game");
        assert_eq!(
            key(&root, &root.join("Assets").join("a.png")).as_deref(),
            Some("Assets/a.png")
        );
        assert_eq!(
            key(&root, Path::new("Assets/./x/../b.mat")).as_deref(),
            Some("Assets/b.mat")
        );
        assert_eq!(key(&root, &std::env::temp_dir().join("other/a")), None);
    }
}
