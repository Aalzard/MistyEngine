//! Headless GPU checks of the sharp sun shafts (skipped without an adapter): a small hole in a
//! roof gives a bright narrow beam with dark fog beside it, the sun's brightness matches the
//! froxel path, and Off (in the override or the Quality level) is the froxel-only image. Plus
//! an ignored 1080p GPU-time benchmark.

use glam::Vec3;

use super::gpu_tests::{Scene, cube, fog, light};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{Environment, FogSettings, Renderer};
use crate::scene::{LightType, Shadows, SunShaftsQuality};

const W: u32 = 960;
const H: u32 = 540;

fn render(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    renderer: &mut Renderer,
    scene: &Scene,
    frames: usize,
) -> Vec<u8> {
    let target = renderer.create_target(device, W, H);
    for _ in 0..frames {
        renderer.render(device, queue, &target, &scene.frame());
    }
    let image = read_back(device, queue, &target);
    // `MISTY_FOG_TEST_DIR=<dir>` saves every image for a look.
    if let Some(dir) = std::env::var_os("MISTY_FOG_TEST_DIR") {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::path::Path::new(&dir).join(format!("sharp_{n:02}.png"));
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

fn with_quality(scene: &Scene, q: SunShaftsQuality) -> FogSettings {
    let mut f = scene.post.fog;
    f.volume.sun_shafts_quality = q;
    f
}

/// A roof over the whole view with a 25 cm square hole 27 m ahead, the sun straight above:
/// one thin vertical beam of light through dark (shadowed) fog.
fn roof_with_hole() -> Scene {
    let (y, t, h) = (6.0, 0.5, 0.125);
    let (z0, z1, hz) = (-15.0, 80.0, 15.0);
    let span = |a: f32, b: f32| ((a + b) * 0.5, (b - a).abs());
    let (lx, lw) = span(-30.0, -h);
    let (rx, rw) = span(h, 30.0);
    let (fz, fw) = span(z0, hz - h);
    let (bz, bw) = span(hz + h, z1);
    let (mz, mw) = span(z0, z1);
    let meshes = vec![
        cube(Vec3::new(lx, y, mz), Vec3::new(lw, t, mw)),
        cube(Vec3::new(rx, y, mz), Vec3::new(rw, t, mw)),
        cube(Vec3::new(0.0, y, fz), Vec3::new(2.0 * h, t, fw)),
        cube(Vec3::new(0.0, y, bz), Vec3::new(2.0 * h, t, bw)),
    ];
    let sun = light(LightType::Directional, Vec3::ZERO, Vec3::NEG_Y, 60.0);
    let mut fog = fog(true);
    fog.volume.attenuation_distance = 8.0;
    // Temporal accumulation on, as in a game: one frame of a 1.6 m step crossing a 25 cm beam
    // is a dotted line.
    fog.volume.denoising = true;
    Scene::dark(meshes, vec![sun], fog)
}

#[test]
fn hole_in_a_roof_gives_a_sharp_bright_beam() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut scene = roof_with_hole();
    let mut renderer = Renderer::new(&device);
    let rows = H / 2 - 40..H / 2 + 60;
    let c = W / 2;
    // (beam, fog just beside it, columns at least half as bright as the beam).
    let measure = |image: &[u8]| {
        let beam = mean(image, c - 2..c + 2, rows.clone());
        let side = 0.5
            * (mean(image, c - 14..c - 8, rows.clone()) + mean(image, c + 8..c + 14, rows.clone()));
        let width = (c - 40..c + 40)
            .filter(|x| mean(image, *x..*x + 1, rows.clone()) > 0.5 * beam)
            .count();
        (beam, side, width)
    };
    let sharp = render(&device, &queue, &mut renderer, &scene, 24);
    scene.post.fog = with_quality(&scene, SunShaftsQuality::Off);
    let froxel = render(&device, &queue, &mut renderer, &scene, 24);
    let (beam, side, width) = measure(&sharp);
    let (f_beam, f_side, f_width) = measure(&froxel);
    eprintln!("sharp: beam {beam:.1}, side {side:.1}, {width} px");
    eprintln!("froxels: beam {f_beam:.1}, side {f_side:.1}, {f_width} px");
    assert!(beam > 25.0, "the beam shows: {beam}");
    assert!(
        beam > 4.0 * side.max(1.0),
        "sharp contrast: {beam} vs {side}"
    );
    // The froxel grid (7.5 px per cell here) blurs the beam into its neighbours.
    assert!(
        2 * width < f_width,
        "sharp {width} px vs froxels {f_width} px"
    );
}

/// Unshadowed uniform fog lit by the sun: marching it per pixel gives the froxel brightness.
#[test]
fn sharp_shafts_match_the_froxel_brightness() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut sun = light(
        LightType::Directional,
        Vec3::ZERO,
        Vec3::new(0.3, -1.0, 0.6),
        1.5,
    );
    sun.shadows = Shadows::None;
    let ground = cube(Vec3::new(0.0, -0.5, 20.0), Vec3::new(80.0, 1.0, 80.0));
    let mut scene = Scene::dark(vec![ground], vec![sun], fog(true));
    scene.post.fog.volume.attenuation_distance = 40.0;
    scene.environment = Environment::default();
    let mut renderer = Renderer::new(&device);
    let sharp = render(&device, &queue, &mut renderer, &scene, 1);
    scene.post.fog = with_quality(&scene, SunShaftsQuality::Off);
    let froxel = render(&device, &queue, &mut renderer, &scene, 1);
    let all = |i: &[u8]| mean(i, 0..W, 0..H);
    let (s, f) = (all(&sharp), all(&froxel));
    eprintln!("sharp {s:.1}, froxel {f:.1}");
    assert!(f > 30.0);
    assert!((s - f).abs() < 0.06 * f, "sharp {s} vs froxel {f}");
}

