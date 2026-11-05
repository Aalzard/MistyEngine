//! Occlusion culling with a baked potentially visible set (PVS), like the reference engine's
//! Occlusion Culling window.
//!
//! * **Bake** ([`bake`]): the space the camera can be in (Occlusion Areas marked View Volume, or
//!   the bounds of the static geometry) is cut into a uniform grid of view cells. From sample
//!   points on each cell's corners, edges, faces and centre, rays are cast against the Occluder
//!   Static triangles (the reflection-probe BVH) into a small cube of depths; every Occludee
//!   Static renderer whose box shows through a texel is potentially visible from that cell. Cells
//!   share their lattice points, the points are computed in parallel, and each cell's set is
//!   widened by its neighbours' sets so the gaps between samples stay conservative.
//! * **Data** ([`data`]): one bit per occludee per distinct set, cells index the sets; saved as
//!   `Assets/Scenes/<Scene>/OcclusionData.occ` and named by the scene's Lighting settings.
//! * **Runtime** ([`runtime`]): a camera with Occlusion Culling on finds its cell; occludees not
//!   in the cell's set are skipped by the main pass (they still cast shadows). Dynamic objects can
//!   be culled by the renderer's GPU Hi-Z pass ("Dynamic Occlusion").

pub mod bake;
pub mod data;
pub mod runtime;
pub mod store;

#[cfg(test)]
mod tests;

use std::sync::Arc;

use serde::{Deserialize, Serialize};

pub use data::{CellLookup, OcclusionData};

/// File extension of baked occlusion data.
pub const EXTENSION: &str = "occ";

/// Occlusion window ▸ Bake parameters and the baked data of a scene.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct OcclusionSettings {
    /// Smallest object that hides others, in metres; also the view cell size.
    pub smallest_occluder: f32,
    /// Smallest gap the camera should see through, in metres (sets the ray density).
    pub smallest_hole: f32,
    /// Percent of back faces a sample point may see before it counts as inside geometry
    /// (100 = never).
    pub backface_threshold: f32,
    /// Cull non-static renderers with the GPU depth pyramid (Hi-Z) while baked data is used.
    pub dynamic_occlusion: bool,
    /// Project path of the baked `.occ` file.
    pub data: Option<String>,
}

impl Default for OcclusionSettings {
    fn default() -> Self {
        Self {
            smallest_occluder: 5.0,
            smallest_hole: 0.25,
            backface_threshold: 100.0,
            dynamic_occlusion: true,
            data: None,
        }
    }
}

impl OcclusionSettings {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }

    /// Project path of a scene's occlusion data: `Assets/Scenes/Level.scene` →
    /// `Assets/Scenes/Level/OcclusionData.occ`.
    pub fn asset_path_for(scene_path: &str) -> String {
        let stem = scene_path.strip_suffix(".scene").unwrap_or(scene_path);
        format!("{stem}/OcclusionData.{EXTENSION}")
    }

    /// Side of a view cell.
    pub fn cell_size(&self) -> f32 {
        self.smallest_occluder.max(0.25)
    }

    /// Cube-map resolution of each sample point (rays per face side).
    pub fn rays_per_side(&self) -> u32 {
        ((16.0 / self.smallest_hole.max(0.01).sqrt()) as u32).clamp(12, 64)
    }
}

/// Which mesh draws the main pass skips this frame (renderer input, see
/// [`crate::render::Environment::occlusion`]).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct OcclusionMask {
    /// Per mesh draw (indices past the end are drawn).
    pub hidden: Arc<Vec<bool>>,
    /// Also cull with the GPU Hi-Z pyramid (dynamic occlusion).
    pub dynamic: bool,
}
