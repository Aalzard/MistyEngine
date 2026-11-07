//! Procedural ParticlesDemo scene in `SampleProject`: a campfire (fire, sparks, smoke) with a
//! flickering light and a script that bursts sparks on Space. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_particles_demo`; the other test checks the
//! committed files still load and match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::particles::{
    AlphaKey, Burst, ColorKey, Gradient, MinMax, MinMaxColor, ParticleBlend, ParticleSystem,
    ShapeKind, SimulationSpace,
};
use misty_engine::scene::{
    ClearFlags, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef,
    Scripts, Shadows, Transform,
};

const WOOD: &str = "Assets/Materials/Wood.mat";
const DIRT: &str = "Assets/Materials/Dirt.mat";
const SCRIPT: &str = "Assets/Scripts/SparkBurst.lua";

/// Space emits a handful of extra sparks through the ParticleSystem API.
const SPARK_BURST: &str = r#"-- Press Space to throw extra sparks; the count shows how the API works.
local SparkBurst = { amount = 40 }

function SparkBurst:Start()
    self.sparks = self.gameObject:GetComponent("ParticleSystem")
end

function SparkBurst:Update()
    if Input.GetKeyDown("space") then
        self.sparks:Emit(self.amount)
        Debug.Log("Sparks alive:", self.sparks.particleCount)
    end
end

return SparkBurst
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn wood() -> Material {
    Material {
        color: [0.32, 0.19, 0.11, 1.0],
        smoothness: 0.15,
        ..Default::default()
    }
}

/// Dark earth, so the fire light stands out at night.
fn dirt() -> Material {
    Material {
        color: [0.14, 0.12, 0.11, 1.0],
        smoothness: 0.05,
        ..Default::default()
    }
}

fn keys(colors: &[(f32, [f32; 3])], alphas: &[(f32, f32)]) -> Gradient {
    Gradient {
        color_keys: colors
            .iter()
            .map(|&(time, color)| ColorKey { time, color })
            .collect(),
        alpha_keys: alphas
            .iter()
            .map(|&(time, alpha)| AlphaKey { time, alpha })
            .collect(),
    }
}

/// Upward cone system with the given look; the caller tweaks the rest.
fn cone(angle: f32, radius: f32) -> ParticleSystem {
    let mut p = ParticleSystem::default();
    p.shape.shape = ShapeKind::Cone;
    p.shape.angle = angle;
    p.shape.radius = radius;
    p.main.simulation_space = SimulationSpace::World;
    p
}

fn fire() -> ParticleSystem {
    let mut p = cone(12.0, 0.35);
    p.main.start_lifetime = MinMax::between(0.6, 1.1);
    p.main.start_speed = MinMax::between(1.2, 2.2);
    p.main.start_size = MinMax::between(0.5, 0.9);
    p.main.start_color = MinMaxColor {
        a: [1.0, 0.62, 0.2, 1.0],
        b: [1.0, 0.35, 0.08, 1.0],
        random: true,
    };
    p.main.gravity_modifier = -0.1;
    p.main.max_particles = 300;
    p.emission.rate_over_time = MinMax::constant(35.0);
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = keys(
        &[
            (0.0, [1.0, 0.95, 0.7]),
            (0.4, [1.0, 0.5, 0.15]),
            (1.0, [0.5, 0.08, 0.04]),
        ],
        &[(0.0, 0.0), (0.15, 1.0), (1.0, 0.0)],
    );
    p.size_over_lifetime.enabled = true;
    p.size_over_lifetime.start = 1.0;
    p.size_over_lifetime.end = 0.3;
    p.renderer.blend = ParticleBlend::Additive;
    p.renderer.intensity = 1.8;
    p
}

fn sparks() -> ParticleSystem {
    let mut p = cone(28.0, 0.2);
    p.main.start_lifetime = MinMax::between(0.8, 1.6);
    p.main.start_speed = MinMax::between(2.5, 4.5);
    p.main.start_size = MinMax::between(0.05, 0.1);
    p.main.start_color = MinMaxColor {
        a: [1.0, 0.75, 0.35, 1.0],
        ..Default::default()
    };
    p.main.gravity_modifier = 0.35;
    p.emission.rate_over_time = MinMax::constant(18.0);
    p.emission.bursts = vec![Burst {
        time: 0.5,
        count: 12,
        cycles: 0,
        interval: 1.5,
        probability: 1.0,
    }];
    p.main.duration = 3.0;
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = keys(
        &[(0.0, [1.0, 1.0, 1.0]), (1.0, [1.0, 0.4, 0.1])],
        &[(0.0, 1.0), (0.7, 1.0), (1.0, 0.0)],
    );
    p.size_over_lifetime.enabled = true;
    p.size_over_lifetime.start = 1.0;
    p.size_over_lifetime.end = 0.2;
    p.renderer.blend = ParticleBlend::Additive;
    p.renderer.intensity = 4.0;
    p
}

