//! Scenes of the Standard and PP Ready templates.

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

/// Everything on, tuned for a warm sunset: clouds, light volumetric fog with sharp god rays,
/// screen sun shafts, bloom, ACES, a touch of vignette.
fn pp_ready_volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.5;
    v.bloom.threshold = 1.0;
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 0.4;
    v.vignette.active = true;
    v.vignette.intensity = 0.25;
    let c = &mut v.clouds;
    c.active = true;
    CloudPreset::Sparse.apply(c);
    // Light haze: the sun's rays show through it without hiding the horizon.
    let f = &mut v.fog;
    f.active = true;
    f.volumetric = true;
    f.max_fog_distance = 4000.0;
    f.attenuation_distance = 900.0;
    f.base_height = 0.0;
    f.maximum_height = 80.0;
    f.anisotropy = 0.7;
    f.volumetric_distance = 150.0;
    v.sun_shafts.active = true;
    v.sun_shafts.intensity = 0.8;
    v
}

/// PP Ready: the Standard platform and cube above an endless ocean, a low sunset sun facing
/// the camera, and a cinematic camera low at one corner of the platform.
pub fn pp_ready() -> Scene {
    let mut s = Scene::default_3d("Main");
    let top = 1.5;
    // From the south-west corner, looking across the cube towards the sun.
    if let Some(e) = place(
        &mut s,
        "Main Camera",
        Vec3::new(-14.0, top + 3.2, -14.0),
        Vec3::new(8.0, 45.0, 0.0),
    ) && let Ok(mut c) = s.world.get::<&mut Camera>(e)
    {
        c.far = 5000.0;
        c.fov = 40.0;
    }
    // 8° above the horizon, a little right of the cube as seen from the camera.
    sun(&mut s, Vec3::new(8.0, 235.0, 0.0), [1.0, 0.66, 0.4], 1.8);
    let sky = &mut s.render_settings.skybox;
    sky.sun_disk = SunDisk::HighQuality;
    sky.sun_size = 0.05;
    sky.atmosphere_thickness = 1.2;
    platform(&mut s, top);
    let sea = water::spawn_water(&mut s, "Ocean", WaterKind::Ocean, Vec3::ZERO, Vec2::ZERO);
    if let Ok(mut w) = s.world.get::<&mut water::WaterSurface>(sea) {
        w.settings.set_storminess(0.2);
    }
    add_volume(&mut s, pp_ready_volume());
    s
}
