//! Procedural WaterDemo and ShaderDemo scenes in `SampleProject`.
//!
//! WaterDemo: an FFT ocean to the horizon at sunset around a sand island with rocks, a palm and
//! a hologram beacon (custom shader), floating crates (Floater + Rigidbody) and a slowly
//! orbiting camera (`CameraOrbit.lua`). ShaderDemo: three custom `.shader` materials (a lit
//! hologram, a vertex-displaced wobble, an unlit scrolling gradient) and a scene-reading force
//! field. Regenerate with `cargo test -p misty_editor -- --ignored write_water_demo`; the other
//! tests check the committed files and that the water scene plays.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::scene::{
    BoxCollider, Camera, Light, MeshFilter, MeshRenderer, PrimitiveMesh, Rigidbody, Scene,
    ScriptRef, ScriptValue, Scripts, Shadows, Transform,
};
use misty_engine::water::{self, Floater, WaterKind, WaterSurface};

const WATER_SCENE: &str = "Assets/Scenes/WaterDemo.scene";
const SHADER_SCENE: &str = "Assets/Scenes/ShaderDemo.scene";
const ORBIT: &str = "Assets/Scripts/CameraOrbit.lua";
const SAND: &str = "Assets/Materials/Sand.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";

/// The demo's shaders: (file, source).
const SHADERS: [(&str, &str); 4] = [
    ("Assets/Shaders/Hologram.shader", HOLOGRAM),
    ("Assets/Shaders/Wobble.shader", WOBBLE),
    ("Assets/Shaders/Scroll Gradient.shader", GRADIENT),
    ("Assets/Shaders/Force Field.shader", FORCE_FIELD),
];

const HOLOGRAM: &str = r#"// Hologram: see-through, glowing, with scan lines rolling upwards and a bright rim.
// @shader surface
// @blend alpha
// @cull off
// @property _Color color "Color" = (0.25, 0.85, 1.0, 1.0)
// @property _RimPower range(0.5, 8) "Rim Sharpness" = 2.5 // Higher = thinner bright edge
// @property _LineDensity range(1, 80) "Lines per Metre" = 18
// @property _ScrollSpeed range(0, 3) "Scroll Speed" = 0.4 // Metres per second
// @property _Glow range(0, 10) = 2.5 // Emission strength (above 1 blooms)
// @property _Flicker range(0, 1) = 0.15

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let rim = pow(1.0 - abs(dot(input.world_normal, input.view_dir)), material._RimPower);
    let y = (input.world_pos.y - input.time * material._ScrollSpeed) * material._LineDensity;
    let band = smoothstep(0.55, 1.0, 0.5 + 0.5 * sin(y * 6.2831));
    let flicker = 1.0 - material._Flicker * (0.5 + 0.5 * sin(input.time * 37.0) * sin(input.time * 11.0));
    let c = material._Color.rgb;
    o.albedo = c * 0.05;
    o.smoothness = 0.9;
    o.emission = c * (rim * 1.5 + band * 0.5 + 0.1) * material._Glow * flicker;
    o.alpha = clamp((rim + band * 0.4 + 0.08) * material._Color.a, 0.0, 1.0);
    return o;
}
"#;

const WOBBLE: &str = r#"// Wobble: vertices pushed along their normals by moving waves (vertex displacement).
// @shader surface
// @property _ColorA color "Color A" = (1.0, 0.45, 0.2, 1)
// @property _ColorB color "Color B" = (0.95, 0.2, 0.55, 1)
// @property _Amount range(0, 0.3) = 0.08 // How far vertices move
// @property _Frequency range(1, 20) = 7
// @property _Speed range(0, 6) = 2
// @property _Smoothness range(0, 1) = 0.7

fn wave(p: vec3<f32>) -> f32 {
    return sin(p.y * material._Frequency + globals.time.x * material._Speed)
        * cos(p.x * material._Frequency * 0.7 - globals.time.x * material._Speed * 0.6);
}

