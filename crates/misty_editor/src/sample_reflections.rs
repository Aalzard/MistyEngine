//! ReflectionsDemo sample scene in `SampleProject`: a room with red, green and blue walls and a
//! doorway out to the sky, chrome / gold / copper spheres on a glossy tiled floor, an outdoor
//! area with a big chrome ball, and reflection probes made by Generate Probes and baked with a
//! second bounce. Regenerate with `cargo test --release -p misty_editor -- --ignored
//! write_reflections_demo` (bakes on the GPU); the other tests check the committed files.

use std::collections::BTreeMap;
use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::render::MaterialParams;
use misty_engine::render::probes::{self, ProbeData, ProbeInstance, ProbeSet, placement};
use misty_engine::render_scene::MaterialSource;
use misty_engine::scene::render_settings::SunDisk;
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, ReflectionProbe,
    ReflectionProbeGroup, Scene, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/ReflectionsDemo.scene";
const FOLDER: &str = "Assets/Materials/Reflections";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn path(name: &str) -> String {
    format!("{FOLDER}/{name}.mat")
}

fn mat(color: [f32; 3], metallic: f32, smoothness: f32) -> Material {
    Material {
        color: [color[0], color[1], color[2], 1.0],
        metallic,
        smoothness,
        ..Default::default()
    }
}

/// The demo's materials by name.
fn materials() -> Vec<(&'static str, Material)> {
    vec![
        ("Red Plaster", mat([0.75, 0.1, 0.08], 0.0, 0.25)),
        ("Green Plaster", mat([0.12, 0.6, 0.15], 0.0, 0.25)),
        ("Blue Plaster", mat([0.1, 0.22, 0.75], 0.0, 0.25)),
        ("White Plaster", mat([0.85, 0.83, 0.8], 0.0, 0.2)),
        ("Glossy Tiles", mat([0.16, 0.16, 0.17], 0.0, 0.9)),
        ("Chrome", mat([0.95, 0.95, 0.95], 1.0, 0.96)),
        ("Gold", mat([1.0, 0.78, 0.34], 1.0, 0.88)),
        ("Copper", mat([0.95, 0.6, 0.45], 1.0, 0.62)),
        ("Lawn", mat([0.3, 0.42, 0.2], 0.0, 0.1)),
        ("Concrete", mat([0.55, 0.55, 0.53], 0.0, 0.35)),
    ]
}

/// Probe generator settings of the demo.
fn generator() -> ReflectionProbeGroup {
    ReflectionProbeGroup {
        spacing: 4.0,
        height: 1.5,
        merge: true,
        resolution: 128,
        second_bounce: true,
    }
}

fn add(s: &mut Scene, name: &str, mesh: PrimitiveMesh, at: Vec3, scale: Vec3, material: &str) {
    let e = s.spawn(name, None);
    let mut t = Transform::from_position(at);
    t.scale = scale;
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(path(material)),
            ..Default::default()
        },
    );
    s.game_object_mut(e).is_static = true;
}

