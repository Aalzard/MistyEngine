//! Name index of everything under `Assets/` for the palette's Project provider.
//!
//! The first build walks the folder on a background thread. After that the index stays fresh
//! incrementally: each frame a few folders have their modified time checked (round robin) and
//! only changed folders are re-listed, so a project with thousands of files never hitches.

use std::collections::BTreeMap;
use std::ops::Bound;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};
use std::time::SystemTime;

use super::fuzzy::{self, Match, Mode};
use super::query::Query;

/// One file or folder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct IndexedAsset {
    /// Project-relative path with `/` ("Assets/Materials/Red.mat").
    pub rel: String,
    /// File name with extension.
    pub name: String,
    /// `t:` type name (see [`crate::project::type_for_path`]).
    pub ty: &'static str,
    pub is_dir: bool,
}

/// A folder's direct children and the folder time they were listed at.
#[derive(Clone, Debug, Default)]
struct Folder {
    modified: Option<SystemTime>,
    items: Vec<IndexedAsset>,
}

type Folders = BTreeMap<PathBuf, Folder>;

pub struct AssetIndex {
    root: PathBuf,
    folders: Folders,
    /// Background build in flight.
    pending: Option<Receiver<Folders>>,
    /// Next folder to check (round robin).
    cursor: Option<PathBuf>,
    /// The index must be rebuilt (Assets ▸ Refresh, project changed).
    stale: bool,
    /// Bumped on every change, so cached results know to re-run.
    pub generation: u64,
}

/// Folder times checked per frame while the palette is open.
pub const CHECKS_PER_FRAME: usize = 24;

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

/// List one folder (hidden files and `.meta` skipped, as in the Project window).
fn list(root: &Path, dir: &Path) -> Folder {
    let modified = mtime(dir);
    let items = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') || name.ends_with(".meta") {
                return None;
            }
            let path = e.path();
            let is_dir = e.file_type().is_ok_and(|t| t.is_dir());
            let rel = path
                .strip_prefix(root)
                .unwrap_or(&path)
                .to_string_lossy()
                .replace('\\', "/");
            Some(IndexedAsset {
                ty: crate::project::type_for_path(&path, is_dir),
                rel,
                name,
                is_dir,
            })
        })
        .collect();
    Folder { modified, items }
}

/// List `dir` and every folder below it into `out`.
fn walk(root: &Path, dir: &Path, out: &mut Folders) {
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let folder = list(root, &d);
        stack.extend(
            folder
                .items
                .iter()
                .filter(|i| i.is_dir)
                .map(|i| root.join(&i.rel)),
        );
        out.insert(d, folder);
    }
}

