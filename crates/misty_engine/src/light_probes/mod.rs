//! Light probes: baked indirect light for moving objects.
//!
//! A [`LightProbeGroup`] lists probe positions. Baking (`bake`) captures the static scene
//! (static renderers, terrain, sky, lights) as a small cube at every probe, projects it onto
//! order-2 spherical harmonics and saves `Assets/Scenes/<Scene>/LightProbes.probes` (`data`),
//! referenced by the scene's Lighting settings. At runtime the probes are tetrahedralized
//! (`delaunay`, our own Bowyer–Watson) and every non-static renderer with Light Probes set to
//! Blend Probes gets the SH interpolated at its bounds centre (`field`), which replaces the
//! ambient light in `mesh.wgsl` (blended per draw in `render/probes/gpu.rs`).

pub mod bake;
pub mod data;
pub mod delaunay;
pub mod field;
pub mod store;

#[cfg(test)]
mod gpu_tests;

use std::sync::Arc;

use glam::Vec3;
use serde::{Deserialize, Serialize};

pub use data::LightProbeData;
pub use field::{LightProbeField, Weights};

use crate::scene::Scene;

/// File extension of baked light probes.
pub const EXTENSION: &str = "probes";

/// Probe positions of one object (local space), like a Light Probe Group.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LightProbeGroup {
    pub enabled: bool,
    pub probes: Vec<Vec3>,
}

impl Default for LightProbeGroup {
    /// Eight probes on the corners of a 2 m cube, like a new group in other editors.
    fn default() -> Self {
        let mut probes = Vec::with_capacity(8);
        for i in 0..8 {
            probes.push(Vec3::new(
                if i & 1 == 0 { -1.0 } else { 1.0 },
                if i & 2 == 0 { -1.0 } else { 1.0 },
                if i & 4 == 0 { -1.0 } else { 1.0 },
            ));
        }
        Self {
            enabled: true,
            probes,
        }
    }
}

impl LightProbeGroup {
    /// Replace the probes with a grid over the box `center ± size / 2` (local space), at most
    /// `spacing` apart and at least two per axis (one along a flat axis).
    pub fn fill_box(&mut self, center: Vec3, size: Vec3, spacing: f32) {
        let spacing = spacing.max(0.05);
        let count = |extent: f32| {
            if extent <= 1e-4 {
                1
            } else {
                ((extent / spacing).ceil() as usize + 1).clamp(2, 64)
            }
        };
        let n = [count(size.x), count(size.y), count(size.z)];
        let lo = center - size * 0.5;
        let at = |i: usize, axis: usize| {
            if n[axis] == 1 {
                center[axis]
            } else {
                lo[axis] + size[axis] * i as f32 / (n[axis] - 1) as f32
            }
        };
        self.probes.clear();
        for y in 0..n[1] {
            for z in 0..n[2] {
                for x in 0..n[0] {
                    self.probes.push(Vec3::new(at(x, 0), at(y, 1), at(z, 2)));
                }
            }
        }
    }
}

/// Mesh Renderer ▸ Light Probes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LightProbeUsage {
    Off,
    #[default]
    BlendProbes,
}

impl LightProbeUsage {
    pub const ALL: [Self; 2] = [Self::Off, Self::BlendProbes];

    pub fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::BlendProbes => "Blend Probes",
        }
    }

    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

/// Probes compared by identity (what views and the GPU cache key on).
#[derive(Clone)]
pub struct FieldHandle(pub Arc<LightProbeField>);

impl PartialEq for FieldHandle {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0)
    }
}

impl std::fmt::Debug for FieldHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "LightProbes({} probes)", self.0.positions.len())
    }
}

/// Project path of a scene's baked light probes: `Assets/Scenes/Level.scene` →
/// `Assets/Scenes/Level/LightProbes.probes`.
pub fn asset_path_for(scene_path: &str) -> String {
    let stem = scene_path.strip_suffix(".scene").unwrap_or(scene_path);
    format!("{stem}/LightProbes.{EXTENSION}")
}

/// Active, enabled Light Probe Groups, in Hierarchy order.
pub fn scene_groups(scene: &Scene) -> Vec<(hecs::Entity, LightProbeGroup)> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let g = scene.world.get::<&LightProbeGroup>(e).ok()?;
            g.enabled.then(|| (e, (*g).clone()))
        })
        .collect()
}

/// World positions of every probe of the scene's groups.
pub fn scene_positions(scene: &Scene) -> Vec<Vec3> {
    scene_groups(scene)
        .into_iter()
        .flat_map(|(e, g)| {
            let m = scene.world_matrix(e);
            g.probes
                .into_iter()
                .map(move |p| m.transform_point3(p))
                .collect::<Vec<_>>()
        })
        .collect()
}

