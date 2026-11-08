//! GPU checks of Auto Exposure, Depth of Field and Motion Blur on the shared test scene
//! (skipped without an adapter). `MISTY_POSTFX_TEST_DIR=dir` saves the images.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::quality::{QualityLevel, Upscaling};
use crate::render::gpu_tests::{headless, read_back, test_lights, test_meshes};
use crate::render::{Environment, Frame, MeshDraw, PostSettings, Renderer};
use crate::scene::{
    AdaptationMode, AutoExposureOverride, DepthOfFieldOverride, DofMode, MotionBlurOverride,
    PostFxQuality,
};

const W: u32 = 320;
const H: u32 = 200;

fn camera(yaw: f32) -> Camera {
    Camera {
        position: Vec3::new(0.0, 3.0, -12.0),
        rotation: Quat::from_rotation_y(yaw.to_radians())
            * Quat::from_rotation_x(12f32.to_radians()),
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.3,
        far: 80.0,
    }
}

/// Renders `views` in turn into one target (each a camera and draws) and returns the last image.
fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    quality: &QualityLevel,
    post: PostSettings,
    views: &[(Camera, Vec<MeshDraw>)],
    lights_scale: f32,
) -> Vec<u8> {
    // A dimmed scene goes dark all over: the sky too (else it keeps the average up).
    let skybox = lights_scale >= 1.0;
    let mut lights = test_lights();
    for l in &mut lights {
        l.intensity *= lights_scale;
    }
    let environment = Environment::default();
    let mut renderer = Renderer::with_queue(device, queue);
    renderer.set_quality(device, quality);
    let target = renderer.create_view_target(device, W, H);
    for (cam, meshes) in views {
        let frame = Frame {
            camera: cam,
            grid: None,
            skybox,
            background: [0.1 * lights_scale; 3],
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
        renderer.render(device, queue, &target, &frame);
    }
    read_back(device, queue, &target)
}

fn still(frames: usize) -> Vec<(Camera, Vec<MeshDraw>)> {
    (0..frames).map(|_| (camera(0.0), test_meshes())).collect()
}

fn save(name: &str, px: &[u8], (w, h): (u32, u32)) {
    if let Some(dir) = std::env::var_os("MISTY_POSTFX_TEST_DIR") {
        let img = image::RgbaImage::from_raw(w, h, px.to_vec()).expect("size");
        img.save(std::path::Path::new(&dir).join(name))
            .expect("png");
    }
}

/// Sum of absolute horizontal and vertical luminance steps inside `rows` (a sharpness score).
fn detail(px: &[u8], w: u32, rows: std::ops::Range<u32>) -> f64 {
    let luma = |x: u32, y: u32| {
        let i = ((y * w + x) * 4) as usize;
        px[i] as f64 * 0.3 + px[i + 1] as f64 * 0.59 + px[i + 2] as f64 * 0.11
    };
    let mut sum = 0.0;
    for y in rows {
        for x in 1..w {
            sum += (luma(x, y) - luma(x - 1, y)).abs();
            if y > 0 {
                sum += (luma(x, y) - luma(x, y - 1)).abs();
            }
        }
    }
    sum
}

fn mean(px: &[u8]) -> f64 {
    px.chunks(4)
        .map(|p| (p[0] as f64 + p[1] as f64 + p[2] as f64) / 3.0)
        .sum::<f64>()
        / (px.len() / 4) as f64
}

fn quality() -> QualityLevel {
    QualityLevel {
        temporal_aa: true,
        ..QualityLevel::high()
    }
}

#[test]
fn inactive_overrides_leave_the_image_alone() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let q = quality();
    // (As many frames as below: TAA converges over them.)
    let plain = render(&device, &queue, &q, PostSettings::default(), &still(5), 1.0);
    // Values set, overrides unticked: nothing runs.
    let post = PostSettings {
        depth_of_field: DepthOfFieldOverride {
            mode: DofMode::Bokeh,
            aperture: 1.0,
            ..Default::default()
        },
        motion_blur: MotionBlurOverride {
            intensity: 1.0,
            ..Default::default()
        },
        auto_exposure: AutoExposureOverride {
            compensation: 3.0,
            ..Default::default()
        },
        ..Default::default()
    };
    assert!(render(&device, &queue, &q, post, &still(5), 1.0) == plain);
    // Auto Exposure on for a frame, then off: back to exactly the fixed exposure.
    let mut renderer = Renderer::with_queue(&device, &queue);
    renderer.set_quality(&device, &q);
    let target = renderer.create_view_target(&device, W, H);
    let (meshes, lights) = (test_meshes(), test_lights());
    let environment = Environment::default();
    let cam = camera(0.0);
    for on in [true, true, false, false, false] {
        let mut post = PostSettings::default();
        post.auto_exposure.active = on;
        post.auto_exposure.compensation = 2.0;
        let frame = Frame {
            camera: &cam,
            grid: None,
            skybox: true,
            background: [0.1; 3],
            environment: &environment,
            post,
            lines: &[],
            overlay: &[],
            meshes: &meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &lights,
            water: &[],
        };
        renderer.render(&device, &queue, &target, &frame);
    }
    assert!(read_back(&device, &queue, &target) == plain);
}

