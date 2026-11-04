//! Turn the ECS scene into renderer input: the game camera, mesh draws, particles, lights,
//! the lighting environment and post-processing.
//! Shared by the editor's views and the standalone player.

use std::sync::Arc;

use glam::Vec3;
use hecs::Entity;

use crate::camera::{Camera, Projection};
use crate::draw_cache::DrawCache;
use crate::render::{
    Environment, Frame, LightData, LocalFog, MaterialParams, MeshDraw, ParticleBatch, PostSettings,
};
use crate::scene::{
    Camera as CameraComponent, ClearFlags, FogVolume, Light, LightType, MeshRenderer,
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

/// Visible meshes, one draw per Materials element, then Sprite Renderers; `outline(e)` = draw the
/// selection outline around `e`.
pub fn mesh_draws(
    scene: &Scene,
    materials: &dyn MaterialSource,
    outline: impl Fn(Entity) -> bool,
) -> Vec<MeshDraw> {
    mesh_draws_culled(scene, materials, u32::MAX, outline)
}

/// Is `e`'s layer in a camera's culling mask?
pub fn in_mask(scene: &Scene, e: Entity, mask: u32) -> bool {
    mask & (1u32 << scene.game_object(e).layer.min(31)) != 0
}

/// [`mesh_draws`] of the objects whose layer is in `culling_mask` (a one-off
/// [`DrawCache`]; views keep their own cache to only rebuild what changed).
pub fn mesh_draws_culled(
    scene: &Scene,
    materials: &dyn MaterialSource,
    culling_mask: u32,
    outline: impl Fn(Entity) -> bool,
) -> Vec<MeshDraw> {
    let draws = DrawCache::new().update(scene, materials, culling_mask, outline);
    Arc::try_unwrap(draws).unwrap_or_else(|a| (*a).clone())
}

/// Enabled lights in the scene.
pub fn lights(scene: &Scene) -> Vec<LightData> {
    scene
        .active_with::<Light>()
        .into_iter()
        .filter_map(|e| {
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
                shadow: crate::render::LightShadow {
                    size: match l.kind {
                        LightType::Directional => l.shadow_angle,
                        _ => l.shadow_radius,
                    },
                    bias: l.shadow_bias,
                    normal_bias: l.shadow_normal_bias,
                },
                volumetric: crate::render::VolumetricLight {
                    multiplier: l.volumetric_multiplier,
                    shadow_dimmer: l.volumetric_shadow_dimmer,
                },
                contact_shadows: l.contact_shadows,
                area: crate::render::AreaShape {
                    size: glam::Vec2::new(l.width, l.height),
                    right: rot * Vec3::X,
                    up: rot * Vec3::Y,
                    two_sided: l.two_sided,
                    show_emitter: l.show_emitter,
                },
            })
        })
        .collect()
}

/// The scene's Lighting settings with its main Directional Light as the sun, its baked or
/// realtime reflection probes, and its enabled Fog Volumes.
pub fn environment(scene: &Scene, lights: &[LightData]) -> Environment {
    let mut env = Environment::new(scene.render_settings.clone(), lights)
        .with_probes(crate::render::probes::scene_probes(scene));
    env.fog_volumes = fog_volumes(scene);
    env
}

/// Enabled, active Fog Volumes in Hierarchy order.
pub fn fog_volumes(scene: &Scene) -> Vec<LocalFog> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let v = scene.world.get::<&FogVolume>(e).ok()?;
            v.enabled.then(|| LocalFog::new(&v, scene.world_matrix(e)))
        })
        .collect()
}

/// Effects from the enabled, active Post-process Volume with the highest priority
/// (the first in Hierarchy order on ties); the baseline when there is none.
pub fn post_settings(scene: &Scene) -> PostSettings {
    let mut best: Option<PostProcessVolume> = None;
    for e in scene.active_with::<PostProcessVolume>() {
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
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }]
}

/// The enabled, active camera with the highest Depth renders the game.
pub fn game_camera(scene: &Scene) -> Option<(Camera, CameraComponent)> {
    scene
        .active_with::<CameraComponent>()
        .into_iter()
        .filter_map(|e| {
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
    /// Shared with the view's [`DrawCache`] (no copy per frame).
    pub meshes: Arc<Vec<MeshDraw>>,
    pub lights: Vec<LightData>,
    pub particles: Vec<ParticleBatch>,
    pub terrains: Vec<crate::terrain::TerrainDraw>,
    pub water: Vec<crate::water::WaterDraw>,
    pub environment: Environment,
    pub post: PostSettings,
}

impl GameView {
    /// `None` when no camera is rendering.
    pub fn new(scene: &Scene, materials: &dyn MaterialSource) -> Option<Self> {
        Self::cached(scene, materials, &mut DrawCache::new())
    }

    /// [`GameView::new`] reusing the previous frame's draws through `cache`.
    pub fn cached(
        scene: &Scene,
        materials: &dyn MaterialSource,
        cache: &mut DrawCache,
    ) -> Option<Self> {
        crate::profile_scope!("Render.GatherScene", crate::profiler::Category::Rendering);
        let (camera, settings) = game_camera(scene)?;
        let lights = lights(scene);
        // Only the layers in the camera's Culling Mask are drawn (lights light everything).
        let mask = settings.culling_mask;
        let keep = |e: Entity| in_mask(scene, e, mask);
        Some(Self {
            camera,
            meshes: cache.update(scene, materials, mask, |_| false),
            environment: environment(scene, &lights),
            post: post_settings(scene),
            lights,
            particles: crate::particles::batches_where(scene, keep),
            terrains: crate::terrain::draws_where(scene, keep),
            water: crate::water::draws_where(scene, keep),
            settings,
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
            overlay: &[],
            meshes: &self.meshes,
            particles: &self.particles,
            terrains: &self.terrains,
            shaded: true,
            lights: &self.lights,
            water: &self.water,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene::{MeshFilter, PrimitiveMesh};

    struct NoMaterials;
    impl MaterialSource for NoMaterials {
        fn params(&self, _: Option<&str>) -> MaterialParams {
            MaterialParams::default()
        }
    }

    fn cube(s: &mut Scene, name: &str, layer: u8) {
        let e = s.spawn(name, None);
        s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(e, MeshRenderer::default());
        s.game_object_mut(e).layer = layer;
    }

    #[test]
    fn culling_mask_hides_layers_from_the_game_camera() {
        let mut s = Scene::new("t");
        let cam = s.spawn("Camera", None);
        s.add(cam, CameraComponent::default());
        cube(&mut s, "Shown", 0);
        cube(&mut s, "Hidden", 8);
        let view = GameView::new(&s, &NoMaterials).unwrap();
        assert_eq!(view.meshes.len(), 2, "Everything by default");
        s.world
            .get::<&mut CameraComponent>(cam)
            .unwrap()
            .culling_mask = !(1 << 8);
        let view = GameView::new(&s, &NoMaterials).unwrap();
        assert_eq!(view.meshes.len(), 1);
        // The editor's views ignore camera masks.
        assert_eq!(mesh_draws(&s, &NoMaterials, |_| false).len(), 2);
        // Old scenes without the field render everything.
        let c: CameraComponent = serde_json::from_str("{}").unwrap();
        assert_eq!(c.culling_mask, u32::MAX);
    }
}