/// The scene's baked probes (Lighting settings ▸ light probes file), if any.
pub fn scene_field(scene: &Scene) -> Option<FieldHandle> {
    let rel = scene.render_settings.light_probes.as_ref()?;
    let field = store::load(&crate::assets::absolute(rel))?;
    (!field.is_empty()).then_some(FieldHandle(field))
}

/// Irradiance / π SH at `position` from the scene's probes, or `None` without baked probes.
pub fn interpolated(scene: &Scene, position: Vec3) -> Option<[Vec3; 9]> {
    let field = scene_field(scene)?;
    let mut hint = 0;
    field.0.sample(position, &mut hint)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_box_makes_a_grid_with_the_spacing() {
        let mut g = LightProbeGroup::default();
        assert_eq!(g.probes.len(), 8);
        g.fill_box(Vec3::new(0.0, 1.5, 0.0), Vec3::new(8.0, 3.0, 4.0), 2.0);
        // 5 × 3 × 3: ceil(8/2)+1, ceil(3/2)+1, ceil(4/2)+1.
        assert_eq!(g.probes.len(), 45);
        let lo = g.probes.iter().fold(Vec3::INFINITY, |a, p| a.min(*p));
        let hi = g.probes.iter().fold(Vec3::NEG_INFINITY, |a, p| a.max(*p));
        assert_eq!(
            (lo, hi),
            (Vec3::new(-4.0, 0.0, -2.0), Vec3::new(4.0, 3.0, 2.0))
        );
        g.fill_box(Vec3::ZERO, Vec3::new(2.0, 0.0, 2.0), 1.0);
        assert!(g.probes.iter().all(|p| p.y == 0.0));
        assert_eq!(g.probes.len(), 9);
    }

    use crate::render::probes::cube::Cube;
    use crate::render::probes::sh;

    /// Projection of known radiance functions matches their analytic coefficients: a constant
    /// c has c·√(4π) in band 0 only; L(d) = d.y has √(4π/3) in coefficient 1 only.
    #[test]
    fn sh_projection_matches_analytic_coefficients() {
        let constant = sh::project(&Cube::from_fn(32, |_| Vec3::new(0.5, 1.0, 2.0)));
        let k0 = (4.0 * std::f32::consts::PI).sqrt();
        assert!((constant[0] - Vec3::new(0.5, 1.0, 2.0) * k0).length() < 1e-3);
        for c in &constant[1..] {
            assert!(c.length() < 1e-3, "{c}");
        }
        let linear = sh::project(&Cube::from_fn(32, |d| Vec3::splat(d.y)));
        let k1 = (4.0 * std::f32::consts::PI / 3.0).sqrt();
        assert!((linear[1].x - k1).abs() < 1e-2, "{}", linear[1].x);
        for (i, c) in linear.iter().enumerate() {
            if i != 1 {
                assert!(c.length() < 1e-2, "{i}: {c}");
            }
        }
    }

    /// Irradiance from SH: a constant sky gives its colour on every normal; L(d) = 1 + d.y gives
    /// E(n)/π = 1 + (2/3)·n.y (the cosine lobe's band-1 factor).
    #[test]
    fn irradiance_evaluation_matches_analytic_values() {
        let sky = LightProbeField::new(
            vec![Vec3::ZERO],
            &[sh::project(&Cube::from_fn(16, |_| Vec3::splat(0.7)))],
        );
        for n in [Vec3::Y, Vec3::NEG_Z, Vec3::new(1.0, -1.0, 0.5).normalize()] {
            assert!((sh::eval(&sky.irradiance[0], n) - Vec3::splat(0.7)).length() < 1e-3);
        }
        let ramp = sh::project(&Cube::from_fn(32, |d| Vec3::splat(1.0 + d.y)));
        let irr = sh::cosine_convolve(ramp);
        for n in [Vec3::Y, Vec3::NEG_Y, Vec3::X, Vec3::new(0.0, 0.6, 0.8)] {
            let want = 1.0 + 2.0 / 3.0 * n.y;
            assert!((sh::eval(&irr, n).x - want).abs() < 1e-2, "{n}");
        }
    }

    #[test]
    fn asset_path_sits_next_to_the_scene() {
        assert_eq!(
            asset_path_for("Assets/Scenes/LightProbesDemo.scene"),
            "Assets/Scenes/LightProbesDemo/LightProbes.probes"
        );
    }
}
