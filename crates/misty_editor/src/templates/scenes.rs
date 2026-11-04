//! Scenes of the Empty, Standard and Ocean templates.

use std::path::Path;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    Camera, CloudPreset, Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene,
    Shadows, TonemapMode, Transform,
};
use misty_engine::water::{self, WaterKind};

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

/// A global volume that is present but switched off, every override inactive.
pub fn disabled_volume() -> PostProcessVolume {
    PostProcessVolume {
        enabled: false,
        ..Default::default()
    }
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

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, material: &str) -> Entity {
    let e = s.spawn(name, None);
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

/// The slab (top face at `top`) with a 1 m cube in the middle for scale.
fn platform(s: &mut Scene, top: f32) {
    let slab = Transform {
        position: Vec3::new(0.0, top - PLATFORM_SIZE.y * 0.5, 0.0),
        scale: PLATFORM_SIZE,
        ..Default::default()
    };
    mesh(s, "Platform", PrimitiveMesh::Cube, slab, PLATFORM_MAT);
    let cube = Transform::from_position(Vec3::new(0.0, top + 0.5, 0.0));
    mesh(s, "Cube", PrimitiveMesh::Cube, cube, ACCENT_MAT);
}

fn sun(s: &mut Scene, euler: Vec3, color: [f32; 3], intensity: f32) {
    if let Some(e) = place(s, "Directional Light", Vec3::new(0.0, 10.0, 0.0), euler) {
        let mut l = s.world.get::<&mut Light>(e).unwrap();
        l.color = color;
        l.intensity = intensity;
        l.shadows = Shadows::Soft;
    }
}

fn camera(s: &mut Scene, position: Vec3, euler: Vec3, far: f32) {
    if let Some(e) = place(s, "Main Camera", position, euler)
        && let Ok(mut c) = s.world.get::<&mut Camera>(e)
    {
        c.far = far;
    }
}

/// Main Camera, Directional Light and a disabled Post-process Volume.
pub fn empty() -> Scene {
    let mut s = Scene::default_3d("Main");
    add_volume(&mut s, disabled_volume());
    s
}

/// A floating platform under a procedural sky, soft sun shadows; post-processing off.
pub fn standard() -> Scene {
    let mut s = Scene::default_3d("Main");
    camera(
        &mut s,
        Vec3::new(0.0, 8.0, -20.0),
        Vec3::new(20.0, 0.0, 0.0),
        1000.0,
    );
    sun(&mut s, Vec3::new(42.0, -35.0, 0.0), [1.0, 0.96, 0.9], 1.3);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    // A soft blue-grey below the horizon instead of brown earth: the platform floats.
    sky.ground = [0.42, 0.47, 0.54];
    platform(&mut s, 0.0);
    add_volume(&mut s, disabled_volume());
    s
}

fn ocean_volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.4;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.3;
    // A few fair-weather clouds.
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Sparse.apply(c);
    // Light haze over the water, thinning out with height.
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 4000.0;
    f.attenuation_distance = 4000.0;
    f.base_height = 0.0;
    f.maximum_height = 150.0;
    f.anisotropy = 0.4;
    f.volumetric = false;
    v
}

/// The platform above an endless ocean, a lower warm sun, a few clouds, tonemapping, bloom
/// and haze.
pub fn ocean() -> Scene {
    let mut s = Scene::default_3d("Main");
    camera(
        &mut s,
        Vec3::new(0.0, 8.0, -27.0),
        Vec3::new(12.0, 0.0, 0.0),
        5000.0,
    );
    sun(&mut s, Vec3::new(22.0, -40.0, 0.0), [1.0, 0.88, 0.72], 1.6);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    sky.sun_size = 0.045;
    sky.atmosphere_thickness = 1.1;
    platform(&mut s, 1.5);
    let sea = water::spawn_water(&mut s, "Ocean", WaterKind::Ocean, Vec3::ZERO, Vec2::ZERO);
    if let Ok(mut w) = s.world.get::<&mut water::WaterSurface>(sea) {
        w.settings.set_storminess(0.25);
    }
    add_volume(&mut s, ocean_volume());
    s
}