/// Off in the override and Off in the Quality level both give the froxel-only image, which
/// is the image from before sharp shafts existed (the froxels light the sun as they did).
#[test]
fn sharp_shafts_off_is_the_froxel_image() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut scene = roof_with_hole();
    scene.environment = Environment::default();
    let mut renderer = Renderer::new(&device);
    let sharp = render(&device, &queue, &mut renderer, &scene, 2);
    scene.post.fog = with_quality(&scene, SunShaftsQuality::Off);
    let off = render(&device, &queue, &mut renderer, &scene, 2);
    assert!(off != sharp, "sharp shafts change the image");
    let mut low = crate::quality::QualityLevel::high();
    low.sun_shafts = crate::quality::Tier::Off;
    let mut tiered = Renderer::new(&device);
    tiered.set_quality(&device, &low);
    scene.post.fog = with_quality(&scene, SunShaftsQuality::Sharp);
    let tier_off = render(&device, &queue, &mut tiered, &scene, 2);
    let differing = off
        .chunks(4)
        .zip(tier_off.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "{differing} pixels differ");
}

/// `cargo test --release -p misty_engine sharp_shafts_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: GPU time of the sharp sun shafts at 1080p"]
fn sharp_shafts_benchmark() {
    use crate::quality::{QualityLevel, Tier};
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) = crate::scene::Scene::load(&root.join("Assets/Scenes/VolumetricFogDemo.scene"))
        .expect("VolumetricFogDemo.scene (generate it with write_volumetric_fog_demo)");
    let materials = crate::assets::AssetCache::open(&root);
    let view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
    let mut renderer = Renderer::with_queue(&device, &queue);
    let target = renderer.create_target(&device, 1920, 1080);
    let configs = [Tier::Off, Tier::Low, Tier::Medium, Tier::High]
        .into_iter()
        .flat_map(|t| [(t, true), (t, false)]);
    for (tier, noise) in configs {
        let mut level = QualityLevel::high();
        level.sun_shafts = tier;
        renderer.set_quality(&device, &level);
        let mut frame = view.frame();
        if !noise {
            frame.post.fog.volume.noise_intensity = 0.0;
        }
        let mut total = 0.0;
        let mut n = 0;
        for i in 0..120 {
            renderer.render(&device, &queue, &target, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 30
                && let Some(ms) = renderer.gpu_ms()
            {
                total += ms;
                n += 1;
            }
        }
        println!(
            "sun shafts {}, fog noise {}: GPU {:.3} ms per frame ({n} samples)",
            tier.label(),
            if noise { "on" } else { "off" },
            total / n.max(1) as f32
        );
    }
}
