//! World Partition (lite): a big world split into grid cells that stream in and out around the
//! player.
//!
//! - A **World Partition** component on an object of the persistent scene holds the settings
//!   (cell size, grid origin, radii, the per-frame budget) and the list of cells. Each cell is
//!   its own sub-scene file (`<folder>/Cells/Cell_<x>_<z>.scene`), loaded as an additive scene
//!   when it streams in ([`convert`] splits an existing scene into cells).
//! - Objects with **Always Loaded** stay in the persistent scene, as do cameras, lights,
//!   UI and other objects that are not "somewhere" in the world.
//! - **Streaming Source** components say where to load around (each with its own load and
//!   unload radius, or the partition's); without one, the main camera is the source. A cell
//!   loads when a source comes within its load radius and unloads once every source is beyond
//!   its unload radius (the gap between the two is the hysteresis that stops a cell flickering
//!   in and out at a border).
//! - [`streamer::Streamer`] reads and parses cell files and reads the assets they name on the
//!   background pool, then adds them to the world a slice at a time within a per-frame budget.
//! - [`hlod`] builds merged proxy meshes of the cells ("Build HLODs") that are drawn in place of
//!   unloaded cells, and [`tiles`] splits a big terrain into stitched tile terrains, one per
//!   cell, that stream like any other object.

pub mod convert;
#[cfg(test)]
mod demo_tests;
pub mod hlod;
pub mod streamer;
#[cfg(test)]
mod tests;
pub mod tiles;

use glam::{Vec2, Vec3};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::scene::Scene;

pub use streamer::{CellPhase, StreamHooks, Streamer, StreamingStats};

/// A grid cell's coordinates.
pub type CellCoord = (i32, i32);

/// The World Partition settings of a scene, on an object of its persistent scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct WorldPartition {
    pub enabled: bool,
    /// Side of a grid cell (metres).
    pub cell_size: f32,
    /// World x/z of the corner of cell (0, 0).
    pub origin: [f32; 2],
    /// Project folder of the cell scenes and HLODs (`<folder>/Cells`, `<folder>/HLOD`).
    pub folder: String,
    /// Default radii of streaming sources (metres): cells load within `load_radius` and
    /// unload beyond `unload_radius`.
    pub load_radius: f32,
    pub unload_radius: f32,
    /// HLOD proxies of unloaded cells are drawn up to this distance (0 = at any distance).
    pub hlod_distance: f32,
    /// Main-thread time streaming may spend per frame adding and removing objects (ms).
    pub budget_ms: f32,
    /// Cells read and parsed on the background pool at once.
    pub max_loads: u32,
    /// The cells that have a sub-scene (made by Convert).
    pub cells: Vec<CellInfo>,
}

impl Default for WorldPartition {
    fn default() -> Self {
        Self {
            enabled: true,
            cell_size: 256.0,
            origin: [0.0, 0.0],
            folder: String::new(),
            load_radius: 600.0,
            unload_radius: 750.0,
            hlod_distance: 0.0,
            budget_ms: 2.0,
            max_loads: 4,
            cells: Vec::new(),
        }
    }
}

/// One cell of a partition.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CellInfo {
    pub x: i32,
    pub z: i32,
    /// Objects in the cell's sub-scene (every level of the hierarchy).
    pub objects: u32,
    /// Lowest and highest point of the cell's contents (metres; gizmos, HLOD bounds).
    pub height: [f32; 2],
    /// "Build HLODs" made a proxy for it.
    pub hlod: bool,
}

impl CellInfo {
    pub fn coord(&self) -> CellCoord {
        (self.x, self.z)
    }
}

impl WorldPartition {
    pub fn cell_size(&self) -> f32 {
        self.cell_size.max(1.0)
    }

    /// The cell a world point is in.
    pub fn cell_of(&self, p: Vec3) -> CellCoord {
        let s = self.cell_size();
        (
            ((p.x - self.origin[0]) / s).floor() as i32,
            ((p.z - self.origin[1]) / s).floor() as i32,
        )
    }

    /// World x/z rectangle of a cell: (min, max).
    pub fn cell_rect(&self, c: CellCoord) -> (Vec2, Vec2) {
        let s = self.cell_size();
        let min = Vec2::new(
            self.origin[0] + c.0 as f32 * s,
            self.origin[1] + c.1 as f32 * s,
        );
        (min, min + Vec2::splat(s))
    }

    /// Centre of a cell on the x/z plane.
    pub fn cell_center(&self, c: CellCoord) -> Vec2 {
        let (lo, hi) = self.cell_rect(c);
        (lo + hi) * 0.5
    }

    /// Distance on the x/z plane from `p` to the nearest point of a cell (0 inside it).
    pub fn distance_to_cell(&self, p: Vec3, c: CellCoord) -> f32 {
        let (lo, hi) = self.cell_rect(c);
        let q = Vec2::new(p.x, p.z);
        q.clamp(lo, hi).distance(q)
    }

    /// The unload radius actually used: never inside the load radius.
    pub fn unload_radius(&self) -> f32 {
        self.unload_radius.max(self.load_radius)
    }

    /// Project-relative path of a cell's sub-scene.
    pub fn cell_path(&self, c: CellCoord) -> String {
        format!("{}/Cells/{}.scene", self.folder, cell_name(c))
    }

    /// Project-relative path of a cell's HLOD proxy.
    pub fn hlod_path(&self, c: CellCoord) -> String {
        format!("{}/HLOD/{}.hlod", self.folder, cell_name(c))
    }

