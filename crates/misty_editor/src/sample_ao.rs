//! AmbientOcclusionDemo sample scene in `SampleProject`: a corner room with stacked crates,
//! columns on plinths, spheres resting on the floor, a row of cubes at increasing heights
//! above the ground (the AO under them fades with the gap) and a sphere bobbing up and down
//! (`VerticalBobber.lua`). A global Post-process Volume turns on Ambient Occlusion and Contact
//! Shadows; the sun casts contact shadows. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_ao_demo`; the other tests check the
//! committed files still match, that the scene plays, and (on a GPU) that AO darkens it.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, ScriptRef, Scripts,
    Shadows, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/AmbientOcclusionDemo.scene";
const BOBBER: &str = "Assets/Scripts/VerticalBobber.lua";
/// The demo's own materials: (file, colour, smoothness).
const MATERIALS: [(&str, [f32; 4], f32); 4] = [
    (
        "Assets/Materials/AO Concrete.mat",
        [0.62, 0.62, 0.6, 1.0],
        0.15,
    ),
    (
        "Assets/Materials/AO Plaster.mat",
        [0.86, 0.84, 0.8, 1.0],
        0.1,
    ),
    (
        "Assets/Materials/AO Crate.mat",
        [0.62, 0.44, 0.27, 1.0],
        0.2,
    ),
    ("Assets/Materials/AO Clay.mat", [0.78, 0.4, 0.3, 1.0], 0.35),
];
const CONCRETE: &str = MATERIALS[0].0;
const PLASTER: &str = MATERIALS[1].0;
const CRATE: &str = MATERIALS[2].0;
const CLAY: &str = MATERIALS[3].0;
/// Gaps under the floating cubes, in metres.
const GAPS: [f32; 5] = [0.0, 0.1, 0.3, 0.6, 1.2];

const BOBBER_LUA: &str = r#"-- VerticalBobber: floats the object up and down above where it starts.
local VerticalBobber = {
    height = 2,                      -- metres above the start at the top
    speed = 0.8,                     -- radians per second
}

function VerticalBobber:Start()
    self.base = self.transform.position
end

function VerticalBobber:Update()
    local lift = (1 - Mathf.Cos(Time.time * self.speed)) * 0.5 * self.height
    self.transform.position = Vector3.new(self.base.x, self.base.y + lift, self.base.z)
end

