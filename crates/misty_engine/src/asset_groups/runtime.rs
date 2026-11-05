//! Loading addressables while the game runs: handles, bundle reference counts and unloading.
//!
//! [`AssetSystem::load`] turns an address or label into asset paths and the bundles that hold
//! them (dependencies first) and returns a handle id at once; [`AssetSystem::update`] (once a
//! frame) finishes loads: local bundles are mounted into the asset file system, remote ones are
//! downloaded on a background thread, hash-checked and cached first. A bundle stays mounted
//! while any handle needs it; [`AssetSystem::release`] drops a handle and unmounts the bundles
//! nothing needs any more.

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, TryRecvError, channel};

use super::catalog::{CATALOG_FILE, Catalog, PLAYER_DIR};
use super::settings::{AssetGroupSettings, PlayModeScript};

/// Where loads come from.
#[derive(Clone, Debug)]
pub enum Provider {
    /// Project files, addresses from the Asset Groups settings (no bundles).
    Database(AssetGroupSettings),
    /// Project files, bundles tracked from a planned catalog (nothing mounted).
    Simulated(Catalog),
    /// Bundle files: local ones in `local_dir`, remote ones downloaded into `cache_dir`.
    Bundles {
        catalog: Catalog,
        local_dir: PathBuf,
        cache_dir: PathBuf,
    },
    /// Nothing to load from (every load fails with `reason`).
    Unavailable(String),
}

/// A handle's progress.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    Loading,
    Succeeded,
    Failed,
}

impl Status {
    pub fn name(self) -> &'static str {
        match self {
            Status::Loading => "Loading",
            Status::Succeeded => "Succeeded",
            Status::Failed => "Failed",
        }
    }
}

/// What a handle was asked for.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum HandleKind {
    Load,
    /// Instantiate the (first) prefab under `parent` once loaded.
    Instantiate {
        parent: Option<hecs::Entity>,
    },
}

/// One load.
#[derive(Clone, Debug)]
pub struct Handle {
    pub key: String,
    pub kind: HandleKind,
    pub status: Status,
    /// Asset paths of the matching entries.
    pub paths: Vec<String>,
    /// Bundles this handle holds a reference to (load order).
    pub bundles: Vec<String>,
    pub error: Option<String>,
    /// The object an Instantiate handle created.
    pub instance: Option<hecs::Entity>,
}

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
enum BundleState {
    Queued,
    Downloading,
    Loaded,
    Failed,
}

struct LoadedBundle {
    refs: u32,
    state: BundleState,
    mount: Option<u64>,
    error: Option<String>,
    download: Option<Receiver<Result<PathBuf, String>>>,
    /// A finished download waiting for its dependencies to load.
    downloaded: Option<PathBuf>,
}

/// Counters shown by the demo and the tests.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct AssetStats {
    /// Handles that finished loading successfully.
    pub loaded: u64,
    /// Handles released.
    pub released: u64,
    /// Handles currently alive.
    pub live_handles: usize,
    /// Bundles currently loaded.
    pub live_bundles: usize,
    pub bundles_loaded: u64,
    pub bundles_unloaded: u64,
}

pub struct AssetSystem {
    root: PathBuf,
    provider: Provider,
    handles: BTreeMap<u64, Handle>,
    next: u64,
    bundles: HashMap<String, LoadedBundle>,
    stats: AssetStats,
    /// Bundle names in the order they finished loading.
    pub load_log: Vec<String>,
    /// Handles finished by the last [`AssetSystem::update`] (scripts fire callbacks for them).
    finished: Vec<u64>,
}

impl Drop for AssetSystem {
    fn drop(&mut self) {
        for b in self.bundles.values() {
            if let Some(id) = b.mount {
                crate::vfs::unmount_bundle(id);
            }
        }
    }
}

impl AssetSystem {
    pub fn new(root: &Path, provider: Provider) -> Self {
        Self {
            root: root.to_path_buf(),
            provider,
            handles: BTreeMap::new(),
            next: 1,
            bundles: HashMap::new(),
            stats: AssetStats::default(),
            load_log: Vec::new(),
            finished: Vec::new(),
        }
    }

