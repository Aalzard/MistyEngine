//! Headless GPU checks of temporal anti-aliasing and the motion vectors (skipped without an
//! adapter): TAA off draws with the plain camera matrix, a still scene stays sharp, a turning
//! camera keeps the image in place, a moving cube leaves no trail, and grass in the wind
//! does not smear. `MISTY_TAA_TEST_DIR=dir` saves the images.

use glam::{Mat4, Quat, Vec3};

use super::gpu_tests::{draw, headless, material, read_back, test_lights, test_meshes};
use super::{Environment, Frame, LightData, MeshDraw, PostSettings, Renderer, RenderingMode};
use crate::camera::{Camera, Projection};
use crate::quality::QualityLevel;
use crate::scene::PrimitiveMesh;

const W: u32 = 320;
const H: u32 = 200;

fn save(name: &str, px: &[u8], w: u32, h: u32) {
    if let Some(dir) = std::env::var_os("MISTY_TAA_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(name);
        image::RgbaImage::from_raw(w, h, px.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

fn camera(yaw: f32) -> Camera {
    Camera {
        position: Vec3::new(0.0, 7.0, -12.0),
        rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(28f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 60.0,
    }
}

fn quality(taa: bool) -> QualityLevel {
    QualityLevel {
        temporal_aa: taa,
        ..QualityLevel::high()
    }
}

/// Yaw that moves the image centre by one pixel.
fn pixel_yaw(fov_y: f32, height: u32) -> f32 {
    (2.0 * (fov_y.to_radians() * 0.5).tan() / height as f32).atan()
}

struct Scene {
    meshes: Vec<MeshDraw>,
    lights: Vec<LightData>,
    environment: Environment,
}

impl Scene {
    fn test() -> Self {
        Self {
            meshes: test_meshes(),
            lights: test_lights(),
            environment: Environment::default(),
        }
    }

    fn frame<'a>(&'a self, camera: &'a Camera) -> Frame<'a> {
        Frame {
            camera,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &self.environment,
            post: PostSettings::default(),
            lines: &[],
            overlay: &[],
            meshes: &self.meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &self.lights,
            water: &[],
        }
    }
}

/// Render `frames` frames of `scene` with camera `cam(i)` (and `edit(i, meshes)` applied
/// first) and return the last `keep` images.
#[allow(clippy::too_many_arguments)]
fn sequence(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    quality: &QualityLevel,
    scene: &mut Scene,
    frames: usize,
    keep: usize,
    cam: impl Fn(usize) -> Camera,
    edit: impl Fn(usize, &mut Vec<MeshDraw>),
) -> Vec<Vec<u8>> {
    let mut renderer = Renderer::with_queue(device, queue);
    renderer.set_quality(device, quality);
    let target = renderer.create_target(device, W, H);
    let mut out = Vec::new();
    for i in 0..frames {
        edit(i, &mut scene.meshes);
        let c = cam(i);
        renderer.render(device, queue, &target, &scene.frame(&c));
        if i + keep >= frames {
            out.push(read_back(device, queue, &target));
        }
    }
    out
}

fn luma(px: &[u8], i: usize) -> f32 {
    0.2126 * px[i * 4] as f32 + 0.7152 * px[i * 4 + 1] as f32 + 0.0722 * px[i * 4 + 2] as f32
}

/// Mean gradient magnitude of the luma (edge contrast).
fn edge_contrast(px: &[u8], w: u32, h: u32) -> f32 {
    let mut sum = 0.0;
    let mut n = 0.0;
    for y in 0..h as usize - 1 {
        for x in 0..w as usize - 1 {
            let i = y * w as usize + x;
            let c = luma(px, i);
            sum += (luma(px, i + 1) - c).abs() + (luma(px, i + w as usize) - c).abs();
            n += 1.0;
        }
    }
    sum / n
}

/// Mean absolute difference (r, g, b) between `a` and `b` shifted right by `dx` pixels over
/// the middle half of the image.
fn shifted_difference(a: &[u8], b: &[u8], w: u32, h: u32, dx: i32) -> f32 {
    let mut sum = 0.0f32;
    let mut n = 0.0f32;
    for y in h / 4..h * 3 / 4 {
        for x in w / 4..w * 3 / 4 {
            let xb = x as i32 + dx;
            if xb < 0 || xb >= w as i32 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            let j = ((y * w + xb as u32) * 4) as usize;
            for c in 0..3 {
                sum += (a[i + c] as f32 - b[j + c] as f32).abs();
            }
            n += 3.0;
        }
    }
    sum / n.max(1.0)
}

/// Mean absolute difference (r, g, b) inside a pixel rectangle.
fn region_difference(a: &[u8], b: &[u8], w: u32, (x0, y0, x1, y1): (u32, u32, u32, u32)) -> f32 {
    let mut sum = 0.0f32;
    let mut n = 0.0f32;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * w + x) * 4) as usize;
            for c in 0..3 {
                sum += (a[i + c] as f32 - b[i + c] as f32).abs();
            }
            n += 3.0;
        }
    }
    sum / n.max(1.0)
}

