//! Convert: split a scene's objects into grid-cell sub-scenes.
//!
//! Every top-level object of the active (persistent) scene goes into the cell under its
//! anchor (the middle of its meshes, the middle of a terrain, else its position), except the
//! objects that stay: those marked Always Loaded or holding the partition itself, a Streaming
//! Source, a camera, a directional light, UI, wind or a terrain larger than a cell (split it into
//! tiles first, see [`super::tiles`]). Each cell's objects are written to its sub-scene file and
//! removed from the scene; the partition lists the cells.

use std::collections::BTreeMap;
use std::path::Path;

use glam::Vec3;
use hecs::Entity;

use super::{AlwaysLoaded, CellCoord, CellInfo, StreamingSource, WorldPartition};
use crate::scene::{Camera, Light, LightType, MeshFilter, MeshRenderer, Scene, Terrain};

/// What [`convert`] did.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ConvertReport {
    pub cells: usize,
    /// Top-level objects moved into cells / kept in the persistent scene.
    pub moved: usize,
    pub kept: usize,
    pub warnings: Vec<String>,
}

/// Does top-level object `e` stay in the persistent scene? (`Some(reason)` when it does.)
pub fn stays(scene: &Scene, e: Entity, wp: &WorldPartition) -> Option<&'static str> {
    let subtree: Vec<Entity> = subtree(scene, e);
    let any = |f: &dyn Fn(Entity) -> bool| subtree.iter().any(|x| f(*x));
    let has = |x: Entity, name: &str| {
        crate::scene::registry::find(name).is_some_and(|info| (info.has)(&scene.world, x))
    };
    if any(&|x| scene.world.get::<&AlwaysLoaded>(x).is_ok()) {
        return Some("Always Loaded");
    }
    if any(&|x| scene.world.get::<&WorldPartition>(x).is_ok()) {
        return Some("World Partition");
    }
    if any(&|x| scene.world.get::<&StreamingSource>(x).is_ok()) {
        return Some("Streaming Source");
    }
    if any(&|x| scene.world.get::<&Camera>(x).is_ok()) {
        return Some("camera");
    }
    if any(&|x| {
        scene
            .world
            .get::<&Light>(x)
            .is_ok_and(|l| l.kind == LightType::Directional)
    }) {
        return Some("directional light");
    }
    if any(&|x| {
        [
            "Canvas",
            "Rect Transform",
            "Event System",
            "Wind Zone",
            "Post-process Volume",
            "World Builder",
        ]
        .iter()
        .any(|n| has(x, n))
    }) {
        return Some("not placed in the world");
    }
    let too_big = subtree.iter().any(|x| {
        let Ok(t) = scene.world.get::<&Terrain>(*x) else {
            return false;
        };
        crate::terrain::store::get(&t.terrain_data).is_some_and(|(d, _)| {
            d.size.x > wp.cell_size() * 1.01 || d.size.z > wp.cell_size() * 1.01
        })
    });
    if too_big {
        return Some("terrain larger than a cell (split it into tiles)");
    }
    None
}

fn subtree(scene: &Scene, e: Entity) -> Vec<Entity> {
    let mut out = vec![e];
    let mut i = 0;
    while i < out.len() {
        out.extend(scene.children(out[i]));
        i += 1;
    }
    out
}

/// World bounds (min, max) of the meshes under `e`, or of its terrain.
pub fn world_bounds(scene: &Scene, e: Entity) -> Option<(Vec3, Vec3)> {
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let mut any = false;
    for x in subtree(scene, e) {
        if let Ok(t) = scene.world.get::<&Terrain>(x)
            && let Some((d, _)) = crate::terrain::store::get(&t.terrain_data)
        {
            let p = scene.world_position(x);
            let (a, b) = d.bounds();
            lo = lo.min(p + a);
            hi = hi.max(p + b);
            any = true;
        }
        let has_renderer = scene.world.get::<&MeshRenderer>(x).is_ok();
        let mesh = scene
            .world
            .get::<&MeshFilter>(x)
            .ok()
            .and_then(|f| f.mesh.clone());
        if let (true, Some(mesh)) = (has_renderer, mesh)
            && let Some((a, b)) = crate::models::bounds(&mesh)
        {
            let m = scene.world_matrix(x);
            for i in 0..8 {
                let c = Vec3::new(
                    if i & 1 == 0 { a.x } else { b.x },
                    if i & 2 == 0 { a.y } else { b.y },
                    if i & 4 == 0 { a.z } else { b.z },
                );
                let w = m.transform_point3(c);
                lo = lo.min(w);
                hi = hi.max(w);
            }
            any = true;
        }
    }
    any.then_some((lo, hi))
}

/// The point that decides a top-level object's cell.
pub fn anchor(scene: &Scene, e: Entity) -> Vec3 {
    match world_bounds(scene, e) {
        Some((lo, hi)) => (lo + hi) * 0.5,
        None => scene.world_position(e),
    }
}

/// Which cell each movable top-level object of the active scene goes to (Hierarchy order).
pub fn plan(scene: &Scene, wp: &WorldPartition) -> (BTreeMap<CellCoord, Vec<Entity>>, Vec<Entity>) {
    let mut cells: BTreeMap<CellCoord, Vec<Entity>> = BTreeMap::new();
    let mut kept = Vec::new();
    for r in scene.roots_in(scene.active_scene()) {
        if stays(scene, r, wp).is_some() {
            kept.push(r);
        } else {
            cells
                .entry(wp.cell_of(anchor(scene, r)))
                .or_default()
                .push(r);
        }
    }
    (cells, kept)
}