fn vertex(v: VertexData) -> VertexData {
    var o = v;
    o.position += v.normal * wave(v.position) * material._Amount;
    return o;
}

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let t = 0.5 + 0.5 * sin(input.world_pos.y * 3.0 + input.time);
    o.albedo = mix(material._ColorA.rgb, material._ColorB.rgb, t);
    o.smoothness = material._Smoothness;
    return o;
}
"#;

const GRADIENT: &str = r#"// Scroll Gradient: unlit stripes of two colours sliding across the surface.
// @shader unlit
// @property _ColorA color "Color A" = (0.1, 0.9, 0.6, 1)
// @property _ColorB color "Color B" = (0.15, 0.25, 0.95, 1)
// @property _Stripes range(1, 30) = 6
// @property _Speed range(-3, 3) = 0.5 // Stripes per second

fn fragment(input: SurfaceInput) -> vec4<f32> {
    let s = fract((input.uv.x + input.uv.y) * material._Stripes * 0.5 - input.time * material._Speed);
    let t = smoothstep(0.35, 0.5, s) - smoothstep(0.85, 1.0, s);
    let c = mix(material._ColorA.rgb, material._ColorB.rgb, t);
    return vec4<f32>(apply_fog(c, input.world_pos), 1.0);
}
"#;

const FORCE_FIELD: &str = r#"// Force Field: bends the view of what is behind it and glows where it touches the scene.
// Transparent queue, so it can read the opaque scene's colour and depth.
// @shader surface
// @blend premultiply
// @property _Color color "Color" = (0.4, 0.7, 1.0, 1)
// @property _Distortion range(0, 0.1) = 0.03
// @property _EdgeWidth range(0.05, 2) "Contact Width" = 0.4 // Glow band where it meets geometry (m)

fn surface(input: SurfaceInput) -> SurfaceOutput {
    var o = default_output(input);
    let rim = pow(1.0 - abs(dot(input.world_normal, input.view_dir)), 3.0);
    let wobble = vec2<f32>(sin(input.world_pos.y * 9.0 + input.time * 2.0), cos(input.world_pos.x * 9.0 + input.time * 1.7));
    let behind = scene_color_at(input.screen_uv + wobble * material._Distortion * (0.3 + rim));
    let gap = scene_distance(input.screen_uv) - distance(input.world_pos, view.camera_pos.xyz);
    let contact = 1.0 - clamp(gap / material._EdgeWidth, 0.0, 1.0);
    o.albedo = vec3<f32>(0.0);
    o.smoothness = 0.95;
    o.emission = behind * 0.9 + material._Color.rgb * (rim * 1.5 + contact * 3.0);
    o.alpha = 0.0;
    return o;
}
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Demo materials: (file, material).
fn materials() -> Vec<(&'static str, Material)> {
    let custom = |shader: &str| Material {
        shader: shader.into(),
        ..Default::default()
    };
    vec![
        ("Assets/Materials/Hologram.mat", custom(SHADERS[0].0)),
        ("Assets/Materials/Wobble.mat", custom(SHADERS[1].0)),
        ("Assets/Materials/Scroll Gradient.mat", custom(SHADERS[2].0)),
        ("Assets/Materials/Force Field.mat", custom(SHADERS[3].0)),
        (
            "Assets/Materials/Island Rock.mat",
            Material {
                color: [0.36, 0.33, 0.3, 1.0],
                smoothness: 0.25,
                ..Default::default()
            },
        ),
        (
            "Assets/Materials/Palm Leaves.mat",
            Material {
                color: [0.18, 0.5, 0.16, 1.0],
                smoothness: 0.3,
                ..Default::default()
            },
        ),
        (
            "Assets/Materials/Seabed.mat",
            Material {
                color: [0.78, 0.7, 0.5, 1.0],
                smoothness: 0.1,
                ..Default::default()
            },
        ),
    ]
}

fn shape(
    s: &mut Scene,
    name: &str,
    mesh: PrimitiveMesh,
    material: &str,
    t: Transform,
    parent: Option<misty_engine::hecs::Entity>,
) -> misty_engine::hecs::Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    e
}

fn scaled(pos: Vec3, scale: Vec3) -> Transform {
    Transform {
        position: pos,
        scale,
        ..Default::default()
    }
}

