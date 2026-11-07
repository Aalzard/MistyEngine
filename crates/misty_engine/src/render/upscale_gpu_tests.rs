//! Headless GPU checks of temporal upscaling (Quality ▸ Upscaling, `fs_taau` in taa.wgsl):
//! an upscaled target renders at the internal size and jitters by internal pixels, a still
//! scene converges sharper than a plain stretch of the same internal resolution, the image
//! follows a turning camera, a moving cube leaves no trail and grass in the wind neither
//! shimmers nor smears. Skipped without an adapter; `MISTY_TAA_TEST_DIR=dir` saves images.
//!
//! The measurements are ignored tests, run on purpose in release:
//! `cargo test --release -p misty_engine upscale_ -- --ignored --nocapture --test-threads=1`
//! (`upscale_cost`: GPU time per mode and level; `upscale_screenshots`: native-vs-upscaled
//! crops and difference images in docs/screenshots/gfx; `upscale_motion`: shimmer and smear
//! while the wind blows, turning and walking).

use glam::{Mat4, Quat, Vec3};

use super::gpu_tests::{draw, headless, material, read_back};
use super::taa_gpu_tests::{
    H, Scene, W, camera, edge_contrast, flicker, meadow, pixel_yaw, quality, region_difference,
    save, sequence, shifted_difference,
};
use super::trees::gpu_tests::{Shot, meadow_view};
use super::{MeshDraw, Renderer, RenderingMode};
use crate::camera::Projection;
use crate::quality::{QualityLevel, Upscaling};
use crate::render_scene::GameView;
use crate::scene::PrimitiveMesh;

const MODES: [Upscaling; 3] = [
    Upscaling::Quality,
    Upscaling::Balanced,
    Upscaling::Performance,
];

/// High with TAA and `mode` upscaling.
fn upscaled(mode: Upscaling) -> QualityLevel {
    QualityLevel {
        upscaling: mode,
        ..quality(true)
    }
}

fn mode_name(mode: Upscaling) -> &'static str {
    match mode {
        Upscaling::Off => "native",
        Upscaling::Quality => "quality",
        Upscaling::Balanced => "balanced",
        Upscaling::Performance => "performance",
        Upscaling::Custom => "custom",
    }
}

