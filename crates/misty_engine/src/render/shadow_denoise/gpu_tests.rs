//! Headless GPU checks of the soft shadows: penumbrae that widen with the blocker's height
//! and the light's size, rectangle lights (anisotropic penumbrae, the hemicube's wide
//! coverage), the temporal mask's smoothness and stability, and Low quality staying on the
//! old path. `MISTY_GPU_TEST_DIR=<dir>` saves the images.

use glam::{Mat4, Quat, Vec2, Vec3};

use super::super::gpu_tests::{headless, read_back};
use super::super::*;
use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::scene::PrimitiveMesh;

const SIZE: u32 = 512;

fn draw(mesh: PrimitiveMesh, model: Mat4) -> MeshDraw {
    MeshDraw {
        mesh: mesh.into(),
        model,
        material: MaterialParams {
            smoothness: 0.0,
            ..Default::default()
        },
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// A white-ish floor and a thin plank: `min`..`max` in x / z at `height`.
fn plank_scene(min: Vec2, max: Vec2, height: f32) -> Vec<MeshDraw> {
    let centre = (min + max) * 0.5;
    let size = max - min;
    vec![
        draw(
            PrimitiveMesh::Plane,
            Mat4::from_scale(Vec3::new(3.0, 1.0, 3.0)),
        ),
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(
                Vec3::new(size.x, 0.04, size.y),
                Quat::IDENTITY,
                Vec3::new(centre.x, height, centre.y),
            ),
        ),
    ]
}

fn light(kind: LightType, position: Vec3, intensity: f32) -> LightData {
    LightData {
        kind,
        position,
        // Straight down: local +Z = -Y, local X = X, local Y = +Z.
        direction: Vec3::NEG_Y,
        color: [1.0; 3],
        intensity,
        range: 25.0,
        spot_angle: 150.0,
        shadows: Shadows::Soft,
        contact_shadows: false,
        area: AreaShape {
            size: Vec2::ONE,
            right: Vec3::X,
            up: Vec3::Z,
            two_sided: false,
            show_emitter: false,
        },
        shadow: LightShadow::default(),
        volumetric: Default::default(),
    }
}

/// A big spot light `height` m above the origin with a source of `radius` m.
fn spot(height: f32, radius: f32) -> LightData {
    LightData {
        shadow: LightShadow {
            size: radius,
            ..Default::default()
        },
        ..light(LightType::Spot, Vec3::new(0.0, height, 0.0), 6.0)
    }
}

/// A downward rectangle light `w` × `d` m (along x / z) 3 m above the origin.
fn rectangle(w: f32, d: f32) -> LightData {
    let mut l = light(LightType::Rectangle, Vec3::new(0.0, 3.0, 0.0), 6.0);
    l.area.size = Vec2::new(w, d);
    l
}

/// Top-down orthographic camera over `centre` (x, z), `half` m to each side.
fn top_down(centre: Vec2, half: f32) -> Camera {
    Camera {
        position: Vec3::new(centre.x, 20.0, centre.y),
        // Image right = +X, image up = +Z.
        rotation: Quat::from_rotation_x(90f32.to_radians()),
        projection: Projection::Orthographic { size: half },
        near: 0.3,
        far: 60.0,
    }
}

/// No ambient light: shadowed = black, so visibility ratios are clean.
fn dark() -> Environment {
    let mut e = Environment::default();
    e.settings.environment_lighting.intensity = 0.0;
    e.settings.reflection_intensity = 0.0;
    e
}

struct View {
    camera: Camera,
    meshes: Vec<MeshDraw>,
    lights: Vec<LightData>,
}

impl View {
    fn frame<'a>(&'a self, environment: &'a Environment) -> Frame<'a> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &self.meshes,
            particles: &[],
            terrains: &[],
            water: &[],
            shaded: true,
            lights: &self.lights,
        }
    }
}