/// Room interior: x -4..4, y 0..3.2, z 0..8; the doorway (1.6 × 2.4 m) is in the z = 0 wall.
fn geometry(s: &mut Scene) {
    use PrimitiveMesh::{Cube, Plane, Sphere};
    let v = Vec3::new;
    add(s, "Lawn", Plane, v(0.0, 0.0, 0.0), v(2.4, 1.0, 2.4), "Lawn");
    add(
        s,
        "Floor",
        Cube,
        v(0.0, 0.01, 4.0),
        v(8.0, 0.02, 8.0),
        "Glossy Tiles",
    );
    add(
        s,
        "Red Wall",
        Cube,
        v(-4.1, 1.6, 4.0),
        v(0.2, 3.2, 8.4),
        "Red Plaster",
    );
    add(
        s,
        "Green Wall",
        Cube,
        v(4.1, 1.6, 4.0),
        v(0.2, 3.2, 8.4),
        "Green Plaster",
    );
    add(
        s,
        "Blue Wall",
        Cube,
        v(0.0, 1.6, 8.1),
        v(8.0, 3.2, 0.2),
        "Blue Plaster",
    );
    add(
        s,
        "Front Wall L",
        Cube,
        v(-2.4, 1.6, -0.1),
        v(3.2, 3.2, 0.2),
        "White Plaster",
    );
    add(
        s,
        "Front Wall R",
        Cube,
        v(2.4, 1.6, -0.1),
        v(3.2, 3.2, 0.2),
        "White Plaster",
    );
    add(
        s,
        "Lintel",
        Cube,
        v(0.0, 2.8, -0.1),
        v(1.6, 0.8, 0.2),
        "White Plaster",
    );
    add(
        s,
        "Ceiling",
        Cube,
        v(0.0, 3.3, 4.0),
        v(8.4, 0.2, 8.6),
        "White Plaster",
    );
    add(
        s,
        "Chrome Sphere",
        Sphere,
        v(-1.6, 0.62, 4.6),
        Vec3::splat(1.2),
        "Chrome",
    );
    add(
        s,
        "Gold Sphere",
        Sphere,
        v(1.5, 0.52, 5.4),
        Vec3::splat(1.0),
        "Gold",
    );
    add(
        s,
        "Copper Sphere",
        Sphere,
        v(0.1, 0.42, 6.6),
        Vec3::splat(0.8),
        "Copper",
    );
    add(
        s,
        "Plinth",
        Cube,
        v(2.7, 0.4, 2.2),
        v(0.8, 0.8, 0.8),
        "White Plaster",
    );
    add(
        s,
        "Big Chrome Ball",
        Sphere,
        v(-4.5, 1.2, -5.5),
        Vec3::splat(2.4),
        "Chrome",
    );
    add(
        s,
        "Pillar",
        Cube,
        v(4.5, 1.5, -4.5),
        v(0.8, 3.0, 0.8),
        "Concrete",
    );
    add(
        s,
        "Low Wall",
        Cube,
        v(0.0, 0.4, -9.0),
        v(6.0, 0.8, 0.4),
        "Concrete",
    );
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("ReflectionsDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        // Just inside the doorway, looking into the room.
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.55, 0.6))
                .with_euler(Vec3::new(10.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // Low enough to shine through the doorway onto the tiles.
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 6.0, -6.0))
                .with_euler(Vec3::new(32.0, 12.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 1.3;
        l.shadows = Shadows::Soft;
    }
    s.render_settings.skybox.sun_disk = SunDisk::HighQuality;
    geometry(&mut s);
    let lamp = s.spawn("Ceiling Lamp", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(0.0, 2.9, 4.6)));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.88, 0.72],
            intensity: 1.4,
            range: 9.0,
            shadows: Shadows::Soft,
            ..Default::default()
        },
    );

    // Generate Probes, with the baked files the writer saves.
    let placed = placement::place(&placement::scene_geometry(&s), &generator());
    crate::probe_tools::spawn_group(&mut s, &generator(), &placed);
    let list = probes::scene_probe_list(&s);
    for (i, (e, _)) in list.iter().enumerate() {
        let mut p = s.world.get::<&mut ReflectionProbe>(*e).unwrap();
        p.baked = Some(probes::asset_path_for(SCENE, i));
    }
    s
}

/// The demo's materials without reading the project folder.
struct Demo(BTreeMap<String, MaterialParams>);

impl Demo {
    fn new() -> Self {
        Self(
            materials()
                .into_iter()
                .map(|(n, m)| (path(n), m.params()))
                .collect(),
        )
    }
}

impl MaterialSource for Demo {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.0.get(r)).cloned().unwrap_or_default()
    }
}

/// A device without a window (`None` = no adapter on this machine).
fn headless() -> Option<(wgpu::Device, wgpu::Queue)> {
    let instance =
        wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());
    let adapter =
        pollster::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions::default()))
            .ok()?;
    pollster::block_on(adapter.request_device(&Default::default())).ok()
}

use misty_engine::wgpu;

#[test]
#[ignore = "writes into SampleProject and bakes on the GPU; run on purpose to regenerate"]
fn write_reflections_demo() {
    let root = project();
    std::fs::create_dir_all(root.join(FOLDER)).unwrap();
    for (name, m) in materials() {
        m.save(&root.join(path(name))).unwrap();
    }
    let scene = demo_scene();
    scene.save(&root.join(SCENE)).unwrap();
    let (device, queue) = headless().expect("a GPU adapter");
    let mut renderer = misty_engine::render::Renderer::new(&device);
    let baked = probes::capture::bake_scene(
        &mut renderer,
        &device,
        &queue,
        &scene,
        &Demo::new(),
        generator().second_bounce,
    );
    for (i, (_, data)) in baked.iter().enumerate() {
        data.save(&root.join(probes::asset_path_for(SCENE, i)))
            .unwrap();
    }
}

