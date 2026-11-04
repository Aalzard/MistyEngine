//! Headless GPU checks of the fog (skipped without an adapter): shadowed shafts, a visible
//! spot cone, local volumes in the density view, analytic vs froxel agreement, sun shafts,
//! and fog off leaving the image untouched. Plus an ignored GPU-time benchmark.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, FogDebug, FogSettings, Frame, LightData, LocalFog, MaterialParams, MeshDraw,
    PostSettings, Renderer, VolumetricLight,
};
use crate::scene::{FogOverride, FogQuality, FogVolume, LightType, PrimitiveMesh, Shadows};

const W: u32 = 320;
const H: u32 = 180;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 2.0, -12.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 100.0,
    }
}

fn light(kind: LightType, position: Vec3, direction: Vec3, intensity: f32) -> LightData {
    LightData {
        kind,
        position,
        direction: direction.normalize(),
        color: [1.0; 3],
        intensity,
        range: 20.0,
        spot_angle: 30.0,
        shadows: Shadows::Hard,
        shadow: Default::default(),
        volumetric: VolumetricLight::default(),
    }
}

fn cube(center: Vec3, size: Vec3) -> MeshDraw {
    MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::from_scale_rotation_translation(size, Quat::IDENTITY, center),
        material: MaterialParams::default(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
    }
}

/// Thick uniform fog (base height far above everything) reaching 64 m.
fn fog(volumetric: bool) -> FogSettings {
    FogSettings::from_override(&FogOverride {
        active: true,
        max_fog_distance: 64.0,
        volumetric_distance: 64.0,
        attenuation_distance: 20.0,
        base_height: 50.0,
        maximum_height: 100.0,
        anisotropy: 0.0,
        volumetric,
        quality: FogQuality::Medium,
        denoising: false,
        ..Default::default()
    })
}

/// Black background, no ambient light: only light scattered by the fog shows.
fn dark_environment() -> Environment {
    let mut e = Environment::default();
    e.settings.environment_lighting.intensity = 0.0;
    e
}

struct Scene {
    meshes: Vec<MeshDraw>,
    lights: Vec<LightData>,
    environment: Environment,
    post: PostSettings,
    skybox: bool,
    camera: Camera,
}

impl Scene {
    fn dark(meshes: Vec<MeshDraw>, lights: Vec<LightData>, fog: FogSettings) -> Self {
        Self {
            meshes,
            lights,
            environment: dark_environment(),
            post: PostSettings {
                fog,
                bloom_intensity: 0.0,
                ..Default::default()
            },
            skybox: false,
            camera: camera(),
        }
    }

    fn frame(&self) -> Frame<'_> {
        Frame {
            camera: &self.camera,
            grid: None,
            skybox: self.skybox,
            background: [0.0; 3],
            environment: &self.environment,
            post: self.post,
            lines: &[],
            overlay: &[],
            meshes: &self.meshes,
            particles: &[],
            terrains: &[],
            shaded: true,
            lights: &self.lights,
        }
    }
}

/// Render `frames` times into one target and read the last image back.
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
        let path = std::path::Path::new(&dir).join(format!("fog_{n:02}.png"));
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

/// A roof over the left half of the view, the sun straight above: fog under the roof is in
/// shadow, fog to the right is lit.
#[test]
fn shadowed_fog_is_darker_than_lit_fog() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let roof = cube(Vec3::new(-10.5, 6.0, 20.0), Vec3::new(20.0, 0.5, 70.0));
    let sun = light(LightType::Directional, Vec3::ZERO, Vec3::NEG_Y, 2.0);
    let mut renderer = Renderer::new(&device);
    let mut scene = Scene::dark(vec![roof], vec![sun], fog(true));
    let lit = render(&device, &queue, &mut renderer, &scene, 1);
    let rows = H / 2 - 8..H / 2 + 8;
    let (left, right) = (
        mean(&lit, 30..110, rows.clone()),
        mean(&lit, 210..290, rows.clone()),
    );
    eprintln!("shadowed {left:.1}, lit {right:.1}");
    assert!(right > 20.0, "lit fog shows: {right}");
    assert!(right > left * 1.6, "shafts: shadowed {left} vs lit {right}");

    // Shadow dimmer 0: the light ignores its shadows in the fog, both sides match.
    scene.lights[0].volumetric.shadow_dimmer = 0.0;
    let flat = render(&device, &queue, &mut renderer, &scene, 1);
    let (l, r) = (
        mean(&flat, 30..110, rows.clone()),
        mean(&flat, 210..290, rows.clone()),
    );
    assert!((l - r).abs() < 0.1 * r, "{l} vs {r}");
    // Multiplier 0: no light in the fog at all.
    scene.lights[0].volumetric.multiplier = 0.0;
    let none = render(&device, &queue, &mut renderer, &scene, 1);
    assert!(mean(&none, 0..W, rows) < 1.0);
}

