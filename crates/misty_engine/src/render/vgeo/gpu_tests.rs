//! Misty Virtual Geometry on the GPU: the cluster path draws what the ordinary mesh path
//! draws (run with the other GPU tests), plus the rock field demo: screenshots and frame time
//! against LOD Groups (ignored; run on purpose in release):
//! `cargo test --release -p misty_engine --lib vgeo_field -- --ignored --nocapture --test-threads=1`
//! `MISTY_VGEO_ROCKS=<n>` (default 3000), `MISTY_VGEO_SUBDIV=<n>` (6 = 82 k triangles per rock),
//! `MISTY_VGEO_SIZE=WxH` (default 1920x1080), `MISTY_GFX_DIR` writes the shots elsewhere.

use std::path::PathBuf;

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::render::environment::Environment;
use crate::render::gpu_tests::{headless, read_back};
use crate::render::lod::LodRange;
use crate::render::{
    Frame, LightData, MaterialParams, MeshDraw, RenderOptions, RenderTarget, Renderer,
};
use crate::scene::{LightType, MeshRef, PrimitiveMesh, Shadows};
use crate::vgeo::rock;

fn screenshots() -> PathBuf {
    match std::env::var("MISTY_GFX_DIR") {
        Ok(d) => PathBuf::from(d),
        Err(_) => PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx"),
    }
}

fn sun() -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(-0.45, -0.75, 0.5).normalize(),
        color: [1.0, 0.96, 0.9],
        intensity: 2.2,
        range: 10.0,
        spot_angle: 30.0,
        shadows: Shadows::Hard,
        contact_shadows: false,
        area: Default::default(),
        shadow: Default::default(),
        volumetric: Default::default(),
    }
}

fn rock_material() -> MaterialParams {
    MaterialParams {
        color: [0.46, 0.43, 0.40, 1.0],
        smoothness: 0.18,
        ..Default::default()
    }
}

fn ground_material() -> MaterialParams {
    MaterialParams {
        color: [0.30, 0.33, 0.24, 1.0],
        smoothness: 0.05,
        ..Default::default()
    }
}

