//! ScreenSpaceGIDemo sample scene in `SampleProject`: a white room with a red wall on the left,
//! a green wall on the right and an open front, a skylight in the ceiling that lets the sun in
//! onto the floor by the red wall, a white sphere and two white blocks, and a glossy floor. A
//! global Post-process Volume turns on Screen Space Global Illumination (the red and green
//! walls tint the floor and the blocks), Screen Space Reflections (the blocks show in the
//! floor) and Ambient Occlusion. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_ssgi_demo`; screenshots with
//! `cargo test -p misty_editor -- --ignored write_ssgi_screenshots`.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, ScreenSpaceQuality,
    Shadows, TonemapMode, Transform,
};

const SCENE: &str = "Assets/Scenes/ScreenSpaceGIDemo.scene";
/// The demo's own materials: (file, colour, smoothness).
const MATERIALS: [(&str, [f32; 4], f32); 4] = [
    (
        "Assets/Materials/SSGI White.mat",
        [0.85, 0.85, 0.83, 1.0],
        0.15,
    ),
    ("Assets/Materials/SSGI Red.mat", [0.8, 0.07, 0.05, 1.0], 0.1),
    (
        "Assets/Materials/SSGI Green.mat",
        [0.1, 0.62, 0.12, 1.0],
        0.1,
    ),
    (
        "Assets/Materials/SSGI Glossy Floor.mat",
        [0.72, 0.72, 0.7, 1.0],
        0.88,
    ),
];
const WHITE: &str = MATERIALS[0].0;
const RED: &str = MATERIALS[1].0;
const GREEN: &str = MATERIALS[2].0;
const FLOOR: &str = MATERIALS[3].0;
/// Room interior: x in [-HALF, HALF], z in [-HALF, HALF], y in [0, HEIGHT].
const HALF: f32 = 4.0;
const HEIGHT: f32 = 4.0;
const WALL: f32 = 0.3;

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
    let gi = &mut v.screen_space_gi;
    gi.active = true;
    gi.intensity = 1.2;
    gi.radius = 4.0;
    gi.quality = ScreenSpaceQuality::High;
    let r = &mut v.screen_space_reflections;
    r.active = true;
    r.max_roughness = 0.5;
    let ao = &mut v.ambient_occlusion;
    ao.active = true;
    ao.radius = 1.0;
    v
}

/// Floor, red and green side walls, back wall and a ceiling around a skylight.
fn room(s: &mut Scene) {
    let span = 2.0 * (HALF + WALL);
    add_mesh(
        s,
        "Floor",
        PrimitiveMesh::Cube,
        at(
            Vec3::new(0.0, -WALL * 0.5, 0.0),
            Vec3::new(span, WALL, span),
        ),
        FLOOR,
    );
    let side = Vec3::new(WALL, HEIGHT, span);
    let x = HALF + WALL * 0.5;
    add_mesh(
        s,
        "Red Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(-x, HEIGHT * 0.5, 0.0), side),
        RED,
    );
    add_mesh(
        s,
        "Green Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(x, HEIGHT * 0.5, 0.0), side),
        GREEN,
    );
    add_mesh(
        s,
        "Back Wall",
        PrimitiveMesh::Cube,
        at(
            Vec3::new(0.0, HEIGHT * 0.5, HALF + WALL * 0.5),
            Vec3::new(span, HEIGHT, WALL),
        ),
        WHITE,
    );
    // Ceiling in four slabs around a 2 × 2 m skylight centred at (0.5, 0.5).
    let y = HEIGHT + WALL * 0.5;
    let (lo, hi) = (-0.5, 1.5);
    let edge = -(HALF + WALL);
    let far = HALF + WALL;
    let slabs = [
        ("Ceiling Left", (edge, lo), (edge, far)),
        ("Ceiling Right", (hi, far), (edge, far)),
        ("Ceiling Front", (lo, hi), (edge, lo)),
        ("Ceiling Back", (lo, hi), (hi, far)),
    ];
    for (name, (x0, x1), (z0, z1)) in slabs {
        add_mesh(
            s,
            name,
            PrimitiveMesh::Cube,
            at(
                Vec3::new((x0 + x1) * 0.5, y, (z0 + z1) * 0.5),
                Vec3::new(x1 - x0, WALL, z1 - z0),
            ),
            WHITE,
        );
    }
}