fn save(name: &str, pixels: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(format!("{name}.png"));
        image::RgbaImage::from_raw(SIZE, SIZE, pixels.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

struct Gpu {
    device: wgpu::Device,
    queue: wgpu::Queue,
}

impl Gpu {
    fn new() -> Option<Self> {
        let (device, queue) = headless()?;
        Some(Self { device, queue })
    }

    fn renderer(&self, quality: &QualityLevel) -> Renderer {
        let mut r = Renderer::with_queue(&self.device, &self.queue);
        r.set_quality(&self.device, quality);
        r
    }

    /// `frames` renders of `view` (the temporal mask settles); the last image.
    fn render(
        &self,
        r: &mut Renderer,
        target: &RenderTarget,
        view: &View,
        frames: usize,
    ) -> Vec<u8> {
        let environment = dark();
        for _ in 0..frames {
            r.render(&self.device, &self.queue, target, &view.frame(&environment));
        }
        read_back(&self.device, &self.queue, target)
    }

    /// Visibility: the image over the same view with shadows off (1 = lit), per pixel.
    fn visibility(
        &self,
        quality: &QualityLevel,
        view: &View,
        frames: usize,
        name: &str,
    ) -> Vec<f32> {
        let mut r = self.renderer(quality);
        let target = r.create_target(&self.device, SIZE, SIZE);
        let shadowed = self.render(&mut r, &target, view, frames);
        save(name, &shadowed);
        let unshadowed = View {
            camera: view.camera,
            meshes: view.meshes.clone(),
            lights: (view.lights.iter())
                .map(|l| LightData {
                    shadows: Shadows::None,
                    ..*l
                })
                .collect(),
        };
        let mut r = self.renderer(quality);
        let target = r.create_target(&self.device, SIZE, SIZE);
        let lit = self.render(&mut r, &target, &unshadowed, 1);
        shadowed
            .chunks(4)
            .zip(lit.chunks(4))
            .map(|(s, l)| {
                let s: f32 = s[..3].iter().map(|v| *v as f32).sum();
                let l: f32 = l[..3].iter().map(|v| *v as f32).sum();
                s / l.max(1.0)
            })
            .collect()
    }
}

/// Pixel of world (x, z) in a top-down view of `centre` ± `half`.
fn pixel(centre: Vec2, half: f32, x: f32, z: f32) -> (usize, usize) {
    let px = ((x - centre.x + half) / (2.0 * half) * SIZE as f32) as usize;
    let py = ((centre.y + half - z) / (2.0 * half) * SIZE as f32) as usize;
    (px.min(SIZE as usize - 1), py.min(SIZE as usize - 1))
}

fn at(v: &[f32], (x, y): (usize, usize)) -> f32 {
    v[y * SIZE as usize + x]
}

/// Pixels between 10% and 90% of the way from the darkest to the brightest value.
fn edge_width(profile: &[f32]) -> usize {
    let lit = profile.iter().cloned().fold(f32::MIN, f32::max);
    let dark = profile.iter().cloned().fold(f32::MAX, f32::min);
    assert!(lit - dark > 0.2, "no shadow edge here: {dark}..{lit}");
    let (lo, hi) = (dark + 0.1 * (lit - dark), dark + 0.9 * (lit - dark));
    profile.iter().filter(|v| **v > lo && **v < hi).count()
}

/// Visibility along +X from x0 to x1 at depth z.
fn row(v: &[f32], centre: Vec2, half: f32, z: f32, x0: f32, x1: f32) -> Vec<f32> {
    let (a, y) = pixel(centre, half, x0, z);
    let (b, _) = pixel(centre, half, x1, z);
    (a..b).map(|x| at(v, (x, y))).collect()
}

/// Visibility along +Z from z0 to z1 at x.
fn column(v: &[f32], centre: Vec2, half: f32, x: f32, z0: f32, z1: f32) -> Vec<f32> {
    let (px, a) = pixel(centre, half, x, z0);
    let (_, b) = pixel(centre, half, x, z1);
    (b..a).rev().map(|y| at(v, (px, y))).collect()
}

const CENTRE: Vec2 = Vec2::new(4.0, 0.0);
const HALF: f32 = 4.0;

/// The far-edge penumbra (pixels) of a plank over x = 1..3 at `height` under the spot light.
fn spot_penumbra(gpu: &Gpu, height: f32, radius: f32) -> usize {
    let view = View {
        camera: top_down(CENTRE, HALF),
        meshes: plank_scene(Vec2::new(1.0, -0.6), Vec2::new(3.0, 0.6), height),
        lights: vec![spot(4.0, radius)],
    };
    let name = format!("shadows_spot_h{height}_r{radius}");
    let v = gpu.visibility(&QualityLevel::high(), &view, 24, &name);
    // The plank's far edge (x = 3) lands at 3 · 4 / (4 - h).
    edge_width(&row(&v, CENTRE, HALF, 0.0, 3.1, 8.0))
}

#[test]
fn penumbra_grows_with_blocker_distance_and_light_size() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Penumbra = 2 r h / (H - h): 2r/3 at h = 1, 2r at h = 2.
    let low = spot_penumbra(&gpu, 1.0, 0.3);
    let high = spot_penumbra(&gpu, 2.0, 0.3);
    eprintln!("spot r = 0.3: h = 1 → {low} px, h = 2 → {high} px (64 px per m)");
    assert!(
        high as f32 >= 2.0 * low as f32,
        "higher blockers soften more: {low} → {high}"
    );
    // Expected 0.4 m and 1.2 m wide penumbrae → about 13 and 38 px (10–90% ≈ 80% of them).
    assert!((6..=24).contains(&low), "{low}");
    assert!((20..=60).contains(&high), "{high}");
    let small = spot_penumbra(&gpu, 1.5, 0.15);
    let big = spot_penumbra(&gpu, 1.5, 0.45);
    eprintln!("spot h = 1.5: r = 0.15 → {small} px, r = 0.45 → {big} px");
    assert!(
        big as f32 >= 2.0 * small as f32,
        "bigger lights soften more: {small} → {big}"
    );
}

#[test]
fn rectangle_lights_cast_anisotropic_shadows_over_a_hemisphere() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // A 1.6 × 0.4 m light 3 m up; a 1 m plank at 1 m over x, z = 1..2: shadow over 1.5..3.
    let centre = Vec2::new(3.0, 3.0);
    let half = 3.0;
    let view = View {
        camera: top_down(centre, half),
        meshes: plank_scene(Vec2::new(1.0, 1.0), Vec2::new(2.0, 2.0), 1.0),
        lights: vec![rectangle(1.6, 0.4)],
    };
    let v = gpu.visibility(&QualityLevel::high(), &view, 24, "shadows_rect");
    let umbra = at(&v, pixel(centre, half, 2.25, 2.25));
    assert!(umbra < 0.5, "rectangle lights cast shadows: {umbra}");
    // Across the long side the penumbra is about 4× wider than across the short one.
    let along_x = edge_width(&row(&v, centre, half, 2.25, 2.05, 5.5));
    let along_z = edge_width(&column(&v, centre, half, 2.25, 2.05, 5.5));
    eprintln!("rectangle penumbra: {along_x} px along its width, {along_z} px along its depth");
    assert!(
        along_x as f32 >= 2.0 * along_z as f32,
        "{along_x} vs {along_z}"
    );

    // 68° off the light's axis (outside the old single 120° map): still shadowed.
    let centre = Vec2::new(6.0, 0.0);
    let view = View {
        camera: top_down(centre, half),
        meshes: plank_scene(Vec2::new(4.5, -0.5), Vec2::new(5.5, 0.5), 1.0),
        lights: vec![rectangle(1.0, 1.0)],
    };
    let v = gpu.visibility(&QualityLevel::high(), &view, 8, "shadows_rect_wide");
    let far = at(&v, pixel(centre, half, 7.5, 0.0));
    assert!(far < 0.6, "the hemicube shadows wide angles: {far}");
}