/// Split the active scene into cell sub-scenes under `root` (the project folder): writes
/// `<folder>/Cells/Cell_x_z.scene` for every cell with objects (deleting cell files left from
/// an earlier conversion), removes those objects from the scene and lists the cells in the
/// World Partition component of `partition`. Cells loaded as streamed scenes are merged back
/// first, so converting again re-splits everything (after a change of cell size).
pub fn convert(scene: &mut Scene, partition: Entity, root: &Path) -> Result<ConvertReport, String> {
    let mut wp = scene
        .world
        .get::<&WorldPartition>(partition)
        .map(|w| (*w).clone())
        .map_err(|_| "the object has no World Partition".to_string())?;
    if wp.folder.trim().is_empty() {
        return Err("set the World Partition's folder first".into());
    }
    let mut report = ConvertReport::default();
    // Objects of loaded cells come back to the persistent scene first.
    let active = scene.active_scene();
    let cell_scenes: Vec<_> = scene
        .scenes()
        .iter()
        .filter(|s| {
            s.id != active
                && s.path
                    .as_deref()
                    .is_some_and(|p| super::cell_of_path(p).is_some())
        })
        .map(|s| (s.id, s.loaded))
        .collect();
    let mut merged = std::collections::BTreeSet::new();
    for (id, loaded) in &cell_scenes {
        if *loaded {
            if let Some(c) = scene
                .scene_path(*id)
                .as_deref()
                .and_then(super::cell_of_path)
            {
                merged.insert(c);
            }
            for r in scene.roots_in(*id) {
                scene.move_to_scene(r, active);
            }
        }
    }
    for (id, _) in cell_scenes {
        let _ = scene.remove_scene(id);
    }
    let unloaded: Vec<CellInfo> = wp
        .cells
        .iter()
        .filter(|c| !merged.contains(&c.coord()))
        .cloned()
        .collect();
    if !unloaded.is_empty() {
        // Cells not loaded in the editor: read their objects back too.
        for c in &unloaded {
            let path = root.join(wp.cell_path(c.coord()));
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            match scene.load_additive(&wp.cell_path(c.coord()), &text) {
                Ok((id, roots, _)) => {
                    for r in roots {
                        scene.move_to_scene(r, active);
                    }
                    let _ = scene.remove_scene(id);
                }
                Err(e) => report
                    .warnings
                    .push(format!("{}: {e}", super::cell_name(c.coord()))),
            }
        }
    }

    let (cells, kept) = plan(scene, &wp);
    report.kept = kept.len();
    for r in &kept {
        if let Some(why) = stays(scene, *r, &wp)
            && why.starts_with("terrain")
        {
            report
                .warnings
                .push(format!("'{}' stays loaded: {why}", scene.name(*r)));
        }
    }
    let dir = root.join(&wp.folder).join("Cells");
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // Old cell files go (with their .meta).
    if let Ok(list) = std::fs::read_dir(&dir) {
        for f in list.flatten() {
            let name = f.file_name().to_string_lossy().to_string();
            let stem = name.strip_suffix(".meta").unwrap_or(&name);
            if super::cell_of_path(stem).is_some() {
                let _ = std::fs::remove_file(f.path());
            }
        }
    }
    let mut infos = Vec::new();
    for (c, roots) in &cells {
        let text = scene.subtrees_json(roots, true);
        let path = root.join(wp.cell_path(*c));
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut lo = f32::MAX;
        let mut hi = f32::MIN;
        let mut objects = 0;
        for r in roots {
            objects += subtree(scene, *r).len() as u32;
            let (a, b) = world_bounds(scene, *r).unwrap_or_else(|| {
                let p = scene.world_position(*r);
                (p, p)
            });
            lo = lo.min(a.y);
            hi = hi.max(b.y);
        }
        infos.push(CellInfo {
            x: c.0,
            z: c.1,
            objects,
            height: [lo, hi],
            hlod: false,
        });
        report.moved += roots.len();
    }
    for roots in cells.values() {
        for r in roots {
            scene.despawn(*r);
        }
    }
    report.cells = infos.len();
    wp.cells = infos;
    if let Ok(mut w) = scene.world.get::<&mut WorldPartition>(partition) {
        *w = wp;
    }
    Ok(report)
}

/// Write every loaded cell sub-scene back to its file (the editor's Save for cells), keeping
/// the partition's cell list (object counts) up to date.
pub fn save_loaded_cells(scene: &mut Scene, root: &Path) -> Result<usize, String> {
    let Some((owner, mut wp)) = super::find(scene) else {
        return Ok(0);
    };
    let mut saved = 0;
    for entry in scene.scenes().to_vec() {
        let Some(path) = entry.path.as_deref() else {
            continue;
        };
        let Some(c) = super::cell_of_path(path) else {
            continue;
        };
        if !entry.loaded || entry.partial {
            continue;
        }
        let roots = scene.roots_in(entry.id);
        let text = scene.subtrees_json(&roots, true);
        let file = root.join(path);
        std::fs::write(&file, text).map_err(|e| format!("{}: {e}", file.display()))?;
        let objects = roots.iter().map(|r| subtree(scene, *r).len() as u32).sum();
        match wp.cells.iter_mut().find(|i| i.coord() == c) {
            Some(info) => info.objects = objects,
            None => wp.cells.push(CellInfo {
                x: c.0,
                z: c.1,
                objects,
                ..Default::default()
            }),
        }
        saved += 1;
    }
    if let Ok(mut w) = scene.world.get::<&mut WorldPartition>(owner) {
        *w = wp;
    }
    Ok(saved)
}