/// Bilinear stretch of a `w` × `h` RGBA image to `ow` × `oh` (what Render Scale below 1 did
/// before the upscaler: the same internal resolution, no temporal reconstruction).
fn stretch(px: &[u8], (w, h): (u32, u32), (ow, oh): (u32, u32)) -> Vec<u8> {
    let mut out = vec![0u8; (ow * oh * 4) as usize];
    let at = |x: i64, y: i64, c: usize| {
        let x = x.clamp(0, w as i64 - 1) as u32;
        let y = y.clamp(0, h as i64 - 1) as u32;
        px[((y * w + x) * 4) as usize + c] as f32
    };
    for oy in 0..oh {
        for ox in 0..ow {
            let sx = (ox as f32 + 0.5) * w as f32 / ow as f32 - 0.5;
            let sy = (oy as f32 + 0.5) * h as f32 / oh as f32 - 0.5;
            let (x0, y0) = (sx.floor(), sy.floor());
            let (fx, fy) = (sx - x0, sy - y0);
            let (x0, y0) = (x0 as i64, y0 as i64);
            for c in 0..4 {
                let top = at(x0, y0, c) * (1.0 - fx) + at(x0 + 1, y0, c) * fx;
                let bottom = at(x0, y0 + 1, c) * (1.0 - fx) + at(x0 + 1, y0 + 1, c) * fx;
                out[((oy * ow + ox) * 4) as usize + c] =
                    (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    out
}

/// `frames` renders of `view` at `size` (a view target: upscaled per `quality`); the last
/// image.
fn render_view(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    view: &GameView,
    quality: &QualityLevel,
    size: (u32, u32),
    frames: usize,
) -> Vec<u8> {
    let mut r = Renderer::with_queue(device, queue);
    r.set_quality(device, quality);
    let target = r.create_view_target(device, size.0, size.1);
    let frame = view.frame();
    for _ in 0..frames {
        r.render(device, queue, &target, &frame);
    }
    read_back(device, queue, &target)
}

/// Peak signal-to-noise ratio of `b` against `a` (rgb), in dB.
fn psnr(a: &[u8], b: &[u8]) -> f32 {
    let mut sum = 0.0f64;
    let mut n = 0.0f64;
    for (p, q) in a.chunks(4).zip(b.chunks(4)) {
        for c in 0..3 {
            let d = p[c] as f64 - q[c] as f64;
            sum += d * d;
            n += 1.0;
        }
    }
    let mse = (sum / n.max(1.0)).max(1e-6);
    (10.0 * (255.0 * 255.0 / mse).log10()) as f32
}

/// An upscaled view target renders the scene at the internal size and shows it at the
/// output size, the jitter moves by up to half an *internal* pixel, and the target no longer
/// fits once Upscaling is switched off.
#[test]
fn upscaled_target_renders_at_the_internal_size() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut r = Renderer::with_queue(&device, &queue);
    r.set_quality(&device, &upscaled(Upscaling::Performance));
    let target = r.create_view_target(&device, W, H);
    assert!(target.upscaled());
    assert_eq!(target.size(), (W / 2, H / 2));
    assert_eq!(target.output_size(), (W, H));
    assert!(r.view_target_fits(&target, W, H));
    assert!(!r.view_target_fits(&target, W + 2, H));
    let scene = Scene::test();
    let cam = camera(0.0);
    let expected = cam.view_projection(W as f32 / H as f32);
    let w3 = expected.row(3);
    let mut largest = 0.0f32;
    for _ in 0..12 {
        r.render(&device, &queue, &target, &scene.frame(&cam));
        // drawn = translate(j) × expected: row 0 of the difference is j.x × row 3.
        let d = r.last_view_proj - expected;
        let jx = d.row(0).dot(w3) / w3.dot(w3);
        let px = jx * (W / 2) as f32 * 0.5;
        assert!(px.abs() <= 0.5 + 1e-3, "jitter {px} internal px");
        largest = largest.max(px.abs());
    }
    // Output-pixel jitter would stay within a quarter of an internal pixel.
    assert!(largest > 0.3, "jitter not in internal pixels: {largest}");
    let px = read_back(&device, &queue, &target);
    assert_eq!(px.len(), (W * H * 4) as usize);
    let mean = px.chunks(4).map(|p| p[1] as f32).sum::<f32>() / (W * H) as f32;
    assert!(mean > 10.0, "upscaled image is black: {mean}");
    // Upscaling off: a native target again.
    r.set_quality(&device, &quality(true));
    assert!(!r.view_target_fits(&target, W, H));
    let native = r.create_view_target(&device, W, H);
    assert!(!native.upscaled());
    assert_eq!(native.size(), (W, H));
}

/// A still scene: every mode's converged image is closer to native TAA and sharper than a
/// bilinear stretch of the same internal resolution, and Quality keeps most of native's edge
/// contrast.
#[test]
fn upscaler_keeps_a_still_scene_sharp() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut scene = Scene::test();
    let still = |_| camera(0.0);
    let native = sequence(
        &device,
        &queue,
        &quality(true),
        &mut scene,
        24,
        1,
        still,
        |_, _| {},
    )
    .remove(0);
    save("upscale_static_native.png", &native, W, H);
    let e_native = edge_contrast(&native, W, H);
    for mode in MODES {
        let up = sequence(
            &device,
            &queue,
            &upscaled(mode),
            &mut scene,
            64,
            1,
            still,
            |_, _| {},
        )
        .remove(0);
        // The same internal resolution with native TAA, stretched.
        let small = crate::render::target::render_size((W, H), mode.scale(1.0));
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &quality(true));
        let t = r.create_target(&device, small.0, small.1);
        let cam = camera(0.0);
        let frame = scene.frame(&cam);
        for _ in 0..24 {
            r.render(&device, &queue, &t, &frame);
        }
        let stretched = stretch(&read_back(&device, &queue, &t), small, (W, H));
        save(
            &format!("upscale_static_{}.png", mode_name(mode)),
            &up,
            W,
            H,
        );
        save(
            &format!("upscale_static_{}_stretch.png", mode_name(mode)),
            &stretched,
            W,
            H,
        );
        let (e_up, e_st) = (edge_contrast(&up, W, H), edge_contrast(&stretched, W, H));
        let all = (0, 0, W, H);
        let (d_up, d_st) = (
            region_difference(&native, &up, W, all),
            region_difference(&native, &stretched, W, all),
        );
        eprintln!(
            "{:<11} edge contrast {:.1} % of native (stretch {:.1} %), off native by {d_up:.2} levels (stretch {d_st:.2}), PSNR {:.1} dB (stretch {:.1})",
            mode_name(mode),
            e_up / e_native * 100.0,
            e_st / e_native * 100.0,
            psnr(&native, &up),
            psnr(&native, &stretched),
        );
        assert!(
            e_up > e_st,
            "{mode:?}: no sharper than a stretch: {e_up} vs {e_st}"
        );
        assert!(d_up < d_st, "{mode:?}: further from native than a stretch");
        if mode == Upscaling::Quality {
            assert!(
                e_up > 0.85 * e_native,
                "Quality softened: {e_up} vs {e_native}"
            );
        }
    }
}

/// Turning one output pixel a frame at Performance: consecutive frames are shifted copies
/// about as closely as native without TAA (no lag, no shimmer from the coarser samples).
#[test]
fn upscaler_follows_a_turning_camera() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let step = pixel_yaw(50.0, H);
    let mut scene = Scene::test();
    let turning = |i: usize| camera(step * i.saturating_sub(40) as f32);
    let off = sequence(
        &device,
        &queue,
        &quality(false),
        &mut scene,
        48,
        2,
        turning,
        |_, _| {},
    );
    let shifted = |f: &[Vec<u8>]| {
        shifted_difference(&f[0], &f[1], W, H, 1).min(shifted_difference(&f[0], &f[1], W, H, -1))
    };
    let d_off = shifted(&off);
    for mode in [Upscaling::Quality, Upscaling::Performance] {
        let up = sequence(
            &device,
            &queue,
            &upscaled(mode),
            &mut scene,
            48,
            2,
            turning,
            |_, _| {},
        );
        save(
            &format!("upscale_turning_{}.png", mode_name(mode)),
            &up[1],
            W,
            H,
        );
        let d_up = shifted(&up);
        let unshifted = shifted_difference(&up[0], &up[1], W, H, 0);
        eprintln!(
            "turning 1 px/frame, shifted difference: native TAA off {d_off:.2}, {} {d_up:.2} (unshifted {unshifted:.2})",
            mode_name(mode)
        );
        assert!(
            d_up < d_off + 1.5,
            "{mode:?} shimmers or lags: {d_up} vs {d_off}"
        );
        assert!(
            d_up < unshifted,
            "{mode:?}: the image did not follow the camera"
        );
    }
}