#[test]
fn spot_light_cone_is_visible_in_fog() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let spot = light(LightType::Spot, Vec3::new(0.0, 7.0, 8.0), Vec3::NEG_Y, 6.0);
    let mut renderer = Renderer::new(&device);
    let scene = Scene::dark(Vec::new(), vec![spot], fog(true));
    let image = render(&device, &queue, &mut renderer, &scene, 1);
    let (cone, side) = (
        mean(&image, 150..170, 40..120),
        mean(&image, 10..40, 40..120),
    );
    eprintln!("cone {cone:.1}, side {side:.1}");
    assert!(
        cone > 15.0 && cone > side * 3.0,
        "cone {cone} vs side {side}"
    );
    // Without fog the cone has nothing to light.
    let mut off = scene;
    off.post.fog = FogSettings::default();
    let black = render(&device, &queue, &mut renderer, &off, 1);
    assert!(mean(&black, 0..W, 0..H) < 0.5);
}

#[test]
fn local_fog_volume_shows_in_the_density_view() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let mut fog = fog(true);
    // Practically no global fog: only the box.
    fog.volume.attenuation_distance = 1e6;
    fog.debug = FogDebug::Density;
    let mut scene = Scene::dark(Vec::new(), Vec::new(), fog);
    let volume = FogVolume {
        size: [6.0, 6.0, 6.0],
        fog_distance: 2.0,
        blend_distance: 0.5,
        ..Default::default()
    };
    scene.environment.fog_volumes = vec![LocalFog::new(
        &volume,
        Mat4::from_translation(Vec3::new(-5.0, 2.0, 4.0)),
    )];
    let mut renderer = Renderer::new(&device);
    let image = render(&device, &queue, &mut renderer, &scene, 1);
    let rows = H / 2 - 10..H / 2 + 10;
    let (inside, outside) = (
        mean(&image, 40..90, rows.clone()),
        mean(&image, 230..300, rows),
    );
    eprintln!("box {inside:.1}, outside {outside:.1}");
    assert!(inside > 150.0 && outside < 5.0, "{inside} vs {outside}");
}

/// Unshadowed uniform fog lit by the sun and the sky: the froxel volume and the analytic fog
/// agree, and temporal accumulation converges to the same result.
#[test]
fn froxel_and_analytic_fog_agree() {
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
    // A ground plane gives every distance up to the horizon.
    let ground = cube(Vec3::new(0.0, -0.5, 20.0), Vec3::new(80.0, 1.0, 80.0));
    let mut scene = Scene::dark(vec![ground], vec![sun], fog(true));
    scene.post.fog.volume.attenuation_distance = 40.0;
    scene.environment = Environment::default();
    let mut renderer = Renderer::new(&device);
    let froxel = render(&device, &queue, &mut renderer, &scene, 1);
    scene.post.fog.volume.denoising = true;
    let temporal = render(&device, &queue, &mut renderer, &scene, 24);
    scene.post.fog.volume.volumetric = false;
    let analytic = render(&device, &queue, &mut renderer, &scene, 1);
    let all = |i: &[u8]| mean(i, 0..W, 0..H);
    let (f, t, a) = (all(&froxel), all(&temporal), all(&analytic));
    eprintln!("froxel {f:.1}, temporal {t:.1}, analytic {a:.1}");
    assert!(a > 30.0);
    assert!((f - a).abs() < 0.08 * a, "froxel {f} vs analytic {a}");
    assert!((t - f).abs() < 0.05 * f, "temporal {t} vs froxel {f}");
}

