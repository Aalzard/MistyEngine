//! LightProbesDemo sample scene in `SampleProject`: an open-fronted room with a bright red wall
//! on the right and a blue wall on the left, a grid of light probes filling it (baked into
//! `Assets/Scenes/LightProbesDemo/LightProbes.probes`) and a white sphere a script swings between
//! the walls: lit by the probes it turns red near the red wall and blue near the blue one. A
//! second sphere with Light Probes off shows the plain ambient light for comparison.
//!
//! Regenerate (and re-bake on the GPU) with
//! `cargo test -p misty_editor -- --ignored write_light_probes_demo`; the other tests check the
//! committed files still match.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::light_probes::{self, LightProbeData, LightProbeGroup, LightProbeUsage};
use misty_engine::render::MaterialParams;
use misty_engine::render_scene::MaterialSource;
use misty_engine::scene::{
    GameObject, Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, ScriptRef,
    Scripts, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/LightProbesDemo.scene";
const RED: &str = "Assets/Materials/Probe Red Wall.mat";
const BLUE: &str = "Assets/Materials/Probe Blue Wall.mat";
const WHITE: &str = "Assets/Materials/Probe Room White.mat";
const SPHERE: &str = "Assets/Materials/Probe Sphere.mat";
const SCRIPT: &str = "Assets/Scripts/ProbeSwing.lua";

const SCRIPT_LUA: &str = r#"-- ProbeSwing: swings the sphere between the red and the blue wall. Its Mesh Renderer blends
-- the baked light probes (Light Probes: Blend Probes), so it picks up the colour of the wall it
-- comes near. Every few seconds it logs the light the probes give at its position
-- (LightProbes.GetInterpolatedProbe).
local ProbeSwing = {
    range = 3.6,
    speed = 0.6,
    height = 1.1,
    logEvery = 3,
}

function ProbeSwing:Start()
    self._next = 0
end

function ProbeSwing:Update()
    local x = Mathf.Sin(Time.time * self.speed) * self.range
    self.transform.position = Vector3.new(x, self.height, 0)
    if Time.time >= self._next then
        self._next = Time.time + self.logEvery
        local probe = LightProbes.GetInterpolatedProbe(self.transform.position)
        local c = probe:Evaluate(Vector3.new(0, 1, 0))
        Debug.Log(string.format("Light from the probes at x = %.1f: %.2f %.2f %.2f", x, c.r, c.g, c.b))
    end
end

return ProbeSwing
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 4] {
    let m = |color: [f32; 3], smoothness, emission: f32| Material {
        color: [color[0], color[1], color[2], 1.0],
        smoothness,
        emission_enabled: emission > 0.0,
        emission: if emission > 0.0 { color } else { [0.0; 3] },
        emission_intensity: emission,
        ..Default::default()
    };
    [
        (RED, m([0.95, 0.06, 0.04], 0.15, 1.0)),
        (BLUE, m([0.05, 0.16, 0.95], 0.15, 1.0)),
        (WHITE, m([0.78, 0.77, 0.75], 0.1, 0.0)),
        (SPHERE, m([0.92, 0.92, 0.92], 0.35, 0.0)),
    ]
}

fn slab(s: &mut Scene, parent: Entity, name: &str, at: Vec3, size: Vec3, material: &str) {
    let e = s.spawn(name, Some(parent));
    let mut t = Transform::from_position(at);
    t.scale = size;
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
    s.world.get::<&mut GameObject>(e).unwrap().is_static = true;
}

fn sphere(s: &mut Scene, name: &str, at: Vec3, size: f32, probes: LightProbeUsage) -> Entity {
    let e = s.spawn(name, None);
    let mut t = Transform::from_position(at);
    t.scale = Vec3::splat(size);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Sphere));
    s.add(
        e,
        MeshRenderer {
            material: Some(SPHERE.into()),
            light_probes: probes,
            ..Default::default()
        },
    );
    e
}