fn orbit(radius: f64, height: f64, speed: f64, center: Vec3) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: ORBIT.into(),
            fields: [
                ("radius", ScriptValue::Number(radius)),
                ("height", ScriptValue::Number(height)),
                ("speed", ScriptValue::Number(speed)),
                ("center", ScriptValue::Vector3(center.to_array())),
            ]
            .into_iter()
            .map(|(k, v)| (k.to_owned(), v))
            .collect(),
            ..Default::default()
        }],
    }
}

pub fn water_scene() -> Scene {
    let mut s = Scene::default_3d("WaterDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 6.0, -48.0))
                .with_euler(Vec3::new(8.0, 0.0, 0.0)),
        );
        s.world.get::<&mut Camera>(cam).unwrap().far = 5000.0;
        s.add(cam, orbit(48.0, 6.0, 0.04, Vec3::new(0.0, 1.5, 0.0)));
    }
    // Low, warm sunset sun.
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(7.0, 200.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.7, 0.45];
        l.intensity = 1.4;
        l.shadows = Shadows::Soft;
    }
    let sea = water::spawn_water(&mut s, "Ocean", WaterKind::Ocean, Vec3::ZERO, Vec2::ZERO);
    s.world
        .get::<&mut WaterSurface>(sea)
        .unwrap()
        .settings
        .set_storminess(0.3);

    let island = s.spawn("Island", None);
    shape(
        &mut s,
        "Sand",
        PrimitiveMesh::Sphere,
        SAND,
        scaled(Vec3::new(0.0, -3.2, 0.0), Vec3::new(70.0, 10.0, 70.0)),
        Some(island),
    );
    shape(
        &mut s,
        "Seabed",
        PrimitiveMesh::Plane,
        "Assets/Materials/Seabed.mat",
        scaled(Vec3::new(0.0, -14.0, 0.0), Vec3::new(30.0, 1.0, 30.0)),
        Some(island),
    );
    for (i, (pos, size)) in [
        (Vec3::new(14.0, 1.0, -18.0), Vec3::new(4.0, 3.0, 3.5)),
        (Vec3::new(17.5, 0.2, -14.0), Vec3::new(2.5, 2.0, 2.2)),
        (Vec3::new(-20.0, 0.4, 10.0), Vec3::new(5.0, 3.5, 4.0)),
        (Vec3::new(-24.0, -0.5, 16.0), Vec3::new(3.0, 2.5, 3.0)),
    ]
    .into_iter()
    .enumerate()
    {
        shape(
            &mut s,
            &format!("Rock {}", i + 1),
            PrimitiveMesh::Sphere,
            "Assets/Materials/Island Rock.mat",
            scaled(pos, size),
            Some(island),
        );
    }
    let palm = s.spawn("Palm", Some(island));
    s.set_transform(
        palm,
        Transform::from_position(Vec3::new(-4.0, 1.5, 3.0)).with_euler(Vec3::new(0.0, 0.0, 8.0)),
    );
    shape(
        &mut s,
        "Trunk",
        PrimitiveMesh::Cylinder,
        WOOD,
        scaled(Vec3::new(0.0, 3.0, 0.0), Vec3::new(0.45, 3.0, 0.45)),
        Some(palm),
    );
    shape(
        &mut s,
        "Leaves",
        PrimitiveMesh::Sphere,
        "Assets/Materials/Palm Leaves.mat",
        scaled(Vec3::new(0.3, 6.3, 0.0), Vec3::new(5.0, 1.2, 5.0)),
        Some(palm),
    );
    // Custom-shader beacon on the beach.
    shape(
        &mut s,
        "Hologram Beacon",
        PrimitiveMesh::Cylinder,
        "Assets/Materials/Hologram.mat",
        scaled(Vec3::new(6.0, 3.5, -6.0), Vec3::new(1.6, 2.0, 1.6)),
        None,
    );

    // Floating crates around the island, dropped from just above the water.
    let crates = s.spawn("Crates", None);
    for i in 0..7 {
        let a = i as f32 / 7.0 * std::f32::consts::TAU + 0.4;
        let r = 31.0 + (i % 3) as f32 * 4.0;
        let pos = Vec3::new(a.sin() * r, 1.5 + (i % 2) as f32, a.cos() * r);
        let size = 1.0 + (i % 3) as f32 * 0.3;
        let mut t = scaled(pos, Vec3::splat(size));
        t.set_euler(Vec3::new(10.0 * i as f32, 37.0 * i as f32, 5.0));
        let c = shape(
            &mut s,
            &format!("Crate {}", i + 1),
            PrimitiveMesh::Cube,
            WOOD,
            t,
            Some(crates),
        );
        s.add(
            c,
            Rigidbody {
                mass: 30.0 * size * size * size,
                ..Default::default()
            },
        );
        s.add(c, BoxCollider::default());
        s.add(c, Floater::default());
    }
    s
}

