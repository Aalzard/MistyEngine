//! Procedural SplinesDemo scene in `SampleProject`: a winding road (Spline Extrude, Road
//! profile) with lamp posts along one side (Spline Instantiate of a prefab) and traffic cones
//! along the other (random scale), a car driving round it (Spline Animate by speed), and a
//! closed neon tube loop (Circle profile) with an orb ping-ponging along it (Ease In Out).
//! `SplineRider.lua` shows the Lua API: it reads the road length and pauses / resumes the car
//! with P.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_splines_demo`; the other
//! tests check the committed files still match, and play the scene.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::scene::prefab::Prefab;
use misty_engine::scene::{
    BoxCollider, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef,
    Scripts, Shadows, Transform,
};
use misty_engine::splines::components::{
    AnimateMethod, Easing, ExtrudeProfile, InstantiateItem, InstantiateMethod, LoopMode,
};
use misty_engine::splines::{
    Knot, Spline, SplineAnimate, SplineContainer, SplineExtrude, SplineInstantiate,
};

const SCENE: &str = "Assets/Scenes/SplinesDemo.scene";
const LAMP_PREFAB: &str = "Assets/Prefabs/Spline Lamp Post.prefab";
const RIDER: &str = "Assets/Scripts/SplineRider.lua";
const ASPHALT: &str = "Assets/Materials/Spline Asphalt.mat";
const GRASS: &str = "Assets/Materials/Spline Grass.mat";
const NEON: &str = "Assets/Materials/Spline Neon.mat";
const BULB: &str = "Assets/Materials/Spline Bulb.mat";
const ORB: &str = "Assets/Materials/Spline Orb.mat";
// Existing sample materials.
const IRON: &str = "Assets/Materials/Lamp Iron.mat";
const PAINT: &str = "Assets/Materials/Car Paint.mat";
const GLASS: &str = "Assets/Materials/Car Glass.mat";
const CONE: &str = "Assets/Materials/Traffic Cone.mat";

const RIDER_SRC: &str = r#"-- SplineRider: reads the spline the car follows and pauses / resumes it with P.
local SplineRider = {
    road = "Winding Road",
}

function SplineRider:Start()
    self.anim = self.gameObject:GetComponent("SplineAnimate")
    local road = GameObject.Find(self.road):GetComponent("SplineContainer")
    local start = road:EvaluatePosition(0)
    Debug.Log(string.format("Road: %.1f m, %d knots, starts at (%.1f, %.1f)",
        road:CalculateLength(), road:KnotCount(), start.x, start.z))
end

function SplineRider:Update()
    if Input.GetKeyDown("p") then
        if self.anim.isPlaying then self.anim:Pause() else self.anim:Play() end
    end
end

return SplineRider
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    let glow = |color: [f32; 3], intensity| Material {
        emission_enabled: true,
        emission: color,
        emission_intensity: intensity,
        ..m([color[0], color[1], color[2], 1.0], 0.0, 0.8)
    };
    [
        (
            ASPHALT,
            Material {
                // The checker shows the UVs running along the road.
                albedo_map: Some("Assets/Textures/Checker.png".into()),
                ..m([0.32, 0.32, 0.34, 1.0], 0.0, 0.35)
            },
        ),
        (GRASS, m([0.24, 0.42, 0.16, 1.0], 0.0, 0.15)),
        (NEON, glow([0.15, 0.85, 1.0], 2.5)),
        (BULB, glow([1.0, 0.82, 0.5], 4.0)),
        (ORB, glow([1.0, 0.3, 0.75], 3.0)),
    ]
}

fn mesh(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    p: PrimitiveMesh,
    t: Transform,
    mat: &str,
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(p));
    s.add(
        e,
        MeshRenderer {
            material: Some(mat.into()),
            ..Default::default()
        },
    );
    e
}

fn scaled(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

/// The lamp post prefab: a pole, an arm reaching over the road (−X) and a glowing bulb.
fn lamp_prefab() -> Prefab {
    let mut s = Scene::new("Lamp");
    let root = s.spawn("Spline Lamp Post", None);
    mesh(
        &mut s,
        "Pole",
        Some(root),
        PrimitiveMesh::Cylinder,
        scaled(Vec3::new(0.0, 2.5, 0.0), Vec3::new(0.18, 2.5, 0.18)),
        IRON,
    );
    mesh(
        &mut s,
        "Arm",
        Some(root),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(-0.7, 4.95, 0.0), Vec3::new(1.5, 0.12, 0.12)),
        IRON,
    );
    let bulb = mesh(
        &mut s,
        "Bulb",
        Some(root),
        PrimitiveMesh::Sphere,
        scaled(Vec3::new(-1.35, 4.75, 0.0), Vec3::splat(0.45)),
        BULB,
    );
    s.add(
        bulb,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.8, 0.55],
            intensity: 2.5,
            range: 11.0,
            shadows: Shadows::None,
            ..Default::default()
        },
    );
    Prefab::from_scene(&s, root)
}

