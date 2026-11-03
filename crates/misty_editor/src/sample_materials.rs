//! MaterialsDemo sample scene in `SampleProject`: a row of spheres showing the material options
//! (Transparent glass, a Fade ghost whose instance colour pulses from Lua, a Cutout lattice and a
//! metallic-mapped sphere), a Cutout fence casting clipped shadows, an Unlit sign and an
//! emission-mapped panel. The textures are generated here too. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_materials_demo`; the other tests check the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::render::RenderingMode;
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef, Scripts, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/MaterialsDemo.scene";
const SCRIPT: &str = "Assets/Scripts/MaterialPulse.lua";
const FLOOR: &str = "Assets/Materials/Checker.mat";

const FENCE_PNG: &str = "Assets/Textures/Fence.png";
const SIGN_PNG: &str = "Assets/Textures/Sign.png";
const GLOW_PNG: &str = "Assets/Textures/PanelGlow.png";
const STRIPES_PNG: &str = "Assets/Textures/MetalStripes.png";

const GLASS: &str = "Assets/Materials/Glass.mat";
const GHOST: &str = "Assets/Materials/Ghost.mat";
const FENCE: &str = "Assets/Materials/Fence.mat";
const SIGN: &str = "Assets/Materials/Sign.mat";
const PANEL: &str = "Assets/Materials/Glow Panel.mat";
const STRIPED: &str = "Assets/Materials/Striped Metal.mat";

/// Pulses the object's own material (`renderer.material` is a per-object copy).
const PULSE_LUA: &str = r#"-- Pulses this object's own material colour and alpha.
-- `renderer.material` is a copy for this object only; other objects using Ghost.mat keep theirs.
local MaterialPulse = { speed = 2, minAlpha = 0.1, maxAlpha = 0.6 }

function MaterialPulse:Start()
    self.renderer = self.gameObject:GetComponent("MeshRenderer")
    self.base = self.renderer.sharedMaterial.color
end

function MaterialPulse:Update()
    local t = (math.sin(Time.time * self.speed) + 1) * 0.5
    local c = self.base
    local a = self.minAlpha + (self.maxAlpha - self.minAlpha) * t
    self.renderer.material.color = Color.new(c.r, c.g + 0.3 * t, c.b, a)
end

return MaterialPulse
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Chain-link fence: diagonal wires, holes fully transparent.
fn fence_png(size: u32) -> RgbaImage {
    let cell = size as f32 / 6.0;
    RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (x as f32 + 0.5, y as f32 + 0.5);
        let d1 = ((u + v) % cell).min(cell - (u + v) % cell);
        let d2 = ((u - v + size as f32) % cell).min(cell - (u - v + size as f32) % cell);
        let frame = x < 6 || y < 6 || x >= size - 6 || y >= size - 6;
        if d1 < 3.5 || d2 < 3.5 || frame {
            let shade = if frame { 120 } else { 175 };
            Rgba([shade, shade, shade + 10, 255])
        } else {
            Rgba([0, 0, 0, 0])
        }
    })
}

/// Hazard sign: yellow with a black border and black chevrons.
fn sign_png(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let border = x < 10 || y < 10 || x >= w - 10 || y >= h - 10;
        // Chevrons pointing right: stripes along |y - h/2| + x.
        let chevron = ((x as i32 + (y as i32 - h as i32 / 2).abs()) / 24) % 2 == 0;
        let inner = x > 24 && x < w - 24 && y > 24 && y < h - 24;
        if border || (inner && chevron) {
            Rgba([20, 20, 20, 255])
        } else {
            Rgba([250, 200, 20, 255])
        }
    })
}