/// A cube crossing the view at 10 output px a frame, upscaled at Performance: where it was
/// two frames ago the ground matches a converged frame without the cube.
#[test]
fn upscaler_moving_cube_leaves_no_ghost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let cam = camera(0.0);
    let aspect = W as f32 / H as f32;
    let red = material([0.9, 0.2, 0.2, 1.0], RenderingMode::Opaque);
    let ground = draw(
        PrimitiveMesh::Plane,
        Mat4::from_scale(Vec3::new(4.0, 1.0, 4.0)),
        &material([0.5, 0.5, 0.5, 1.0], RenderingMode::Opaque),
    );
    let (depth_z, y) = (-2.0f32, 2.0f32);
    let px_x = |x: f32| {
        let ndc = cam
            .view_projection(aspect)
            .project_point3(Vec3::new(x, y, depth_z));
        (ndc.x * 0.5 + 0.5) * W as f32
    };
    let dx = 10.0 / (px_x(1.0) - px_x(0.0));
    let cube_at = |i: usize| {
        let x = -3.0 + dx * i.saturating_sub(24) as f32;
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
    let frames = 44;
    let quality = upscaled(Upscaling::Performance);
    let mut scene = Scene::test();
    scene.meshes = vec![ground.clone(), cube_at(0)];
    let edit = |i: usize, meshes: &mut Vec<MeshDraw>| {
        meshes.truncate(1);
        meshes.push(cube_at(i));
    };
    let on = sequence(
        &device,
        &queue,
        &quality,
        &mut scene,
        frames,
        1,
        |_| cam,
        edit,
    );
    // Converged reference: the ground alone (the trail region is ground in both).
    scene.meshes = vec![ground.clone()];
    let clean = sequence(
        &device,
        &queue,
        &quality,
        &mut scene,
        64,
        1,
        |_| cam,
        |_, _| {},
    );
    save("upscale_ghost_on.png", &on[0], W, H);
    save("upscale_ghost_clean.png", &clean[0], W, H);
    let now = px_x(-3.0 + dx * (frames - 1 - 24) as f32);
    let before = px_x(-3.0 + dx * (frames - 3 - 24) as f32);
    let ndc = cam
        .view_projection(aspect)
        .project_point3(Vec3::new(0.0, y, depth_z));
    let cy = ((0.5 - ndc.y * 0.5) * H as f32) as u32;
    let x0 = (before - 10.0).max(0.0) as u32;
    let x1 = (before + 4.0).min(now - 14.0).max(x0 as f32 + 1.0) as u32;
    let region = (x0, cy.saturating_sub(10), x1, (cy + 10).min(H));
    let trail = region_difference(&on[0], &clean[0], W, region);
    eprintln!("behind the cube (Performance): {trail:.2} levels off the clean frame");
    assert!(trail < 4.0, "a ghost trail follows the cube: {trail}");
}