/// Share of pixels that change by more than 8 levels in a channel between two frames.
fn flicker(a: &[u8], b: &[u8]) -> f32 {
    let jumps = a
        .chunks(4)
        .zip(b.chunks(4))
        .filter(|(p, q)| (0..3).any(|c| (p[c] as i32 - q[c] as i32).abs() > 8))
        .count();
    jumps as f32 / (a.len() / 4) as f32
}

/// With TAA off the view uniform holds the camera's own matrix, bit for bit; with TAA on the
/// jitter moves it after the first frame.
#[test]
fn taa_off_draws_with_the_unjittered_camera() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let scene = Scene::test();
    let cam = camera(0.0);
    let expected = cam.view_projection(W as f32 / H as f32);
    for taa in [false, true] {
        let mut renderer = Renderer::with_queue(&device, &queue);
        renderer.set_quality(&device, &quality(taa));
        let target = renderer.create_target(&device, W, H);
        let mut jittered = 0;
        for _ in 0..4 {
            renderer.render(&device, &queue, &target, &scene.frame(&cam));
            let drawn = renderer.last_view_proj;
            if drawn.to_cols_array() != expected.to_cols_array() {
                jittered += 1;
            }
        }
        if taa {
            assert!(jittered >= 2, "TAA on: {jittered} jittered frames of 4");
        } else {
            assert_eq!(jittered, 0, "TAA off must draw the plain camera matrix");
        }
    }
}

/// A still scene converges without going soft: the edge contrast after 16 frames of TAA is
/// at least 90 % of the single MSAA frame's.
#[test]
fn taa_keeps_a_still_scene_sharp() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut scene = Scene::test();
    let off = sequence(
        &device,
        &queue,
        &quality(false),
        &mut scene,
        1,
        1,
        |_| camera(0.0),
        |_, _| {},
    );
    let on = sequence(
        &device,
        &queue,
        &quality(true),
        &mut scene,
        16,
        1,
        |_| camera(0.0),
        |_, _| {},
    );
    save("static_off.png", &off[0], W, H);
    save("static_on.png", &on[0], W, H);
    let (a, b) = (edge_contrast(&off[0], W, H), edge_contrast(&on[0], W, H));
    eprintln!(
        "edge contrast: TAA off {a:.3}, TAA on {b:.3} ({:.1} %)",
        b / a * 100.0
    );
    assert!(b >= 0.9 * a, "TAA softened the image: {b} vs {a}");
}

/// Turning one pixel a frame: frame N+1 is frame N shifted by a pixel, with TAA on about as
/// closely as with it off (no lag, no shimmer).
#[test]
fn taa_follows_a_turning_camera() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = pixel_yaw(50.0, H);
    let mut scene = Scene::test();
    let turning = |i: usize| camera(step * i.saturating_sub(16) as f32);
    let off = sequence(
        &device,
        &queue,
        &quality(false),
        &mut scene,
        24,
        2,
        turning,
        |_, _| {},
    );
    let on = sequence(
        &device,
        &queue,
        &quality(true),
        &mut scene,
        24,
        2,
        turning,
        |_, _| {},
    );
    let still = sequence(
        &device,
        &queue,
        &quality(true),
        &mut scene,
        24,
        2,
        |_| camera(0.0),
        |_, _| {},
    );
    save("turning_off.png", &off[1], W, H);
    save("turning_on.png", &on[1], W, H);
    let d_off = shifted_difference(&off[0], &off[1], W, H, 1)
        .min(shifted_difference(&off[0], &off[1], W, H, -1));
    let d_on = shifted_difference(&on[0], &on[1], W, H, 1)
        .min(shifted_difference(&on[0], &on[1], W, H, -1));
    let d_still = shifted_difference(&still[0], &still[1], W, H, 0);
    let unshifted = shifted_difference(&on[0], &on[1], W, H, 0);
    eprintln!(
        "turning 1 px/frame, shifted difference: TAA off {d_off:.2}, TAA on {d_on:.2} (unshifted {unshifted:.2}); still TAA {d_still:.2}"
    );
    assert!(
        d_on < d_off + 1.5,
        "TAA shimmers or lags while turning: {d_on} vs {d_off}"
    );
    assert!(d_on < unshifted, "the image did not follow the camera");
}

