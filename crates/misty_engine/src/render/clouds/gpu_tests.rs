//! Headless GPU checks of the clouds (skipped without an adapter): clouds whiten the sky,
//! geometry in front hides them, their shadows darken the ground, and the Off tier draws
//! nothing. Plus an ignored 1080p GPU-time benchmark.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::{QualityLevel, Tier};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, Frame, LightData, MaterialParams, MeshDraw, PostSettings, Renderer,
};
use crate::scene::{CloudPreset, CloudsOverride, LightType, PrimitiveMesh, Shadows};

const W: u32 = 192;
const H: u32 = 108;

fn sun() -> LightData {
    LightData {
        kind: LightType::Directional,
        position: Vec3::ZERO,
        direction: Vec3::new(0.2, -0.7, 0.6).normalize(),
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

fn clouds(preset: CloudPreset) -> PostSettings {
    let mut o = CloudsOverride {
        active: true,
        wind_speed: 0.0,
        ..Default::default()
    };
    preset.apply(&mut o);
    PostSettings {
        clouds: o,
        ..Default::default()
    }
}

fn plane(y: f32, size: f32) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Plane.into(),
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(size, 1.0, size),
            Quat::IDENTITY,
            Vec3::new(0.0, y, 0.0),
        ),
        material: MaterialParams::default(),
        cast_shadows: false,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

/// Render `frames` frames (temporal history settles) and read back the image; returns the
/// median GPU time too.
fn render(
    camera: &Camera,
    post: PostSettings,
    meshes: &[MeshDraw],
    tier: Tier,
    size: (u32, u32),
    frames: usize,
) -> Option<(Vec<u8>, Option<f32>)> {
    let (device, queue) = headless()?;
    let lights = [sun()];
    let environment = Environment::new(Default::default(), &lights);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let quality = QualityLevel {
        volumetric_clouds: tier,
        // Repeated renders are compared exactly: no TAA jitter.
        temporal_aa: false,
        ..QualityLevel::high()
    };
    renderer.set_quality(&device, &quality);
    let target = renderer.create_target(&device, size.0, size.1);
    let mut times = Vec::new();
    for _ in 0..frames {
        let frame = Frame {
            camera,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &environment,
            post,
            lines: &[],
            overlay: &[],
            meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        renderer.render(&device, &queue, &target, &frame);
        let _ = device.poll(wgpu::PollType::wait_indefinitely());
        if let Some(ms) = renderer.gpu_ms() {
            times.push(ms);
        }
    }
    times.sort_by(f32::total_cmp);
    Some((
        read_back(&device, &queue, &target),
        times.get(times.len() / 2).copied(),
    ))
}

/// Render `frames` frames, the camera of frame `i` given by `camera(i)`, and read back the
/// last `keep` images (temporal history settles over the earlier ones).
fn render_sequence(
    camera: impl Fn(usize) -> Camera,
    post: PostSettings,
    tier: Tier,
    size: (u32, u32),
    frames: usize,
    keep: usize,
) -> Option<Vec<Vec<u8>>> {
    let (device, queue) = headless()?;
    let lights = [sun()];
    let environment = Environment::new(Default::default(), &lights);
    let mut renderer = Renderer::with_queue(&device, &queue);
    let quality = QualityLevel {
        volumetric_clouds: tier,
        temporal_aa: false,
        ..QualityLevel::high()
    };
    renderer.set_quality(&device, &quality);
    let target = renderer.create_target(&device, size.0, size.1);
    let mut out = Vec::new();
    for i in 0..frames {
        let cam = camera(i);
        let frame = Frame {
            camera: &cam,
            grid: None,
            skybox: true,
            background: [0.0; 3],
            environment: &environment,
            post,
            lines: &[],
            overlay: &[],
            meshes: &[],
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        renderer.render(&device, &queue, &target, &frame);
        if i + keep >= frames {
            out.push(read_back(&device, &queue, &target));
        }
    }
    Some(out)
}

/// Mean absolute difference (r, g, b) between `a` and `b` shifted right by `dx` pixels, over
/// the middle half of a `w` × `h` image (where a yaw of one pixel is a uniform shift).
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

/// Mean absolute difference (r, g, b) over the sky (upper 45 % of the image, the middle half
/// across) between frame `b` seen by `cam_b` and frame `a` seen by `cam_a`, each pixel of
/// `b` reprojected exactly (its direction through `cam_a`, bilinear in `a`).
fn sky_difference(a: &[u8], b: &[u8], cam_a: &Camera, cam_b: &Camera, w: u32, h: u32) -> f32 {
    band_difference(a, b, cam_a, cam_b, w, h, (0.0, 0.45)).0
}

/// [`sky_difference`] over rows `rows.0`..`rows.1` (fractions of the height): mean
/// difference, share of pixels off by more than 8 levels, and the difference image (×8).
#[allow(clippy::too_many_arguments)]
fn band_difference(
    a: &[u8],
    b: &[u8],
    cam_a: &Camera,
    cam_b: &Camera,
    w: u32,
    h: u32,
    rows: (f32, f32),
) -> (f32, f32, Vec<u8>) {
    window_difference(a, b, cam_a, cam_b, (w, h), rows, (0.25, 0.75))
}

/// [`band_difference`] over a window: rows and columns as fractions of the image.
fn window_difference(
    a: &[u8],
    b: &[u8],
    cam_a: &Camera,
    cam_b: &Camera,
    (w, h): (u32, u32),
    rows: (f32, f32),
    cols: (f32, f32),
) -> (f32, f32, Vec<u8>) {
    let aspect = w as f32 / h as f32;
    let sample = |x: f32, y: f32, c: usize| {
        let (x0, y0) = (x.floor(), y.floor());
        let (fx, fy) = (x - x0, y - y0);
        let at = |px: f32, py: f32| {
            let px = (px.max(0.0) as u32).min(w - 1);
            let py = (py.max(0.0) as u32).min(h - 1);
            a[((py * w + px) * 4) as usize + c] as f32
        };
        (at(x0, y0) * (1.0 - fx) + at(x0 + 1.0, y0) * fx) * (1.0 - fy)
            + (at(x0, y0 + 1.0) * (1.0 - fx) + at(x0 + 1.0, y0 + 1.0) * fx) * fy
    };
    let mut sum = 0.0f32;
    let mut n = 0.0f32;
    let mut jumps = 0.0f32;
    let mut diff = vec![0u8; (w * h * 4) as usize];
    for y in (h as f32 * rows.0) as u32..(h as f32 * rows.1) as u32 {
        for x in (w as f32 * cols.0) as u32..(w as f32 * cols.1) as u32 {
            let ndc = glam::Vec2::new(
                (x as f32 + 0.5) / w as f32 * 2.0 - 1.0,
                1.0 - (y as f32 + 0.5) / h as f32 * 2.0,
            );
            // Exact rays (no inverted matrices, whose far plane is imprecise).
            let dir = cam_b.rotation * (ndc * half_tan(cam_b, aspect)).extend(1.0);
            let v = cam_a.rotation.inverse() * dir;
            if v.z <= 0.0 {
                continue;
            }
            let p = v.truncate() / (v.z * half_tan(cam_a, aspect));
            let (ax, ay) = (
                (p.x * 0.5 + 0.5) * w as f32 - 0.5,
                (0.5 - p.y * 0.5) * h as f32 - 0.5,
            );
            if ax < 1.0 || ay < 1.0 || ax >= w as f32 - 2.0 || ay >= h as f32 - 2.0 {
                continue;
            }
            let i = ((y * w + x) * 4) as usize;
            let mut worst = 0.0f32;
            for c in 0..3 {
                let d = (b[i + c] as f32 - sample(ax, ay, c)).abs();
                sum += d;
                worst = worst.max(d);
                diff[i + c] = (d * 8.0).min(255.0) as u8;
            }
            diff[i + 3] = 255;
            jumps += f32::from(u8::from(worst > 8.0));
            n += 3.0;
        }
    }
    (sum / n.max(1.0), jumps / (n / 3.0).max(1.0), diff)
}

/// Tangents of half the field of view (x, y) of a perspective camera.
fn half_tan(cam: &Camera, aspect: f32) -> glam::Vec2 {
    let fov_y = match cam.projection {
        Projection::Perspective { fov_y } => fov_y,
        Projection::Orthographic { .. } => 60.0,
    };
    let t = (fov_y.to_radians() * 0.5).tan();
    glam::Vec2::new(t * aspect, t)
}

/// Turning the camera by one pixel a frame moves the clouds by one pixel: frame N+1 equals
/// frame N shifted (no shimmer, no lag), nearly as closely as two frames of a still camera.
#[test]
fn clouds_stay_put_while_turning() {
    let (w, h) = (384u32, 216u32);
    let fov_y = 60f32;
    // Yaw that moves the image centre by one pixel.
    let step = (2.0 * (fov_y.to_radians() * 0.5).tan() / h as f32).atan();
    let cam = |yaw: f32| Camera {
        position: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-25f32.to_radians()),
        projection: Projection::Perspective { fov_y },
        near: 0.1,
        far: 2000.0,
    };
    let post = clouds(CloudPreset::Cloudy);
    let Some(still) = render_sequence(|_| cam(0.0), post, Tier::High, (w, h), 24, 2) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // 16 still frames to settle, then eight frames of turning.
    let turning = render_sequence(
        |i| cam(step * i.saturating_sub(16) as f32),
        post,
        Tier::High,
        (w, h),
        24,
        2,
    )
    .unwrap();
    save_sized("turning_a.png", &turning[0], w, h);
    save_sized("turning_b.png", &turning[1], w, h);
    let rest = shifted_difference(&still[0], &still[1], w, h, 0);
    // A yaw to the left (positive about +Y in a left-handed view) moves the image right.
    let moved_a = shifted_difference(&turning[0], &turning[1], w, h, 1);
    let moved_b = shifted_difference(&turning[0], &turning[1], w, h, -1);
    let moved = moved_a.min(moved_b);
    let unshifted = shifted_difference(&turning[0], &turning[1], w, h, 0);
    eprintln!(
        "still frames differ by {rest:.2}, turning frames (shifted) by {moved:.2} ({moved_a:.2} / {moved_b:.2}), unshifted {unshifted:.2}"
    );
    assert!(
        moved < rest + 2.0,
        "clouds shimmer while turning: {moved} vs {rest}"
    );
}

/// Turning speeds, tiers and wind: prints the shifted frame-to-frame difference of each.
#[test]
#[ignore = "experiment: prints only"]
fn clouds_turning_experiment() {
    let (w, h) = (384u32, 216u32);
    let fov_y = 60f32;
    let step = (2.0 * (fov_y.to_radians() * 0.5).tan() / h as f32).atan();
    let cam = |yaw: f32| Camera {
        position: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-25f32.to_radians()),
        projection: Projection::Perspective { fov_y },
        near: 0.1,
        far: 2000.0,
    };
    for tier in [Tier::Medium, Tier::High] {
        for wind in [0.0, 8.0] {
            let mut post = clouds(CloudPreset::Cloudy);
            post.clouds.wind_speed = wind;
            let Some(still) = render_sequence(|_| cam(0.0), post, tier, (w, h), 24, 2) else {
                return;
            };
            let rest = shifted_difference(&still[0], &still[1], w, h, 0);
            for px in [1i32, 4, 12, 24] {
                let turning = render_sequence(
                    |i| cam(step * px as f32 * i.saturating_sub(16) as f32),
                    post,
                    tier,
                    (w, h),
                    24,
                    2,
                )
                .unwrap();
                save_sized(
                    &format!("exp_{tier:?}_w{wind}_{px}px_a.png"),
                    &turning[0],
                    w,
                    h,
                );
                save_sized(
                    &format!("exp_{tier:?}_w{wind}_{px}px_b.png"),
                    &turning[1],
                    w,
                    h,
                );
                let moved = shifted_difference(&turning[0], &turning[1], w, h, -px);
                let unshifted = shifted_difference(&turning[0], &turning[1], w, h, 0);
                let yaw = |i: usize| step * px as f32 * i.saturating_sub(16) as f32;
                let sky =
                    sky_difference(&turning[0], &turning[1], &cam(yaw(22)), &cam(yaw(23)), w, h);
                let sky_rest = sky_difference(&still[0], &still[1], &cam(0.0), &cam(0.0), w, h);
                eprintln!(
                    "{tier:?} wind {wind}: {px} px/frame: still {rest:.2}, shifted {moved:.2}, unshifted {unshifted:.2}; sky exact {sky:.2} (still {sky_rest:.2})"
                );
            }
        }
    }
}

fn save_sized(name: &str, px: &[u8], w: u32, h: u32) {
    if let Some(dir) = std::env::var_os("MISTY_CLOUD_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(name);
        image::RgbaImage::from_raw(w, h, px.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

fn looking(pitch_up: f32) -> Camera {
    Camera {
        position: Vec3::new(0.0, 2.0, 0.0),
        rotation: Quat::from_rotation_x(-pitch_up.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 2000.0,
    }
}

/// Mean (r, g, b) of a pixel rectangle.
fn mean(px: &[u8], x0: u32, y0: u32, x1: u32, y1: u32) -> [f32; 3] {
    let mut sum = [0.0; 3];
    let mut n = 0.0;
    for y in y0..y1 {
        for x in x0..x1 {
            let i = ((y * W + x) * 4) as usize;
            for c in 0..3 {
                sum[c] += px[i + c] as f32;
            }
            n += 1.0;
        }
    }
    sum.map(|s| s / n)
}

fn save(name: &str, px: &[u8]) {
    if let Some(dir) = std::env::var_os("MISTY_CLOUD_TEST_DIR") {
        let path = std::path::Path::new(&dir).join(name);
        image::RgbaImage::from_raw(W, H, px.to_vec())
            .expect("size")
            .save(path)
            .expect("png");
    }
}

#[test]
fn overcast_clouds_whiten_the_sky() {
    let cam = looking(35.0);
    let Some((clear, _)) = render(&cam, PostSettings::default(), &[], Tier::Medium, (W, H), 1)
    else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let (cloudy, _) = render(
        &cam,
        clouds(CloudPreset::Overcast),
        &[],
        Tier::Medium,
        (W, H),
        8,
    )
    .unwrap();
    save("overcast_sky.png", &cloudy);
    let a = mean(&clear, 0, 0, W, H / 2);
    let b = mean(&cloudy, 0, 0, W, H / 2);
    // Clear sky is clearly blue; clouds are grey-white (blue no longer dominates).
    assert!(a[2] - a[0] > 25.0, "clear sky {a:?}");
    assert!(
        b[2] - b[0] < a[2] - a[0] - 15.0,
        "cloudy sky {b:?} vs {a:?}"
    );
    // Nothing traced at the Off tier.
    let (off, _) = render(
        &cam,
        clouds(CloudPreset::Overcast),
        &[],
        Tier::Off,
        (W, H),
        2,
    )
    .unwrap();
    assert_eq!(off, clear);
}

#[test]
fn geometry_in_front_hides_the_clouds() {
    let cam = looking(35.0);
    // A wall right in front of the camera covers the lower half of the view.
    let wall = MeshDraw {
        model: Mat4::from_scale_rotation_translation(
            Vec3::new(30.0, 6.0, 1.0),
            Quat::IDENTITY,
            Vec3::new(0.0, 2.0, 6.0),
        ),
        mesh: PrimitiveMesh::Cube.into(),
        ..plane(0.0, 1.0)
    };
    let Some((clear, _)) = render(
        &cam,
        PostSettings::default(),
        std::slice::from_ref(&wall),
        Tier::Medium,
        (W, H),
        1,
    ) else {
        return;
    };
    let (cloudy, _) = render(
        &cam,
        clouds(CloudPreset::Overcast),
        std::slice::from_ref(&wall),
        Tier::Medium,
        (W, H),
        4,
    )
    .unwrap();
    save("wall_in_front.png", &cloudy);
    let a = mean(&clear, 0, H - 10, W, H);
    let b = mean(&cloudy, 0, H - 10, W, H);
    // The wall itself only changes by its (cloud-shadowed) sunlight, never towards cloud white.
    assert!(
        b.iter().zip(&a).all(|(b, a)| *b <= *a + 1.0),
        "{b:?} vs {a:?}"
    );
}

#[test]
fn cloud_shadows_darken_the_ground() {
    let cam = Camera {
        position: Vec3::new(0.0, 30.0, 0.0),
        rotation: Quat::from_rotation_x(70f32.to_radians()),
        projection: Projection::Perspective { fov_y: 60.0 },
        near: 0.1,
        far: 4000.0,
    };
    let ground = [plane(0.0, 400.0)];
    let mut overcast = clouds(CloudPreset::Overcast);
    overcast.clouds.shadow_opacity = 1.0;
    let Some((lit, _)) = render(
        &cam,
        PostSettings::default(),
        &ground,
        Tier::Medium,
        (W, H),
        1,
    ) else {
        return;
    };
    let (shaded, _) = render(&cam, overcast, &ground, Tier::Medium, (W, H), 2).unwrap();
    overcast.clouds.shadows = false;
    let (unshadowed, _) = render(&cam, overcast, &ground, Tier::Medium, (W, H), 2).unwrap();
    save("ground_shadow.png", &shaded);
    let l = mean(&lit, 0, H / 2, W, H)[1];
    let s = mean(&shaded, 0, H / 2, W, H)[1];
    let u = mean(&unshadowed, 0, H / 2, W, H)[1];
    assert!(s < l - 10.0, "shadowed {s} vs lit {l}");
    assert!((u - l).abs() < 2.0, "shadows off: {u} vs {l}");
}

/// `cargo test --release -p misty_engine -- --ignored clouds_benchmark --nocapture`
#[test]
#[ignore = "benchmark: cloud GPU cost at 1080p per tier"]
fn clouds_benchmark() {
    let cam = looking(12.0);
    let ground = [plane(0.0, 400.0)];
    let size = (1920, 1080);
    let Some((_, base)) = render(
        &cam,
        PostSettings::default(),
        &ground,
        Tier::Medium,
        size,
        60,
    ) else {
        return;
    };
    eprintln!("1080p GPU without clouds: {base:?} ms");
    for preset in [CloudPreset::Cloudy, CloudPreset::Overcast] {
        for tier in [Tier::Low, Tier::Medium, Tier::High] {
            let (_, ms) = render(&cam, clouds(preset), &ground, tier, size, 60).unwrap();
            eprintln!("1080p GPU {preset:?} clouds {tier:?}: {ms:?} ms");
        }
    }
}

/// MeadowDemo with TAA off, irregular mouse-look turning: per frame pair the sky and the
/// horizon band against an exact reprojection of the frame before.
#[test]
#[ignore = "experiment: prints only"]
fn meadow_mouse_look_experiment() {
    use crate::render_scene::GameView;
    use crate::scene::Scene;
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    crate::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join("Assets/Scenes/MeadowDemo.scene")).expect("scene");
    let materials = crate::assets::AssetCache::open(&root);
    let Some((device, queue)) = headless() else {
        return;
    };
    let (w, h) = (960u32, 540u32);
    let mut view = GameView::new(&scene, &materials).expect("camera");
    let fov_y = match view.camera.projection {
        Projection::Perspective { fov_y } => fov_y,
        Projection::Orthographic { .. } => 60.0,
    };
    let step = (2.0 * (fov_y.to_radians() * 0.5).tan() / h as f32).atan();
    let base = view.camera.rotation;
    for taa in [false, true] {
        let mut r = Renderer::with_queue(&device, &queue);
        r.set_quality(
            &device,
            &QualityLevel {
                temporal_aa: taa,
                ..QualityLevel::high()
            },
        );
        let target = r.create_target(&device, w, h);
        let mut frames = Vec::new();
        let mut cams = Vec::new();
        let mut yaw = 0.0;
        for i in 0..30usize {
            if i >= 16 {
                yaw += step * MOUSE_STEPS[i % MOUSE_STEPS.len()];
            }
            view.camera.rotation = Quat::from_rotation_y(yaw) * base;
            view.environment.grass_time = Some(2.0 + i as f32 / 60.0);
            let frame = view.frame();
            r.render(&device, &queue, &target, &frame);
            if i >= 20 {
                frames.push(read_back(&device, &queue, &target));
                cams.push(view.camera);
            }
        }
        let mut sky = String::new();
        let mut horizon = String::new();
        for k in 0..frames.len() - 1 {
            let (d, j, diff) = band_difference(
                &frames[k],
                &frames[k + 1],
                &cams[k],
                &cams[k + 1],
                w,
                h,
                (0.0, 0.4),
            );
            sky += &format!(" {d:.2}/{:.1}%", j * 100.0);
            let (d, j, _) = band_difference(
                &frames[k],
                &frames[k + 1],
                &cams[k],
                &cams[k + 1],
                w,
                h,
                (0.4, 0.6),
            );
            horizon += &format!(" {d:.2}/{:.1}%", j * 100.0);
            save_sized(&format!("meadow_taa{taa}_{k}.png"), &frames[k], w, h);
            save_sized(&format!("meadow_taa{taa}_{k}_diff.png"), &diff, w, h);
        }
        eprintln!("MeadowDemo TAA {taa} sky:{sky}\n  horizon:{horizon}");
    }
}

/// Mouse-look steps in pixels a frame (irregular, mostly sub-texel of the cloud image).
const MOUSE_STEPS: [f32; 12] = [
    3.3, 7.9, 2.1, 12.6, 5.5, 0.7, 9.2, 4.4, 15.1, 1.6, 6.8, 10.3,
];

/// Turning with a Scene-view fly camera (dynamic clipping close to the ground: near 1 cm, far
/// 1 km, ~1 km from the origin) and TAA off: each frame's sky matches the frame before
/// reprojected exactly, nearly as closely as two frames of a still camera do. With rays
/// unprojected through the inverted view-projection the far plane was off by several pixels,
/// differently for every camera orientation, and the clouds shook while looking around.
#[test]
fn clouds_hold_still_while_looking_around() {
    let (w, h) = (480u32, 270u32);
    let fov_y = 60f32;
    let step = (2.0 * (fov_y.to_radians() * 0.5).tan() / h as f32).atan();
    let yaw_at = |i: usize| -> f32 {
        step * MOUSE_STEPS
            .iter()
            .cycle()
            .take(i.saturating_sub(12))
            .sum::<f32>()
    };
    let cam = |yaw: f32| Camera {
        position: Vec3::new(500.0, 20.0, -800.0),
        rotation: Quat::from_rotation_y(yaw) * Quat::from_rotation_x(-20f32.to_radians()),
        projection: Projection::Perspective { fov_y },
        near: 0.01,
        far: 1000.0,
    };
    let post = clouds(CloudPreset::Cloudy);
    let n = 22;
    let Some(still) = render_sequence(|_| cam(0.0), post, Tier::Medium, (w, h), n, 2) else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let turning = render_sequence(|i| cam(yaw_at(i)), post, Tier::Medium, (w, h), n, 8).unwrap();
    let window = |a: &[u8], b: &[u8], ca: &Camera, cb: &Camera| {
        window_difference(a, b, ca, cb, (w, h), (0.0, 0.45), (0.1, 0.9))
    };
    let (rest, _, _) = window(&still[0], &still[1], &cam(0.0), &cam(0.0));
    let mut worst = 0.0f32;
    let mut jumps = 0.0f32;
    for k in 0..turning.len() - 1 {
        let (i0, i1) = (n - turning.len() + k, n - turning.len() + k + 1);
        let (d, j, diff) = window(
            &turning[k],
            &turning[k + 1],
            &cam(yaw_at(i0)),
            &cam(yaw_at(i1)),
        );
        save_sized(&format!("looking_{k}.png"), &turning[k], w, h);
        save_sized(&format!("looking_{k}_diff.png"), &diff, w, h);
        worst = worst.max(d);
        jumps = jumps.max(j);
    }
    eprintln!(
        "looking around (TAA off): sky off its exact reprojection by {worst:.2} at worst ({:.2} % of pixels > 8 levels); still camera {rest:.2}",
        jumps * 100.0
    );
    assert!(
        worst < rest + 0.6,
        "the clouds shake while turning: {worst} vs {rest}"
    );
    assert!(jumps < 0.005, "the clouds shake while turning: {jumps}");
}
