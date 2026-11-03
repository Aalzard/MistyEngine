//! Navigation: nav mesh baking from static geometry, path finding and Nav Mesh Agents.
//!
//! The bake (`bake`) rasterises the triangles of static Mesh Renderers and Terrains into a
//! voxel heightfield (`heightfield`), keeps the spans an agent fits on (slope, clearance, step),
//! erodes them by the agent radius and merges the cells into planar rectangles (`polys`). The
//! result is a polygon [`NavMesh`] saved next to the scene (`navmesh`). Paths are A* over the
//! polygons plus string pulling (`path`); agents follow them on the mesh surface (`agent`).

pub mod agent;
pub mod bake;
mod funnel;
pub mod geometry;
mod heightfield;
pub mod navmesh;
pub mod path;
mod polys;
pub mod store;

#[cfg(test)]
pub(crate) mod tests;

use glam::Vec3;
use serde::{Deserialize, Serialize};

pub use agent::{NavRuntime, ObstacleAvoidance};
pub use navmesh::{NavMesh, NavPoly, NearestHit};
pub use path::{NavPath, PathStatus};

/// File extension of baked nav mesh assets.
pub const EXTENSION: &str = "navmesh";

/// Navigation window â–¸ Agents: the agent the nav mesh is baked for.
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

/// Per-scene navigation settings (the scene file's `"navigation"` block).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavigationSettings {
    pub agent: AgentSettings,
    /// Use `voxel_size` instead of a third of the agent radius.
    pub manual_voxel_size: bool,
    pub voxel_size: f32,
    /// Islands smaller than this (mÂ²) are dropped.
    pub min_region_area: f32,
    /// Project path of the baked asset (None = not baked).
    pub nav_mesh: Option<String>,
}

impl Default for NavigationSettings {
    fn default() -> Self {
        Self {
            agent: AgentSettings::default(),
            manual_voxel_size: false,
            voxel_size: 0.1667,
            min_region_area: 2.0,
            nav_mesh: None,
        }
    }
}

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

    /// Where a scene's nav mesh is saved: `Assets/Scenes/Level.scene` â†’
    /// `Assets/Scenes/Level/NavMesh.navmesh`.
    pub fn asset_path_for(scene_path: &str) -> String {
        let stem = scene_path
            .strip_suffix(".scene")
            .unwrap_or(scene_path)
            .to_owned();
        format!("{stem}/NavMesh.{EXTENSION}")
    }
}

/// Moves an object along nav mesh paths (`SetDestination` from scripts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshAgent {
    pub enabled: bool,
    /// Height of the object's pivot above the nav mesh surface.
    pub base_offset: f32,
    /// Top speed along the path (m/s).
    pub speed: f32,
    /// Turning speed (degrees/s) towards the movement direction.
    pub angular_speed: f32,
    /// Speed change per second (m/sÂ²).
    pub acceleration: f32,
    /// Stop this close to the destination.
    pub stopping_distance: f32,
    /// Slow down before the destination instead of overshooting.
    pub auto_braking: bool,
    pub radius: f32,
    pub height: f32,
    pub obstacle_avoidance: ObstacleAvoidance,
    /// 0 = most important; agents with a higher number give way.
    pub priority: i32,
}

impl Default for NavMeshAgent {
    fn default() -> Self {
        Self {
            enabled: true,
            base_offset: 0.0,
            speed: 3.5,
            angular_speed: 120.0,
            acceleration: 8.0,
            stopping_distance: 0.0,
            auto_braking: true,
            radius: 0.5,
            height: 2.0,
            obstacle_avoidance: ObstacleAvoidance::HighQuality,
            priority: 50,
        }
    }
}

/// Footprint of a Nav Mesh Obstacle.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObstacleShape {
    #[default]
    Box,
    Capsule,
}

impl ObstacleShape {
    pub const ALL: [ObstacleShape; 2] = [Self::Box, Self::Capsule];

    pub fn label(self) -> &'static str {
        match self {
            Self::Box => "Box",
            Self::Capsule => "Capsule",
        }
    }
}

/// Something agents steer around (moving or not). It doesn't cut holes into the nav mesh.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshObstacle {
    pub enabled: bool,
    pub shape: ObstacleShape,
    pub center: Vec3,
    /// Box size (local, scaled with the object).
    pub size: Vec3,
    /// Capsule radius and height.
    pub radius: f32,
    pub height: f32,
}

impl Default for NavMeshObstacle {
    fn default() -> Self {
        Self {
            enabled: true,
            shape: ObstacleShape::Box,
            center: Vec3::ZERO,
            size: Vec3::ONE,
            radius: 0.5,
            height: 2.0,
        }
    }
}