/// Grass in the wind (MeadowDemo, crouching) upscaled: thin blades shimmer much less than
/// native without TAA, and the blowing blades keep most of the contrast of the same view
/// with the wind stopped (no smear).
#[test]
fn upscaled_grass_in_the_wind_does_not_smear() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut view = meadow(true);
    let (w, h) = (640u32, 360u32);
    let mut run = |q: &QualityLevel, blowing: bool| {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, q);
        let target = r.create_view_target(&device, w, h);
        let mut frames = Vec::new();
        for i in 0..42 {
            let t = if blowing { i as f32 / 60.0 } else { 0.0 };
            view.environment.grass_time = Some(2.0 + t);
            r.render(&device, &queue, &target, &view.frame());
            if i >= 40 {
                frames.push(read_back(&device, &queue, &target));
            }
        }
        (
            flicker(&frames[0], &frames[1]),
            edge_contrast(&frames[1], w, h),
            frames.remove(1),
        )
    };
    let (off, _, _) = run(&quality(false), true);
    for mode in [Upscaling::Quality, Upscaling::Performance] {
        let (on, contrast, img) = run(&upscaled(mode), true);
        let (_, still, _) = run(&upscaled(mode), false);
        save(&format!("upscale_wind_{}.png", mode_name(mode)), &img, w, h);
        eprintln!(
            "{}: pixels changing > 8 levels a frame {:.2} % (native TAA off {:.2} %); blowing grass keeps {:.1} % of the stopped grass's edge contrast",
            mode_name(mode),
            on * 100.0,
            off * 100.0,
            contrast / still * 100.0
        );
        assert!(
            on < (off * 0.5).max(0.01),
            "{mode:?} does not settle the grass"
        );
        assert!(
            contrast > still * 0.85,
            "{mode:?} smears the grass: {contrast} vs {still}"
        );
    }
}

// ---- Measurements (ignored; run on purpose in release) ---------------------------------

fn level(name: &str) -> QualityLevel {
    match name {
        "low" => QualityLevel::low(),
        "medium" => QualityLevel::medium(),
        "ultra" => QualityLevel::ultra(),
        _ => QualityLevel::high(),
    }
}