fn mesh_draw(mesh: MeshRef, model: Mat4, material: &MaterialParams) -> MeshDraw {
    MeshDraw {
        mesh,
        model,
        material: material.clone(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

fn ground(half: f32) -> MeshDraw {
    // The Plane primitive is 10 × 10 m.
    let model = Mat4::from_scale(Vec3::new(half / 5.0, 1.0, half / 5.0));
    mesh_draw(PrimitiveMesh::Plane.into(), model, &ground_material())
}

fn camera(position: Vec3, target: Vec3) -> Camera {
    let forward = (target - position).normalize();
    Camera {
        position,
        rotation: Quat::from_rotation_arc(Vec3::Z, forward),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 2000.0,
    }
}

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    target: &RenderTarget,
    camera: &Camera,
    meshes: &[MeshDraw],
    frames: usize,
) -> Vec<u8> {
    let environment = Environment::default();
    let lights = [sun()];
    let frame = Frame {
        camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: Default::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    for _ in 0..frames {
        r.render(device, queue, target, &frame);
    }
    read_back(device, queue, target)
}

/// Mean absolute difference (levels) and the share of pixels off by more than 24 levels.
fn difference(a: &[u8], b: &[u8]) -> (f32, f32) {
    let mut sum = 0u64;
    let mut off = 0usize;
    for (pa, pb) in a.chunks(4).zip(b.chunks(4)) {
        let d = (0..3)
            .map(|k| (pa[k] as i32 - pb[k] as i32).unsigned_abs())
            .max()
            .unwrap_or(0);
        sum += d as u64;
        off += (d > 24) as usize;
    }
    let n = (a.len() / 4).max(1);
    (sum as f32 / n as f32, off as f32 / n as f32)
}

fn quality(virtual_geometry: bool, error: f32) -> QualityLevel {
    QualityLevel {
        virtual_geometry,
        virtual_geometry_error: error,
        temporal_aa: false,
        ..QualityLevel::medium()
    }
}

/// A rock drawn through clusters looks like the same rock drawn as a mesh (at a fine error
/// the finest clusters are the source triangles), casts the same shadow, and a coarser error
/// draws fewer triangles without opening holes.
#[test]
fn virtual_rock_matches_the_mesh_path() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let data = rock::rock(5, 2);
    let source = data.indices.len() / 3;
    let plain = MeshRef::Asset(crate::models::runtime::register(
        "vgeo-test-rock-plain",
        data.clone(),
    ));
    let virt = MeshRef::Asset(crate::models::runtime::register_virtual(
        "vgeo-test-rock",
        data,
    ));
    let at = Mat4::from_scale_rotation_translation(
        Vec3::splat(1.0),
        Quat::from_rotation_y(0.7),
        Vec3::new(0.0, 0.6, 0.0),
    );
    let cam = camera(Vec3::new(2.5, 2.2, -4.0), Vec3::new(0.0, 0.5, 0.0));
    let scene = |mesh: &MeshRef| vec![ground(20.0), mesh_draw(mesh.clone(), at, &rock_material())];

    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &quality(false, 1.0));
    let target = r.create_target(&device, 480, 320);
    let reference = render(&device, &queue, &mut r, &target, &cam, &scene(&plain), 4);

    let mut v = Renderer::with_queue(&device, &queue);
    v.set_quality(&device, &quality(true, 0.1));
    let target_v = v.create_target(&device, 480, 320);
    let fine = render(&device, &queue, &mut v, &target_v, &cam, &scene(&virt), 4);
    let fine_stats = v.virtual_geometry_stats(&device, &queue);
    // The rock is covered and nothing was dropped.
    assert_eq!(fine_stats.instances, 1);
    assert_eq!(fine_stats.dropped, 0);
    assert!(
        fine_stats.triangles > 0 && fine_stats.shadow_triangles > 0,
        "{fine_stats:?}"
    );
    // Where the device can, the main pass resolves the clusters from the visibility buffer.
    let f = device.features();
    let can_vis = f.contains(wgpu::Features::PRIMITIVE_INDEX)
        && f.contains(wgpu::Features::MULTI_DRAW_INDIRECT_COUNT);
    assert_eq!(fine_stats.visibility, can_vis, "{fine_stats:?}");
    let (mean, off) = difference(&reference, &fine);
    assert!(
        mean < 1.5 && off < 0.004,
        "fine: mean {mean}, {:.3} % off",
        off * 100.0
    );
    // The same clusters drawn lit directly (no visibility buffer).
    v.set_virtual_geometry_visibility(false);
    let lit = render(&device, &queue, &mut v, &target_v, &cam, &scene(&virt), 4);
    assert!(!v.virtual_geometry_stats(&device, &queue).visibility);
    v.set_virtual_geometry_visibility(true);
    let (lit_mean, lit_off) = difference(&reference, &lit);
    assert!(
        lit_mean < 1.5 && lit_off < 0.004,
        "lit: mean {lit_mean}, {:.3} % off",
        lit_off * 100.0
    );
    eprintln!(
        "against the mesh path: visibility buffer mean {mean:.3} ({:.3} % off), lit {lit_mean:.3} ({:.3} % off)",
        off * 100.0,
        lit_off * 100.0
    );
    // The mesh path without virtual geometry on draws the virtual mesh as a mesh.
    let mut o = Renderer::with_queue(&device, &queue);
    o.set_quality(&device, &quality(false, 1.0));
    let target_o = o.create_target(&device, 480, 320);
    let off_path = render(&device, &queue, &mut o, &target_o, &cam, &scene(&virt), 4);
    assert_eq!(difference(&reference, &off_path).0, 0.0);

    v.set_quality(&device, &quality(true, 4.0));
    let coarse = render(&device, &queue, &mut v, &target_v, &cam, &scene(&virt), 4);
    let coarse_stats = v.virtual_geometry_stats(&device, &queue);
    let (mean, off) = difference(&reference, &coarse);
    assert!(
        coarse_stats.triangles < fine_stats.triangles,
        "{coarse_stats:?} vs {fine_stats:?}"
    );
    assert!(
        mean < 4.0 && off < 0.03,
        "coarse: mean {mean}, {:.3} % off",
        off * 100.0
    );
    eprintln!(
        "source {source} tris; fine {} tris ({} clusters); 4 px {} tris; shadow {} / {}",
        fine_stats.triangles,
        fine_stats.clusters,
        coarse_stats.triangles,
        fine_stats.shadow_triangles,
        coarse_stats.shadow_triangles
    );
    crate::models::runtime::unregister("vgeo-test-rock");
}

/// Rocks of the field: the same three generated rocks for both paths.
struct Rocks {
    virtual_keys: Vec<MeshRef>,
    /// LOD 0..3 per rock (all, ¼, 1⁄16, 1⁄64 of the triangles).
    lod_keys: Vec<[MeshRef; 4]>,
    /// Object-space size (largest dimension) per rock.
    sizes: Vec<f32>,
    triangles: Vec<[usize; 4]>,
}

fn make_rocks(subdivisions: u32) -> Rocks {
    let mut rocks = Rocks {
        virtual_keys: Vec::new(),
        lod_keys: Vec::new(),
        sizes: Vec::new(),
        triangles: Vec::new(),
    };
    for seed in 0..3u32 {
        let data = rock::rock(subdivisions, 11 + seed * 5);
        let (lo, hi) = data.bounds();
        rocks.sizes.push((hi - lo).max_element());
        let levels = [
            data.clone(),
            rock::simplified(&data, 0.25),
            rock::simplified(&data, 1.0 / 16.0),
            rock::simplified(&data, 1.0 / 64.0),
        ];
        rocks
            .triangles
            .push(levels.clone().map(|l| l.indices.len() / 3));
        let keys = levels.map(|l| {
            let name = format!("vgeo-field-rock{seed}-lod{}", l.indices.len());
            MeshRef::Asset(crate::models::runtime::register(&name, l))
        });
        rocks.lod_keys.push(keys);
        let name = format!("vgeo-field-rock{seed}-virtual");
        rocks
            .virtual_keys
            .push(MeshRef::Asset(crate::models::runtime::register_virtual(
                &name, data,
            )));
    }
    rocks
}

#[derive(Clone, Copy, PartialEq)]
enum Path {
    /// Every rock at full detail (no LOD).
    Full,
    /// Four-level LOD Groups with the default switch heights (60 %, 30 %, 10 %, never culled).
    LodGroup,
    Virtual,
}

/// A field of `n` rocks scattered over 320 × 320 m (sizes 0.5–3 m, a few 6 m boulders).
fn field(rocks: &Rocks, n: usize, path: Path) -> Vec<MeshDraw> {
    let mut draws = vec![ground(200.0)];
    let mut state = 0x9e37_79b9u32;
    let mut rand = || {
        state ^= state << 13;
        state ^= state >> 17;
        state ^= state << 5;
        state as f32 / u32::MAX as f32
    };
    let side = (n as f32).sqrt().ceil() as usize;
    let spacing = 320.0 / side as f32;
    let material = rock_material();
    for i in 0..n {
        let (gx, gz) = ((i % side) as f32, (i / side) as f32);
        let x = -160.0 + (gx + rand()) * spacing;
        let z = -100.0 + (gz + rand()) * spacing;
        let kind = (rand() * 3.0) as usize % 3;
        let boulder = rand() < 0.03;
        let scale = if boulder {
            3.0 + 2.0 * rand()
        } else {
            0.25 + 1.25 * rand() * rand() + 0.25 * rand()
        };
        let rotation = Quat::from_rotation_y(rand() * std::f32::consts::TAU)
            * Quat::from_rotation_x((rand() - 0.5) * 0.4);
        let model = Mat4::from_scale_rotation_translation(
            Vec3::splat(scale),
            rotation,
            Vec3::new(x, 0.25 * scale, z),
        );
        match path {
            Path::Full => draws.push(mesh_draw(rocks.lod_keys[kind][0].clone(), model, &material)),
            Path::Virtual => draws.push(mesh_draw(
                rocks.virtual_keys[kind].clone(),
                model,
                &material,
            )),
            Path::LodGroup => {
                let center = model.transform_point3(Vec3::ZERO);
                let size = rocks.sizes[kind] * scale;
                let heights = [f32::INFINITY, 0.6, 0.3, 0.1, 0.0];
                for level in 0..4 {
                    let mut d = mesh_draw(rocks.lod_keys[kind][level].clone(), model, &material);
                    d.lod = Some(LodRange {
                        center,
                        size,
                        min: heights[level + 1],
                        max: heights[level],
                        fade: 0.0,
                    });
                    draws.push(d);
                }
            }
        }
    }
    draws
}

#[derive(Clone, Copy)]
enum Shot {
    /// Standing in the field, looking across it.
    Field,
    /// Close to a group of rocks: detail where it is needed.
    Close,
    /// From above, the whole field.
    Aerial,
}

impl Shot {
    fn name(self) -> &'static str {
        match self {
            Shot::Field => "field",
            Shot::Close => "close",
            Shot::Aerial => "aerial",
        }
    }

    fn camera(self) -> Camera {
        match self {
            Shot::Field => camera(Vec3::new(0.0, 2.2, -112.0), Vec3::new(8.0, 0.0, -40.0)),
            Shot::Close => camera(Vec3::new(-3.0, 1.6, -96.0), Vec3::new(2.0, 0.3, -86.0)),
            Shot::Aerial => camera(Vec3::new(0.0, 140.0, -230.0), Vec3::new(0.0, 0.0, 0.0)),
        }
    }
}

