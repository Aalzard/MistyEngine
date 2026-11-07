//! LightmapDemo sample scene in `SampleProject`: a Cornell-box-like room (red left wall, green
//! right wall, white floor, ceiling and back wall, two white blocks) lit by a Baked rectangle
//! light in the ceiling, an emissive orange panel on the back wall and a Mixed point light
//! (realtime direct light and shadows, baked bounce light). Everything static is lightmapped:
//! the floor picks up red and green from the walls. A dynamic sphere (not static) is lit in
//! realtime by the Mixed light only.
//!
//! The baked lightmap (`Assets/Scenes/LightmapDemo/Lightmap-0.lightmap`, low resolution to keep
//! it small) is committed so the demo looks right on open. Regenerate scene, materials and
//! lightmap with `cargo test -p misty_editor --release -- --ignored write_lightmap_demo`; the
//! other tests check the committed files still match and render.

use std::path::PathBuf;

use glam::{Quat, Vec3};
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::lightmap::{self, bake};
use misty_engine::render::MaterialParams;
use misty_engine::render_scene::MaterialSource;
use misty_engine::scene::render_settings::{AmbientSource, SkyboxKind};
use misty_engine::scene::{
    Light, LightMode, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene,
    Shadows, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/LightmapDemo.scene";
const WHITE: &str = "Assets/Materials/Lightmap White.mat";
const RED: &str = "Assets/Materials/Lightmap Red.mat";
const GREEN: &str = "Assets/Materials/Lightmap Green.mat";
const PANEL: &str = "Assets/Materials/Lightmap Orange Glow.mat";
const SPHERE: &str = "Assets/Materials/Lightmap Sphere.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 3], smoothness| Material {
        color: [color[0], color[1], color[2], 1.0],
        metallic: 0.0,
        smoothness,
        ..Default::default()
    };
    [
        (WHITE, m([0.78, 0.76, 0.72], 0.1)),
        (RED, m([0.75, 0.08, 0.06], 0.1)),
        (GREEN, m([0.12, 0.6, 0.12], 0.1)),
        (
            PANEL,
            Material {
                emission_enabled: true,
                emission: [1.0, 0.55, 0.15],
                emission_intensity: 4.0,
                ..m([0.2, 0.2, 0.2], 0.3)
            },
        ),
        (SPHERE, m([0.6, 0.62, 0.68], 0.6)),
    ]
}

/// The demo's materials without reading the project.
struct DemoMaterials;

impl MaterialSource for DemoMaterials {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        materials()
            .iter()
            .find(|(p, _)| Some(*p) == rel)
            .map(|(_, m)| m.params())
            .unwrap_or_default()
    }
}

fn object(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    kind: PrimitiveMesh,
    t: Transform,
    mat: &str,
    is_static: bool,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    s.game_object_mut(e).is_static = is_static;
    e
}

fn placed(position: Vec3, rotation: Quat, scale: Vec3) -> Transform {
    Transform {
        position,
        rotation,
        scale,
        ..Default::default()
    }
}

/// A 5 × 5 × 5 m room open towards the camera (−Z), from Planes facing inwards.
fn room(s: &mut Scene) {
    use std::f32::consts::FRAC_PI_2;
    let room = s.spawn("Room", None);
    s.game_object_mut(room).is_static = true;
    let half = Vec3::new(0.5, 1.0, 0.5);
    let walls = [
        ("Floor", WHITE, Vec3::new(0.0, 0.0, 2.5), Quat::IDENTITY),
        (
            "Ceiling",
            WHITE,
            Vec3::new(0.0, 5.0, 2.5),
            Quat::from_rotation_x(std::f32::consts::PI),
        ),
        (
            "Back Wall",
            WHITE,
            Vec3::new(0.0, 2.5, 5.0),
            Quat::from_rotation_x(-FRAC_PI_2),
        ),
        (
            "Left Wall",
            RED,
            Vec3::new(-2.5, 2.5, 2.5),
            Quat::from_rotation_z(-FRAC_PI_2),
        ),
        (
            "Right Wall",
            GREEN,
            Vec3::new(2.5, 2.5, 2.5),
            Quat::from_rotation_z(FRAC_PI_2),
        ),
    ];
    for (name, mat, at, rot) in walls {
        object(
            s,
            name,
            Some(room),
            PrimitiveMesh::Plane,
            placed(at, rot, half),
            mat,
            true,
        );
    }
    let turn = |deg: f32| Quat::from_rotation_y(deg.to_radians());
    object(
        s,
        "Tall Block",
        Some(room),
        PrimitiveMesh::Cube,
        placed(
            Vec3::new(-0.9, 1.5, 3.2),
            turn(18.0),
            Vec3::new(1.3, 3.0, 1.3),
        ),
        WHITE,
        true,
    );
    object(
        s,
        "Short Block",
        Some(room),
        PrimitiveMesh::Cube,
        placed(
            Vec3::new(1.0, 0.7, 1.7),
            turn(-17.0),
            Vec3::new(1.3, 1.4, 1.3),
        ),
        WHITE,
        true,
    );
    // Emissive panel on the back wall (the Quad faces −Z, into the room).
    object(
        s,
        "Glow Panel",
        Some(room),
        PrimitiveMesh::Quad,
        placed(
            Vec3::new(1.2, 2.6, 4.98),
            Quat::IDENTITY,
            Vec3::new(1.4, 0.5, 1.0),
        ),
        PANEL,
        true,
    );
}

