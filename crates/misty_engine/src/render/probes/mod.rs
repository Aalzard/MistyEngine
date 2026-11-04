//! Reflection probes: box-projected, prefiltered cubemaps for specular reflections and SH9 for
//! diffuse ambient, blended per pixel from a short per-draw list (`gpu`, `probes.wgsl`).
//!
//! Baking (`capture`) renders the scene into a cube with this renderer, prefilters it with GGX
//! importance sampling (`prefilter`), projects SH (`sh`) and saves it next to the scene (`data`,
//! loaded through `store`). `placement` lays probes out automatically, `realtime` re-captures
//! Realtime probes one face per frame.

pub mod brdf;
mod bvh;
pub mod capture;
pub mod cube;
pub mod data;
pub(crate) mod gpu;
pub mod placement;
pub mod prefilter;
pub mod realtime;
pub mod sh;
pub mod store;

#[cfg(test)]
mod gpu_tests;

use std::sync::Arc;

use glam::Vec3;

pub use data::ProbeData;

use crate::scene::reflection_probe::ProbeMode;
use crate::scene::{ReflectionProbe, Scene};

/// File extension of baked probes.
pub const EXTENSION: &str = "probe";

/// Shared probe data compared by identity (cheap, and what the GPU cache keys on).
#[derive(Clone)]
pub struct ProbeHandle(pub Arc<ProbeData>);

impl PartialEq for ProbeHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for ProbeHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ProbeHandle({}px)", self.0.size())
    }
}

/// One probe as the renderer sees it.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeInstance {
    /// Capture point (box projection origin).
    pub position: Vec3,
    pub box_min: Vec3,
    pub box_max: Vec3,
    pub blend_distance: f32,
    pub importance: i32,
    pub intensity: f32,
    pub box_projection: bool,
    pub data: ProbeHandle,
}

impl ProbeInstance {
    /// Instance of a component at `position` with `data`.
    pub fn new(p: &ReflectionProbe, position: Vec3, data: Arc<ProbeData>) -> Self {
        let (box_min, box_max) = p.world_box(position);
        Self {
            position,
            box_min,
            box_max,
            blend_distance: p.blend_distance.max(0.0),
            importance: p.importance,
            intensity: p.intensity.max(0.0),
            box_projection: p.box_projection,
            data: ProbeHandle(data),
        }
    }

    pub fn volume(&self) -> f32 {
        (self.box_max - self.box_min)
            .max(Vec3::ZERO)
            .element_product()
    }
}

/// The probes of one view; empty = sky reflections and ambient only.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ProbeSet {
    pub probes: Vec<ProbeInstance>,
    /// Scene view "Reflection Probes" mode: colour pixels by the probes that light them.
    pub debug: bool,
}

/// Project path of the `index`-th baked probe of a scene: `Assets/Scenes/Level.scene` →
/// `Assets/Scenes/Level/ReflectionProbe-0.probe`.
pub fn asset_path_for(scene_path: &str, index: usize) -> String {
    let stem = scene_path.strip_suffix(".scene").unwrap_or(scene_path);
    format!("{stem}/ReflectionProbe-{index}.{EXTENSION}")
}

/// Active, enabled probes of a scene, in Hierarchy order.
pub fn scene_probe_list(scene: &Scene) -> Vec<(hecs::Entity, ReflectionProbe)> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let p = scene.world.get::<&ReflectionProbe>(e).ok()?;
            p.enabled.then(|| (e, (*p).clone()))
        })
        .collect()
}

/// Probes with data: Realtime probes use their latest capture (else their baked file).
pub fn scene_probes(scene: &Scene) -> ProbeSet {
    let probes = scene_probe_list(scene)
        .into_iter()
        .filter_map(|(e, p)| {
            let realtime = (p.mode == ProbeMode::Realtime)
                .then(|| store::realtime(e.to_bits().get()))
                .flatten();
            let data = realtime.or_else(|| {
                let rel = p.baked.as_ref()?;
                store::load(&crate::assets::absolute(rel))
            })?;
            Some(ProbeInstance::new(&p, scene.world_position(e), data))
        })
        .collect();
    ProbeSet {
        probes,
        debug: false,
    }
}
