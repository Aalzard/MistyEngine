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
