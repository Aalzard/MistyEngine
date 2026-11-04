//! Headless GPU checks of the glows (skipped without an adapter): a halo brightens the air
//! around its light, an occluder in front fades it, thick fog widens it, and lens flares
//! draw their ghosts. Plus an ignored GPU-time benchmark.

use glam::{Mat4, Quat, Vec3};

use super::GlowSource;
use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, FogSettings, Frame, MaterialParams, MeshDraw, PostSettings, Renderer,
};
use crate::scene::{FogOverride, LensFlare, LightHalo, LightType, PrimitiveMesh};

const W: u32 = 320;
const H: u32 = 180;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 0.0, -10.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn lamp(position: Vec3) -> GlowSource {
    GlowSource {
        kind: LightType::Point,
        position,
        direction: Vec3::Z,
        spot_angle: 30.0,
        color: [1.0, 0.9, 0.7],
        intensity: 2.0,
        halo: Some(LightHalo {
            enabled: true,
            size: 1.5,
            ..Default::default()
        }),
        flare: None,
    }
}

fn wall(center: Vec3, size: Vec3) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center),
        material: MaterialParams {
            color: [0.0, 0.0, 0.0, 1.0],
            ..Default::default()
        },
        cast_shadows: false,
        receive_shadows: false,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }
}

struct Setup {
    camera: Camera,
    environment: Environment,
    meshes: Vec<MeshDraw>,
    post: PostSettings,
}

impl Setup {
    fn new(glows: Vec<GlowSource>) -> Self {
        let mut environment = Environment::default();
        environment.settings.environment_lighting.intensity = 0.0;
        environment.glows = glows;
        Self {
            camera: camera(),
            environment,
            meshes: Vec::new(),
            post: PostSettings {
                bloom_intensity: 0.0,
                ..Default::default()
            },
        }
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: false,
            background: [0.0; 3],
            environment: &self.environment,
            post: self.post,
            lines: &[],
            overlay: &[],
            meshes: &self.meshes,
            particles: &[],
            terrains: &[],
            water: &[],
            shaded: true,
            lights: &[],
        }
    }
}