fn lights(s: &mut Scene) {
    // Baked rectangle in the ceiling, shining down.
    let e = s.spawn("Ceiling Light", None);
    s.set_transform(
        e,
        placed(
            Vec3::new(0.0, 4.97, 2.5),
            Quat::from_rotation_x(std::f32::consts::FRAC_PI_2),
            Vec3::ONE,
        ),
    );
    s.add(
        e,
        Light {
            color: [1.0, 0.93, 0.82],
            intensity: 30.0,
            range: 12.0,
            width: 1.4,
            height: 1.4,
            mode: LightMode::Baked,
            ..Light::rectangle()
        },
    );
    // Mixed point light: realtime direct light (lights the dynamic sphere), baked bounces.
    let p = s.spawn("Mixed Point Light", None);
    s.set_transform(p, Transform::from_position(Vec3::new(1.2, 3.0, -0.2)));
    s.add(
        p,
        Light {
            kind: LightType::Point,
            color: [0.75, 0.85, 1.0],
            intensity: 4.0,
            range: 9.0,
            shadows: Shadows::Soft,
            mode: LightMode::Mixed,
            ..Default::default()
        },
    );
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.3;
    v.bloom.threshold = 1.2;
    v
}

/// The scene before baking.
fn unbaked_scene() -> Scene {
    let mut s = Scene::default_3d("LightmapDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.5, -5.2))
                .with_euler(Vec3::new(2.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.despawn(sun);
    }
    let rs = &mut s.render_settings;
    rs.skybox.kind = SkyboxKind::SolidColor;
    rs.skybox.color = [0.015, 0.015, 0.02];
    rs.environment_lighting.source = AmbientSource::Color;
    rs.environment_lighting.sky_color = [0.03, 0.03, 0.035];
    rs.reflection_intensity = 0.15;
    let lm = &mut rs.lightmapping;
    lm.resolution = 6.0;
    lm.samples = 256;
    lm.bounces = 3;
    lm.max_size = 256;

    room(&mut s);
    lights(&mut s);
    // The dynamic object: not static, so it is lit in realtime (by the Mixed light).
    object(
        &mut s,
        "Dynamic Sphere",
        None,
        PrimitiveMesh::Sphere,
        placed(Vec3::new(0.6, 0.55, 0.6), Quat::IDENTITY, Vec3::splat(1.1)),
        SPHERE,
        false,
    );
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

/// The demo as committed: renderers pointed at the baked lightmap (layout only, no tracing).
pub fn demo_scene() -> Scene {
    let mut s = unbaked_scene();
    let input = bake::input::gather(&s, &DemoMaterials);
    let atlas = bake::layout(&input);
    let results = bake::receiver_results(&input, &atlas);
    let paths = (0..atlas.count)
        .map(|i| lightmap::asset_path_for(SCENE, i))
        .collect();
    crate::lightmap_tools::assign(&mut s, &results, paths);
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate (use --release)"]
fn write_lightmap_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    let s = demo_scene();
    let input = bake::input::gather(&unbaked_scene(), &DemoMaterials);
    let out = bake::bake(&input, &bake::Progress::default()).unwrap();
    for (i, map) in out.maps.iter().enumerate() {
        map.save(&root.join(lightmap::asset_path_for(SCENE, i)))
            .unwrap();
    }
    s.save(&root.join(SCENE)).unwrap();
}

#[test]
fn lightmap_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_lightmap_demo",
    );
    let (scene, report) = Scene::from_json("LightmapDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // One small lightmap on disk, used by every static renderer and not by the sphere.
    let paths = &scene.render_settings.lightmapping.lightmaps;
    assert_eq!(paths.len(), 1);
    let map = lightmap::LightmapData::load(&root.join(&paths[0])).unwrap();
    assert!(map.width <= 256, "{}", map.width);
    assert!(std::fs::metadata(root.join(&paths[0])).unwrap().len() < 400_000);
    for name in ["Floor", "Left Wall", "Tall Block", "Glow Panel"] {
        let e = scene.find_by_name(name).unwrap();
        assert_eq!(
            scene.world.get::<&MeshRenderer>(e).unwrap().lightmap_index,
            0
        );
    }
    let sphere = scene.find_by_name("Dynamic Sphere").unwrap();
    assert_eq!(
        scene
            .world
            .get::<&MeshRenderer>(sphere)
            .unwrap()
            .lightmap_index,
        -1
    );
    // The Baked light no longer lights in realtime; the Mixed one does.
    let lights = misty_engine::render_scene::lights(&scene);
    assert_eq!(lights.len(), 1);
    assert_eq!(lights[0].kind, LightType::Point);
}

/// The committed lightmap: the floor next to the red wall is redder than next to the green
/// one (colour bleeding), and the floor is lit.
#[test]
fn lightmap_demo_bleeds_wall_colours() {
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let map = lightmap::LightmapData::load(&root.join(lightmap::asset_path_for(SCENE, 0))).unwrap();
    let floor = scene.find_by_name("Floor").unwrap();
    let st = scene
        .world
        .get::<&MeshRenderer>(floor)
        .unwrap()
        .lightmap_scale_offset;
    // Floor plane local x, z → uv2 (see lightmap::uv::primitive).
    let at = |x: f32, z: f32| {
        let m = lightmap::uv::MARGIN;
        let uv = glam::Vec2::new((5.0 - x) / 10.0, (5.0 - z) / 10.0);
        let uv2 = glam::Vec2::splat(m * 0.5) + uv * (1.0 - m);
        map.sample(uv2 * glam::Vec2::new(st[0], st[1]) + glam::Vec2::new(st[2], st[3]))
    };
    // World x = local x × 0.5 (the floor is scaled to 5 m).
    let left = at(-4.4, -3.0);
    let right = at(4.4, -3.0);
    eprintln!("floor by the red wall {left}, by the green wall {right}");
    assert!(left.x / left.y > right.x / right.y * 1.3);
    assert!(right.y / right.x > left.y / left.x * 1.3);
    assert!(left.max_element() > 0.05);
}

/// On a GPU: the demo renders lit and not flat, and the lightmapped image differs from the
/// same frame without lightmaps.
#[test]
fn lightmap_demo_renders_lightmapped() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let mut view = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
    // Load the lightmaps from this project (the global project root may point elsewhere).
    view.environment.lightmaps = lightmap::LightmapSet {
        maps: scene
            .render_settings
            .lightmapping
            .lightmaps
            .iter()
            .map(|rel| {
                lightmap::LightmapData::load(&root.join(rel))
                    .ok()
                    .map(|d| lightmap::LightmapHandle(std::sync::Arc::new(d)))
            })
            .collect(),
        debug: lightmap::LightmapDebug::Off,
    };
    assert!(!view.environment.lightmaps.is_empty());
    let lit = crate::sample_render::render(&gpu, &view.frame(), 2);
    let mut unlit_env = view.environment.clone();
    unlit_env.lightmaps = Default::default();
    let unlit = crate::sample_render::render(
        &gpu,
        &misty_engine::render::Frame {
            environment: &unlit_env,
            ..view.frame()
        },
        2,
    );
    let luma = |i: &crate::sample_render::Image| -> Vec<f32> {
        i.pixels
            .chunks(4)
            .map(|p| (p[0] as f32 + p[1] as f32 + p[2] as f32) / 3.0)
            .collect()
    };
    let (a, b) = (luma(&lit), luma(&unlit));
    let mean = a.iter().sum::<f32>() / a.len() as f32;
    let var = a.iter().map(|v| (v - mean).powi(2)).sum::<f32>() / a.len() as f32;
    let diff = a
        .iter()
        .zip(&b)
        .filter(|(x, y)| (*x - *y).abs() > 6.0)
        .count();
    eprintln!(
        "mean {mean}, std {}, pixels changed by lightmaps {diff}",
        var.sqrt()
    );
    assert!(mean > 25.0, "lit: {mean}");
    assert!(var.sqrt() > 12.0, "not flat");
    assert!(diff > a.len() / 5, "lightmaps change the image: {diff}");
    // Floor near the red wall is reddish in the lightmapped render.
    let cam = &view.camera;
    let p = Vec3::new(-2.1, 0.0, 1.5);
    let ndc = cam
        .view_projection(crate::sample_render::WIDTH as f32 / crate::sample_render::HEIGHT as f32)
        .project_point3(p);
    let x = ((ndc.x * 0.5 + 0.5) * crate::sample_render::WIDTH as f32) as usize;
    let y = ((0.5 - ndc.y * 0.5) * crate::sample_render::HEIGHT as f32) as usize;
    let i = (y * crate::sample_render::WIDTH as usize + x) * 4;
    let px = &lit.pixels[i..i + 3];
    assert!(px[0] > px[1], "floor by the red wall: {px:?}");
}