fn road_spline() -> Spline {
    let pts = [
        (-42.0, -18.0),
        (-24.0, -26.0),
        (-4.0, -14.0),
        (8.0, 6.0),
        (2.0, 26.0),
        (-16.0, 38.0),
        (-34.0, 30.0),
        (-46.0, 12.0),
    ];
    Spline::new(
        pts.iter()
            .map(|(x, z)| Knot::at(Vec3::new(*x, 0.03, *z)))
            .collect(),
        false,
    )
}

/// A ring of six knots at alternating heights, closed.
fn loop_spline() -> Spline {
    let knots = (0..6)
        .map(|i| {
            let a = i as f32 / 6.0 * std::f32::consts::TAU;
            let y = if i % 2 == 0 { 2.0 } else { 6.5 };
            Knot::at(Vec3::new(a.cos() * 11.0, y, a.sin() * 11.0))
        })
        .collect();
    Spline::new(knots, true)
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("SplinesDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-2.0, 46.0, -52.0))
                .with_euler(Vec3::new(38.0, 4.0, 0.0)),
        );
    }
    let ground = mesh(
        &mut s,
        "Ground",
        None,
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, -0.5, 8.0), Vec3::new(140.0, 1.0, 110.0)),
        GRASS,
    );
    s.add(ground, BoxCollider::default());

    // The road: a Road-profile extrude along an open Auto Smooth spline.
    let road = s.spawn("Winding Road", None);
    s.add(road, SplineContainer::with(road_spline()));
    s.add_by_name(road, "Spline Extrude");
    *s.world.get::<&mut SplineExtrude>(road).unwrap() = SplineExtrude {
        profile: ExtrudeProfile::Road,
        width: 7.0,
        segments_per_unit: 1.5,
        uv_tiling: 0.15,
        ..Default::default()
    };
    s.world.get::<&mut MeshRenderer>(road).unwrap().material = Some(ASPHALT.into());

    // Lamp posts every 12 m on the right; cones every 6 m on the left, a little random.
    let lamps = s.spawn("Lamp Posts", None);
    s.add(
        lamps,
        SplineInstantiate {
            container: Some(road),
            items: vec![InstantiateItem {
                prefab: Some(LAMP_PREFAB.into()),
                ..Default::default()
            }],
            method: InstantiateMethod::SplineDistance,
            spacing: [12.0, 12.0],
            position_min: Vec3::new(4.6, 0.0, 0.0),
            position_max: Vec3::new(4.6, 0.0, 0.0),
            ..Default::default()
        },
    );
    let cones = s.spawn("Cones", None);
    s.add(
        cones,
        SplineInstantiate {
            container: Some(road),
            items: vec![InstantiateItem {
                primitive: PrimitiveMesh::Cylinder,
                material: Some(CONE.into()),
                ..Default::default()
            }],
            method: InstantiateMethod::SplineDistance,
            spacing: [5.0, 7.0],
            position_min: Vec3::new(-4.2, 0.35, 0.0),
            position_max: Vec3::new(-4.0, 0.35, 0.0),
            rotation_min: Vec3::ZERO,
            rotation_max: Vec3::new(0.0, 90.0, 0.0),
            scale_min: Vec3::new(0.25, 0.35, 0.25),
            scale_max: Vec3::new(0.4, 0.35, 0.4),
            seed: 7,
            ..Default::default()
        },
    );

    // A car-like block driving the road and back again.
    let car = s.spawn("Car", None);
    s.add(
        car,
        SplineAnimate {
            container: Some(road),
            method: AnimateMethod::Speed,
            speed: 9.0,
            loop_mode: LoopMode::PingPong,
            start_offset: 0.12,
            ..Default::default()
        },
    );
    s.add(
        car,
        Scripts {
            list: vec![ScriptRef {
                path: RIDER.into(),
                ..Default::default()
            }],
        },
    );
    mesh(
        &mut s,
        "Body",
        Some(car),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, 0.65, 0.0), Vec3::new(1.9, 0.8, 4.2)),
        PAINT,
    );
    mesh(
        &mut s,
        "Cabin",
        Some(car),
        PrimitiveMesh::Cube,
        scaled(Vec3::new(0.0, 1.3, -0.3), Vec3::new(1.6, 0.6, 2.0)),
        GLASS,
    );

    // A closed neon loop with an orb easing back and forth along it.
    let tube = s.spawn("Neon Loop", None);
    s.set_transform(tube, Transform::from_position(Vec3::new(34.0, 0.0, 14.0)));
    s.add(tube, SplineContainer::with(loop_spline()));
    s.add_by_name(tube, "Spline Extrude");
    *s.world.get::<&mut SplineExtrude>(tube).unwrap() = SplineExtrude {
        radius: 0.35,
        sides: 12,
        segments_per_unit: 3.0,
        ..Default::default()
    };
    s.world.get::<&mut MeshRenderer>(tube).unwrap().material = Some(NEON.into());
    let orb = mesh(
        &mut s,
        "Orb",
        None,
        PrimitiveMesh::Sphere,
        scaled(Vec3::ZERO, Vec3::splat(1.2)),
        ORB,
    );
    s.add(
        orb,
        SplineAnimate {
            container: Some(tube),
            duration: 6.0,
            easing: Easing::EaseInOut,
            loop_mode: LoopMode::PingPong,
            ..Default::default()
        },
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_splines_demo() {
    let root = project();
    for (rel, m) in materials() {
        m.save(&root.join(rel)).unwrap();
    }
    std::fs::create_dir_all(root.join("Assets/Prefabs")).unwrap();
    lamp_prefab().save(&root.join(LAMP_PREFAB)).unwrap();
    std::fs::write(root.join(RIDER), RIDER_SRC).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn splines_demo_matches_its_generator() {
    let root = project();
    for (rel, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(rel)).unwrap(), &m, rel);
    }
    let prefab = std::fs::read_to_string(root.join(LAMP_PREFAB)).unwrap();
    crate::test_util::assert_json_close(&prefab, &lamp_prefab().to_json(), LAMP_PREFAB);
    let script = std::fs::read_to_string(root.join(RIDER)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), RIDER_SRC);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_splines_demo",
    );
    let (scene, report) = Scene::from_json("SplinesDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let road = scene.find_by_name("Winding Road").unwrap();
    let car = scene.find_by_name("Car").unwrap();
    let anim = scene.world.get::<&SplineAnimate>(car).unwrap().container;
    assert_eq!(anim, Some(road), "the car follows the road after loading");
}