/// GPU time of MeadowDemo (the saved player view) per quality level and Upscaling mode at
/// 1920×1080 and 2560×1440: median whole-frame GPU time of 80 frames after 40 to settle,
/// and the median per-pass times (pass timer) of the main pass, the temporal resolve and
/// post-processing; the fastest of `MISTY_ROUNDS` (3) interleaved rounds.
/// `MISTY_SIZES=1920x1080`, `MISTY_QUALITY=high,low` narrow the run.
/// The level's own Render Scale is not applied (the Low level's 0.75 stretch would mix two
/// resolutions into one row).
#[test]
#[ignore = "timing, prints only"]
fn upscale_cost() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let view = meadow_view(Shot::Wide);
    let frame = view.frame();
    let sizes = std::env::var("MISTY_SIZES").unwrap_or_else(|_| "1920x1080,2560x1440".into());
    let levels = std::env::var("MISTY_QUALITY").unwrap_or_else(|_| "high,medium,low".into());
    let rounds: usize = std::env::var("MISTY_ROUNDS")
        .ok()
        .and_then(|r| r.parse().ok())
        .unwrap_or(3);
    let median = |v: &mut Vec<f32>| {
        v.sort_by(f32::total_cmp);
        v.get(v.len() / 2).copied().unwrap_or(f32::NAN)
    };
    let mut configs = Vec::new();
    for size in sizes.split(',') {
        let Some((w, h)) = size
            .split_once('x')
            .and_then(|(w, h)| Some((w.parse::<u32>().ok()?, h.parse::<u32>().ok()?)))
        else {
            continue;
        };
        for name in levels.split(',') {
            for mode in [
                Upscaling::Off,
                Upscaling::Quality,
                Upscaling::Balanced,
                Upscaling::Performance,
            ] {
                configs.push(((w, h), name.to_owned(), mode));
            }
        }
    }
    // Warm the GPU clocks up.
    {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(&device, &QualityLevel::low());
        let t = r.create_view_target(&device, 1920, 1080);
        for _ in 0..60 {
            r.render(&device, &queue, &t, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
        }
    }
    // Per config and round: (GPU ms, main, resolve, post, CPU ms, render size). Rounds go
    // through every config in turn, so a slow spell (clocks, another GPU user) hits one
    // round of each instead of all rounds of one; the table keeps each config's fastest.
    let mut results: Vec<Vec<[f32; 5]>> = vec![Vec::new(); configs.len()];
    let mut render_sizes = vec![(0, 0); configs.len()];
    for round in 0..rounds {
        for (c, ((w, h), name, mode)) in configs.iter().enumerate() {
            let q = QualityLevel {
                upscaling: *mode,
                ..level(name)
            };
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &q);
            let target = r.create_view_target(&device, *w, *h);
            render_sizes[c] = target.size();
            let (mut gpu, mut cpu) = (Vec::new(), Vec::new());
            for i in 0..120 {
                r.render(&device, &queue, &target, &frame);
                let _ = device.poll(wgpu::PollType::wait_indefinitely());
                if i >= 40 {
                    gpu.extend(r.gpu_ms());
                    cpu.push(r.stats().cpu_ms);
                }
            }
            let (mut main, mut resolve, mut post) = (Vec::new(), Vec::new(), Vec::new());
            // `MISTY_PARTS=1`: every part's median too.
            let mut parts: Vec<(&'static str, Vec<f32>)> = Vec::new();
            if r.set_pass_timing(&device, &queue, true) {
                for _ in 0..30 {
                    r.render(&device, &queue, &target, &frame);
                    for &(label, ms) in r.pass_times() {
                        match parts.iter_mut().find(|(l, _)| *l == label) {
                            Some(p) => p.1.push(ms),
                            None => parts.push((label, vec![ms])),
                        }
                    }
                    let sum = |f: &dyn Fn(&str) -> bool| {
                        r.pass_times()
                            .iter()
                            .filter(|(l, _)| f(l))
                            .map(|(_, ms)| ms)
                            .sum::<f32>()
                    };
                    main.push(sum(&|l| l.starts_with("Main")));
                    resolve.push(sum(&|l| l == "TAA"));
                    post.push(sum(&|l| l.starts_with("Bloom")));
                }
            }
            let m = [
                median(&mut gpu),
                median(&mut main),
                median(&mut resolve),
                median(&mut post),
                median(&mut cpu),
            ];
            eprintln!(
                "round {round}: {w}x{h} {name} {}: {:.2} ms",
                mode_name(*mode),
                m[0]
            );
            if std::env::var_os("MISTY_PARTS").is_some() {
                for (label, mut v) in parts {
                    let ms = median(&mut v);
                    if ms >= 0.05 {
                        eprintln!("    {label:<32} {ms:6.2} ms");
                    }
                }
            }
            results[c].push(m);
        }
    }
    eprintln!(
        "\nsize       level   mode         render     GPU ms (rounds)            FPS   main  resolve  post  (CPU ms)"
    );
    for (c, ((w, h), name, mode)) in configs.iter().enumerate() {
        let best = results[c]
            .iter()
            .copied()
            .min_by(|a, b| a[0].total_cmp(&b[0]))
            .unwrap_or([f32::NAN; 5]);
        let all: Vec<String> = results[c].iter().map(|m| format!("{:.1}", m[0])).collect();
        let (rw, rh) = render_sizes[c];
        eprintln!(
            "{w}x{h}  {name:<7} {:<12} {rw:>4}x{rh:<5} {:6.2} ({:<18}) {:6.1} {:6.2} {:6.2} {:5.2}  ({:.2})",
            mode_name(*mode),
            best[0],
            all.join(" "),
            1000.0 / best[0],
            best[1],
            best[2],
            best[3],
            best[4],
        );
    }
}

