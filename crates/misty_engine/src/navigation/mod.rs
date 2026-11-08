//! Navigation: tiled nav mesh baking, path finding, crowds and Nav Mesh components.
//!
//! The build follows the usual voxel recipe, per tile (`tiles`): the walkable triangles of the
//! collected geometry (`geometry`) are rasterised into a heightfield, the spans an agent fits on
//! (slope, clearance, step) become cells, which are eroded by the agent radius and marked with
//! areas and carved by obstacles (`heightfield`). The cells are split into monotone regions
//! (`regions`), each region's outline is traced and simplified (`contours`) and triangulated
//! into convex polygons with detail heights (`polymesh`). Tiles build in parallel on the
//! background pool (`bake`) and can be rebuilt alone at runtime (carving, `UpdateNavMesh`).
//!
//! Queries (`path`, `query`) are A* over polygons with per-area costs and masks plus string
//! pulling (`funnel`), raycasts, nearest points and nearest walls. Agents (`agent`) follow paths,
//! cross off-mesh links and avoid each other with ORCA (`orca`).

pub mod agent;
pub mod bake;
pub mod components;
mod contours;
mod funnel;
pub mod geometry;
mod heightfield;
pub mod navmesh;
pub mod orca;
pub mod path;
mod polymesh;
pub mod query;
mod regions;
pub mod store;
pub mod tiles;

#[cfg(test)]
pub(crate) mod tests;
#[cfg(test)]
mod tests_build;
#[cfg(test)]
mod tests_query;
#[cfg(test)]
mod tests_runtime;

use serde::{Deserialize, Serialize};

pub use agent::{NavEvent, NavRuntime, ObstacleAvoidance};
pub use components::*;
pub use navmesh::{NavMesh, NavPoly, NearestHit, OffMeshLink, PolyRef};
pub use path::{NavPath, PathStatus};
pub use query::{EdgeHit, QueryFilter, RaycastHit};

/// File extension of baked nav mesh assets.
pub const EXTENSION: &str = "navmesh";

/// Number of navigation areas (bits of an area mask).
pub const AREA_COUNT: usize = 32;
/// Built-in area 0: the default walkable surface.
pub const AREA_WALKABLE: u8 = 0;
/// Built-in area 1: removed from the nav mesh.
pub const AREA_NOT_WALKABLE: u8 = 1;
/// Built-in area 2: off-mesh links that are jumps.
pub const AREA_JUMP: u8 = 2;
/// Every area (`NavMesh.AllAreas`).
pub const ALL_AREAS: u32 = u32::MAX;

/// Size of the agent a nav mesh is baked for (one agent type).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentSettings {
    /// How close to walls the agent's centre can get.
    pub radius: f32,
    /// Free space needed above the walkable surface.
    pub height: f32,
    /// Highest step (or ledge) the agent walks up or down.
    pub step_height: f32,
    /// Steepest walkable slope in degrees.
    pub max_slope: f32,
}

impl Default for AgentSettings {
    fn default() -> Self {
        Self {
            radius: 0.5,
            height: 2.0,
            step_height: 0.75,
            max_slope: 45.0,
        }
    }
}

/// A named agent size (Navigation window ▸ Agents). Id 0 is always "Humanoid".
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AgentType {
    pub id: u32,
    pub name: String,
    pub settings: AgentSettings,
}

impl Default for AgentType {
    fn default() -> Self {
        Self {
            id: 1,
            name: "New Agent".into(),
            settings: AgentSettings::default(),
        }
    }
}

/// One navigation area (Navigation window ▸ Areas): a name and a path cost multiplier.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavArea {
    pub name: String,
    /// Path cost per metre walked on this area (at least 1).
    pub cost: f32,
}

impl Default for NavArea {
    fn default() -> Self {
        Self {
            name: String::new(),
            cost: 1.0,
        }
    }
}

impl NavArea {
    fn new(name: &str, cost: f32) -> Self {
        Self {
            name: name.into(),
            cost,
        }
    }
}

/// The three built-in areas (their names can't change).
pub fn builtin_areas() -> Vec<NavArea> {
    vec![
        NavArea::new("Walkable", 1.0),
        NavArea::new("Not Walkable", 1.0),
        NavArea::new("Jump", 2.0),
    ]
}

fn is_builtin_areas(a: &Vec<NavArea>) -> bool {
    *a == builtin_areas()
}

/// Colour of area `i` in the Scene view and the Areas tab (sRGB, 0..1).
pub fn area_color(i: u8) -> [f32; 3] {
    match i {
        0 => [0.3, 0.62, 1.0],
        1 => [0.85, 0.2, 0.2],
        2 => [0.95, 0.75, 0.2],
        _ => {
            // Spread the user areas round the hue circle.
            let h = (i as f32 * 0.618_034).fract();
            hsv(h, 0.65, 0.95)
        }
    }
}

fn hsv(h: f32, s: f32, v: f32) -> [f32; 3] {
    let i = (h * 6.0).floor();
    let f = h * 6.0 - i;
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - f * s), v * (1.0 - (1.0 - f) * s));
    match i as i32 % 6 {
        0 => [v, t, p],
        1 => [q, v, p],
        2 => [p, v, t],
        3 => [p, q, v],
        4 => [t, p, v],
        _ => [v, p, q],
    }
}