impl AssetIndex {
    pub fn new(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            folders: Folders::new(),
            pending: None,
            cursor: None,
            stale: true,
            generation: 0,
        }
    }

    /// Build synchronously (tests, screenshots).
    pub fn build_now(&mut self) {
        let mut folders = Folders::new();
        walk(&self.root, &self.root.join("Assets"), &mut folders);
        self.folders = folders;
        self.pending = None;
        self.stale = false;
        self.generation += 1;
    }

    /// Ask for a full rebuild (done in the background on the next [`Self::tick`]).
    pub fn invalidate(&mut self) {
        self.stale = true;
    }

    pub fn is_building(&self) -> bool {
        self.pending.is_some()
    }

    #[cfg(test)]
    pub fn len(&self) -> usize {
        self.folders.values().map(|f| f.items.len()).sum()
    }

    /// Per-frame upkeep while the palette is open: start / collect the background build, then
    /// check up to `checks` folders for changes. Returns true when the index changed.
    pub fn tick(&mut self, checks: usize) -> bool {
        if self.stale && self.pending.is_none() {
            self.stale = false;
            let (tx, rx) = channel();
            let root = self.root.clone();
            std::thread::spawn(move || {
                let mut folders = Folders::new();
                walk(&root, &root.join("Assets"), &mut folders);
                let _ = tx.send(folders);
            });
            self.pending = Some(rx);
        }
        if let Some(rx) = &self.pending {
            return match rx.try_recv() {
                Ok(folders) => {
                    self.folders = folders;
                    self.pending = None;
                    self.generation += 1;
                    true
                }
                Err(std::sync::mpsc::TryRecvError::Empty) => false,
                Err(_) => {
                    self.pending = None;
                    false
                }
            };
        }
        let mut changed = false;
        for _ in 0..checks.min(self.folders.len()) {
            changed |= self.check_next();
        }
        if changed {
            self.generation += 1;
        }
        changed
    }

    /// Re-list the next folder in round-robin order if its time changed.
    fn check_next(&mut self) -> bool {
        let after = match &self.cursor {
            Some(c) => Bound::Excluded(c.clone()),
            None => Bound::Unbounded,
        };
        let next = self
            .folders
            .range((after, Bound::Unbounded))
            .next()
            .or_else(|| self.folders.iter().next())
            .map(|(p, f)| (p.clone(), f.modified));
        let Some((dir, known)) = next else {
            return false;
        };
        self.cursor = Some(dir.clone());
        let now = mtime(&dir);
        if now == known {
            return false;
        }
        if now.is_none() {
            // Folder gone: drop it and everything below it.
            self.folders.retain(|p, _| !p.starts_with(&dir));
            return true;
        }
        let folder = list(&self.root, &dir);
        // New sub-folders are listed now; removed ones are dropped.
        let subdirs: Vec<PathBuf> = folder
            .items
            .iter()
            .filter(|i| i.is_dir)
            .map(|i| self.root.join(&i.rel))
            .collect();
        let old: Vec<PathBuf> = self
            .folders
            .keys()
            .filter(|p| p.parent() == Some(dir.as_path()))
            .cloned()
            .collect();
        for gone in old.iter().filter(|p| !subdirs.contains(p)) {
            self.folders.retain(|p, _| !p.starts_with(gone));
        }
        for new in subdirs.iter().filter(|p| !old.contains(p)) {
            let mut below = Folders::new();
            walk(&self.root, new, &mut below);
            self.folders.extend(below);
        }
        self.folders.insert(dir, folder);
        true
    }

    /// Assets matching the query (fuzzy name words + `t:` types), best first, at most `cap`.
    pub fn search(&self, q: &Query, cap: usize) -> Vec<(&IndexedAsset, Match)> {
        let mut out: Vec<(&IndexedAsset, Match)> = self
            .folders
            .values()
            .flat_map(|f| &f.items)
            .filter(|a| q.types.is_empty() || q.types.iter().any(|t| t == a.ty))
            .filter_map(|a| Some((a, fuzzy::match_words(&q.words, &a.name, Mode::Fuzzy)?)))
            .collect();
        if q.words.is_empty() {
            // Filters only (`t:Material`): alphabetical, like the Project window.
            out.sort_by_cached_key(|(a, _)| (a.name.to_lowercase(), a.rel.clone()));
        } else {
            out.sort_by(|(a, ma), (b, mb)| {
                mb.score
                    .cmp(&ma.score)
                    .then(a.name.len().cmp(&b.name.len()))
                    .then_with(|| a.rel.cmp(&b.rel))
            });
        }
        out.truncate(cap);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_project(tag: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!("misty_search_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Materials")).unwrap();
        std::fs::write(root.join("Assets/Materials/Red.mat"), "{}").unwrap();
        std::fs::write(root.join("Assets/Main.scene"), "{}").unwrap();
        root
    }

    fn names(index: &AssetIndex, text: &str) -> Vec<String> {
        index
            .search(&Query::parse(text), 100)
            .into_iter()
            .map(|(a, _)| a.name.clone())
            .collect()
    }

    /// Run `tick` until the background build lands.
    fn settle(index: &mut AssetIndex) {
        for _ in 0..500 {
            index.tick(CHECKS_PER_FRAME);
            if !index.is_building() && index.len() > 0 {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        panic!("index never built");
    }

    #[test]
    fn background_build_and_types() {
        let root = temp_project("types");
        let mut index = AssetIndex::new(&root);
        settle(&mut index);
        assert_eq!(names(&index, "red"), ["Red.mat"]);
        assert_eq!(names(&index, "t:Material"), ["Red.mat"]);
        assert_eq!(names(&index, "t:Scene"), ["Main.scene"]);
        assert_eq!(names(&index, "t:Folder"), ["Materials"]);
        assert!(names(&index, "red t:Scene").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn incremental_updates() {
        let root = temp_project("incremental");
        let mut index = AssetIndex::new(&root);
        index.build_now();
        let generation = index.generation;
        // Folder times can be coarse; wait so the change is visible.
        std::thread::sleep(std::time::Duration::from_millis(20));
        std::fs::write(root.join("Assets/Materials/Blue.mat"), "{}").unwrap();
        std::fs::create_dir_all(root.join("Assets/Textures")).unwrap();
        std::fs::write(root.join("Assets/Textures/Grass.png"), "").unwrap();
        // A full round of checks picks up both changes.
        for _ in 0..4 {
            index.tick(CHECKS_PER_FRAME);
        }
        assert!(index.generation > generation);
        assert_eq!(names(&index, "blue"), ["Blue.mat"]);
        assert_eq!(names(&index, "t:Texture"), ["Grass.png"]);
        std::fs::remove_dir_all(root.join("Assets/Textures")).unwrap();
        for _ in 0..4 {
            index.tick(CHECKS_PER_FRAME);
        }
        assert!(names(&index, "grass").is_empty());
        let _ = std::fs::remove_dir_all(root);
    }

    #[test]
    fn thousands_of_assets_stay_fast() {
        let root = temp_project("large");
        for d in 0..40 {
            let dir = root.join(format!("Assets/Pack{d}"));
            std::fs::create_dir_all(&dir).unwrap();
            for f in 0..100 {
                std::fs::write(dir.join(format!("Rock_{d}_{f}.mat")), "").unwrap();
            }
        }
        let mut index = AssetIndex::new(&root);
        index.build_now();
        assert!(index.len() > 4000);
        let start = std::time::Instant::now();
        let hits = index.search(&Query::parse("rock 17 5"), 8);
        let query_time = start.elapsed();
        assert_eq!(hits.len(), 8);
        // One frame of upkeep only stats a few folders.
        let start = std::time::Instant::now();
        index.tick(CHECKS_PER_FRAME);
        let tick_time = start.elapsed();
        // Generous bounds for debug builds on slow machines.
        assert!(query_time.as_millis() < 250, "query took {query_time:?}");
        assert!(tick_time.as_millis() < 50, "tick took {tick_time:?}");
        let _ = std::fs::remove_dir_all(root);
    }
}