#[test]
fn depth_of_field_blurs_what_is_out_of_focus() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let q = quality();
    let plain = render(&device, &queue, &q, PostSettings::default(), &still(3), 1.0);
    // The bottom rows look at the near ground (~6 m), the middle rows at the far rows of
    // objects (~25 m).
    let (near_rows, far_rows) = (160..195, 60..110);
    save("dof_off.png", &plain, (W, H));
    for (name, dof) in [
        (
            "gaussian",
            DepthOfFieldOverride {
                active: true,
                mode: DofMode::Gaussian,
                gaussian_start: 14.0,
                gaussian_end: 24.0,
                gaussian_max_radius: 1.5,
                ..Default::default()
            },
        ),
        (
            "bokeh",
            DepthOfFieldOverride {
                active: true,
                mode: DofMode::Bokeh,
                focus_distance: 7.0,
                focal_length: 85.0,
                aperture: 1.4,
                quality: PostFxQuality::High,
                ..Default::default()
            },
        ),
    ] {
        let post = PostSettings {
            depth_of_field: dof,
            ..Default::default()
        };
        let image = render(&device, &queue, &q, post, &still(3), 1.0);
        save(&format!("dof_{name}.png"), &image, (W, H));
        let far_before = detail(&plain, W, far_rows.clone());
        let far_after = detail(&image, W, far_rows.clone());
        let near_before = detail(&plain, W, near_rows.clone());
        let near_after = detail(&image, W, near_rows.clone());
        eprintln!(
            "{name}: far {far_before:.0} -> {far_after:.0}, near {near_before:.0} -> {near_after:.0}"
        );
        assert!(far_after < far_before * 0.8, "{name}: the far field blurs");
        assert!(
            near_after > near_before * 0.85,
            "{name}: the near ground stays sharp"
        );
    }
    // Bokeh focused far: the near ground blurs instead.
    let post = PostSettings {
        depth_of_field: DepthOfFieldOverride {
            active: true,
            mode: DofMode::Bokeh,
            focus_distance: 30.0,
            focal_length: 85.0,
            aperture: 1.4,
            ..Default::default()
        },
        ..Default::default()
    };
    let image = render(&device, &queue, &q, post, &still(3), 1.0);
    save("dof_bokeh_far_focus.png", &image, (W, H));
    assert!(detail(&image, W, near_rows.clone()) < detail(&plain, W, near_rows) * 0.9);
}