fn env_size() -> (u32, u32) {
    std::env::var("MISTY_VGEO_SIZE")
        .ok()
        .and_then(|s| {
            let (w, h) = s.split_once('x')?;
            Some((w.parse().ok()?, h.parse().ok()?))
        })
        .unwrap_or((1920, 1080))
}

fn save(name: &str, (w, h): (u32, u32), pixels: Vec<u8>) {
    let dir = screenshots();
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("vgeo_{name}.png"));
    image::RgbaImage::from_raw(w, h, pixels)
        .expect("image size")
        .save(&path)
        .unwrap();
    eprintln!("wrote {}", path.display());
}

struct Measured {
    gpu_ms: f32,
    cpu_ms: f32,
    triangles: f64,
    parts: Vec<(&'static str, f32)>,
}

/// Median GPU and CPU time over `frames` frames after `settle`, the triangles drawn and the
/// median of every timed part.
#[allow(clippy::too_many_arguments)]
fn measure(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    r: &mut Renderer,
    target: &RenderTarget,
    camera: &Camera,
    meshes: &[MeshDraw],
    path: Path,
) -> Measured {
    let environment = Environment::default();
    let lights = [sun()];
    let frame = Frame {
        camera,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post: Default::default(),
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        shaded: true,
        lights: &lights,
        water: &[],
    };
    // Full detail runs at a few frames per second: fewer frames.
    let (settle, frames) = if path == Path::Full {
        (4, 12)
    } else {
        (30, 90)
    };
    let (mut gpu, mut cpu) = (Vec::new(), Vec::new());
    let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
    for f in 0..settle + frames {
        r.render(device, queue, target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if f < settle {
            continue;
        }
        if let Some(t) = r.gpu_ms() {
            gpu.push(t);
        }
        cpu.push(r.stats().cpu_ms);
        for &(label, ms) in r.pass_times() {
            match parts.iter_mut().find(|(l, _)| *l == label) {
                Some((_, v)) => v.push(ms),
                None => parts.push((label, vec![ms])),
            }
        }
    }
    let median = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    let triangles = match path {
        Path::Virtual => {
            let s = r.virtual_geometry_stats(device, queue);
            // The ground plane goes through the mesh path.
            (s.triangles + r.stats().triangles) as f64
        }
        _ => r.stats().triangles as f64,
    };
    Measured {
        gpu_ms: median(&mut gpu),
        cpu_ms: median(&mut cpu),
        triangles: triangles / 1e6,
        parts: parts
            .into_iter()
            .map(|(l, mut v)| (l, median(&mut v)))
            .collect(),
    }
}

/// The rock field demo: shots of each path and of the cluster debug view, then the median
/// frame time, triangles and the timed parts of each path per view, at Medium and High.
#[test]
#[ignore = "demo: screenshots and timings, run in release"]
fn vgeo_field() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let n: usize = std::env::var("MISTY_VGEO_ROCKS")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(3000);
    let subdivisions: u32 = std::env::var("MISTY_VGEO_SUBDIV")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(6);
    let size = env_size();
    let started = std::time::Instant::now();
    let rocks = make_rocks(subdivisions);
    eprintln!(
        "{n} rocks, {:?} triangles per LOD level ({:.1} s to generate)",
        rocks.triangles[0],
        started.elapsed().as_secs_f32()
    );
    let lod_field = field(&rocks, n, Path::LodGroup);
    let virtual_field = field(&rocks, n, Path::Virtual);
    let full_field = field(&rocks, n, Path::Full);
    let only = std::env::var("MISTY_GFX_SHOT").unwrap_or_default();
    let shots: Vec<Shot> = [Shot::Field, Shot::Close, Shot::Aerial]
        .into_iter()
        .filter(|s| only.is_empty() || only == s.name())
        .collect();

    // Lay out the pool once before anything is measured (the clusters were built when the
    // meshes were registered, as an import does).
    let build_ms: f32 = rocks
        .virtual_keys
        .iter()
        .filter_map(|k| k.split_asset())
        .filter_map(|(path, _)| crate::models::model(path).ok())
        .map(|m| m.virtual_builds.summary().build_ms)
        .sum();
    {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality(true, 1.0));
        let target = r.create_target(&device, 64, 64);
        render(
            &device,
            &queue,
            &mut r,
            &target,
            &Shot::Field.camera(),
            &virtual_field,
            1,
        );
        let s = r.virtual_geometry_stats(&device, &queue);
        eprintln!(
            "cluster build: {:.1} s for {} meshes ({} source triangles), pool {:.1} MB",
            build_ms / 1000.0,
            s.meshes,
            s.source_triangles,
            s.pool_bytes as f64 / 1e6
        );
    }