pub fn shader_scene() -> Scene {
    let mut s = Scene::default_3d("ShaderDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 3.0, -9.0))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
    }
    shape(
        &mut s,
        "Ground",
        PrimitiveMesh::Plane,
        SAND,
        scaled(Vec3::ZERO, Vec3::new(3.0, 1.0, 3.0)),
        None,
    );
    for (i, (name, mesh, mat)) in [
        ("Hologram", PrimitiveMesh::Capsule, "Hologram"),
        ("Wobble", PrimitiveMesh::Sphere, "Wobble"),
        ("Scroll Gradient", PrimitiveMesh::Cube, "Scroll Gradient"),
    ]
    .into_iter()
    .enumerate()
    {
        let x = (i as f32 - 1.0) * 3.0;
        shape(
            &mut s,
            name,
            mesh,
            &format!("Assets/Materials/{mat}.mat"),
            scaled(Vec3::new(x, 1.2, 0.0), Vec3::splat(1.6)),
            None,
        );
    }
    // The force field half-covers a crate, so the contact glow and distortion show.
    shape(
        &mut s,
        "Crate",
        PrimitiveMesh::Cube,
        WOOD,
        scaled(Vec3::new(0.0, 0.5, 3.2), Vec3::ONE),
        None,
    );
    shape(
        &mut s,
        "Force Field",
        PrimitiveMesh::Sphere,
        "Assets/Materials/Force Field.mat",
        scaled(Vec3::new(0.0, 0.0, 3.2), Vec3::splat(3.0)),
        None,
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_water_demo() {
    let root = project();
    std::fs::create_dir_all(root.join("Assets/Shaders")).unwrap();
    for (path, src) in SHADERS {
        std::fs::write(root.join(path), src).unwrap();
    }
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    water_scene().save(&root.join(WATER_SCENE)).unwrap();
    shader_scene().save(&root.join(SHADER_SCENE)).unwrap();
}

#[test]
fn water_demo_matches_its_generator() {
    let root = project();
    for (path, src) in SHADERS {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        assert_eq!(text.replace("\r\n", "\n"), src, "{path}");
        misty_engine::render::custom_shader::check(src).unwrap_or_else(|e| panic!("{path}: {e}"));
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    for (path, scene) in [(WATER_SCENE, water_scene()), (SHADER_SCENE, shader_scene())] {
        let text = std::fs::read_to_string(root.join(path)).unwrap();
        crate::test_util::assert_json_close(
            &text,
            &scene.to_json(),
            format!("regenerate {path} with write_water_demo"),
        );
        let (_, report) = Scene::from_json("Demo", &text).unwrap();
        assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    }
}

/// Playing: the crates fall into the sea and float, the camera orbits, no script errors.
#[test]
fn water_demo_plays_and_crates_float() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (mut scene, _) = Scene::load(&root.join(WATER_SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let crate1 = scene.find_by_name("Crate 1").unwrap();
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..600 {
        time.advance(1.0 / 60.0);
        misty_engine::clock::set_play_time(time.time);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
    }
    let p = scene.world_position(crate1);
    let sea = water::height_at(&scene, p.x, p.z).unwrap();
    assert!(
        (p.y - sea).abs() < 1.5,
        "crate floats at the surface: {p} vs {sea}"
    );
}