/// Noise left in `v` over a box: RMS of each value minus the mean of its four neighbours two
/// pixels away (a smooth gradient leaves nothing; grain and banding do).
fn roughness(v: &[f32], (x0, y0): (usize, usize), (x1, y1): (usize, usize)) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let c = at(v, (x, y));
            let around =
                (at(v, (x - 2, y)) + at(v, (x + 2, y)) + at(v, (x, y - 2)) + at(v, (x, y + 2)))
                    / 4.0;
            sum += (c - around) * (c - around);
            n += 1.0;
        }
    }
    (sum / n).sqrt()
}

/// High quality without the temporal mask (same taps, one frame).
fn high_direct() -> QualityLevel {
    QualityLevel {
        shadow_temporal: false,
        shadow_denoise: false,
        ..QualityLevel::high()
    }
}

#[test]
fn temporal_mask_smooths_wide_penumbrae_and_stays_stable() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = View {
        camera: top_down(CENTRE, HALF),
        meshes: plank_scene(Vec2::new(1.0, -0.6), Vec2::new(3.0, 0.6), 2.0),
        lights: vec![spot(4.0, 0.4)],
    };
    // The penumbra around x = 6 (1.6 m wide here), away from the plank's corners.
    let a = pixel(CENTRE, HALF, 5.6, 0.25);
    let b = pixel(CENTRE, HALF, 6.4, -0.25);
    let (lo, hi) = ((a.0, a.1), (b.0, b.1));
    let direct = gpu.visibility(&high_direct(), &view, 1, "shadows_direct");
    let smooth = gpu.visibility(&QualityLevel::high(), &view, 24, "shadows_temporal");
    let (rd, rs) = (roughness(&direct, lo, hi), roughness(&smooth, lo, hi));
    eprintln!("penumbra roughness: direct {rd:.4}, temporal {rs:.4}");
    assert!(rs < 0.5 * rd, "accumulation removes the grain: {rd} → {rs}");
    // About one 8-bit step of the summed RGB / lit ratio.
    assert!(rs < 0.012, "smooth gradient: {rs}");

    // Converged, then the camera moves one pixel: the image moves with it and barely changes.
    let mut r = gpu.renderer(&QualityLevel::high());
    let target = r.create_target(&gpu.device, SIZE, SIZE);
    let before = gpu.render(&mut r, &target, &view, 24);
    let step = 2.0 * HALF / SIZE as f32;
    let moved = View {
        camera: Camera {
            position: view.camera.position + Vec3::new(step, 0.0, 0.0),
            ..view.camera
        },
        meshes: view.meshes.clone(),
        lights: view.lights.clone(),
    };
    let after = gpu.render(&mut r, &target, &moved, 1);
    save("shadows_jitter", &after);
    let w = SIZE as usize;
    let (mut total, mut worst, mut n) = (0.0f32, 0.0f32, 0.0f32);
    for y in 0..w {
        for x in 0..w - 1 {
            // Pixel x after the move shows what pixel x + 1 showed before.
            let i = (y * w + x) * 4;
            let j = (y * w + x + 1) * 4;
            for c in 0..3 {
                let d = (after[i + c] as f32 - before[j + c] as f32).abs();
                total += d;
                worst = worst.max(d);
                n += 1.0;
            }
        }
    }
    let mean = total / n;
    eprintln!("1-px camera move: mean difference {mean:.3}, worst {worst}");
    assert!(mean < 0.6, "temporally stable: mean {mean}");
    assert!(worst <= 24.0, "temporally stable: worst {worst}");
}

