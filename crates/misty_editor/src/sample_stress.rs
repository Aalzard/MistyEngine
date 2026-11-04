//! Procedural StressDemo scene in `SampleProject` for the Profiler and the rendering
//! optimisations: 5,040 cubes and spheres on a 72 × 70 grid in four shared materials, every
//! 25th one spinning (`Rotator.lua`), a camera orbiting the field (`CameraOrbit.lua`), a
//! shadowed sun, a shadowed spot light and two point lights. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_stress_demo`; the other tests check the
//! committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Light, LightType, LodFadeMode, LodGroup, LodLevel, MeshFilter, MeshRenderer, PrimitiveMesh,
    Scene, ScriptRef, ScriptValue, Scripts, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/StressDemo.scene";
const STATIC_SCENE: &str = "Assets/Scenes/StressStatic.scene";
const ORBIT: &str = "Assets/Scripts/CameraOrbit.lua";
const ROTATOR: &str = "Assets/Scripts/Rotator.lua";
const GROUND: &str = "Assets/Materials/Sand.mat";
/// The demo's own materials: (file, colour, metallic, smoothness).
const MATERIALS: [(&str, [f32; 4], f32, f32); 4] = [
    (
        "Assets/Materials/Stress Coral.mat",
        [0.95, 0.42, 0.36, 1.0],
        0.0,
        0.35,
    ),
    (
        "Assets/Materials/Stress Teal.mat",
        [0.2, 0.7, 0.68, 1.0],
        0.0,
        0.55,
    ),
    (
        "Assets/Materials/Stress Gold.mat",
        [0.95, 0.75, 0.3, 1.0],
        0.85,
        0.7,
    ),
    (
        "Assets/Materials/Stress Slate.mat",
        [0.42, 0.46, 0.55, 1.0],
        0.2,
        0.3,
    ),
];
const COLUMNS: i32 = 72;
const ROWS: i32 = 70;
const SPACING: f32 = 1.6;

const ORBIT_LUA: &str = r#"-- CameraOrbit: circles a point at a fixed height, always looking at it.
local CameraOrbit = {
    center = Vector3.new(0, 0, 0),
    radius = 70,
    height = 30,
    speed = 0.15,                    -- radians per second
}

function CameraOrbit:Update()
    local a = Time.time * self.speed
    self.transform.position = Vector3.new(
        self.center.x + Mathf.Sin(a) * self.radius,
        self.center.y + self.height,
        self.center.z - Mathf.Cos(a) * self.radius)
    self.transform:LookAt(self.center)
end

return CameraOrbit
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn material(i: usize) -> Material {
    let (_, color, metallic, smoothness) = MATERIALS[i];
    Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    }
}

fn script(path: &str, fields: &[(&str, ScriptValue)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            fields: fields
                .iter()
                .map(|(k, v)| (k.to_string(), v.clone()))
                .collect(),
            ..Default::default()
        }],
    }
}

fn light(s: &mut Scene, name: &str, light: Light, t: Transform) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, light);
}

/// Deterministic 0..1 value per grid cell (heights and material choice).
fn hash(x: i32, z: i32) -> f32 {
    let n = (x.wrapping_mul(73_856_093) ^ z.wrapping_mul(19_349_663)) as u32;
    (n.wrapping_mul(2_654_435_761) >> 8) as f32 / (1u32 << 24) as f32
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("StressDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 30.0, -70.0))
                .with_euler(Vec3::new(23.0, 0.0, 0.0)),
        );
        s.add(
            cam,
            script(
                ORBIT,
                &[
                    ("radius", ScriptValue::Number(70.0)),
                    ("height", ScriptValue::Number(30.0)),
                ],
            ),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.world.get::<&mut Light>(sun).unwrap().shadows = Shadows::Soft;
    }
    let spot = Light {
        kind: LightType::Spot,
        color: [1.0, 0.85, 0.6],
        intensity: 3.0,
        range: 30.0,
        spot_angle: 50.0,
        shadows: Shadows::Hard,
        ..Default::default()
    };
    light(
        &mut s,
        "Spot Light",
        spot,
        Transform::from_position(Vec3::new(-20.0, 14.0, -10.0))
            .with_euler(Vec3::new(70.0, 20.0, 0.0)),
    );
    for (name, pos, color) in [
        (
            "Point Light Blue",
            Vec3::new(25.0, 4.0, 20.0),
            [0.4, 0.6, 1.0],
        ),
        (
            "Point Light Pink",
            Vec3::new(-25.0, 4.0, 25.0),
            [1.0, 0.45, 0.7],
        ),
    ] {
        let point = Light {
            kind: LightType::Point,
            color,
            intensity: 2.5,
            range: 18.0,
            ..Default::default()
        };
        light(&mut s, name, point, Transform::from_position(pos));
    }

    let ground = s.spawn("Ground", None);
    let t = Transform {
        scale: Vec3::new(13.0, 1.0, 13.0),
        ..Default::default()
    };
    s.set_transform(ground, t);
    s.add(ground, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        ground,
        MeshRenderer {
            material: Some(GROUND.into()),
            ..Default::default()
        },
    );

    let field = s.spawn("Field", None);
    for z in 0..ROWS {
        for x in 0..COLUMNS {
            let h = hash(x, z);
            let mesh = if (x + z) % 2 == 0 {
                PrimitiveMesh::Cube
            } else {
                PrimitiveMesh::Sphere
            };
            let size = 0.6 + h * 0.5;
            let pos = Vec3::new(
                (x as f32 - (COLUMNS - 1) as f32 * 0.5) * SPACING,
                size * 0.5,
                (z as f32 - (ROWS - 1) as f32 * 0.5) * SPACING,
            );
            let name = format!("{} {x},{z}", mesh.name());
            let e = s.spawn(&name, Some(field));
            let mut t = Transform::from_position(pos).with_euler(Vec3::new(0.0, h * 90.0, 0.0));
            t.scale = Vec3::splat(size);
            s.set_transform(e, t);
            s.add(e, MeshFilter::primitive(mesh));
            let m = ((h * 7.0) as usize + x as usize) % MATERIALS.len();
            s.add(
                e,
                MeshRenderer {
                    material: Some(MATERIALS[m].0.into()),
                    ..Default::default()
                },
            );
            if (x * ROWS + z) % 25 == 0 {
                s.add(
                    e,
                    script(
                        ROTATOR,
                        &[("speed", ScriptValue::Number(45.0 + 90.0 * h as f64))],
                    ),
                );
            }
        }
    }
    s
}

