//! Headless checks of SSGI and SSR: the bounce off a red wall tints the floor beside it without
//! darkening it beyond AO, a mirror floor shows the cube standing on it, the accumulation
//! settles, and with both off no pass runs and the image is unchanged.

use glam::{Mat4, Quat, Vec3};

use super::super::gpu_tests::{headless, read_back};
use super::super::*;
use crate::camera::{Camera, Projection};
use crate::scene::{
    AmbientOcclusionOverride, PrimitiveMesh, ScreenSpaceGlobalIlluminationOverride,
    ScreenSpaceReflectionsOverride,
};

const W: u32 = 320;
const H: u32 = 240;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 2.2, -5.0),
        rotation: Quat::from_rotation_x(18f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn draw(mesh: PrimitiveMesh, model: Mat4, color: [f32; 3], smoothness: f32) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: MaterialParams {
            color: [color[0], color[1], color[2], 1.0],
            smoothness,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

fn boxed(center: Vec3, size: Vec3, color: [f32; 3], smoothness: f32) -> MeshDraw {
    let model = Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center);
    draw(PrimitiveMesh::Cube, model, color, smoothness)
}

/// White floor, a red wall on the left (inner face at x = -2), a white back wall.
fn room() -> Vec<MeshDraw> {
    vec![
        boxed(
            Vec3::new(0.0, -0.05, 1.0),
            Vec3::new(8.0, 0.1, 10.0),
            [0.8; 3],
            0.1,
        ),
        boxed(
            Vec3::new(-2.15, 1.5, 1.0),
            Vec3::new(0.3, 3.0, 10.0),
            [0.9, 0.08, 0.06],
            0.1,
        ),
        boxed(
            Vec3::new(0.0, 1.5, 3.15),
            Vec3::new(8.0, 3.0, 0.3),
            [0.8; 3],
            0.1,
        ),
    ]
}

/// Sun from the right, low: it lights the red wall's inner face brightly.
fn sun() -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(-1.0, -0.5, 0.3).normalize(),
        color: [1.0; 3],
        intensity: 1.5,
        range: 0.0,
        spot_angle: 0.0,
        shadows: Shadows::None,
        shadow: Default::default(),
        volumetric: Default::default(),
        contact_shadows: false,
        area: Default::default(),
    }
}

fn ssgi(temporal: bool) -> PostSettings {
    PostSettings {
        ssgi: SsgiSettings::from_override(&ScreenSpaceGlobalIlluminationOverride {
            active: true,
            temporal,
            ..Default::default()
        }),
        ..Default::default()
    }
}

fn ssr() -> PostSettings {
    PostSettings {
        ssr: SsrSettings::from_override(&ScreenSpaceReflectionsOverride {
            active: true,
            ..Default::default()
        }),
        ..Default::default()
    }
}

struct View<'a> {
    meshes: &'a [MeshDraw],
    lights: &'a [LightData],
}

fn render(
    renderer: &mut Renderer,
    gpu: &(wgpu::Device, wgpu::Queue),
    target: &RenderTarget,
    view: &View,
    post: PostSettings,
) -> Vec<u8> {
    let (device, queue) = gpu;
    let cam = camera();
    let environment = Environment::default();
    let frame = Frame {
        camera: &cam,
        grid: None,
        skybox: true,
        background: [0.1; 3],
        environment: &environment,
        post,
        lines: &[],
        overlay: &[],
        meshes: view.meshes,
        particles: &[],
        terrains: &[],
        water: &[],
        shaded: true,
        lights: view.lights,
    };
    renderer.render(device, queue, target, &frame);
    read_back(device, queue, target)
}

/// Render `frames` times and keep the last image.
fn settle(
    renderer: &mut Renderer,
    gpu: &(wgpu::Device, wgpu::Queue),
    target: &RenderTarget,
    view: &View,
    post: PostSettings,
    frames: u32,
) -> Vec<u8> {
    let mut image = Vec::new();
    for _ in 0..frames {
        image = render(renderer, gpu, target, view, post);
    }
    image
}

/// Mean sRGB colour (0..255) of a 5×5 block around the pixel showing world point `p`.
fn rgb(image: &[u8], p: Vec3) -> Vec3 {
    let ndc = camera()
        .view_projection(W as f32 / H as f32)
        .project_point3(p);
    let x = ((ndc.x * 0.5 + 0.5) * W as f32) as i32;
    let y = ((0.5 - ndc.y * 0.5) * H as f32) as i32;
    let mut sum = Vec3::ZERO;
    for dy in -2..=2 {
        for dx in -2..=2 {
            let i = (((y + dy) as u32 * W + (x + dx) as u32) * 4) as usize;
            sum += Vec3::new(image[i] as f32, image[i + 1] as f32, image[i + 2] as f32);
        }
    }
    sum / 25.0
}

fn redness(c: Vec3) -> f32 {
    c.x - (c.y + c.z) * 0.5
}

fn mean(image: &[u8]) -> f32 {
    let sum: f64 = image
        .chunks(4)
        .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
        .sum();
    (sum / (image.len() / 4) as f64) as f32
}