    pub fn cell(&self, c: CellCoord) -> Option<&CellInfo> {
        self.cells.iter().find(|i| i.coord() == c)
    }

    /// Grid extent of the cells: (min, max) coordinates (inclusive).
    pub fn bounds(&self) -> Option<(CellCoord, CellCoord)> {
        let first = self.cells.first()?.coord();
        Some(self.cells.iter().fold((first, first), |(lo, hi), c| {
            (
                (lo.0.min(c.x), lo.1.min(c.z)),
                (hi.0.max(c.x), hi.1.max(c.z)),
            )
        }))
    }
}

/// "Cell_3_-2".
pub fn cell_name(c: CellCoord) -> String {
    format!("Cell_{}_{}", c.0, c.1)
}

/// The cell named by a cell scene path ("…/Cells/Cell_3_-2.scene" → (3, -2)).
pub fn cell_of_path(path: &str) -> Option<CellCoord> {
    let file = path.rsplit(['/', '\\']).next()?;
    let stem = file.strip_suffix(".scene")?;
    let rest = stem.strip_prefix("Cell_")?;
    let (x, z) = rest.split_once('_')?;
    Some((x.parse().ok()?, z.parse().ok()?))
}

/// Keeps an object (and its children) in the persistent scene when the world is partitioned.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AlwaysLoaded {}

/// Cells load around this object (the main camera is the source when a scene has none).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StreamingSource {
    pub enabled: bool,
    /// Metres; 0 = the World Partition's.
    pub load_radius: f32,
    pub unload_radius: f32,
}

impl Default for StreamingSource {
    fn default() -> Self {
        Self {
            enabled: true,
            load_radius: 0.0,
            unload_radius: 0.0,
        }
    }
}

/// A place cells load around this frame.
#[derive(Clone, Debug, PartialEq)]
pub struct Source {
    pub name: String,
    pub position: Vec3,
    pub load_radius: f32,
    pub unload_radius: f32,
}

/// The first enabled World Partition of the scene: (object, settings).
pub fn find(scene: &Scene) -> Option<(Entity, WorldPartition)> {
    scene
        .active_with::<WorldPartition>()
        .into_iter()
        .find_map(|e| {
            let wp = scene.world.get::<&WorldPartition>(e).ok()?;
            wp.enabled.then(|| (e, (*wp).clone()))
        })
}

/// Where cells load around this frame: every enabled Streaming Source, else the main camera.
pub fn sources(scene: &Scene, wp: &WorldPartition) -> Vec<Source> {
    let radius = |r: f32, default: f32| if r > 0.0 { r } else { default };
    let mut out: Vec<Source> = scene
        .active_with::<StreamingSource>()
        .into_iter()
        .filter_map(|e| {
            let s = scene.world.get::<&StreamingSource>(e).ok()?;
            if !s.enabled {
                return None;
            }
            let load = radius(s.load_radius, wp.load_radius);
            Some(Source {
                name: scene.name(e),
                position: scene.world_position(e),
                load_radius: load,
                unload_radius: radius(s.unload_radius, wp.unload_radius()).max(load),
            })
        })
        .collect();
    if out.is_empty()
        && let Some(cam) = main_camera(scene)
    {
        out.push(Source {
            name: scene.name(cam),
            position: scene.world_position(cam),
            load_radius: wp.load_radius,
            unload_radius: wp.unload_radius(),
        });
    }
    out
}

/// The camera tagged MainCamera, else the game camera (highest depth).
pub fn main_camera(scene: &Scene) -> Option<Entity> {
    let cams = scene.active_with::<crate::scene::Camera>();
    let enabled = |e: &Entity| {
        scene
            .world
            .get::<&crate::scene::Camera>(*e)
            .is_ok_and(|c| c.enabled)
    };
    cams.iter()
        .copied()
        .filter(enabled)
        .find(|e| scene.game_object(*e).tag == "MainCamera")
        .or_else(|| {
            cams.iter().copied().filter(enabled).max_by(|a, b| {
                let depth = |e: &Entity| {
                    scene
                        .world
                        .get::<&crate::scene::Camera>(*e)
                        .map_or(0.0, |c| c.depth)
                };
                depth(a).total_cmp(&depth(b))
            })
        })
}

/// Should cell `c` be loaded for these sources? `Some(true)` = load it (a source is within
/// its load radius), `Some(false)` = keep it if loaded (within an unload radius), `None` =
/// unload it.
pub fn wanted(wp: &WorldPartition, sources: &[Source], c: CellCoord) -> Option<bool> {
    let mut keep = false;
    for s in sources {
        let d = wp.distance_to_cell(s.position, c);
        if d <= s.load_radius {
            return Some(true);
        }
        if d <= s.unload_radius {
            keep = true;
        }
    }
    keep.then_some(false)
}

/// Is every cell within `radius` of `center` (x/z) loaded? Cells without a sub-scene count as
/// loaded.
pub fn area_loaded(scene: &Scene, wp: &WorldPartition, center: Vec3, radius: f32) -> bool {
    wp.cells
        .iter()
        .filter(|c| wp.distance_to_cell(center, c.coord()) <= radius)
        .all(|c| cell_loaded(scene, wp, c.coord()))
}

/// Is a cell's sub-scene loaded with all of its objects?
pub fn cell_loaded(scene: &Scene, wp: &WorldPartition, c: CellCoord) -> bool {
    let path = wp.cell_path(c);
    scene
        .scenes()
        .iter()
        .any(|s| s.loaded && !s.partial && s.path.as_deref() == Some(path.as_str()))
}
