//! Cloth: makes the object's Skinned Mesh Renderer mesh a simulated cloth while playing (XPBD:
//! distance, bending and long-range attachment constraints, collisions with the listed sphere
//! and capsule colliders, gravity and the scene's Wind Zone). The simulation is in
//! `physics::cloth`; this is only the settings.
//!
//! Each mesh vertex has a **max distance**: how far it may move away from where the skinned
//! (animated) mesh puts it. 0 pins the vertex to the mesh, [`Cloth::FREE`] leaves it free.
//! The values are painted in the Scene view or read from a vertex colour channel.

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::registry::SceneComponent;

/// Vertex colour channel that can fill the max distances.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorChannel {
    #[default]
    Red,
    Green,
    Blue,
    Alpha,
}

impl ColorChannel {
    pub const ALL: [Self; 4] = [Self::Red, Self::Green, Self::Blue, Self::Alpha];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Self::Red => "Red",
            Self::Green => "Green",
            Self::Blue => "Blue",
            Self::Alpha => "Alpha",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Cloth {
    pub enabled: bool,
    /// 0–1: how strongly the cloth keeps its edge lengths (1 = it doesn't stretch).
    pub stretching_stiffness: f32,
    /// 0–1: how strongly it resists folding.
    pub bending_stiffness: f32,
    /// Long-range attachments: free vertices can't get further from the nearest pinned vertex
    /// than they were in the mesh (stops long cloth stretching under its own weight).
    pub use_tethers: bool,
    pub use_gravity: bool,
    /// 0–1: velocity lost per second.
    pub damping: f32,
    /// Constant acceleration (world space, m/s²).
    pub external_acceleration: Vec3,
    /// Random acceleration up to this size (world space, m/s²).
    pub random_acceleration: Vec3,
    /// × the scene's Wind Zone (direction, main strength, turbulence and gusts).
    pub wind_scale: f32,
    /// 0–1: how much of the object's own movement the cloth lags behind (1 = full inertia).
    pub world_velocity_scale: f32,
    /// 0–1: friction against colliders.
    pub friction: f32,
    /// Collision radius of each vertex (metres).
    pub thickness: f32,
    /// Solver sub-steps per second (more = stiffer and steadier, costs more).
    pub solver_frequency: f32,
    /// Keep vertices of different parts of the cloth `self_collision_distance` apart.
    pub self_collision: bool,
    pub self_collision_distance: f32,
    /// Objects with a Sphere Collider the cloth collides with.
    #[serde(with = "super::skinned_mesh::bone_list")]
    pub sphere_colliders: Vec<Option<Entity>>,
    /// Objects with a Capsule Collider the cloth collides with.
    #[serde(with = "super::skinned_mesh::bone_list")]
    pub capsule_colliders: Vec<Option<Entity>>,
    /// Per mesh vertex: metres it may move away from the skinned mesh (0 = pinned,
    /// negative = free). Empty = every vertex free.
    pub max_distances: Vec<f32>,
    /// The channel "Max Distance From Vertex Colours" reads.
    pub color_channel: ColorChannel,
    /// Max distance a full channel value (1) stands for; 0 in the channel pins the vertex.
    pub color_scale: f32,
}

impl Cloth {
    /// A max distance meaning "not constrained".
    pub const FREE: f32 = -1.0;

    /// Max distance of mesh vertex `i` (`None` = free).
    pub fn max_distance(&self, i: usize) -> Option<f32> {
        self.max_distances.get(i).copied().filter(|d| *d >= 0.0)
    }

    /// Size the max distance list to `n` vertices (new ones free).
    pub fn resize_max_distances(&mut self, n: usize) {
        self.max_distances.resize(n, Self::FREE);
    }
}

impl Default for Cloth {
    fn default() -> Self {
        Self {
            enabled: true,
            stretching_stiffness: 1.0,
            bending_stiffness: 0.5,
            use_tethers: true,
            use_gravity: true,
            damping: 0.1,
            external_acceleration: Vec3::ZERO,
            random_acceleration: Vec3::ZERO,
            wind_scale: 1.0,
            world_velocity_scale: 0.5,
            friction: 0.5,
            thickness: 0.02,
            solver_frequency: 240.0,
            self_collision: false,
            self_collision_distance: 0.03,
            sphere_colliders: Vec::new(),
            capsule_colliders: Vec::new(),
            max_distances: Vec::new(),
            color_channel: ColorChannel::Red,
            color_scale: 1.0,
        }
    }
}

impl SceneComponent for Cloth {
    const NAME: &'static str = "Cloth";
    const CATEGORY: &'static str = "Physics";
}