#[test]
fn reflections_demo_matches_its_generator() {
    let root = project();
    for (name, m) in materials() {
        assert_eq!(Material::load(&root.join(path(name))).unwrap(), m, "{name}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_reflections_demo"
    );
    let (scene, report) = Scene::from_json("ReflectionsDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Generated probes: one or two for the room, a coarse grid outside; every one baked.
    let list = probes::scene_probe_list(&scene);
    assert!((4..=40).contains(&list.len()), "{}", list.len());
    let room = |p: Vec3| p.x.abs() < 4.0 && p.z > 0.0 && p.z < 8.0 && p.y < 3.2;
    let indoor: Vec<_> = list
        .iter()
        .filter(|(e, _)| room(scene.world_position(*e)))
        .collect();
    assert!((1..=2).contains(&indoor.len()), "{}", indoor.len());
    for (e, p) in &indoor {
        assert!(p.box_projection, "the room probe fits the walls");
        let (lo, hi) = p.world_box(scene.world_position(*e));
        assert!(
            (lo.x + 4.0).abs() < 0.2 && (hi.x - 4.0).abs() < 0.2,
            "{lo} {hi}"
        );
        assert!(
            (hi.y - 3.2).abs() < 0.2 && (hi.z - 8.0).abs() < 0.2,
            "{lo} {hi}"
        );
    }
    for (i, (_, p)) in list.iter().enumerate() {
        let rel = p.baked.as_deref().unwrap();
        assert_eq!(rel, probes::asset_path_for(SCENE, i));
        let data = ProbeData::load(&root.join(rel)).unwrap();
        assert_eq!(data.size(), 128);
    }
}

/// The baked probes make the room's chrome sphere reflect the coloured walls and darken the
/// indoor ambient compared with sky lighting.
#[test]
fn reflections_demo_renders_with_its_probes() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let view = misty_engine::render_scene::GameView::new(&scene, &Demo::new()).unwrap();
    let mut with = view.environment.clone();
    with.probes = ProbeSet {
        probes: probes::scene_probe_list(&scene)
            .into_iter()
            .map(|(e, p)| {
                let data = ProbeData::load(&root.join(p.baked.as_deref().unwrap())).unwrap();
                ProbeInstance::new(&p, scene.world_position(e), std::sync::Arc::new(data))
            })
            .collect(),
        debug: false,
    };
    let mut without = view.environment.clone();
    without.probes = ProbeSet::default();
    let mut renderer = misty_engine::render::Renderer::new(&device);
    let target = renderer.create_target(&device, 320, 180);
    let mut render = |env: &misty_engine::render::Environment| {
        let mut frame = view.frame();
        frame.environment = env;
        renderer.render(&device, &queue, &target, &frame);
        read_back(&device, &queue, &target)
    };
    let (a, b) = (render(&with), render(&without));
    let differing = a.chunks(4).zip(b.chunks(4)).filter(|(x, y)| x != y).count();
    assert!(differing > 320 * 180 / 3, "{differing}");
    // The room is darker without sky ambient leaking in.
    let mean = |img: &[u8]| img.iter().map(|v| *v as f32).sum::<f32>() / img.len() as f32;
    assert!(mean(&a) < mean(&b), "{} vs {}", mean(&a), mean(&b));
}

/// The final image of `target` as RGBA rows.
fn read_back(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    target: &misty_engine::render::RenderTarget,
) -> Vec<u8> {
    let (w, h) = target.size();
    let size = wgpu::Extent3d {
        width: w,
        height: h,
        depth_or_array_layers: 1,
    };
    let copy = device.create_texture(&wgpu::TextureDescriptor {
        label: None,
        size,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: misty_engine::render::COLOR_FORMAT,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::COPY_SRC,
        view_formats: &[],
    });
    let row = (w * 4).div_ceil(256) * 256;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor {
        label: None,
        size: (row * h) as u64,
        usage: wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::MAP_READ,
        mapped_at_creation: false,
    });
    let mut encoder = device.create_command_encoder(&Default::default());
    target.copy_output(&mut encoder, &copy);
    encoder.copy_texture_to_buffer(
        copy.as_image_copy(),
        wgpu::TexelCopyBufferInfo {
            buffer: &buffer,
            layout: wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(row),
                rows_per_image: Some(h),
            },
        },
        size,
    );
    queue.submit([encoder.finish()]);
    let slice = buffer.slice(..);
    slice.map_async(wgpu::MapMode::Read, |_| {});
    let _ = device.poll(wgpu::PollType::wait_indefinitely());
    let data = slice.get_mapped_range().unwrap();
    (0..h as usize)
        .flat_map(|y| data[y * row as usize..][..w as usize * 4].to_vec())
        .collect()
}