#[test]
fn low_quality_keeps_the_old_path() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = View {
        camera: top_down(CENTRE, HALF),
        meshes: plank_scene(Vec2::new(1.0, -0.6), Vec2::new(3.0, 0.6), 1.0),
        lights: vec![spot(4.0, 0.3), rectangle(1.0, 1.0)],
    };
    let environment = dark();
    for (quality, mask) in [
        (QualityLevel::low(), false),
        (QualityLevel::medium(), false),
        (QualityLevel::high(), true),
    ] {
        let mut r = gpu.renderer(&quality);
        let target = r.create_target(&gpu.device, SIZE, SIZE);
        let frame = view.frame(&environment);
        r.render(&gpu.device, &gpu.queue, &target, &frame);
        assert_eq!(r.shadow_denoise.active(), mask, "{}", quality.name);
        // No screen-space prepass either (AO and contact shadows are off here).
        assert_eq!(r.screen_space_wanted(&frame), mask, "{}", quality.name);
        // Rectangles: one wide map without soft shadows, a hemicube with them.
        let soft = quality.soft_shadows != crate::quality::Tier::Off;
        let plan = super::super::shadows::plan_with(
            &frame,
            &[(Vec3::splat(-1.0), Vec3::ONE)],
            1.0,
            &quality.shadow_settings(),
        );
        assert_eq!(plan.local.len(), 1 + if soft { 5 } else { 1 });
    }
    // Low draws soft lights with one hard compare (soft shadows off): same taps as before.
    assert_eq!(
        QualityLevel::low().shadow_settings().soft,
        crate::quality::Tier::Off
    );
}