/// Builds the generated content and plays a few seconds: the road and tube get meshes, lamps
/// and cones line the road, the car and orb move along their splines.
#[test]
fn splines_demo_builds_and_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    use misty_engine::splines::instantiate::instances;
    let root = project();
    let mut scene = demo_scene();
    misty_engine::splines::update(&mut scene);
    let road = scene.find_by_name("Winding Road").unwrap();
    let tube = scene.find_by_name("Neon Loop").unwrap();
    for e in [road, tube] {
        let mesh = scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
        let mesh = mesh.expect("extrude built a mesh");
        assert!(misty_engine::models::mesh(&mesh).is_some_and(|m| m.indices.len() > 100));
    }
    let length = scene
        .world
        .get::<&SplineContainer>(road)
        .unwrap()
        .path(0)
        .length;
    let lamps = instances(&scene, scene.find_by_name("Lamp Posts").unwrap());
    // The prefab path isn't resolvable without a project root here: copies are primitives.
    assert_eq!(lamps.len(), (length / 12.0).floor() as usize + 1);
    let cones = instances(&scene, scene.find_by_name("Cones").unwrap());
    assert!(cones.len() as f32 > length / 7.0 && (cones.len() as f32) < length / 5.0 + 2.0);

    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let car = scene.find_by_name("Car").unwrap();
    let orb = scene.find_by_name("Orb").unwrap();
    let mut time = Time::default();
    let mut frames = |scene: &mut Scene, n: usize| {
        for _ in 0..n {
            time.advance(1.0 / 30.0);
            host.update(scene, &time, Input::default());
        }
    };
    frames(&mut scene, 1);
    let (car0, orb0) = (scene.world_position(car), scene.world_position(orb));
    frames(&mut scene, 60);
    let moved = scene.world_position(car).distance(car0);
    assert!(
        (moved - 18.0).abs() < 3.0,
        "car drove ~18 m in 2 s: {moved}"
    );
    assert!(scene.world_position(orb).distance(orb0) > 1.0);
    // Facing along the road.
    let c = scene.world.get::<&SplineContainer>(road).unwrap().path(0);
    let n = c.nearest(scene.world_position(car));
    assert!(n.distance < 0.1);
    let fwd = scene.world_rotation(car) * Vec3::Z;
    assert!(fwd.dot(c.tangent(n.t)).abs() > 0.99);
    let logs = host.take_logs();
    for (kind, text) in &logs {
        assert_ne!(*kind, LogKind::Error, "{text}");
    }
    assert!(
        logs.iter().any(|(_, t)| t.starts_with("Road: ")),
        "{logs:?}"
    );
}