    /// A built game: `<root>/Bundles/catalog.json` when it exists, else the Asset Groups
    /// settings (loose project folders), else nothing.
    pub fn auto(root: &Path) -> Self {
        let dir = root.join(PLAYER_DIR);
        let cat = dir.join(CATALOG_FILE);
        if crate::vfs::is_file(&cat) {
            let provider = match Catalog::load(&cat) {
                Ok(catalog) => Provider::Bundles {
                    catalog,
                    local_dir: dir,
                    cache_dir: super::fetch::default_cache_dir(),
                },
                Err(e) => Provider::Unavailable(e),
            };
            return Self::new(root, provider);
        }
        let provider = match AssetGroupSettings::load(root) {
            Ok(s) => Provider::Database(s),
            Err(e) => Provider::Unavailable(e),
        };
        Self::new(root, provider)
    }

    /// The editor's Play mode, per the "Play Mode Script" setting.
    pub fn for_editor(root: &Path, platform: &str) -> Self {
        let settings = AssetGroupSettings::load(root).unwrap_or_default();
        let provider = match settings.play_mode {
            PlayModeScript::UseAssetDatabase => Provider::Database(settings),
            PlayModeScript::SimulateGroups => {
                Provider::Simulated(super::catalog::plan(root, &settings, platform).catalog)
            }
            PlayModeScript::UseBuiltBundles => {
                let dir = super::build::build_dir(root, platform);
                match Catalog::load(&dir.join(CATALOG_FILE)) {
                    Ok(catalog) => Provider::Bundles {
                        catalog,
                        local_dir: dir,
                        cache_dir: root.join("Library/BundleCache"),
                    },
                    Err(_) => Provider::Unavailable(format!(
                        "Play Mode Script is \"Use Built Bundles\" but there are no bundles for \
                         {platform}: use Build ▸ Asset Bundles in the Asset Groups window"
                    )),
                }
            }
        };
        Self::new(root, provider)
    }

    pub fn provider(&self) -> &Provider {
        &self.provider
    }

    pub fn stats(&self) -> AssetStats {
        AssetStats {
            live_handles: self.handles.len(),
            live_bundles: self.bundles.len(),
            ..self.stats
        }
    }

    pub fn handle(&self, id: u64) -> Option<&Handle> {
        self.handles.get(&id)
    }

    pub fn handle_mut(&mut self, id: u64) -> Option<&mut Handle> {
        self.handles.get_mut(&id)
    }

    /// The handle whose `InstantiateAsync` created `e`.
    pub fn handle_for_instance(&self, e: hecs::Entity) -> Option<u64> {
        self.handles
            .iter()
            .find(|(_, h)| h.instance == Some(e))
            .map(|(id, _)| *id)
    }

    /// Reference count of a bundle (0 when not loaded).
    pub fn bundle_refs(&self, name: &str) -> u32 {
        self.bundles.get(name).map_or(0, |b| b.refs)
    }

    /// Whether bundle `name` is mounted / loaded.
    pub fn bundle_loaded(&self, name: &str) -> bool {
        self.bundles
            .get(name)
            .is_some_and(|b| b.state == BundleState::Loaded)
    }

    /// Paths and bundles (load order) for `key`; `Err` when nothing matches.
    fn locate(&self, key: &str) -> Result<(Vec<String>, Vec<String>), String> {
        let (paths, bundles) = match &self.provider {
            Provider::Database(s) => (s.resolve(key), Vec::new()),
            Provider::Simulated(c) | Provider::Bundles { catalog: c, .. } => {
                let entries = c.resolve(key);
                let mut bundles: Vec<String> = Vec::new();
                for e in &entries {
                    for b in c.load_order(&e.bundle) {
                        if !bundles.contains(&b) {
                            bundles.push(b);
                        }
                    }
                }
                (entries.iter().map(|e| e.path.clone()).collect(), bundles)
            }
            Provider::Unavailable(reason) => return Err(reason.clone()),
        };
        if paths.is_empty() {
            return Err(format!(
                "no addressable asset has the address or label '{key}'"
            ));
        }
        Ok((paths, bundles))
    }

