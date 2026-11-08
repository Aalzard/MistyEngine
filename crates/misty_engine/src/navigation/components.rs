//! Nav Mesh components: Agent, Obstacle, Surface, Modifier, Modifier Volume and Link.

use glam::Vec3;
use serde::{Deserialize, Serialize};

use super::{ALL_AREAS, AREA_JUMP, AREA_WALKABLE, ObstacleAvoidance};

fn is_zero_u32(v: &u32) -> bool {
    *v == 0
}
fn is_all(v: &u32) -> bool {
    *v == ALL_AREAS
}
fn is_true(v: &bool) -> bool {
    *v
}
fn is_false(v: &bool) -> bool {
    !*v
}
fn yes() -> bool {
    true
}
fn all_areas() -> u32 {
    ALL_AREAS
}

/// Moves an object along nav mesh paths (`SetDestination` from scripts).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshAgent {
    pub enabled: bool,
    /// Agent type (Navigation window ▸ Agents) whose baked data it walks on.
    #[serde(skip_serializing_if = "is_zero_u32")]
    pub agent_type: u32,
    /// Height of the object's pivot above the nav mesh surface.
    pub base_offset: f32,
    /// Top speed along the path (m/s).
    pub speed: f32,
    /// Turning speed (degrees/s) towards the movement direction.
    pub angular_speed: f32,
    /// Speed change per second (m/s²).
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
    /// Cross off-mesh links on its own (off: stop at the link until `CompleteOffMeshLink`).
    #[serde(skip_serializing_if = "is_true", default = "yes")]
    pub auto_traverse_off_mesh_link: bool,
    /// Plan again when the path ends early (partial) or the nav mesh changes under it.
    #[serde(skip_serializing_if = "is_true", default = "yes")]
    pub auto_repath: bool,
    /// Areas the agent may walk on (bit per area).
    #[serde(skip_serializing_if = "is_all", default = "all_areas")]
    pub area_mask: u32,
}

impl Default for NavMeshAgent {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_type: 0,
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
            auto_traverse_off_mesh_link: true,
            auto_repath: true,
            area_mask: ALL_AREAS,
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

/// Something agents steer around, or (Carve) a hole cut into the nav mesh while it stands still.
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
    /// Cut a hole into the nav mesh (agents plan around it instead of steering).
    #[serde(skip_serializing_if = "is_false")]
    pub carve: bool,
    /// Carve again after moving this far (m).
    pub move_threshold: f32,
    /// Seconds without moving before it counts as stationary.
    pub time_to_stationary: f32,
    /// Only carve while stationary; while moving, agents steer around it.
    #[serde(skip_serializing_if = "is_true", default = "yes")]
    pub carve_only_stationary: bool,
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
            carve: false,
            move_threshold: 0.1,
            time_to_stationary: 0.5,
            carve_only_stationary: true,
        }
    }
}

/// Which objects a NavMesh Surface bakes.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum CollectObjects {
    /// Every active object with a mesh or terrain in the scene.
    #[default]
    All,
    /// Objects touching the surface's box (`center`, `size`); the bake is clipped to it.
    Volume,
    /// This object and its children.
    Children,
}

impl CollectObjects {
    pub const ALL: [CollectObjects; 3] = [Self::All, Self::Volume, Self::Children];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All Game Objects",
            Self::Volume => "Volume",
            Self::Children => "Current Object Hierarchy",
        }
    }
}

/// Bakes a nav mesh for one agent type from the objects it collects (no Static flag needed).
/// Objects with a Nav Mesh Agent or Obstacle are never collected.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshSurface {
    pub enabled: bool,
    pub agent_type: u32,
    pub collect: CollectObjects,
    /// Volume box (local to the object) for `CollectObjects::Volume`.
    pub center: Vec3,
    pub size: Vec3,
    /// Layers to collect (bit per layer).
    pub layer_mask: u32,
    /// Area of surfaces without a Nav Mesh Modifier.
    pub default_area: u8,
    /// Use `voxel_size` instead of a third of the agent radius.
    pub override_voxel_size: bool,
    pub voxel_size: f32,
    /// Use `tile_size` voxels per tile instead of the default.
    pub override_tile_size: bool,
    pub tile_size: u32,
    /// Islands smaller than this (m²) are dropped.
    pub min_region_area: f32,
    /// Project path of the baked data (None = not baked).
    pub nav_mesh_data: Option<String>,
}

impl Default for NavMeshSurface {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_type: 0,
            collect: CollectObjects::All,
            center: Vec3::new(0.0, 2.0, 0.0),
            size: Vec3::new(10.0, 4.0, 10.0),
            layer_mask: u32::MAX,
            default_area: AREA_WALKABLE,
            override_voxel_size: false,
            voxel_size: 0.1667,
            override_tile_size: false,
            tile_size: super::DEFAULT_TILE_SIZE,
            min_region_area: 2.0,
            nav_mesh_data: None,
        }
    }
}

/// Changes how an object (and by default its children) is baked.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshModifier {
    pub enabled: bool,
    /// Leave the object out of every bake.
    pub ignore_from_build: bool,
    /// Bake the object's surfaces with `area`.
    pub override_area: bool,
    pub area: u8,
    /// Also apply to the object's children.
    pub apply_to_children: bool,
    /// Agent types it applies to (-1 = all).
    pub affected_agents: Vec<i32>,
}

impl Default for NavMeshModifier {
    fn default() -> Self {
        Self {
            enabled: true,
            ignore_from_build: false,
            override_area: false,
            area: AREA_WALKABLE,
            apply_to_children: true,
            affected_agents: vec![-1],
        }
    }
}

impl NavMeshModifier {
    pub fn affects(&self, agent_type: u32) -> bool {
        affects(&self.affected_agents, agent_type)
    }
}

pub(crate) fn affects(list: &[i32], agent_type: u32) -> bool {
    list.iter().any(|&a| a == -1 || a == agent_type as i32)
}

/// A box (local to the object) whose nav mesh gets an area (Not Walkable cuts it out).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshModifierVolume {
    pub enabled: bool,
    pub center: Vec3,
    pub size: Vec3,
    pub area: u8,
    /// Agent types it applies to (-1 = all).
    pub affected_agents: Vec<i32>,
}

impl Default for NavMeshModifierVolume {
    fn default() -> Self {
        Self {
            enabled: true,
            center: Vec3::ZERO,
            size: Vec3::new(4.0, 2.0, 4.0),
            area: AREA_WALKABLE,
            affected_agents: vec![-1],
        }
    }
}

impl NavMeshModifierVolume {
    pub fn affects(&self, agent_type: u32) -> bool {
        affects(&self.affected_agents, agent_type)
    }
}

/// An off-mesh connection between two points (local to the object): agents cross gaps, jumps
/// and drops with it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct NavMeshLink {
    pub enabled: bool,
    pub agent_type: u32,
    pub start: Vec3,
    pub end: Vec3,
    /// Width of the crossing (0 = a single point at each end).
    pub width: f32,
    /// Multiplies the area's cost (negative = use the area cost).
    pub cost_modifier: f32,
    /// Agents may cross from end to start too.
    pub bidirectional: bool,
    pub area: u8,
}

impl Default for NavMeshLink {
    fn default() -> Self {
        Self {
            enabled: true,
            agent_type: 0,
            start: Vec3::new(0.0, 0.0, -2.5),
            end: Vec3::new(0.0, 0.0, 2.5),
            width: 0.0,
            cost_modifier: -1.0,
            bidirectional: true,
            area: AREA_JUMP,
        }
    }
}