/// `docs/screenshots/gfx/upscale_<shot>_<mode>.png` (MeadowDemo at 1920×1080, High, still,
/// converged; 1:1 crops of 640×360 from the middle of the frame) for native TAA, the three
/// Upscaling modes and a plain bilinear stretch of the 50 % image, and
/// `upscale_<shot>_<mode>_diff.png`: the absolute difference to native × 4.
#[test]
#[ignore = "writes comparison renders into docs/screenshots/gfx; run on purpose"]
fn upscale_screenshots() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // `MISTY_UPSCALE_DIR` writes elsewhere, `MISTY_UPSCALE_SHOTS=wide,close` picks shots.
    let dir = std::env::var_os("MISTY_UPSCALE_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/gfx")
        });
    let only = std::env::var("MISTY_UPSCALE_SHOTS").ok();
    let (w, h) = (1920u32, 1080u32);
    let region = (640, 360, 640, 360);
    let cut = |px: &[u8]| super::taa_gpu_tests::crop(px, w, region, 1);
    let write = |name: String, img: image::RgbaImage| {
        let path = dir.join(name);
        img.save(&path).expect("png");
        eprintln!("wrote {}", path.display());
    };
    let diff = |a: &[u8], b: &[u8]| -> Vec<u8> {
        a.chunks(4)
            .zip(b.chunks(4))
            .flat_map(|(p, q)| {
                let d = |c: usize| ((p[c] as i32 - q[c] as i32).unsigned_abs() * 4).min(255) as u8;
                [d(0), d(1), d(2), 255]
            })
            .collect()
    };
    for shot in [Shot::Wide, Shot::Close, Shot::Behind] {
        if only
            .as_ref()
            .is_some_and(|o| !o.split(',').any(|s| s == shot.name()))
        {
            continue;
        }
        let view = meadow_view(shot);
        let native = render_view(&device, &queue, &view, &quality(true), (w, h), 48);
        write(format!("upscale_{}_native.png", shot.name()), cut(&native));
        let e_native = edge_contrast(&native, w, h);
        let mut images: Vec<(String, Vec<u8>)> = MODES
            .iter()
            .map(|&m| {
                let img = render_view(&device, &queue, &view, &upscaled(m), (w, h), 72);
                (mode_name(m).to_owned(), img)
            })
            .collect();
        let half = render_view(&device, &queue, &view, &quality(true), (w / 2, h / 2), 48);
        images.push((
            "stretch50".to_owned(),
            stretch(&half, (w / 2, h / 2), (w, h)),
        ));
        for (name, img) in &images {
            eprintln!(
                "{:<7} {name:<11} PSNR vs native {:5.2} dB, mean difference {:.2} levels, edge contrast {:.1} % of native",
                shot.name(),
                psnr(&native, img),
                region_difference(&native, img, w, (0, 0, w, h)),
                edge_contrast(img, w, h) / e_native * 100.0
            );
            write(format!("upscale_{}_{name}.png", shot.name()), cut(img));
            write(
                format!("upscale_{}_{name}_diff.png", shot.name()),
                cut(&diff(&native, img)),
            );
        }
    }
}