/// Emission map: a grid of lit squares (brighter towards the centre) on black.
fn glow_png(size: u32) -> RgbaImage {
    let cells = 6;
    let cell = size / cells;
    RgbaImage::from_fn(size, size, |x, y| {
        let (cx, cy) = (x / cell, y / cell);
        let (lx, ly) = (x % cell, y % cell);
        let lit = lx > 5 && ly > 5 && lx < cell - 5 && ly < cell - 5 && (cx + cy) % 3 != 0;
        if lit {
            let d = (cx as f32 - 2.5).abs() + (cy as f32 - 2.5).abs();
            let v = (255.0 - d * 28.0) as u8;
            Rgba([v, v, v, 255])
        } else {
            Rgba([0, 0, 0, 255])
        }
    })
}

/// Metallic map: R = metallic (polished bands / painted bands), A = smoothness.
fn stripes_png(size: u32) -> RgbaImage {
    let band = size / 8;
    RgbaImage::from_fn(size, size, |_, y| {
        if (y / band).is_multiple_of(2) {
            Rgba([255, 0, 0, 235])
        } else {
            Rgba([0, 0, 0, 90])
        }
    })
}

fn textures() -> [(&'static str, RgbaImage); 4] {
    [
        (FENCE_PNG, fence_png(256)),
        (SIGN_PNG, sign_png(256, 128)),
        (GLOW_PNG, glow_png(192)),
        (STRIPES_PNG, stripes_png(256)),
    ]
}

fn materials() -> [(&'static str, Material); 6] {
    [
        (
            GLASS,
            Material {
                color: [0.7, 0.85, 1.0, 0.18],
                smoothness: 0.95,
                rendering_mode: RenderingMode::Transparent,
                ..Default::default()
            },
        ),
        (
            GHOST,
            Material {
                color: [0.55, 0.65, 1.0, 0.4],
                smoothness: 0.3,
                rendering_mode: RenderingMode::Fade,
                ..Default::default()
            },
        ),
        (
            FENCE,
            Material {
                albedo_map: Some(FENCE_PNG.into()),
                metallic: 0.6,
                smoothness: 0.55,
                rendering_mode: RenderingMode::Cutout,
                alpha_cutoff: 0.5,
                tiling: [2.0, 1.0],
                double_sided: true,
                ..Default::default()
            },
        ),
        (
            SIGN,
            Material {
                shader: "Unlit/Texture".into(),
                albedo_map: Some(SIGN_PNG.into()),
                ..Default::default()
            },
        ),
        (
            PANEL,
            Material {
                color: [0.08, 0.09, 0.1, 1.0],
                smoothness: 0.7,
                emission_enabled: true,
                emission: [0.25, 0.85, 1.0],
                emission_intensity: 3.0,
                emission_map: Some(GLOW_PNG.into()),
                ..Default::default()
            },
        ),
        (
            STRIPED,
            Material {
                color: [0.95, 0.72, 0.35, 1.0],
                metallic_map: Some(STRIPES_PNG.into()),
                smoothness: 1.0,
                ..Default::default()
            },
        ),
    ]
}

fn add_mesh(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

fn at(x: f32, y: f32, z: f32, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(Vec3::new(x, y, z));
    t.scale = scale;
    t
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("MaterialsDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 4.0, -9.0))
                .with_euler(Vec3::new(18.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 3.0, 0.0))
                .with_euler(Vec3::new(32.0, 155.0, 0.0)),
        );
        s.world.get::<&mut Light>(sun).unwrap().shadows = Shadows::Soft;
    }

    add_mesh(
        &mut s,
        "Floor",
        PrimitiveMesh::Plane,
        at(0.0, 0.0, 6.0, Vec3::new(4.0, 1.0, 3.0)),
        FLOOR,
    );
    // The fence stands behind the row, so the glass and the ghost are seen against it.
    add_mesh(
        &mut s,
        "Fence",
        PrimitiveMesh::Quad,
        at(0.0, 1.25, 4.5, Vec3::new(10.0, 2.5, 1.0)),
        FENCE,
    );
    let ball = Vec3::splat(1.5);
    add_mesh(
        &mut s,
        "Glass Sphere",
        PrimitiveMesh::Sphere,
        at(-5.0, 0.75, 0.0, ball),
        GLASS,
    );
    add_mesh(
        &mut s,
        "Ghost Sphere",
        PrimitiveMesh::Sphere,
        at(-2.5, 0.9, 0.0, ball),
        GHOST,
    );
    let ghost = s.find_by_name("Ghost Sphere").unwrap();
    s.add(
        ghost,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    add_mesh(
        &mut s,
        "Lattice Sphere",
        PrimitiveMesh::Sphere,
        at(0.0, 0.75, 0.0, ball),
        FENCE,
    );
    add_mesh(
        &mut s,
        "Striped Metal Sphere",
        PrimitiveMesh::Sphere,
        at(2.5, 0.75, 0.0, ball),
        STRIPED,
    );
    // Emission-mapped panel on a little stand.
    add_mesh(
        &mut s,
        "Glow Panel",
        PrimitiveMesh::Cube,
        at(5.0, 1.0, 0.3, Vec3::new(1.6, 1.6, 0.12)),
        PANEL,
    );
    // Unlit sign on a post: the same brightness whatever the light.
    add_mesh(
        &mut s,
        "Sign Post",
        PrimitiveMesh::Cylinder,
        at(-1.2, 1.0, 3.2, Vec3::new(0.08, 1.0, 0.08)),
        STRIPED,
    );
    add_mesh(
        &mut s,
        "Unlit Sign",
        PrimitiveMesh::Quad,
        at(-1.2, 2.3, 3.1, Vec3::new(1.6, 0.8, 1.0)),
        SIGN,
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_materials_demo() {
    let root = project();
    for (rel, img) in textures() {
        img.save(root.join(rel)).unwrap();
    }
    for (rel, m) in materials() {
        m.save(&root.join(rel)).unwrap();
    }
    std::fs::write(root.join(SCRIPT), PULSE_LUA).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn materials_demo_matches_its_generator() {
    let root = project();
    for (rel, img) in textures() {
        let file = misty_engine::image::open(root.join(rel))
            .unwrap()
            .into_rgba8();
        assert_eq!(file, img, "{rel}");
    }
    for (rel, m) in materials() {
        assert_eq!(Material::load(&root.join(rel)).unwrap(), m, "{rel}");
    }
    let script = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), PULSE_LUA);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_materials_demo"
    );
    let (_, report) = Scene::from_json("MaterialsDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// Playing: the ghost's own copy pulses while Ghost.mat (and the file) stay as they are.
#[test]
fn materials_demo_pulses_an_instance_material() {
    use misty_engine::assets::AssetCache;
    use misty_engine::audio::Audio;
    use misty_engine::runtime::{Runtime, Time};
    use misty_engine::scripting::{Input, LogKind};
    use misty_engine::settings::ProjectSettings;

    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let (audio, _) = Audio::null(root.clone());
    let mut rt =
        Runtime::start_with_audio(&mut scene, root.clone(), &ProjectSettings::default(), audio);
    let mut time = Time::default();
    let cache = AssetCache::open(&root);
    let ghost = scene.find_by_name("Ghost Sphere").unwrap();
    let alpha = |scene: &Scene| {
        let r = scene.world.get::<&MeshRenderer>(ghost).unwrap();
        misty_engine::render_scene::renderer_material(scene, &r, 0, &cache).color[3]
    };
    let mut seen = Vec::new();
    for _ in 0..90 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        seen.push(alpha(&scene));
    }
    let errors: Vec<_> = rt
        .take_logs()
        .into_iter()
        .filter(|l| l.0 == LogKind::Error)
        .collect();
    assert!(errors.is_empty(), "{errors:?}");
    let (lo, hi) = seen
        .iter()
        .fold((1.0f32, 0.0f32), |(lo, hi), a| (lo.min(*a), hi.max(*a)));
    assert!(hi - lo > 0.2, "alpha pulses: {lo}..{hi}");
    assert_eq!(
        cache.materials[GHOST].color[3], 0.4,
        "the asset keeps its alpha"
    );
}