fn objects(s: &mut Scene) {
    add_mesh(
        s,
        "Tall Block",
        PrimitiveMesh::Cube,
        at(Vec3::new(-1.6, 1.2, 1.6), Vec3::new(1.2, 2.4, 1.2))
            .with_euler(Vec3::new(0.0, 18.0, 0.0)),
        WHITE,
    );
    add_mesh(
        s,
        "Short Block",
        PrimitiveMesh::Cube,
        at(Vec3::new(1.7, 0.6, 0.4), Vec3::splat(1.2)).with_euler(Vec3::new(0.0, -20.0, 0.0)),
        WHITE,
    );
    add_mesh(
        s,
        "Sphere",
        PrimitiveMesh::Sphere,
        at(Vec3::new(0.1, 0.6, -1.2), Vec3::splat(1.2)),
        WHITE,
    );
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("ScreenSpaceGIDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 2.1, -8.6))
                .with_euler(Vec3::new(8.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Down through the skylight and towards the red wall: a sunlit patch on the floor by
        // the wall and on its lower part, the rest of the room lit by bounce light.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 6.0, 0.0))
                .with_euler(Vec3::new(52.0, -75.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 2.2;
        l.shadows = Shadows::Soft;
    }
    room(&mut s);
    objects(&mut s);
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_ssgi_demo() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        material(i).save(&root.join(path)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn ssgi_demo_matches_its_generator() {
    let root = project();
    for (i, (path, ..)) in MATERIALS.iter().enumerate() {
        crate::test_util::assert_close(
            &Material::load(&root.join(path)).unwrap(),
            &material(i),
            path,
        );
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_ssgi_demo",
    );
    let (scene, report) = Scene::from_json("ScreenSpaceGIDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.ssgi.active() && post.ssr.active() && post.ao.active());
}

/// Render the demo at `size` with `post`, letting the temporal passes settle.
fn render_demo(
    gpu: &(misty_engine::wgpu::Device, misty_engine::wgpu::Queue),
    post: impl Fn(&mut misty_engine::render::PostSettings),
    size: (u32, u32),
) -> Vec<u8> {
    use misty_engine::render::{Frame, Renderer};
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let view = misty_engine::render_scene::GameView::new(&scene, &assets).unwrap();
    let mut settings = view.post;
    post(&mut settings);
    let frame = Frame {
        post: settings,
        ..view.frame()
    };
    let (device, queue) = gpu;
    let mut renderer = Renderer::new(device);
    let target = renderer.create_target(device, size.0, size.1);
    for _ in 0..32 {
        renderer.render(device, queue, &target, &frame);
    }
    crate::project_thumbnail::read_pixels(device, queue, &target)
}

/// On a GPU: the red wall tints the floor beside it and the green wall the floor beside it.
#[test]
fn ssgi_demo_bleeds_both_wall_colours() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let size = (crate::sample_render::WIDTH, crate::sample_render::HEIGHT);
    let on = render_demo(&gpu, |_| {}, size);
    let off = render_demo(&gpu, |p| p.ssgi.enabled = false, size);
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let camera = misty_engine::render_scene::GameView::new(&scene, &assets)
        .unwrap()
        .camera;
    let colour = |image: &[u8], p: Vec3| {
        let ndc = camera
            .view_projection(size.0 as f32 / size.1 as f32)
            .project_point3(p);
        let x = ((ndc.x * 0.5 + 0.5) * size.0 as f32) as u32;
        let y = ((0.5 - ndc.y * 0.5) * size.1 as f32) as u32;
        let i = ((y * size.0 + x) * 4) as usize;
        Vec3::new(image[i] as f32, image[i + 1] as f32, image[i + 2] as f32)
    };
    let red = |c: Vec3| c.x - (c.y + c.z) * 0.5;
    let green = |c: Vec3| c.y - (c.x + c.z) * 0.5;
    let by_red = Vec3::new(-HALF + 0.4, 0.0, -2.0);
    let by_green = Vec3::new(HALF - 0.4, 0.0, -2.0);
    let (r0, r1) = (colour(&off, by_red), colour(&on, by_red));
    let (g0, g1) = (colour(&off, by_green), colour(&on, by_green));
    eprintln!("by red wall {r0} → {r1}; by green wall {g0} → {g1}");
    assert!(red(r1) > red(r0) + 3.0, "{r0} → {r1}");
    assert!(green(g1) > green(g0) + 2.0, "{g0} → {g1}");
}

#[test]
#[ignore = "writes docs/screenshots/gfx; run on purpose"]
fn write_ssgi_screenshots() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx");
    std::fs::create_dir_all(&dir).unwrap();
    let size = (960, 540);
    type Edit = fn(&mut misty_engine::render::PostSettings);
    let shots: [(&str, Edit); 4] = [
        ("ssgi_demo_off", |p| {
            p.ssgi.enabled = false;
            p.ssr.enabled = false;
        }),
        ("ssgi_demo_ssgi", |p| p.ssr.enabled = false),
        ("ssgi_demo_ssr", |p| p.ssgi.enabled = false),
        ("ssgi_demo_both", |_| {}),
    ];
    for (name, edit) in shots {
        let pixels = render_demo(&gpu, edit, size);
        let file = std::fs::File::create(dir.join(format!("{name}.png"))).unwrap();
        let mut encoder = png::Encoder::new(std::io::BufWriter::new(file), size.0, size.1);
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder.write_header().unwrap();
        writer.write_image_data(&pixels).unwrap();
    }
}
