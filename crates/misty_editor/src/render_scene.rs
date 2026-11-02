//! Turn the ECS scene into renderer input: mesh draws and lights.

use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::render::{LightData, MeshDraw};
use misty_engine::scene::{Light, LightType, MeshFilter, MeshRenderer, Scene, Shadows};

use crate::assets::AssetDatabase;
use crate::selection::Selection;

/// Visible meshes. Selected objects (and their children) get the selection outline.
pub fn mesh_draws(
    scene: &Scene,
    assets: &AssetDatabase,
    selection: Option<&Selection>,
) -> Vec<MeshDraw> {
    let mut out = Vec::new();
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let w = &scene.world;
        let Some(mesh) = w.get::<&MeshFilter>(e).ok().and_then(|f| f.mesh) else {
            continue;
        };
        let Ok(r) = w.get::<&MeshRenderer>(e) else {
            continue;
        };
        if !r.enabled {
            continue;
        }
        let outline =
            selection.is_some_and(|s| s.list().iter().any(|sel| scene.is_ancestor(*sel, e)));
        out.push(MeshDraw {
            mesh,
            model: scene.world_matrix(e),
            material: assets.params(r.material.as_deref()),
            cast_shadows: r.cast_shadows,
            receive_shadows: r.receive_shadows,
            outline,
        });
    }
    out
}

/// Enabled lights in the scene.
pub fn lights(scene: &Scene) -> Vec<LightData> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let l = scene.world.get::<&Light>(e).ok()?;
            if !l.enabled {
                return None;
            }
            let (_, rot, pos) = scene.world_matrix(e).to_scale_rotation_translation();
            Some(LightData {
                kind: l.kind,
                position: pos,
                direction: rot * Vec3::Z,
                color: l.color,
                intensity: l.intensity,
                range: l.range,
                spot_angle: l.spot_angle,
                shadows: l.shadows,
            })
        })
        .collect()
}

/// Scene-view lighting off: one light shining from the camera, no shadows.
pub fn headlight(camera: &Camera) -> Vec<LightData> {
    vec![LightData {
        kind: LightType::Directional,
        position: camera.position,
        direction: (camera.forward() + camera.up() * -0.3).normalize(),
        color: [1.0, 1.0, 1.0],
        intensity: 1.0,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
    }]
}
