//! Turn the ECS scene into renderer input: the game camera, mesh draws, particles, lights,
//! the lighting environment and post-processing.
//! Shared by the editor's views and the standalone player.

use glam::Vec3;
use hecs::Entity;

use crate::camera::{Camera, Projection};
use crate::render::{
    Environment, Frame, LightData, MaterialParams, MeshDraw, ParticleBatch, PostSettings,
};
use crate::scene::{
    Camera as CameraComponent, ClearFlags, Light, LightType, MeshFilter, MeshRenderer,
    PostProcessVolume, Scene, Shadows,
};

/// Where material paths ("Assets/Materials/Red.mat") get their shading values.
pub trait MaterialSource {
    /// Values for a renderer's material; `None` or unknown = the default material.
    fn params(&self, rel: Option<&str>) -> MaterialParams;
}

/// Values of a renderer's material element `i`: the script-made instance, else the asset
/// (with any `sharedMaterial` changes scripts made), else Default-Material.
pub fn renderer_material(
    scene: &Scene,
    r: &MeshRenderer,
    i: usize,
    materials: &dyn MaterialSource,
) -> MaterialParams {
    if let Some(m) = r.instance(i) {
        return m.params();
    }
    let path = r.material_at(i);
    match path.and_then(|p| scene.shared_materials.get(p)) {
        Some(m) => m.params(),
        None => materials.params(path),
    }
}

/// Visible meshes, one draw per Materials element; `outline(e)` = draw the selection outline
/// around `e`.
pub fn mesh_draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    outline: impl Fn(Entity) -> bool,
) -> Vec<MeshDraw> {
    let mut out = Vec::new();
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let w = &scene.world;
        let Some(mesh) = w.get::<&MeshFilter>(e).ok().and_then(|f| f.mesh.clone()) else {
            continue;
        };
        let Ok(r) = w.get::<&MeshRenderer>(e) else {
            continue;
        };
        if !r.enabled {
            continue;
        }
        let model = scene.world_matrix(e);
        let outline = outline(e);
        for i in 0..r.material_count() {
            out.push(MeshDraw {
                mesh: mesh.clone(),
                model,
                material: renderer_material(scene, &r, i, materials),
                cast_shadows: r.cast_shadows,
                receive_shadows: r.receive_shadows,
                // The outline mask needs each mesh once.
                outline: outline && i == 0,
            });
        }
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

/// The scene's Lighting settings with its main Directional Light as the sun.
pub fn environment(scene: &Scene, lights: &[LightData]) -> Environment {
    Environment::new(scene.render_settings.clone(), lights)
}

/// Effects from the enabled, active Post-process Volume with the highest priority
/// (the first in Hierarchy order on ties); the baseline when there is none.
pub fn post_settings(scene: &Scene) -> PostSettings {
    let mut best: Option<PostProcessVolume> = None;
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let Ok(v) = scene.world.get::<&PostProcessVolume>(e) else {
            continue;
        };
        if v.enabled && best.as_ref().is_none_or(|b| v.priority > b.priority) {
            best = Some((*v).clone());
        }
    }
    best.map(|v| PostSettings::from_volume(&v))
        .unwrap_or_default()
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

/// The enabled, active camera with the highest Depth renders the game.
pub fn game_camera(scene: &Scene) -> Option<(Camera, CameraComponent)> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let c = scene.world.get::<&CameraComponent>(e).ok()?;
            c.enabled.then(|| (e, (*c).clone()))
        })
        .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
        .map(|(e, c)| {
            let (_, rotation, position) = scene.world_matrix(e).to_scale_rotation_translation();
            let projection = if c.orthographic {
                Projection::Orthographic { size: c.ortho_size }
            } else {
                Projection::Perspective { fov_y: c.fov }
            };
            let camera = Camera {
                position,
                rotation,
                projection,
                near: c.near,
                far: c.far,
            };
            (camera, c)
        })
}

/// What the game camera sees: everything a [`Frame`] borrows.
pub struct GameView {
    pub camera: Camera,
    pub settings: CameraComponent,
    pub meshes: Vec<MeshDraw>,
    pub lights: Vec<LightData>,
    pub particles: Vec<ParticleBatch>,
    pub terrains: Vec<crate::terrain::TerrainDraw>,
    pub environment: Environment,
    pub post: PostSettings,
}

impl GameView {
    /// `None` when no camera is rendering.
    pub fn new(scene: &Scene, materials: &dyn MaterialSource) -> Option<Self> {
        let (camera, settings) = game_camera(scene)?;
        let lights = lights(scene);
        Some(Self {
            camera,
            settings,
            meshes: mesh_draws(scene, materials, |_| false),
            environment: environment(scene, &lights),
            post: post_settings(scene),
            lights,
            particles: crate::particles::batches(scene),
            terrains: crate::terrain::draws(scene),
        })
    }

    pub fn frame(&self) -> Frame<'_> {
        let bg = self.settings.background;
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: self.settings.clear_flags == ClearFlags::Skybox,
            background: [bg[0], bg[1], bg[2]],
            environment: &self.environment,
            post: self.post,
            lines: &[],
            meshes: &self.meshes,
            particles: &self.particles,
            terrains: &self.terrains,
            shaded: true,
            lights: &self.lights,
        }
    }
}