    /// Start loading `key` (an address or a label); returns the handle id.
    pub fn load(&mut self, key: &str, kind: HandleKind) -> u64 {
        let id = self.next;
        self.next += 1;
        let mut handle = Handle {
            key: key.to_owned(),
            kind,
            status: Status::Loading,
            paths: Vec::new(),
            bundles: Vec::new(),
            error: None,
            instance: None,
        };
        match self.locate(key) {
            Ok((paths, bundles)) => {
                for b in &bundles {
                    self.acquire(b);
                }
                handle.paths = paths;
                handle.bundles = bundles;
            }
            // Reported on the next update, like any other result.
            Err(e) => handle.error = Some(e),
        }
        self.handles.insert(id, handle);
        id
    }

    fn acquire(&mut self, name: &str) {
        let b = self
            .bundles
            .entry(name.to_owned())
            .or_insert_with(|| LoadedBundle {
                refs: 0,
                state: BundleState::Queued,
                mount: None,
                error: None,
                download: None,
                downloaded: None,
            });
        b.refs += 1;
    }

    /// Drop handle `id`: its bundles lose a reference and unload when no handle needs them.
    /// Returns the handle (its `instance` is for the caller to destroy).
    pub fn release(&mut self, id: u64) -> Option<Handle> {
        let h = self.handles.remove(&id)?;
        for name in &h.bundles {
            let gone = match self.bundles.get_mut(name) {
                Some(b) => {
                    b.refs = b.refs.saturating_sub(1);
                    b.refs == 0
                }
                None => false,
            };
            if gone && let Some(b) = self.bundles.remove(name) {
                if let Some(m) = b.mount {
                    crate::vfs::unmount_bundle(m);
                }
                if b.state == BundleState::Loaded {
                    self.stats.bundles_unloaded += 1;
                }
            }
        }
        self.stats.released += 1;
        Some(h)
    }

    /// Release every handle (leaving Play mode).
    pub fn release_all(&mut self) -> Vec<Handle> {
        let ids: Vec<u64> = self.handles.keys().copied().collect();
        ids.into_iter().filter_map(|id| self.release(id)).collect()
    }

    /// Ids of the handles that finished during the last [`AssetSystem::update`].
    pub fn take_finished(&mut self) -> Vec<u64> {
        std::mem::take(&mut self.finished)
    }

    /// Advance loads: start downloads, mount bundles whose dependencies are loaded, finish
    /// handles. Returns the ids of handles that finished now.
    pub fn update(&mut self) -> Vec<u64> {
        self.pump_bundles();
        let mut done = Vec::new();
        for (id, h) in &mut self.handles {
            if h.status != Status::Loading {
                continue;
            }
            if h.error.is_some() {
                h.status = Status::Failed;
                done.push(*id);
                continue;
            }
            let mut ready = true;
            for name in &h.bundles {
                match self.bundles.get(name).map(|b| (b.state, b.error.clone())) {
                    Some((BundleState::Loaded, _)) => {}
                    Some((BundleState::Failed, err)) => {
                        h.error = Some(err.unwrap_or_else(|| format!("{name} failed to load")));
                        h.status = Status::Failed;
                        ready = false;
                        break;
                    }
                    _ => ready = false,
                }
            }
            if h.status == Status::Failed {
                done.push(*id);
            } else if ready {
                h.status = Status::Succeeded;
                self.stats.loaded += 1;
                done.push(*id);
            }
        }
        self.finished.extend(done.iter().copied());
        done
    }

    /// Mount or download bundles, dependencies first.
    fn pump_bundles(&mut self) {
        // A few passes so a dependency mounted this frame unblocks its dependents.
        for _ in 0..8 {
            let mut progressed = false;
            let names: Vec<String> = self.bundles.keys().cloned().collect();
            for name in names {
                progressed |= self.pump_bundle(&name);
            }
            if !progressed {
                break;
            }
        }
    }

    fn deps_loaded(&self, name: &str) -> Result<bool, String> {
        let catalog = match &self.provider {
            Provider::Simulated(c) | Provider::Bundles { catalog: c, .. } => c,
            _ => return Ok(true),
        };
        let Some(b) = catalog.bundle(name) else {
            return Ok(true);
        };
        for d in &b.dependencies {
            match self.bundles.get(d).map(|x| x.state) {
                Some(BundleState::Loaded) => {}
                Some(BundleState::Failed) => return Err(format!("{name}: dependency {d} failed")),
                // Not acquired (a cycle) or still loading.
                None => {}
                _ => return Ok(false),
            }
        }
        Ok(true)
    }