/// Fog and shafts off (even with other values set and fog volumes present) give exactly the
/// image of a renderer that never had fog, also after rendering with fog.
#[test]
fn fog_off_leaves_the_image_unchanged() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let meshes = vec![
        cube(Vec3::new(0.0, -0.5, 10.0), Vec3::new(30.0, 1.0, 30.0)),
        cube(Vec3::new(-2.0, 1.0, 6.0), Vec3::splat(2.0)),
        cube(Vec3::new(3.0, 1.5, 12.0), Vec3::new(1.0, 3.0, 1.0)),
    ];
    let lights = vec![
        light(
            LightType::Directional,
            Vec3::ZERO,
            Vec3::new(0.4, -1.0, 0.5),
            1.2,
        ),
        light(LightType::Spot, Vec3::new(0.0, 5.0, 8.0), Vec3::NEG_Y, 3.0),
        light(LightType::Point, Vec3::new(3.0, 2.0, 4.0), Vec3::Z, 2.0),
    ];
    let mut plain = Scene::dark(meshes, lights, FogSettings::default());
    plain.environment = Environment::default();
    plain.skybox = true;
    plain.post = PostSettings::default();
    let mut fresh = Renderer::new(&device);
    let before = render(&device, &queue, &mut fresh, &plain, 1);

    let mut renderer = Renderer::new(&device);
    let mut foggy = Scene {
        meshes: plain.meshes.clone(),
        lights: plain.lights.clone(),
        environment: plain.environment.clone(),
        post: plain.post,
        skybox: true,
        camera: camera(),
    };
    foggy.post.fog = fog(true);
    foggy.post.sun_shafts.active = true;
    let fogged = render(&device, &queue, &mut renderer, &foggy, 2);
    assert!(fogged != before, "fog changes the image");
    // Inactive overrides with non-default values and a fog volume in the scene.
    foggy.post.fog.volume.active = false;
    foggy.post.sun_shafts.active = false;
    foggy.environment.fog_volumes = vec![LocalFog::new(&FogVolume::default(), Mat4::IDENTITY)];
    let after = render(&device, &queue, &mut renderer, &foggy, 1);
    let differing = before
        .chunks(4)
        .zip(after.chunks(4))
        .filter(|(a, b)| a != b)
        .count();
    assert_eq!(differing, 0, "{differing} pixels differ with fog off");
}

#[test]
fn sun_shafts_brighten_around_the_sun() {
    let Some((device, queue)) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    // Low sun straight ahead, a column partly in front of it.
    let sun = light(
        LightType::Directional,
        Vec3::ZERO,
        Vec3::new(0.0, -0.2, -1.0),
        2.0,
    );
    let column = cube(Vec3::new(1.0, 3.0, 10.0), Vec3::new(1.0, 8.0, 1.0));
    let mut scene = Scene::dark(vec![column], vec![sun], FogSettings::default());
    scene.environment = Environment::new(Default::default(), &scene.lights);
    scene.skybox = true;
    let mut renderer = Renderer::new(&device);
    let off = render(&device, &queue, &mut renderer, &scene, 1);
    scene.post.sun_shafts.active = true;
    scene.post.sun_shafts.intensity = 2.0;
    let on = render(&device, &queue, &mut renderer, &scene, 1);
    let all = |i: &[u8]| mean(i, 0..W, 0..H);
    eprintln!("off {:.1}, on {:.1}", all(&off), all(&on));
    assert!(all(&on) > all(&off) + 1.0);
    // The sun behind the camera: nothing to add.
    scene.camera.rotation = Quat::from_rotation_y(std::f32::consts::PI);
    let away = render(&device, &queue, &mut renderer, &scene, 1);
    scene.post.sun_shafts.active = false;
    let away_off = render(&device, &queue, &mut renderer, &scene, 1);
    assert!(away == away_off);
}

/// `cargo test --release -p misty_engine fog_benchmark -- --ignored --nocapture`
#[test]
#[ignore = "benchmark: GPU time of the fog at 1080p"]
fn fog_benchmark() {
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
    let configs: [(&str, Option<FogQuality>); 4] = [
        ("fog off", None),
        ("Low", Some(FogQuality::Low)),
        ("Medium", Some(FogQuality::Medium)),
        ("High", Some(FogQuality::High)),
    ];
    for (name, quality) in configs {
        let mut frame = view.frame();
        match quality {
            Some(q) => frame.post.fog.volume.quality = q,
            None => frame.post.fog = FogSettings::default(),
        }
        let mut total = 0.0;
        let mut n = 0;
        for i in 0..80 {
            renderer.render(&device, &queue, &target, &frame);
            let _ = device.poll(wgpu::PollType::wait_indefinitely());
            if i >= 20
                && let Some(ms) = renderer.gpu_ms()
            {
                total += ms;
                n += 1;
            }
        }
        println!(
            "{name}: GPU {:.3} ms per frame ({n} samples)",
            total / n.max(1) as f32
        );
    }
}