/// SoftShadowsDemo through its game camera: Medium (direct taps) and High (temporal mask)
/// after 24 frames (`MISTY_GPU_TEST_DIR=<dir>` saves both).
#[test]
fn soft_shadows_demo_renders_smooth_shadows() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) = crate::scene::Scene::load(&root.join("Assets/Scenes/SoftShadowsDemo.scene"))
        .expect("SoftShadowsDemo.scene (generate it with write_soft_shadows_demo)");
    let materials = crate::assets::AssetCache::open(&root);
    let view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
    let (width, height) = (960, 540);
    let mut images = Vec::new();
    for (name, quality, frames) in [
        ("medium", QualityLevel::medium(), 1),
        ("high", QualityLevel::high(), 24),
    ] {
        let mut r = gpu.renderer(&quality);
        let target = r.create_target(&gpu.device, width, height);
        for _ in 0..frames {
            r.render(&gpu.device, &gpu.queue, &target, &view.frame());
        }
        assert_eq!(r.shadow_denoise.active(), name == "high");
        let pixels = read_back(&gpu.device, &gpu.queue, &target);
        if let Some(dir) = std::env::var_os("MISTY_GPU_TEST_DIR") {
            let path = std::path::Path::new(&dir).join(format!("shadows_demo_{name}.png"));
            image::RgbaImage::from_raw(width, height, pixels.clone())
                .expect("size")
                .save(path)
                .expect("png");
        }
        images.push(pixels);
    }
    // Same lighting, so the two images agree on average; the temporal one is smoother.
    let mean = |p: &[u8]| p.iter().map(|v| *v as f64).sum::<f64>() / p.len() as f64;
    let (a, b) = (mean(&images[0]), mean(&images[1]));
    assert!((a - b).abs() < 6.0, "{a} vs {b}");
    assert!(b > 20.0, "lit: {b}");
}

/// GPU time of SoftShadowsDemo at 1920×1080 per quality setup (run on purpose:
/// `cargo test -p misty_engine -- --ignored shadow_cost --nocapture`).
#[test]
#[ignore = "timing, prints only"]
fn shadow_cost() {
    let Some(gpu) = Gpu::new() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) =
        crate::scene::Scene::load(&root.join("Assets/Scenes/SoftShadowsDemo.scene")).unwrap();
    let materials = crate::assets::AssetCache::open(&root);
    let view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
    let setups = [
        ("low (hard)", QualityLevel::low()),
        ("medium (direct 8/12)", QualityLevel::medium()),
        ("high without mask (12/20)", high_direct()),
        ("high (temporal + denoise)", QualityLevel::high()),
        (
            "ultra without mask (16/32)",
            QualityLevel {
                shadow_temporal: false,
                shadow_denoise: false,
                ..QualityLevel::ultra()
            },
        ),
        ("ultra (temporal + denoise)", QualityLevel::ultra()),
    ];
    for (name, quality) in setups {
        let mut r = gpu.renderer(&quality);
        let target = r.create_target(&gpu.device, 1920, 1080);
        let mut times = Vec::new();
        for _ in 0..40 {
            r.render(&gpu.device, &gpu.queue, &target, &view.frame());
            let _ = gpu.device.poll(wgpu::PollType::wait_indefinitely());
            if let Some(ms) = r.gpu_ms() {
                times.push(ms);
            }
        }
        times.sort_by(f32::total_cmp);
        let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
        eprintln!("{name}: {median:.2} ms GPU (median of {})", times.len());
    }
}
