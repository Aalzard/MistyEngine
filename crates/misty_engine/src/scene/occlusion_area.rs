//! Occlusion Area: a box (centred on its object, scaled with it) marking where the camera can be.
//! When a scene has any enabled View Volume areas, the occlusion bake only makes view cells
//! inside them; otherwise it covers the bounds of the static geometry.

use glam::{Mat4, Vec3};
use serde::{Deserialize, Serialize};

use super::SceneComponent;

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcclusionArea {
    pub enabled: bool,
    /// Box size in local units (× the object's scale).
    pub size: [f32; 3],
    /// Box centre in local units.
    pub center: [f32; 3],
    /// The camera can be inside: the bake makes view cells here.
    pub is_view_volume: bool,
}

impl Default for OcclusionArea {
    fn default() -> Self {
        Self {
            enabled: true,
            size: [10.0, 4.0, 10.0],
            center: [0.0, 0.0, 0.0],
            is_view_volume: true,
        }
    }
}

impl OcclusionArea {
    /// World-space box under the object's matrix `m` (axis-aligned around the rotated box).
    pub fn world_bounds(&self, m: &Mat4) -> (Vec3, Vec3) {
        let half = Vec3::from(self.size).abs() * 0.5;
        let c = Vec3::from(self.center);
        let b = crate::render::culling::Aabb::new(c - half, c + half).transformed(m);
        (b.min, b.max)
    }
}

impl SceneComponent for OcclusionArea {
    const NAME: &'static str = "Occlusion Area";
    const CATEGORY: &'static str = "Rendering";
}
