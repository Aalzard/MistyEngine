//! Misty Virtual Geometry builds of a model's meshes (clusters and cluster hierarchy, see
//! [`crate::vgeo`]), made when the model is imported with Virtual Geometry on.
//!
//! A build is cached in `Library/VirtualGeometry/<guid>.vgeo` ([`crate::vgeo::file`]), keyed by
//! each mesh's content hash, and packed into player builds. Missing or stale builds are made
//! on a worker thread (meshes in parallel) and saved; until a mesh's build is ready it draws
//! through the ordinary mesh path, so importing or loading never stalls a frame.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Instant;

use glam::Vec3;

use super::Model;
use crate::render::mesh::MeshData;
use crate::vgeo::{VirtualMesh, file};

/// Folder of the cached builds, relative to the project.
pub const LIBRARY_DIR: &str = "Library/VirtualGeometry";

/// One mesh's build.
#[derive(Clone, Debug)]
pub struct VirtualBuild {
    pub mesh: Arc<VirtualMesh>,
    /// Content hash of the source it was built from.
    pub hash: u64,
    /// Time the build took (when it was made, maybe in an earlier session), in milliseconds.
    pub build_ms: f32,
    /// Read from the `.vgeo` cache instead of built in this session.
    pub cached: bool,
}

/// The builds of a model's meshes (empty slots: not built yet, or not a virtual mesh).
#[derive(Debug, Default)]
pub struct VirtualBuilds {
    slots: Vec<OnceLock<VirtualBuild>>,
    /// Meshes that get a build (non-skinned, with triangles).
    wanted: Vec<bool>,
}

/// What the import Inspector shows.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BuildSummary {
    pub meshes: usize,
    pub built: usize,
    pub clusters: usize,
    /// Most levels of any mesh.
    pub levels: u32,
    pub source_triangles: usize,
    pub build_ms: f32,
    /// Every built mesh came from the cache.
    pub cached: bool,
}

static READY: AtomicU64 = AtomicU64::new(0);

/// Bumped whenever a background build finishes (the renderer then looks again at meshes it
/// drew through the ordinary path).
pub fn ready_generation() -> u64 {
    READY.load(Ordering::Relaxed)
}

impl VirtualBuilds {
    /// Build of mesh `i`, once it is ready.
    pub fn get(&self, i: usize) -> Option<&VirtualBuild> {
        self.slots.get(i)?.get()
    }

    pub fn summary(&self) -> BuildSummary {
        let mut s = BuildSummary {
            meshes: self.wanted.iter().filter(|w| **w).count(),
            cached: true,
            ..Default::default()
        };
        for b in self.slots.iter().filter_map(OnceLock::get) {
            s.built += 1;
            s.clusters += b.mesh.clusters.len();
            s.levels = s.levels.max(b.mesh.levels);
            s.source_triangles += b.mesh.source_triangles;
            s.build_ms += b.build_ms;
            s.cached &= b.cached;
        }
        s
    }

    /// Every wanted mesh is built.
    pub fn complete(&self) -> bool {
        self.slots
            .iter()
            .zip(&self.wanted)
            .all(|(s, w)| !w || s.get().is_some())
    }

    fn new(wanted: Vec<bool>) -> Self {
        Self {
            slots: wanted.iter().map(|_| OnceLock::new()).collect(),
            wanted,
        }
    }
}

/// The source of a build: positions and triangle indices.
struct Source {
    positions: Vec<Vec3>,
    indices: Vec<u32>,
    hash: u64,
}

fn source(data: &MeshData) -> Source {
    let positions: Vec<Vec3> = data.vertices.iter().map(|v| v.position.into()).collect();
    let hash = file::content_hash(&positions, &data.indices);
    Source {
        positions,
        indices: data.indices.clone(),
        hash,
    }
}

fn build(src: &Source) -> VirtualBuild {
    let t = Instant::now();
    let mesh = VirtualMesh::build(&src.positions, &src.indices);
    VirtualBuild {
        mesh: Arc::new(mesh),
        hash: src.hash,
        build_ms: t.elapsed().as_secs_f32() * 1000.0,
        cached: false,
    }
}

/// Cache file of a model (by its GUID), `None` without one.
pub fn cache_path(rel: &str) -> Option<PathBuf> {
    let guid = crate::asset_db::guid_of(rel)
        .or_else(|| crate::asset_db::meta::read_guid(&crate::assets::absolute(rel)))?;
    Some(crate::assets::absolute(&format!(
        "{LIBRARY_DIR}/{guid}.vgeo"
    )))
}

fn wanted(model: &Model) -> Vec<bool> {
    model
        .meshes
        .iter()
        .map(|m| model.virtual_geometry && m.skin.is_none() && !m.data.indices.is_empty())
        .collect()
}

/// Builds of an imported model: read from its cache file where the hashes match, the rest made
/// on a worker thread and the file rewritten. `rel` = the model's project path.
pub(super) fn start(model: &Model, rel: &str) -> Arc<VirtualBuilds> {
    start_at(model, cache_path(rel), rel)
}