    if std::env::var("MISTY_VGEO_SHOTS").map_or(true, |v| v != "0") {
        for &shot in &shots {
            let cam = shot.camera();
            let base = QualityLevel {
                virtual_geometry_error: 1.0,
                ..QualityLevel::high()
            };
            for (label, draws, vg, debug) in [
                ("lodgroup", &lod_field, false, false),
                ("virtual", &virtual_field, true, false),
                ("clusters", &virtual_field, true, true),
            ] {
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(
                    &device,
                    &QualityLevel {
                        virtual_geometry: vg,
                        ..base.clone()
                    },
                );
                r.set_virtual_geometry_debug(debug);
                let target = r.create_target(&device, size.0, size.1);
                let px = render(&device, &queue, &mut r, &target, &cam, draws, 24);
                save(&format!("{}_{label}", shot.name()), size, px);
            }
        }
    }

    let wanted = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "medium,high".into());
    for (qname, q) in [
        ("medium", QualityLevel::medium()),
        ("high", QualityLevel::high()),
    ] {
        if !wanted.split(',').any(|w| w == qname) {
            continue;
        }
        for &shot in &shots {
            let cam = shot.camera();
            let mut rows: Vec<(&str, Measured)> = Vec::new();
            for (label, draws, path, gpu_culling) in [
                ("full detail", &full_field, Path::Full, false),
                ("LOD Groups", &lod_field, Path::LodGroup, false),
                ("LOD Groups + GPU culling", &lod_field, Path::LodGroup, true),
                (
                    "Virtual Geometry 1 px",
                    &virtual_field,
                    Path::Virtual,
                    false,
                ),
                (
                    "Virtual Geometry 2 px",
                    &virtual_field,
                    Path::Virtual,
                    false,
                ),
            ] {
                if path == Path::Full && std::env::var("MISTY_VGEO_FULL").is_ok_and(|v| v == "0") {
                    continue;
                }
                let mut r = Renderer::with_queue(&device, &queue);
                r.set_quality(
                    &device,
                    &QualityLevel {
                        virtual_geometry: path == Path::Virtual,
                        virtual_geometry_error: if label.ends_with("2 px") { 2.0 } else { 1.0 },
                        ..q.clone()
                    },
                );
                r.set_options(RenderOptions {
                    gpu_culling,
                    ..Default::default()
                });
                r.set_pass_timing(&device, &queue, true);
                let target = r.create_target(&device, size.0, size.1);
                let m = measure(&device, &queue, &mut r, &target, &cam, draws, path);
                rows.push((label, m));
            }
            eprintln!("\n{qname} / {} ({}x{}):", shot.name(), size.0, size.1);
            for (label, m) in &rows {
                eprintln!(
                    "  {label:<26} GPU {:>7.2} ms  CPU {:>6.2} ms  {:>8.2} M triangles",
                    m.gpu_ms, m.cpu_ms, m.triangles
                );
            }
            for (label, m) in &rows {
                let parts: Vec<String> = m
                    .parts
                    .iter()
                    .filter(|(_, ms)| *ms >= 0.05)
                    .map(|(l, ms)| format!("{l} {ms:.2}"))
                    .collect();
                eprintln!("  [{label}] {}", parts.join(" | "));
            }
        }
    }
}