return VerticalBobber
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn material(i: usize) -> Material {
    let (_, color, smoothness) = MATERIALS[i];
    Material {
        color,
        smoothness,
        ..Default::default()
    }
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

fn at(position: Vec3, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(position);
    t.scale = scale;
    t
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Neutral;
    let ao = &mut v.ambient_occlusion;
    ao.active = true;
    ao.intensity = 2.0;
    ao.radius = 2.0;
    ao.step_count = 8;
    v.contact_shadows.active = true;
    v.contact_shadows.length = 0.3;
    v
}

fn room(s: &mut Scene) {
    add_mesh(
        s,
        "Floor",
        PrimitiveMesh::Plane,
        at(Vec3::ZERO, Vec3::splat(3.0)),
        CONCRETE,
    );
    let wall = Vec3::new(14.0, 4.0, 0.3);
    add_mesh(
        s,
        "Back Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, 2.0, 6.15), wall),
        PLASTER,
    );
    add_mesh(
        s,
        "Side Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(-7.15, 2.0, 1.0), Vec3::new(0.3, 4.0, 10.0)),
        PLASTER,
    );
}

fn crates(s: &mut Scene) {
    let stack = [
        ("Crate 1", Vec3::new(-6.4, 0.5, 5.4), 1.0, 0.0),
        ("Crate 2", Vec3::new(-5.3, 0.5, 5.4), 1.0, 8.0),
        ("Crate 3", Vec3::new(-5.9, 1.5, 5.35), 1.0, -12.0),
        ("Crate 4", Vec3::new(-6.45, 0.35, 4.3), 0.7, 20.0),
        ("Crate 5", Vec3::new(-6.4, 2.3, 5.4), 0.6, 35.0),
    ];
    for (name, pos, size, yaw) in stack {
        let t = at(pos, Vec3::splat(size)).with_euler(Vec3::new(0.0, yaw, 0.0));
        add_mesh(s, name, PrimitiveMesh::Cube, t, CRATE);
    }
}

fn columns(s: &mut Scene) {
    for (i, x) in [-2.5f32, 0.5, 3.5].into_iter().enumerate() {
        let n = i + 1;
        add_mesh(
            s,
            &format!("Column {n}"),
            PrimitiveMesh::Cylinder,
            at(Vec3::new(x, 2.3, 4.6), Vec3::new(0.7, 2.0, 0.7)),
            PLASTER,
        );
        add_mesh(
            s,
            &format!("Plinth {n}"),
            PrimitiveMesh::Cube,
            at(Vec3::new(x, 0.15, 4.6), Vec3::new(1.2, 0.3, 1.2)),
            PLASTER,
        );
    }
}

fn spheres(s: &mut Scene) {
    for (name, x, z, d) in [
        ("Sphere Large", -3.4, 1.6, 1.2),
        ("Sphere Medium", -2.0, 0.6, 0.8),
        ("Sphere Small", -4.3, 0.2, 0.5),
    ] {
        let t = at(Vec3::new(x, d * 0.5, z), Vec3::splat(d));
        add_mesh(s, name, PrimitiveMesh::Sphere, t, CLAY);
    }
    let bob = s.spawn("Bobbing Sphere", None);
    s.set_transform(bob, at(Vec3::new(-0.6, 0.4, 2.2), Vec3::splat(0.8)));
    s.add(bob, MeshFilter::primitive(PrimitiveMesh::Sphere));
    s.add(
        bob,
        MeshRenderer {
            material: Some(CLAY.into()),
            ..Default::default()
        },
    );
    s.add(
        bob,
        Scripts {
            list: vec![ScriptRef {
                path: BOBBER.into(),
                ..Default::default()
            }],
        },
    );
}

/// Cubes at increasing heights: the darkening under them fades as the gap grows.
fn floating(s: &mut Scene) {
    for (i, gap) in GAPS.into_iter().enumerate() {
        let x = 1.0 + i as f32 * 1.4;
        let t = at(Vec3::new(x, 0.4 + gap, 0.8), Vec3::splat(0.8));
        add_mesh(
            s,
            &format!("Cube Gap {gap:.1}"),
            PrimitiveMesh::Cube,
            t,
            CRATE,
        );
    }
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("AmbientOcclusionDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-0.5, 4.2, -6.5))
                .with_euler(Vec3::new(24.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Sun from the left over the side wall: the crate corner is in its shadow, lit by the
        // sky alone, where ambient occlusion shows best.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 5.0, 0.0))
                .with_euler(Vec3::new(38.0, 115.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 1.0;
        l.shadows = Shadows::Soft;
        l.contact_shadows = true;
    }
    room(&mut s);
    crates(&mut s);
    columns(&mut s);
    spheres(&mut s);
    floating(&mut s);
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_ao_demo() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        material(i).save(&root.join(path)).unwrap();
    }
    std::fs::write(root.join(BOBBER), BOBBER_LUA).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn ao_demo_matches_its_generator() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), material(i));
    }
    let lua = std::fs::read_to_string(root.join(BOBBER)).unwrap();
    assert_eq!(lua.replace("\r\n", "\n"), BOBBER_LUA);
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert!(
        text.replace("\r\n", "\n") == demo_scene().to_json(),
        "regenerate with write_ao_demo"
    );
    let (scene, report) = Scene::from_json("AmbientOcclusionDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.ao.active() && post.contact_shadows.active());
    let lights = misty_engine::render_scene::lights(&scene);
    assert!(lights.iter().any(|l| l.contact_shadows));
}

/// Playing: the bobbing sphere rises off the floor and comes back, without script errors.
#[test]
fn ao_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let (mut scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::ZERO);
    let bob = scene.find_by_name("Bobbing Sphere").unwrap();
    let start = scene.world_position(bob).y;
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut highest = start;
    for _ in 0..240 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
        highest = highest.max(scene.world_position(bob).y);
    }
    assert!(highest > start + 1.5, "rises: {start} → {highest}");
}

/// On a GPU: the demo's AO darkens the image and the shaded floor beside a resting sphere,
/// and leaves the open floor alone.
#[test]
fn ao_demo_renders_contact_darkening() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let view = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
    let on = crate::sample_render::render(&gpu, &view.frame(), 8);
    let mut no_ao = view.post;
    no_ao.ao.enabled = false;
    let off = crate::sample_render::render(
        &gpu,
        &misty_engine::render::Frame {
            post: no_ao,
            ..view.frame()
        },
        1,
    );
    let floor = |x: f32, z: f32| Vec3::new(x, 0.0, z);
    let ratio = |p: Vec3| on.brightness(&view.camera, p) / off.brightness(&view.camera, p);
    // In the side wall's shadow (sky light only): in front of the medium sphere, then open floor.
    let touching = ratio(floor(-2.0, 0.6 - 0.45));
    let open = ratio(floor(-2.5, -2.0));
    let mean = |i: &crate::sample_render::Image| i.pixels.iter().map(|&c| c as f64).sum::<f64>();
    let overall = mean(&on) / mean(&off);
    eprintln!("AO/no AO: touching {touching}, open {open}, whole image {overall}");
    assert!(touching < open - 0.02, "{touching} vs {open}");
    assert!(open > 0.98, "{open}");
    assert!(overall < 0.995, "{overall}");
}
