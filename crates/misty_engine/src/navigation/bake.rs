//! Bake entry points: static scene geometry + agent settings → [`NavMesh`].

use super::geometry::{self, BakeGeometry};
use super::heightfield::{self, Config};
use super::{NavMesh, NavigationSettings, polys};
use crate::scene::Scene;

/// Bake `scene`'s static geometry (on the calling thread).
pub fn bake_scene(scene: &Scene) -> NavMesh {
    bake(&geometry::collect(scene), &scene.navigation, &mut |_, _| {})
}

/// Bake prepared geometry; `progress(fraction, stage)` is called between stages.
pub fn bake(
    geo: &BakeGeometry,
    settings: &NavigationSettings,
    progress: &mut dyn FnMut(f32, &'static str),
) -> NavMesh {
    let a = settings.agent;
    let cfg = Config {
        cell: settings.cell_size(),
        climb: a.step_height.max(0.0),
        height: a.height.max(0.01),
        walkable_cos: a.max_slope.clamp(0.0, 89.9).to_radians().cos(),
        radius: a.radius.max(0.0),
        min_region_area: settings.min_region_area.max(0.0),
    };
    progress(0.1, "Voxelizing");
    let Some(field) = heightfield::build(geo, &cfg) else {
        return NavMesh::new(a, cfg.cell, Vec::new());
    };
    progress(0.7, "Building polygons");
    let polys = polys::build(&field);
    progress(1.0, "Done");
    NavMesh::new(a, field.cell, polys)
}