/// Per-scene navigation settings (the scene file's `"navigation"` block).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationSettings {
    /// The Humanoid agent type (id 0); the Navigation window's Bake tab bakes for it.
    pub agent: AgentSettings,
    /// More agent types (ids 1 and up), each with its own baked data (NavMesh Surfaces).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub agent_types: Vec<AgentType>,
    /// Up to 32 areas; missing entries are unnamed with cost 1.
    #[serde(skip_serializing_if = "is_builtin_areas")]
    pub areas: Vec<NavArea>,
    /// Use `voxel_size` instead of a third of the agent radius.
    pub manual_voxel_size: bool,
    pub voxel_size: f32,
    /// Islands smaller than this (m²) are dropped.
    pub min_region_area: f32,
    /// Tile width in voxels (0 = the default).
    #[serde(skip_serializing_if = "is_zero")]
    pub tile_size: u32,
    /// Project path of the baked asset (None = not baked).
    pub nav_mesh: Option<String>,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            agent: AgentSettings::default(),
            agent_types: Vec::new(),
            areas: builtin_areas(),
            manual_voxel_size: false,
            voxel_size: 0.1667,
            min_region_area: 2.0,
            tile_size: 0,
            nav_mesh: None,
        }
    }
}

/// Tile width in voxels when nothing overrides it.
pub const DEFAULT_TILE_SIZE: u32 = 64;

impl NavigationSettings {
    /// Voxel width used by the bake.
    pub fn cell_size(&self) -> f32 {
        let auto = self.agent.radius / 3.0;
        let s = if self.manual_voxel_size {
            self.voxel_size
        } else {
            auto
        };
        s.clamp(0.01, 10.0)
    }

    /// Where a scene's nav mesh is saved: `Assets/Scenes/Level.scene` →
    /// `Assets/Scenes/Level/NavMesh.navmesh`.
    pub fn asset_path_for(scene_path: &str) -> String {
        let stem = scene_path
            .strip_suffix(".scene")
            .unwrap_or(scene_path)
            .to_owned();
        format!("{stem}/NavMesh.{EXTENSION}")
    }

    /// Where a NavMesh Surface's data is saved: `Assets/Scenes/Level/NavMesh-<object>.navmesh`.
    pub fn surface_asset_path_for(scene_path: &str, object: &str) -> String {
        let stem = scene_path.strip_suffix(".scene").unwrap_or(scene_path);
        let clean: String = object
            .chars()
            .map(|c| {
                if c.is_alphanumeric() || c == '-' || c == '_' || c == ' ' {
                    c
                } else {
                    '_'
                }
            })
            .collect();
        format!("{stem}/NavMesh-{}.{EXTENSION}", clean.trim())
    }

    /// Every agent type, Humanoid first.
    pub fn all_agent_types(&self) -> Vec<AgentType> {
        let mut v = vec![AgentType {
            id: 0,
            name: "Humanoid".into(),
            settings: self.agent,
        }];
        v.extend(self.agent_types.iter().filter(|t| t.id != 0).cloned());
        v
    }

    /// Settings of agent type `id` (None when it doesn't exist).
    pub fn agent_settings(&self, id: u32) -> Option<AgentSettings> {
        if id == 0 {
            return Some(self.agent);
        }
        self.agent_types
            .iter()
            .find(|t| t.id == id)
            .map(|t| t.settings)
    }

    /// Name of agent type `id`.
    pub fn agent_type_name(&self, id: u32) -> String {
        if id == 0 {
            return "Humanoid".into();
        }
        self.agent_types
            .iter()
            .find(|t| t.id == id)
            .map_or_else(|| format!("Missing ({id})"), |t| t.name.clone())
    }

    /// Add an agent type with the next free id; returns the id.
    pub fn add_agent_type(&mut self, name: &str) -> u32 {
        let id = self.agent_types.iter().map(|t| t.id).max().unwrap_or(0) + 1;
        self.agent_types.push(AgentType {
            id,
            name: name.into(),
            settings: AgentSettings::default(),
        });
        id
    }

    /// Area `i` (unnamed with cost 1 when it was never set).
    pub fn area(&self, i: usize) -> NavArea {
        self.areas.get(i).cloned().unwrap_or_default()
    }

    /// Set area `i`'s name and cost (the list grows as needed).
    pub fn set_area(&mut self, i: usize, area: NavArea) {
        if i >= AREA_COUNT {
            return;
        }
        if self.areas.len() <= i {
            self.areas.resize(i + 1, NavArea::default());
        }
        self.areas[i] = area;
        // Trailing unnamed defaults aren't kept.
        while self.areas.len() > 3 && self.areas.last() == Some(&NavArea::default()) {
            self.areas.pop();
        }
    }

    /// Path costs of all 32 areas (at least 1).
    pub fn area_costs(&self) -> [f32; AREA_COUNT] {
        std::array::from_fn(|i| self.area(i).cost.max(1.0))
    }

    /// Index of the area called `name` (`NavMesh.GetAreaFromName`).
    pub fn area_from_name(&self, name: &str) -> Option<usize> {
        (0..AREA_COUNT).find(|&i| !name.is_empty() && self.area(i).name == name)
    }

    /// Display name of area `i` ("Area 7" when unnamed).
    pub fn area_label(&self, i: usize) -> String {
        let a = self.area(i);
        if a.name.is_empty() {
            format!("Area {i}")
        } else {
            a.name
        }
    }
}