#[test]
fn motion_blur_follows_the_camera_and_moving_objects() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let q = quality();
    // The camera turns 2° per frame (no cut).
    let turning: Vec<_> = (0..4)
        .map(|i| (camera(i as f32 * 2.0), test_meshes()))
        .collect();
    let blur = |camera_motion_blur| PostSettings {
        motion_blur: MotionBlurOverride {
            active: true,
            intensity: 1.0,
            camera_motion_blur,
            camera_clamp: 0.2,
            maximum_velocity: 0.2,
            sample_count: 16,
        },
        ..Default::default()
    };
    let plain = render(&device, &queue, &q, PostSettings::default(), &turning, 1.0);
    let blurred = render(&device, &queue, &q, blur(true), &turning, 1.0);
    let objects_only = render(&device, &queue, &q, blur(false), &turning, 1.0);
    save("mb_turning_off.png", &plain, (W, H));
    save("mb_turning_on.png", &blurred, (W, H));
    let rows = 40..190;
    let (a, b) = (
        detail(&plain, W, rows.clone()),
        detail(&blurred, W, rows.clone()),
    );
    eprintln!("turning: {a:.0} -> {b:.0}");
    assert!(b < a * 0.85, "camera turn blurs");
    // Camera Motion Blur off: nothing moves in the world, so (nearly) nothing blurs.
    let c = detail(&objects_only, W, rows.clone());
    assert!(c > a * 0.97, "objects only: {a:.0} -> {c:.0}");
    // A still camera and one object sliding sideways: only around the object changes.
    let moving: Vec<_> = (0..4)
        .map(|i| {
            let mut meshes = test_meshes();
            meshes.push(crate::render::gpu_tests::draw(
                crate::scene::PrimitiveMesh::Cube,
                Mat4::from_translation(Vec3::new(-2.0 + i as f32 * 0.6, 2.5, -4.0)),
                &crate::render::gpu_tests::material(
                    [1.0, 1.0, 1.0, 1.0],
                    crate::render::RenderingMode::Opaque,
                ),
            ));
            (camera(0.0), meshes)
        })
        .collect();
    let plain = render(&device, &queue, &q, PostSettings::default(), &moving, 1.0);
    let blurred = render(&device, &queue, &q, blur(true), &moving, 1.0);
    save("mb_object_off.png", &plain, (W, H));
    save("mb_object_on.png", &blurred, (W, H));
    let changed: Vec<u32> = plain
        .chunks(4)
        .zip(blurred.chunks(4))
        .enumerate()
        .filter(|(_, (p, b))| p.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 24))
        .map(|(i, _)| i as u32)
        .collect();
    assert!(
        changed.len() > 100,
        "the moving cube smears: {}",
        changed.len()
    );
    // Everything that changed is on the cube's rows (the rest of the scene is still).
    let rows: Vec<u32> = changed.iter().map(|i| i / W).collect();
    let (top, bottom) = (rows.iter().min().unwrap(), rows.iter().max().unwrap());
    assert!(bottom - top < H / 2, "rows {top}..{bottom}");
}

#[test]
fn auto_exposure_adapts_to_dark_and_bright_scenes() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let q = quality();
    let auto = |adaptation| PostSettings {
        auto_exposure: AutoExposureOverride {
            active: true,
            adaptation,
            limit_min: -6.0,
            limit_max: 6.0,
            ..Default::default()
        },
        ..Default::default()
    };
    let fixed = auto(AdaptationMode::Fixed);
    let dark_plain = render(&device, &queue, &q, PostSettings::default(), &still(2), 0.1);
    let dark_auto = render(&device, &queue, &q, fixed, &still(2), 0.1);
    let bright_plain = render(&device, &queue, &q, PostSettings::default(), &still(2), 8.0);
    let bright_auto = render(&device, &queue, &q, fixed, &still(2), 8.0);
    let (dp, da, bp, ba) = (
        mean(&dark_plain),
        mean(&dark_auto),
        mean(&bright_plain),
        mean(&bright_auto),
    );
    eprintln!("dark {dp:.1} -> {da:.1}, bright {bp:.1} -> {ba:.1}");
    save("ae_dark_off.png", &dark_plain, (W, H));
    save("ae_dark_on.png", &dark_auto, (W, H));
    assert!(da > dp * 1.5, "a dark scene is brightened");
    assert!(ba < bp * 0.9, "a bright scene is darkened");
    // Both end up nearer each other than without adaptation.
    assert!((ba - da).abs() < (bp - dp).abs() * 0.6);
    // Progressive adaptation gets there over frames, not at once.
    let first = render(
        &device,
        &queue,
        &q,
        auto(AdaptationMode::Progressive),
        &still(2),
        0.1,
    );
    assert!(mean(&first) < da, "progressive starts slower");
}

#[test]
fn effects_run_with_the_upscaler() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let q = QualityLevel {
        upscaling: Upscaling::Performance,
        ..quality()
    };
    let turning: Vec<_> = (0..3)
        .map(|i| (camera(i as f32 * 2.0), test_meshes()))
        .collect();
    let plain = render(&device, &queue, &q, PostSettings::default(), &turning, 1.0);
    let post = PostSettings {
        depth_of_field: DepthOfFieldOverride {
            active: true,
            mode: DofMode::Bokeh,
            focus_distance: 7.0,
            aperture: 1.4,
            focal_length: 85.0,
            ..Default::default()
        },
        motion_blur: MotionBlurOverride {
            active: true,
            ..Default::default()
        },
        auto_exposure: AutoExposureOverride {
            active: true,
            ..Default::default()
        },
        ..Default::default()
    };
    let all = render(&device, &queue, &q, post, &turning, 1.0);
    save("upscaled_all.png", &all, (W, H));
    let rows = 60..110;
    assert!(detail(&all, W, rows.clone()) < detail(&plain, W, rows) * 0.9);
}