    fn pump_bundle(&mut self, name: &str) -> bool {
        let state = match self.bundles.get(name) {
            Some(b) => b.state,
            None => return false,
        };
        match state {
            BundleState::Loaded | BundleState::Failed => false,
            BundleState::Downloading => {
                let result = match self.bundles.get_mut(name).and_then(|b| b.download.as_ref()) {
                    Some(rx) => match rx.try_recv() {
                        Ok(r) => r,
                        Err(TryRecvError::Empty) => return false,
                        Err(TryRecvError::Disconnected) => Err(format!("{name}: download stopped")),
                    },
                    None => Err(format!("{name}: download lost")),
                };
                match result {
                    Ok(path) => {
                        // Downloaded; mounted once its dependencies are in.
                        if let Some(b) = self.bundles.get_mut(name) {
                            b.download = None;
                            b.downloaded = Some(path);
                            b.state = BundleState::Queued;
                        }
                    }
                    Err(e) => self.fail_bundle(name, e),
                }
                true
            }
            BundleState::Queued => {
                let info = match &self.provider {
                    Provider::Bundles { catalog, .. } => catalog.bundle(name).cloned(),
                    _ => None,
                };
                let downloaded = self.bundles.get(name).and_then(|b| b.downloaded.clone());
                // Remote bundles start downloading right away, whatever their dependencies do.
                if let (Some(info), None) = (&info, &downloaded)
                    && info.remote
                {
                    self.start_download(name, info.clone());
                    return true;
                }
                match self.deps_loaded(name) {
                    Ok(true) => {}
                    Ok(false) => return false,
                    Err(e) => {
                        self.fail_bundle(name, e);
                        return true;
                    }
                }
                match (&self.provider, info, downloaded) {
                    (_, _, Some(path)) => self.finish_bundle(name, Some(path)),
                    (Provider::Bundles { local_dir, .. }, Some(info), None) => {
                        let path = local_dir.join(&info.name);
                        self.finish_bundle(name, Some(path));
                    }
                    (Provider::Bundles { .. }, None, None) => {
                        self.fail_bundle(name, format!("{name} is not in the catalog"));
                    }
                    _ => self.finish_bundle(name, None),
                }
                true
            }
        }
    }

    fn start_download(&mut self, name: &str, info: super::catalog::CatalogBundle) {
        let Provider::Bundles { cache_dir, .. } = &self.provider else {
            return;
        };
        let url = format!("{}/{}", info.url, info.name);
        let cache = cache_dir.clone();
        let (tx, rx) = channel();
        let spawned = std::thread::Builder::new()
            .name(format!("bundle {name}"))
            .spawn(move || {
                let r = super::fetch::download_verified(&url, &info.name, &info.hash, &cache);
                let _ = tx.send(r);
            });
        match spawned {
            Ok(_) => {
                if let Some(b) = self.bundles.get_mut(name) {
                    b.state = BundleState::Downloading;
                    b.download = Some(rx);
                }
            }
            Err(e) => self.fail_bundle(name, e.to_string()),
        }
    }

    /// Mount the bundle file at `path` (nothing in simulation) and mark it loaded.
    fn finish_bundle(&mut self, name: &str, path: Option<PathBuf>) {
        let mount = match path {
            Some(p) => match crate::pak::Pak::open(&p) {
                Ok(pak) => Some(crate::vfs::mount_bundle(&self.root, pak)),
                Err(e) => {
                    self.fail_bundle(name, format!("{}: {e}", p.display()));
                    return;
                }
            },
            None => None,
        };
        if let Some(b) = self.bundles.get_mut(name) {
            b.state = BundleState::Loaded;
            b.mount = mount;
        }
        self.stats.bundles_loaded += 1;
        self.load_log.push(name.to_owned());
    }

    fn fail_bundle(&mut self, name: &str, error: String) {
        if let Some(b) = self.bundles.get_mut(name) {
            b.state = BundleState::Failed;
            b.error = Some(error);
        }
    }
}