fn render(device: &wgpu::Device, queue: &wgpu::Queue, r: &mut Renderer, s: &Setup) -> Vec<u8> {
    let target = r.create_target(device, W, H);
    r.render(device, queue, &target, &s.frame());
    let image = read_back(device, queue, &target);
    // `MISTY_GLOW_TEST_DIR=<dir>` saves every image for a look.
    if let Some(dir) = std::env::var_os("MISTY_GLOW_TEST_DIR") {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::path::Path::new(&dir).join(format!("glow_{n:02}.png"));
        image::RgbaImage::from_raw(W, H, image.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    image
}

/// Mean brightness (0..255) of a pixel rectangle.
fn mean(image: &[u8], x: std::ops::Range<u32>, y: std::ops::Range<u32>) -> f32 {
    let (mut sum, mut n) = (0.0, 0.0);
    for py in y {
        for px in x.clone() {
            let i = ((py * W + px) * 4) as usize;
            sum += (image[i] as f32 + image[i + 1] as f32 + image[i + 2] as f32) / 3.0;
            n += 1.0;
        }
    }
    sum / n
}

/// Pixels brighter than `level` (the size of the glow).
fn lit_pixels(image: &[u8], level: u8) -> usize {
    image.chunks(4).filter(|p| p[0].max(p[1]) > level).count()
}

const CENTER_X: std::ops::Range<u32> = W / 2 - 4..W / 2 + 4;
const CENTER_Y: std::ops::Range<u32> = H / 2 - 4..H / 2 + 4;

#[test]
fn halo_glows_and_fades_behind_an_occluder() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    let mut s = Setup::new(vec![lamp(Vec3::ZERO)]);
    let lit = render(&device, &queue, &mut renderer, &s);
    let centre = mean(&lit, CENTER_X, CENTER_Y);
    let corner = mean(&lit, 0..20, 0..20);
    eprintln!("centre {centre:.1}, corner {corner:.1}");
    assert!(centre > 120.0, "halo shows: {centre}");
    assert!(corner < 1.0, "only around the light: {corner}");

    // A black wall between the camera and the light hides it completely.
    s.meshes = vec![wall(Vec3::new(0.0, 0.0, -4.0), Vec3::new(8.0, 8.0, 0.2))];
    let hidden = render(&device, &queue, &mut renderer, &s);
    // The wall's own shading is flat: no glow bump left at the light.
    let behind = mean(&hidden, CENTER_X, CENTER_Y);
    let wall_only = mean(&hidden, W / 2 + 60..W / 2 + 68, CENTER_Y);
    assert!(
        (behind - wall_only).abs() < 2.0,
        "occluded: {behind} vs {wall_only}"
    );

    // A wall edge right at the light: partly covered, partly visible (a smooth fade).
    s.meshes = vec![wall(Vec3::new(-4.0, 0.0, -4.0), Vec3::new(8.0, 8.0, 0.2))];
    let half = render(&device, &queue, &mut renderer, &s);
    let right = mean(&half, W / 2 + 6..W / 2 + 20, CENTER_Y);
    let full_right = mean(&lit, W / 2 + 6..W / 2 + 20, CENTER_Y);
    eprintln!("half-covered {right:.1} of {full_right:.1}");
    assert!(
        right > 2.0 && right < full_right * 0.85,
        "{right} vs {full_right}"
    );

    // The light's own lamp shade (just around it) does not hide it.
    s.meshes = vec![wall(Vec3::new(0.0, 0.0, -0.2), Vec3::new(0.6, 0.6, 0.1))];
    let shade = render(&device, &queue, &mut renderer, &s);
    let around = mean(&shade, W / 2 + 25..W / 2 + 35, CENTER_Y);
    assert!(around > mean(&lit, W / 2 + 25..W / 2 + 35, CENTER_Y) * 0.6);
}

#[test]
fn fog_spreads_the_halo() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    let mut s = Setup::new(vec![lamp(Vec3::ZERO)]);
    let clear = render(&device, &queue, &mut renderer, &s);
    s.post.fog = FogSettings::from_override(&FogOverride {
        active: true,
        attenuation_distance: 12.0,
        base_height: 50.0,
        max_fog_distance: 64.0,
        // Analytic fog: only the halo's own response is compared (no lit froxels).
        volumetric: false,
        albedo: [0.0; 3],
        ..Default::default()
    });
    let foggy = render(&device, &queue, &mut renderer, &s);
    let (a, b) = (lit_pixels(&clear, 30), lit_pixels(&foggy, 30));
    eprintln!("glow area clear {a}, foggy {b}");
    assert!(b as f32 > a as f32 * 1.5, "{a} → {b}");
}

#[test]
fn lens_flare_draws_ghosts_across_the_centre() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut renderer = Renderer::new(&device);
    let mut light = lamp(Vec3::new(-3.0, 1.5, 0.0));
    light.halo = None;
    light.intensity = 6.0;
    light.flare = Some(LensFlare::default());
    let s = Setup::new(vec![light]);
    let image = render(&device, &queue, &mut renderer, &s);
    // The light is up-left; the ghosts reach the opposite (down-right) quadrant.
    let opposite = mean(&image, W / 2..W, H / 2..H);
    assert!(opposite > 0.3, "ghosts: {opposite}");
    // Quality Low turns flares off.
    renderer.set_quality(&device, &crate::quality::QualityLevel::low());
    let off = render(&device, &queue, &mut renderer, &s);
    assert!(mean(&off, 0..W, 0..H) < 0.05);
}

/// GPU time of the glow pass at 1080p with many halos and a flare:
/// `cargo test --release -p misty_engine glow_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: GPU time of halos and flares at 1080p"]
fn glow_benchmark() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut lamps: Vec<GlowSource> = (0..64)
        .map(|i| {
            lamp(Vec3::new(
                (i % 8) as f32 * 2.0 - 7.0,
                (i / 8) as f32 - 3.5,
                4.0,
            ))
        })
        .collect();
    lamps[0].flare = Some(LensFlare {
        ghosts: 8,
        ..Default::default()
    });
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1920, 1080);
    for (name, glows) in [("no glows", Vec::new()), ("64 halos + 1 flare", lamps)] {
        let s = Setup::new(glows);
        let mut times = Vec::new();
        for i in 0..80 {
            renderer.render(&device, &queue, &target, &s.frame());
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 20
                && let Some(ms) = renderer.gpu_ms()
            {
                times.push(ms);
            }
        }
        times.sort_by(f32::total_cmp);
        let median = times.get(times.len() / 2).copied().unwrap_or(f32::NAN);
        eprintln!("{name:>22}: {median:.3} ms GPU");
    }
}