/// The probes: a grid filling the room.
fn probe_group() -> LightProbeGroup {
    let mut g = LightProbeGroup::default();
    g.fill_box(Vec3::new(0.0, 1.7, 0.0), Vec3::new(9.2, 2.8, 7.2), 1.6);
    g
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("LightProbesDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.2, 10.5))
                .with_euler(Vec3::new(6.0, 180.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 0.2;
    }
    let room = s.spawn("Room", None);
    s.world.get::<&mut GameObject>(room).unwrap().is_static = true;
    let w = 10.0;
    let (h, d) = (3.6, 8.0);
    let size = |x, y, z| Vec3::new(x, y, z);
    slab(
        &mut s,
        room,
        "Floor",
        size(0.0, -0.1, 0.0),
        size(w + 0.4, 0.2, d + 0.4),
        WHITE,
    );
    slab(
        &mut s,
        room,
        "Ceiling",
        size(0.0, h + 0.1, 0.0),
        size(w + 0.4, 0.2, d + 0.4),
        WHITE,
    );
    slab(
        &mut s,
        room,
        "Back Wall",
        size(0.0, h / 2.0, -d / 2.0 - 0.1),
        size(w, h, 0.2),
        WHITE,
    );
    slab(
        &mut s,
        room,
        "Red Wall",
        size(w / 2.0 + 0.1, h / 2.0, 0.0),
        size(0.2, h, d),
        RED,
    );
    slab(
        &mut s,
        room,
        "Blue Wall",
        size(-w / 2.0 - 0.1, h / 2.0, 0.0),
        size(0.2, h, d),
        BLUE,
    );
    slab(
        &mut s,
        room,
        "Pedestal",
        size(0.0, 0.3, -2.6),
        size(1.2, 0.6, 1.2),
        WHITE,
    );

    let lamp = s.spawn("Ceiling Light", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(0.0, h - 0.3, 0.0)));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.95, 0.88],
            intensity: 1.2,
            range: 12.0,
            shadows: Shadows::Soft,
            ..Default::default()
        },
    );

    let probes = s.spawn("Light Probe Group", None);
    s.add(probes, probe_group());

    let mover = sphere(
        &mut s,
        "Swinging Sphere",
        Vec3::new(0.0, 1.1, 0.0),
        1.0,
        LightProbeUsage::BlendProbes,
    );
    s.add(
        mover,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    sphere(
        &mut s,
        "Sphere (Light Probes Off)",
        Vec3::new(0.0, 1.0, -2.6),
        0.8,
        LightProbeUsage::Off,
    );
    s.render_settings.light_probes = Some(light_probes::asset_path_for(SCENE));
    s
}

/// The demo's materials without reading the project folder.
struct Demo(BTreeMap<&'static str, MaterialParams>);

impl Demo {
    fn new() -> Self {
        Self(
            materials()
                .into_iter()
                .map(|(p, m)| (p, m.params()))
                .collect(),
        )
    }
}

impl MaterialSource for Demo {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.0.get(r)).cloned().unwrap_or_default()
    }
}

#[test]
#[ignore = "writes into SampleProject and bakes on the GPU; run on purpose to regenerate"]
fn write_light_probes_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    std::fs::write(root.join(SCRIPT), SCRIPT_LUA).unwrap();
    let scene = demo_scene();
    scene.save(&root.join(SCENE)).unwrap();
    let (device, queue) = crate::sample_render::headless().expect("a GPU adapter");
    let mut renderer = misty_engine::render::Renderer::new(&device);
    let data = light_probes::bake::bake_scene(
        &mut renderer,
        &device,
        &queue,
        &scene,
        &Demo::new(),
        light_probes::scene_positions(&scene),
        Default::default(),
    )
    .expect("probes");
    data.save(&root.join(light_probes::asset_path_for(SCENE)))
        .unwrap();
}

#[test]
fn light_probes_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    let script = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), SCRIPT_LUA);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_light_probes_demo",
    );
    let (scene, report) = Scene::from_json("LightProbesDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // The committed bake matches the probe layout.
    let rel = scene.render_settings.light_probes.clone().unwrap();
    let data = LightProbeData::load(&root.join(&rel)).unwrap();
    let positions = light_probes::scene_positions(&scene);
    assert_eq!(positions.len(), 126);
    crate::test_util::assert_close(&data.positions, &positions, &rel);
}

/// The baked probes see red by the red wall and blue by the blue wall, and the moving sphere's
/// interpolated light follows.
#[test]
fn light_probes_demo_probes_are_tinted_by_the_walls() {
    let root = project();
    let data = LightProbeData::load(&root.join(light_probes::asset_path_for(SCENE))).unwrap();
    let field = misty_engine::light_probes::LightProbeField::from_data(&data);
    let mut hint = 0;
    let light = |x: f32, hint: &mut u32| {
        let sh = field.sample(Vec3::new(x, 1.1, 0.0), hint).unwrap();
        // Average over the sideways directions the sphere shows the camera.
        [Vec3::X, Vec3::NEG_X, Vec3::Y, Vec3::Z]
            .iter()
            .map(|n| misty_engine::render::probes::sh::eval(&sh, *n))
            .sum::<Vec3>()
    };
    let near_red = light(3.6, &mut hint);
    let near_blue = light(-3.6, &mut hint);
    let middle = light(0.0, &mut hint);
    assert!(near_red.x > near_red.z * 1.3, "{near_red}");
    assert!(near_blue.z > near_blue.x * 1.3, "{near_blue}");
    assert!(near_red.x > middle.x && near_blue.z > middle.z, "{middle}");
}