/// A cube crossing the view at 10 px a frame leaves the background behind it clean within
/// two frames (motion vectors, no history trail).
#[test]
fn moving_cube_leaves_no_ghost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let cam = camera(0.0);
    let aspect = W as f32 / H as f32;
    let red = material([0.9, 0.2, 0.2, 1.0], RenderingMode::Opaque);
    // Ground only, and a cube sliding along x at a fixed depth in front of the camera.
    let ground = draw(
        PrimitiveMesh::Plane,
        Mat4::from_scale(Vec3::new(4.0, 1.0, 4.0)),
        &material([0.5, 0.5, 0.5, 1.0], RenderingMode::Opaque),
    );
    let depth_z = -2.0f32;
    let y = 2.0f32;
    // World x that moves the cube's centre by 10 pixels a frame.
    let px_x = |x: f32| {
        let ndc = cam
            .view_projection(aspect)
            .project_point3(Vec3::new(x, y, depth_z));
        (ndc.x * 0.5 + 0.5) * W as f32
    };
    let per_unit = px_x(1.0) - px_x(0.0);
    let dx = 10.0 / per_unit;
    let cube_at = |i: usize| {
        let x = -3.0 + dx * i as f32;
        draw(
            PrimitiveMesh::Cube,
            Mat4::from_scale_rotation_translation(
                Vec3::splat(0.8),
                Quat::IDENTITY,
                Vec3::new(x, y, depth_z),
            ),
            &red,
        )
    };
    let mut scene = Scene::test();
    scene.meshes = vec![ground.clone(), cube_at(0)];
    let frames = 20;
    let edit = |i: usize, meshes: &mut Vec<MeshDraw>| {
        meshes.truncate(1);
        meshes.push(cube_at(i));
    };
    let on = sequence(
        &device,
        &queue,
        &quality(true),
        &mut scene,
        frames,
        1,
        |_| cam,
        edit,
    );
    scene.meshes = vec![ground.clone(), cube_at(frames - 1)];
    let off = sequence(
        &device,
        &queue,
        &quality(false),
        &mut scene,
        1,
        1,
        |_| cam,
        |_, _| {},
    );
    save("ghost_on.png", &on[0], W, H);
    save("ghost_off.png", &off[0], W, H);
    // Where the cube was two frames ago (its trailing half) the ground must match the
    // single frame without any history.
    let now = px_x(-3.0 + dx * (frames - 1) as f32);
    let before = px_x(-3.0 + dx * (frames - 3) as f32);
    let ndc = cam
        .view_projection(aspect)
        .project_point3(Vec3::new(0.0, y, depth_z));
    let cy = ((0.5 - ndc.y * 0.5) * H as f32) as u32;
    let x0 = (before - 10.0).max(0.0) as u32;
    let x1 = (before + 4.0).min(now - 14.0).max(x0 as f32 + 1.0) as u32;
    let region = (x0, cy.saturating_sub(10), x1, (cy + 10).min(H));
    let trail = region_difference(&on[0], &off[0], W, region);
    let whole = region_difference(&on[0], &off[0], W, (0, 0, W, H));
    eprintln!("behind the cube: {trail:.2} levels off the clean frame (whole image {whole:.2})");
    assert!(trail < 4.0, "a ghost trail follows the cube: {trail}");
}

/// MeadowDemo as saved, or crouching in the grass (`close`).
fn meadow(close: bool) -> crate::render_scene::GameView {
    use crate::scene::{Scene as SceneAsset, Transform};
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    crate::assets::set_project_root(&root);
    let (mut s, _) =
        SceneAsset::load(&root.join("Assets/Scenes/MeadowDemo.scene")).expect("MeadowDemo.scene");
    if close && let Some(cam) = s.find_by_name("Main Camera") {
        // Crouching, looking down into the grass (positive pitch looks down).
        let p = s.world_position(cam);
        s.set_transform(
            cam,
            Transform::from_position(p - Vec3::Y * 0.8).with_euler(Vec3::new(16.0, 8.0, 0.0)),
        );
    }
    let materials = crate::assets::AssetCache::open(&root);
    crate::render_scene::GameView::new(&s, &materials).expect("camera")
}