/// Shimmer and smear while moving, MeadowDemo at 960×540 (High), per mode (native without
/// TAA, native TAA, the three Upscaling modes, and native TAA at 50 % stretched):
/// - **wind**: still camera, wind blowing — share of pixels jumping by more than 8 levels
///   between the last two frames (shimmer), as in `grass_in_the_wind_does_not_smear`;
/// - **turning** 2 px a frame — consecutive frames compared shifted by the turn (lower =
///   steadier), as in `taa_follows_a_turning_camera`, and the edge contrast against the
///   still image (smear);
/// - **walking** forward at 1.4 m/s (60 frames a second) — edge contrast against the still
///   image (smear).
#[test]
#[ignore = "image quality in motion, prints only"]
fn upscale_motion() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (w, h) = (960u32, 540u32);
    let base = meadow(false);
    let fov = match base.camera.projection {
        Projection::Perspective { fov_y } => fov_y,
        Projection::Orthographic { .. } => 60.0,
    };
    let step = pixel_yaw(fov, h);
    let forward = base.camera.rotation * Vec3::NEG_Z;
    // (name, quality, render size, stretched to the output)
    let mut setups: Vec<(&str, QualityLevel, (u32, u32))> = vec![
        ("native, TAA off", quality(false), (w, h)),
        ("native TAA", quality(true), (w, h)),
    ];
    for m in MODES {
        setups.push((mode_name(m), upscaled(m), (w, h)));
    }
    setups.push(("50 % stretched", quality(true), (w / 2, h / 2)));
    #[derive(Clone, Copy)]
    enum Motion {
        Still,
        Wind,
        Turning,
        Walking,
    }
    eprintln!(
        "{:<16} {:>9} {:>9} {:>9} {:>10} {:>10}",
        "mode", "wind %", "contrast", "turn diff", "turn smear", "walk smear"
    );
    for (name, q, size) in setups {
        let run = |motion: Motion| {
            let mut view = meadow(false);
            let mut r = Renderer::with_queue(&device, &queue);
            r.set_quality(&device, &q);
            let target = r.create_view_target(&device, size.0, size.1);
            let mut last = Vec::new();
            let frames = 48usize;
            for i in 0..frames {
                let moving = i.saturating_sub(40) as f32;
                let t = match motion {
                    Motion::Still => 0.0,
                    _ => i as f32 / 60.0,
                };
                view.environment.grass_time = Some(2.0 + t);
                match motion {
                    Motion::Turning => {
                        view.camera.rotation =
                            Quat::from_rotation_y(2.0 * step * moving) * base.camera.rotation;
                    }
                    Motion::Walking => {
                        view.camera.position =
                            base.camera.position + forward * (1.4 / 60.0) * moving;
                    }
                    _ => {}
                }
                r.render(&device, &queue, &target, &view.frame());
                if i + 2 >= frames {
                    let px = read_back(&device, &queue, &target);
                    last.push(if size == (w, h) {
                        px
                    } else {
                        stretch(&px, size, (w, h))
                    });
                }
            }
            last
        };
        let still = run(Motion::Still);
        let wind = run(Motion::Wind);
        let turning = run(Motion::Turning);
        let walking = run(Motion::Walking);
        let e_still = edge_contrast(&still[1], w, h);
        let turn_diff = shifted_difference(&turning[0], &turning[1], w, h, 2)
            .min(shifted_difference(&turning[0], &turning[1], w, h, -2));
        eprintln!(
            "{name:<16} {:>8.2}% {:>9.2} {:>9.2} {:>9.1}% {:>9.1}%",
            flicker(&wind[0], &wind[1]) * 100.0,
            e_still,
            turn_diff,
            edge_contrast(&turning[1], w, h) / e_still * 100.0,
            edge_contrast(&walking[1], w, h) / e_still * 100.0,
        );
        let tag = name.replace([' ', ',', '%'], "");
        save(&format!("motion_{tag}_turning.png"), &turning[1], w, h);
        save(&format!("motion_{tag}_walking.png"), &walking[1], w, h);
    }
}