fn save(name: &str, image: &[u8]) {
    // `MISTY_GPU_TEST_PNG=<dir>` saves the images for a look.
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_PNG") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(W, H, image.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

#[test]
fn ssgi_bleeds_the_red_wall_onto_the_floor() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let (meshes, lights) = (room(), [sun()]);
    let view = View {
        meshes: &meshes,
        lights: &lights,
    };
    let off = settle(&mut renderer, &gpu, &target, &view, Default::default(), 2);
    let on = settle(&mut renderer, &gpu, &target, &view, ssgi(true), 24);
    let ao = PostSettings {
        ao: AoSettings::from_override(&AmbientOcclusionOverride {
            active: true,
            full_resolution: true,
            ..Default::default()
        }),
        ..Default::default()
    };
    let ao_only = settle(&mut renderer, &gpu, &target, &view, ao, 12);
    save("ssgi_room_off", &off);
    save("ssgi_room_on", &on);
    save("ssgi_room_ao", &ao_only);

    let beside = Vec3::new(-1.6, 0.0, 0.0);
    let (a, b) = (rgb(&off, beside), rgb(&on, beside));
    eprintln!("floor beside the red wall: off {a}, on {b}");
    assert!(
        redness(b) > redness(a) + 6.0,
        "colour bleeding: {} → {}",
        redness(a),
        redness(b)
    );
    // The bounce only adds light where rays hit; elsewhere it never darkens past what AO does.
    let (m_off, m_on, m_ao) = (mean(&off), mean(&on), mean(&ao_only));
    eprintln!("mean brightness: off {m_off}, SSGI {m_on}, AO {m_ao}");
    assert!(m_on >= m_ao - 2.0, "{m_on} vs AO {m_ao}");
    let open = Vec3::new(1.0, 0.0, -1.5);
    let (o_off, o_on) = (rgb(&off, open), rgb(&on, open));
    assert!(
        o_on.element_sum() >= o_off.element_sum() - 3.0 * 12.0,
        "open floor: {o_off} → {o_on}"
    );

    // With AO on too, the traced visibility is not applied again (no double darkening).
    let both = PostSettings {
        ssgi: ssgi(true).ssgi,
        ..ao
    };
    let both = settle(&mut renderer, &gpu, &target, &view, both, 16);
    save("ssgi_room_ao_and_ssgi", &both);
    assert!(mean(&both) >= m_ao - 1.0, "{} vs {m_ao}", mean(&both));
}

#[test]
fn ssr_mirrors_the_cube_in_a_glossy_floor() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let mut floor = boxed(
        Vec3::new(0.0, -0.05, 1.0),
        Vec3::new(8.0, 0.1, 10.0),
        [0.9; 3],
        1.0,
    );
    floor.material.metallic = 1.0;
    let meshes = vec![
        floor,
        boxed(Vec3::new(0.0, 0.5, 0.0), Vec3::ONE, [0.95, 0.1, 0.05], 0.2),
    ];
    let lights = [sun()];
    let view = View {
        meshes: &meshes,
        lights: &lights,
    };
    let off = settle(&mut renderer, &gpu, &target, &view, Default::default(), 2);
    let on = settle(&mut renderer, &gpu, &target, &view, ssr(), 6);
    save("ssr_mirror_off", &off);
    save("ssr_mirror_on", &on);
    // Mirror image of the cube's front face, seen in the floor in front of it.
    let mirrored = Vec3::new(0.0, -0.3, -0.5);
    let (a, b) = (rgb(&off, mirrored), rgb(&on, mirrored));
    eprintln!("mirrored cube: off {a}, on {b}");
    assert!(redness(b) > redness(a) + 25.0, "{a} → {b}");
    // Far from the cube the floor still reflects the sky.
    let side = Vec3::new(2.5, 0.0, -1.5);
    let (s0, s1) = (rgb(&off, side), rgb(&on, side));
    assert!((s0 - s1).abs().max_element() < 12.0, "{s0} vs {s1}");
}

#[test]
fn temporal_accumulation_settles() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&gpu.0);
    let target = renderer.create_target(&gpu.0, W, H);
    let (meshes, lights) = (room(), [sun()]);
    let view = View {
        meshes: &meshes,
        lights: &lights,
    };
    let mut post = ssgi(true);
    post.ssgi.denoise = false;
    let frames: Vec<Vec<u8>> = (0..12)
        .map(|_| render(&mut renderer, &gpu, &target, &view, post))
        .collect();
    let diff = |a: &[u8], b: &[u8]| {
        a.iter()
            .zip(b)
            .map(|(&x, &y)| (x as f32 - y as f32).abs())
            .sum::<f32>()
            / a.len() as f32
    };
    // Frame 1 is the first traced one (frame 0 has no lit history yet).
    let early = diff(&frames[1], &frames[2]);
    let late = diff(&frames[10], &frames[11]);
    eprintln!("frame-to-frame change: early {early}, late {late}");
    assert!(late < early * 0.6, "{late} vs {early}");
}

