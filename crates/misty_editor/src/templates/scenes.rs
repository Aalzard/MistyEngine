//! The Floating Platform template's scene, and the pieces every generated template shares:
//! the sky volume (bloom, tonemapping, volumetric clouds, sun shafts), the sun and the camera.

use std::path::Path;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    Camera, CloudPreset, Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene,
    Shadows, TonemapLook, TonemapMode, Transform,
};

pub const PLATFORM_MAT: &str = "Assets/Materials/Platform.mat";
pub const ACCENT_MAT: &str = "Assets/Materials/Accent.mat";
/// The platform slab: 20 × 1 × 20 metres.
pub const PLATFORM_SIZE: Vec3 = Vec3::new(20.0, 1.0, 20.0);
/// Name of every template's post-processing object.
pub const VOLUME: &str = "Post-process Volume";

pub fn write_materials(root: &Path) -> Result<(), String> {
    let platform = Material {
        color: [0.6, 0.62, 0.64, 1.0],
        smoothness: 0.25,
        ..Default::default()
    };
    let accent = Material {
        color: [0.27, 0.55, 0.85, 1.0],
        smoothness: 0.55,
        ..Default::default()
    };
    platform.save(&root.join(PLATFORM_MAT))?;
    accent.save(&root.join(ACCENT_MAT))
}

/// The global volume every template starts with: AgX tonemapping (Punchy look), bloom for the
/// glow of the sun and bright surfaces, volumetric clouds, screen-space sun shafts and a touch
/// of vignette.
pub fn sky_volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Agx;
    v.tonemapping.look = TonemapLook::Punchy;
    v.bloom.active = true;
    v.bloom.intensity = 0.4;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.3;
    v.vignette.active = true;
    v.vignette.intensity = 0.2;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Sparse.apply(c);
    v.sun_shafts.active = true;
    v.sun_shafts.intensity = 0.6;
    v
}

pub fn add_volume(s: &mut Scene, volume: PostProcessVolume) {
    let e = s.spawn(VOLUME, None);
    s.add(e, volume);
}

pub fn place(s: &mut Scene, name: &str, position: Vec3, euler: Vec3) -> Option<Entity> {
    let e = s.find_by_name(name)?;
    let mut t = Transform::from_position(position).with_euler(euler);
    t.scale = Vec3::ONE;
    s.set_transform(e, t);
    Some(e)
}

/// A primitive with a material (no collider) under `parent`.
pub fn mesh_in(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, material: &str) -> Entity {
    mesh_in(s, name, None, kind, t, material)
}

/// A transform at `position` with `scale` (no rotation).
pub fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// The slab (top face at `top`) with a 1 m cube in the middle for scale.
fn platform(s: &mut Scene, top: f32) {
    let slab = scaled(
        Vec3::new(0.0, top - PLATFORM_SIZE.y * 0.5, 0.0),
        PLATFORM_SIZE,
    );
    mesh(s, "Platform", PrimitiveMesh::Cube, slab, PLATFORM_MAT);
    let cube = Transform::from_position(Vec3::new(0.0, top + 0.5, 0.0));
    mesh(s, "Cube", PrimitiveMesh::Cube, cube, ACCENT_MAT);
}

/// The scene's Directional Light turned to `euler` (degrees), with soft shadows.
pub fn sun(s: &mut Scene, euler: Vec3, color: [f32; 3], intensity: f32) {
    if let Some(e) = place(s, "Directional Light", Vec3::new(0.0, 10.0, 0.0), euler) {
        let mut l = s.world.get::<&mut Light>(e).unwrap();
        l.color = color;
        l.intensity = intensity;
        l.shadows = Shadows::Soft;
    }
}

/// The scene's Main Camera at `position`, turned to `euler`, seeing `far` metres.
pub fn camera(s: &mut Scene, position: Vec3, euler: Vec3, far: f32) {
    if let Some(e) = place(s, "Main Camera", position, euler)
        && let Ok(mut c) = s.world.get::<&mut Camera>(e)
    {
        c.far = far;
    }
}

/// Floating Platform: a platform in the sky under sparse volumetric clouds, the afternoon sun
/// ahead and to the right of the camera (its glow and rays in view), soft shadows.
pub fn floating_platform() -> Scene {
    let mut s = Scene::default_3d("Main");
    camera(
        &mut s,
        Vec3::new(0.0, 5.5, -22.0),
        Vec3::new(9.0, 0.0, 0.0),
        2000.0,
    );
    sun(&mut s, Vec3::new(15.0, 205.0, 0.0), [1.0, 0.88, 0.74], 1.5);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    // A soft blue-grey below the horizon instead of brown earth: the platform floats.
    sky.ground = [0.42, 0.47, 0.54];
    platform(&mut s, 0.0);
    add_volume(&mut s, sky_volume());
    s
}