/// "Stress Static": the same field with every object that doesn't spin (and the ground)
/// marked Static, so Play mode merges them into chunked static batches; plus a row of
/// LOD Groups (sphere → capsule → cube, cross-fading) along the near edge.
pub fn static_scene() -> Scene {
    let mut s = demo_scene();
    s.name = "StressStatic".into();
    let spinners: Vec<_> = s
        .world
        .query::<(Entity, &Scripts)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    let field = s.find_by_name("Field").expect("field");
    for e in s.children(field) {
        if !spinners.contains(&e) {
            s.game_object_mut(e).is_static = true;
        }
    }
    if let Some(g) = s.find_by_name("Ground") {
        s.game_object_mut(g).is_static = true;
    }
    let lods = s.spawn("LOD Demo", None);
    for i in 0..12 {
        let group = s.spawn(&format!("LOD Group {i}"), Some(lods));
        let x = (i as f32 - 5.5) * 6.0;
        s.set_transform(group, Transform::from_position(Vec3::new(x, 1.5, -62.0)));
        let mut g = LodGroup {
            fade_mode: LodFadeMode::CrossFade,
            fade_width: 0.25,
            ..Default::default()
        };
        g.lods = vec![0.12, 0.05, 0.015]
            .into_iter()
            .zip([
                PrimitiveMesh::Sphere,
                PrimitiveMesh::Capsule,
                PrimitiveMesh::Cube,
            ])
            .enumerate()
            .map(|(k, (h, mesh))| {
                let e = s.spawn(&format!("LOD{k} {}", mesh.name()), Some(group));
                let t = Transform {
                    scale: Vec3::splat(2.5),
                    ..Default::default()
                };
                s.set_transform(e, t);
                s.add(e, MeshFilter::primitive(mesh));
                s.add(
                    e,
                    MeshRenderer {
                        material: Some(MATERIALS[k % MATERIALS.len()].0.into()),
                        ..Default::default()
                    },
                );
                LodLevel {
                    screen_height: h,
                    renderers: vec![e],
                }
            })
            .collect();
        s.add(group, g);
    }
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_stress_demo() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        material(i).save(&root.join(path)).unwrap();
    }
    std::fs::write(root.join(ORBIT), ORBIT_LUA).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
    static_scene().save(&root.join(STATIC_SCENE)).unwrap();
}

#[test]
fn stress_static_matches_its_generator_and_round_trips() {
    let text = std::fs::read_to_string(project().join(STATIC_SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == static_scene().to_json(),
        "regenerate with write_stress_demo"
    );
    let (scene, report) = Scene::from_json("StressStatic", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let statics = scene
        .world
        .query::<&misty_engine::scene::GameObject>()
        .iter()
        .filter(|g| g.is_static)
        .count();
    assert!(statics > 4000, "{statics}");
    // LOD references survive the file.
    let groups: Vec<LodGroup> = scene.world.query::<&LodGroup>().iter().cloned().collect();
    assert_eq!(groups.len(), 12);
    for g in &groups {
        for l in &g.lods {
            assert_eq!(l.renderers.len(), 1);
            assert!(scene.world.get::<&MeshRenderer>(l.renderers[0]).is_ok());
        }
    }
}

#[test]
fn stress_demo_matches_its_generator() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), material(i));
    }
    let lua = std::fs::read_to_string(root.join(ORBIT)).unwrap();
    assert_eq!(lua.replace("\r\n", "\n"), ORBIT_LUA);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_stress_demo"
    );
    let (scene, report) = Scene::from_json("StressDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let meshes = scene.world.query::<&MeshFilter>().iter().count();
    assert_eq!(meshes, (COLUMNS * ROWS) as usize + 1);
}

/// Playing: the camera orbits and the spinners turn, without script errors.
#[test]
fn stress_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    let cam = scene.find_by_name("Main Camera").unwrap();
    let spinner = scene.find_by_name("Cube 0,0").unwrap();
    let start = (scene.world_position(cam), scene.transform(spinner).rotation);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..60 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
    }
    let cam_pos = scene.world_position(cam);
    assert!(cam_pos.distance(start.0) > 1.0, "camera orbits");
    assert!((cam_pos.y - 30.0).abs() < 1e-3);
    assert!(
        scene.transform(spinner).rotation.angle_between(start.1) > 0.3,
        "spinner turns"
    );
}