/// Grass in the wind (MeadowDemo, wind strength 0.12) with TAA on: the share of pixels
/// jumping by more than 8 levels a frame drops well below the TAA-off share (the shimmer of
/// thin blades settles), and the moving blades stay about as crisp as the same view with the
/// wind stopped (the history follows them through the wind's motion vectors instead of
/// smearing them).
#[test]
fn grass_in_the_wind_does_not_smear() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut view = meadow(true);
    let (w, h) = (640u32, 360u32);
    // (TAA, wind blowing): share of jumping pixels and edge contrast of the last frame.
    let mut runs = Vec::new();
    for (taa, blowing) in [(false, true), (true, true), (true, false)] {
        let mut renderer = Renderer::with_queue(&device, &queue);
        renderer.set_quality(&device, &quality(taa));
        let target = renderer.create_target(&device, w, h);
        let mut frames = Vec::new();
        for i in 0..26 {
            // A fixed clock: 60 frames a second whatever the GPU takes (or stopped).
            let t = if blowing { i as f32 / 60.0 } else { 0.0 };
            view.environment.grass_time = Some(2.0 + t);
            renderer.render(&device, &queue, &target, &view.frame());
            if i >= 24 {
                frames.push(read_back(&device, &queue, &target));
            }
        }
        save(&format!("wind_taa_{taa}_{blowing}.png"), &frames[1], w, h);
        runs.push((
            flicker(&frames[0], &frames[1]),
            edge_contrast(&frames[1], w, h),
        ));
    }
    let (off, on, still) = (runs[0], runs[1], runs[2]);
    eprintln!(
        "pixels changing > 8 levels a frame: TAA off {:.2} %, TAA on {:.2} %; edge contrast of the blowing grass with TAA {:.1} % of the stopped grass's",
        off.0 * 100.0,
        on.0 * 100.0,
        on.1 / still.1 * 100.0
    );
    assert!(
        on.0 < off.0 * 0.5,
        "TAA does not settle the grass: {runs:?}"
    );
    assert!(on.1 > still.1 * 0.85, "grass smears under TAA: {runs:?}");
}

/// Crop `(x, y, w, h)` of an image, scaled up `scale` times (nearest).
fn crop(px: &[u8], width: u32, (x, y, w, h): (u32, u32, u32, u32), scale: u32) -> image::RgbaImage {
    let mut out = image::RgbaImage::new(w * scale, h * scale);
    for oy in 0..h * scale {
        for ox in 0..w * scale {
            let i = (((y + oy / scale) * width + x + ox / scale) * 4) as usize;
            out.put_pixel(ox, oy, image::Rgba([px[i], px[i + 1], px[i + 2], 255]));
        }
    }
    out
}

/// `docs/screenshots/gfx/taa_<MISTY_TAA_TAG>_{static,turning,wind}.png`: MeadowDemo with TAA
/// on, still (24 frames), turning 3 px a frame, and crouching in the grass in the wind;
/// 2× crops of the middle so the detail shows. `MISTY_TAA_TAG` = before / after.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn taa_screenshots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let tag = std::env::var("MISTY_TAA_TAG").unwrap_or_else(|_| "after".into());
    let dir =
        std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx");
    let (w, h) = (960u32, 540u32);
    let fov = 60.0;
    let step = pixel_yaw(fov, h);
    for (name, close, turn) in [
        ("static", false, 0.0),
        ("turning", false, 3.0),
        ("wind", true, 0.0),
    ] {
        let mut view = meadow(close);
        let base = view.camera.rotation;
        let mut renderer = Renderer::with_queue(&device, &queue);
        renderer.set_quality(&device, &quality(true));
        let target = renderer.create_target(&device, w, h);
        let mut last = Vec::new();
        for i in 0..28usize {
            view.environment.grass_time = Some(2.0 + i as f32 / 60.0);
            let yaw = step * turn * i.saturating_sub(16) as f32;
            view.camera.rotation = Quat::from_rotation_y(yaw) * base;
            renderer.render(&device, &queue, &target, &view.frame());
            last = read_back(&device, &queue, &target);
        }
        let region = if close {
            (240, 200, 480, 270)
        } else {
            (240, 150, 480, 270)
        };
        let path = dir.join(format!("taa_{tag}_{name}.png"));
        crop(&last, w, region, 2).save(&path).expect("png");
        eprintln!("wrote {}", path.display());
    }
}

/// Median GPU time at 1920×1080 of MeadowDemo with TAA off and on (velocity pass and TAA
/// cost; compare with the baseline build).
#[test]
#[ignore = "timing, prints only"]
fn taa_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut view = meadow(false);
    for taa in [false, true] {
        let mut renderer = Renderer::with_queue(&device, &queue);
        renderer.set_quality(&device, &quality(taa));
        let target = renderer.create_target(&device, 1920, 1080);
        let mut times = Vec::new();
        for i in 0..140 {
            view.environment.grass_time = Some(2.0 + i as f32 / 60.0);
            renderer.render(&device, &queue, &target, &view.frame());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 40
                && let Some(ms) = renderer.gpu_ms()
            {
                times.push(ms);
            }
        }
        times.sort_by(f32::total_cmp);
        let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
        eprintln!("MeadowDemo 1080p High, TAA {taa}: {median:.2} ms GPU (median of 100)");
    }
}