/// A light's emitter mesh with Contribute GI off sends nothing through SSGI (its light is
/// already the light's direct lighting): the floor under it isn't lit twice. With
/// Contribute GI on the same glow does bounce onto the floor.
#[test]
fn emitters_without_contribute_gi_send_no_bounce() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut meshes = room();
    meshes[0].material.color = [0.4, 0.4, 0.4, 1.0];
    meshes[1].material.color = [0.4, 0.4, 0.4, 1.0];
    // A bright emissive panel standing on the floor, facing the camera's side of the room.
    let mut panel = boxed(
        Vec3::new(0.0, 0.6, 1.6),
        Vec3::new(1.6, 1.2, 0.05),
        [1.0; 3],
        0.0,
    );
    panel.material.emission = [1.0; 3];
    panel.material.emission_intensity = 4.0;
    let lights = [LightData {
        intensity: 0.3,
        ..sun()
    }];
    let floor = Vec3::new(0.0, 0.0, 0.9);
    let brightness = |contribute_gi: bool, post: PostSettings| {
        let mut m = meshes.clone();
        let mut p = panel.clone();
        p.material.contribute_gi = contribute_gi;
        m.push(p);
        let view = View {
            meshes: &m,
            lights: &lights,
        };
        let mut renderer = Renderer::new(&gpu.0);
        renderer.set_quality(&gpu.0, &crate::quality::QualityLevel::high());
        let target = renderer.create_target(&gpu.0, W, H);
        let image = settle(&mut renderer, &gpu, &target, &view, post, 16);
        let gi = if post.ssgi.active() { "on" } else { "off" };
        save(&format!("ssgi_emitter_gi_{contribute_gi}_{gi}"), &image);
        rgb(&image, floor).element_sum() / 3.0
    };
    // No bloom: only the light that reaches the floor counts.
    let no_bloom = |p: PostSettings| PostSettings {
        bloom_intensity: 0.0,
        ..p
    };
    let off = brightness(false, no_bloom(Default::default()));
    let excluded = brightness(false, no_bloom(ssgi(true)));
    let included = brightness(true, no_bloom(ssgi(true)));
    eprintln!("floor before the panel: off {off}, excluded {excluded}, included {included}");
    assert!(
        included > off + 15.0,
        "the glow bounces: {off} → {included}"
    );
    assert!(
        excluded < off + 6.0,
        "an emitter without GI adds nothing: {off} → {excluded}"
    );
}

#[test]
fn nothing_runs_while_both_are_off() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (meshes, lights) = (room(), [sun()]);
    let view = View {
        meshes: &meshes,
        lights: &lights,
    };
    let mut fresh = Renderer::new(&gpu.0);
    let target = fresh.create_target(&gpu.0, W, H);
    let baseline = render(&mut fresh, &gpu, &target, &view, Default::default());

    let mut r = Renderer::new(&gpu.0);
    let target = r.create_target(&gpu.0, W, H);
    let both = PostSettings {
        ssr: ssr().ssr,
        ..ssgi(true)
    };
    settle(&mut r, &gpu, &target, &view, both, 2);
    let captured = |r: &mut Renderer, post| {
        r.begin_capture(CaptureRequest::default());
        let image = render(r, &gpu, &target, &view, post);
        (image, r.end_capture().expect("captured").roots())
    };
    let (_, on) = captured(&mut r, both);
    for name in [
        "Screen Space Global Illumination",
        "Screen Space Reflections",
        "Screen Space Lighting History",
    ] {
        assert!(on.iter().any(|n| n == name), "{name} missing: {on:?}");
    }
    let (off_image, off) = captured(&mut r, Default::default());
    assert!(
        !off.iter().any(|n| n.starts_with("Screen Space")),
        "passes while off: {off:?}"
    );
    assert!(off_image == baseline, "off renders as if never on");
}

/// GPU frame time of the room at 1920×1080 with each effect (`--ignored --nocapture`).
#[test]
#[ignore = "benchmark; run on purpose"]
fn cost_at_1080p() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (device, queue) = (&gpu.0, &gpu.1);
    let (meshes, lights) = (room(), [sun()]);
    let view = View {
        meshes: &meshes,
        lights: &lights,
    };
    let half = |mut p: PostSettings| {
        p.ssgi.full_resolution = false;
        p
    };
    let both = PostSettings {
        ssr: ssr().ssr,
        ..ssgi(true)
    };
    let configs = [
        ("off", PostSettings::default()),
        ("SSGI half res", half(ssgi(true))),
        ("SSR full res", ssr()),
        ("SSGI + SSR", both),
    ];
    for (name, post) in configs {
        let mut r = Renderer::with_queue(device, queue);
        let target = r.create_target(device, 1920, 1080);
        let mut total = 0.0;
        let frames = 60;
        for i in 0..frames + 10 {
            render(&mut r, &gpu, &target, &view, post);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 10 {
                total += r.gpu_ms().unwrap_or(0.0);
            }
        }
        println!("{name}: GPU {:.3} ms", total / frames as f32);
    }
}