fn smoke() -> ParticleSystem {
    let mut p = cone(15.0, 0.3);
    p.main.start_lifetime = MinMax::between(2.5, 3.5);
    p.main.start_speed = MinMax::between(0.6, 1.0);
    p.main.start_size = MinMax::between(0.8, 1.3);
    p.main.start_color = MinMaxColor {
        a: [0.3, 0.3, 0.3, 0.45],
        b: [0.18, 0.18, 0.2, 0.35],
        random: true,
    };
    p.main.prewarm = true;
    p.emission.rate_over_time = MinMax::constant(8.0);
    p.color_over_lifetime.enabled = true;
    p.color_over_lifetime.gradient = keys(
        &[(0.0, [1.0, 1.0, 1.0]), (1.0, [1.0, 1.0, 1.0])],
        &[(0.0, 0.0), (0.2, 1.0), (1.0, 0.0)],
    );
    p.size_over_lifetime.enabled = true;
    p.size_over_lifetime.start = 0.6;
    p.size_over_lifetime.end = 2.0;
    p
}

/// Emitters face up: local +Z (the emission axis) rotated onto +Y.
fn emitter_at(pos: Vec3) -> Transform {
    Transform::from_position(pos).with_euler(Vec3::new(-90.0, 0.0, 0.0))
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("ParticlesDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.0, -5.5))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.clear_flags = ClearFlags::SolidColor;
        c.background = [0.03, 0.035, 0.06, 0.0];
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Moonlight: dim and blue.
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 0.25;
        l.color = [0.55, 0.65, 1.0];
    }
    let floor = s.spawn("Ground", None);
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        floor,
        MeshRenderer {
            material: Some(DIRT.into()),
            ..Default::default()
        },
    );

    for (i, yaw) in [35.0f32, -35.0, 90.0].into_iter().enumerate() {
        let log = s.spawn(&format!("Log {}", i + 1), None);
        let mut t = Transform::from_position(Vec3::new(0.0, 0.12 + i as f32 * 0.1, 0.0))
            .with_euler(Vec3::new(0.0, yaw, 0.0));
        t.scale = Vec3::new(1.4, 0.2, 0.2);
        s.set_transform(log, t);
        s.add(log, MeshFilter::primitive(PrimitiveMesh::Cube));
        s.add(
            log,
            MeshRenderer {
                material: Some(WOOD.into()),
                ..Default::default()
            },
        );
    }

    let light = s.spawn("Fire Light", None);
    s.set_transform(light, Transform::from_position(Vec3::new(0.0, 0.9, 0.0)));
    s.add(
        light,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.6, 0.25],
            intensity: 3.0,
            range: 7.0,
            shadows: Shadows::None,
            ..Default::default()
        },
    );

    let fire_e = s.spawn("Fire", None);
    s.set_transform(fire_e, emitter_at(Vec3::new(0.0, 0.2, 0.0)));
    s.add(fire_e, fire());
    let sparks_e = s.spawn("Sparks", Some(fire_e));
    s.add(sparks_e, sparks());
    s.add(
        sparks_e,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    let smoke_e = s.spawn("Smoke", Some(fire_e));
    s.set_transform(smoke_e, Transform::from_position(Vec3::new(0.0, 0.0, 1.0)));
    s.add(smoke_e, smoke());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_particles_demo() {
    let root = project();
    wood().save(&root.join(WOOD)).unwrap();
    dirt().save(&root.join(DIRT)).unwrap();
    std::fs::write(root.join(SCRIPT), SPARK_BURST).unwrap();
    demo_scene()
        .save(&root.join("Assets/Scenes/ParticlesDemo.scene"))
        .unwrap();
}

#[test]
fn particles_demo_matches_its_generator() {
    let root = project();
    crate::test_util::assert_close(&Material::load(&root.join(WOOD)).unwrap(), &wood(), WOOD);
    crate::test_util::assert_close(&Material::load(&root.join(DIRT)).unwrap(), &dirt(), DIRT);
    let script = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), SPARK_BURST);
    let (scene, report) = Scene::load(&root.join("Assets/Scenes/ParticlesDemo.scene")).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    for (name, expected) in [("Fire", fire()), ("Sparks", sparks()), ("Smoke", smoke())] {
        let e = scene.find_by_name(name).unwrap();
        let p = scene.world.get::<&ParticleSystem>(e).unwrap();
        crate::test_util::assert_close(&*p, &expected, name);
    }
}