/// [`start`] with the cache file at `path`.
fn start_at(model: &Model, path: Option<PathBuf>, rel: &str) -> Arc<VirtualBuilds> {
    let wanted = wanted(model);
    let builds = Arc::new(VirtualBuilds::new(wanted.clone()));
    if !wanted.contains(&true) {
        return builds;
    }
    let cached = path
        .as_deref()
        .and_then(|p| crate::vfs::read(p).ok())
        .and_then(|b| file::read(&b))
        .unwrap_or_default();
    let mut missing = Vec::new();
    for (i, m) in model.meshes.iter().enumerate() {
        if !wanted[i] {
            continue;
        }
        let src = source(&m.data);
        let vertices = m.data.vertices.len() as u32;
        let hit = cached
            .get(i)
            .and_then(Option::as_ref)
            .filter(|e| e.hash == src.hash && e.mesh.indices.iter().all(|&v| v < vertices));
        match hit {
            Some(e) => {
                let _ = builds.slots[i].set(VirtualBuild {
                    mesh: Arc::new(e.mesh.clone()),
                    hash: e.hash,
                    build_ms: e.build_ms,
                    cached: true,
                });
            }
            None => missing.push((i, src)),
        }
    }
    if missing.is_empty() {
        return builds;
    }
    let worker = builds.clone();
    let spawned = std::thread::Builder::new()
        .name("misty vgeo build".into())
        .spawn(move || {
            use rayon::prelude::*;
            let made: Vec<(usize, VirtualBuild)> = missing
                .par_iter()
                .map(|(i, src)| (*i, build(src)))
                .collect();
            for (i, b) in made {
                let _ = worker.slots[i].set(b);
            }
            if let Some(path) = path {
                save(&worker, &path);
            }
            READY.fetch_add(1, Ordering::Relaxed);
        });
    if spawned.is_err() {
        eprintln!("{rel}: could not start the virtual geometry build");
    }
    builds
}

/// Write every ready build of `builds` to `path`.
fn save(builds: &VirtualBuilds, path: &std::path::Path) {
    let entries: Vec<Option<(u64, f32, &VirtualMesh)>> = builds
        .slots
        .iter()
        .map(|s| s.get().map(|b| (b.hash, b.build_ms, &*b.mesh)))
        .collect();
    let bytes = file::write(&entries);
    let written = path
        .parent()
        .map_or(Ok(()), std::fs::create_dir_all)
        .and_then(|()| std::fs::write(path, bytes));
    if let Err(e) = written {
        eprintln!("{}: {e}", path.display());
    }
}

/// Builds made right away (runtime meshes: tests and demos that time the build themselves).
pub(super) fn now(model: &Model) -> Arc<VirtualBuilds> {
    let builds = VirtualBuilds::new(wanted(model));
    for (i, m) in model.meshes.iter().enumerate() {
        if builds.wanted[i] {
            let _ = builds.slots[i].set(build(&source(&m.data)));
        }
    }
    Arc::new(builds)
}

/// Make (or refresh) the cache file of the model at `rel` and wait for it: player builds pack
/// it. `Ok(None)` when the model doesn't use virtual geometry.
pub fn bake(rel: &str) -> Result<Option<PathBuf>, String> {
    let model = super::model(rel)?;
    if !model.virtual_geometry {
        return Ok(None);
    }
    let path = cache_path(rel).ok_or_else(|| format!("{rel}: no GUID"))?;
    // The import started the build: wait for its worker, then write the file (the same bytes
    // the worker writes).
    while !model.virtual_builds.complete() {
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    save(&model.virtual_builds, &path);
    Ok(Some(path))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rock_model() -> Model {
        let data = crate::vgeo::rock::rock(3, 7);
        let mut model = Model {
            virtual_geometry: true,
            ..Default::default()
        };
        model
            .meshes
            .push(super::super::ModelMesh::new("rock".into(), data, None));
        model
    }

    fn wait(builds: &VirtualBuilds) {
        let t = Instant::now();
        while !builds.complete() {
            assert!(t.elapsed().as_secs() < 60, "build never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn imports_build_on_a_worker_then_read_the_cache() {
        let dir = std::env::temp_dir().join(format!("misty-vgeo-cache-{}", std::process::id()));
        let path = dir.join("Library/VirtualGeometry/test.vgeo");
        let _ = std::fs::remove_dir_all(&dir);
        let model = rock_model();

        let first = start_at(&model, Some(path.clone()), "rock.glb");
        wait(&first);
        let built = first.get(0).expect("built").clone();
        assert!(!built.cached);
        // The worker writes the file after the builds land.
        let t = Instant::now();
        while file::read(&std::fs::read(&path).unwrap_or_default()).is_none() {
            assert!(t.elapsed().as_secs() < 30, "cache file never written");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }

        let again = start_at(&model, Some(path.clone()), "rock.glb");
        let read = again.get(0).expect("read synchronously from the cache");
        assert!(read.cached);
        assert_eq!(read.mesh.clusters, built.mesh.clusters);
        assert_eq!(read.mesh.indices, built.mesh.indices);
        assert_eq!(again.summary().clusters, built.mesh.clusters.len());

        // An edited mesh doesn't match the cached hash: built again.
        let mut edited = rock_model();
        edited.meshes[0].data.vertices[0].position[0] += 0.01;
        let rebuilt = start_at(&edited, Some(path), "rock.glb");
        wait(&rebuilt);
        assert!(!rebuilt.get(0).expect("rebuilt").cached);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
