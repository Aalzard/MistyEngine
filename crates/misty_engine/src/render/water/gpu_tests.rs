//! Headless GPU checks of fog over water (skipped without an adapter): distant water under
//! dense fog comes out as the fog, near water stays mostly clear, for the Lighting window's fog
//! and the volumetric fog, on a flat test ocean and on WaterDemo.

use glam::{Mat4, Quat, Vec3};

use crate::camera::{Camera, Projection};
use crate::render::gpu_tests::{headless, read_back};
use crate::render::{
    Environment, FogSettings, Frame, MaterialParams, MeshDraw, PostSettings, Renderer,
};
use crate::scene::render_settings::FogMode;
use crate::scene::{FogOverride, FogQuality, PrimitiveMesh, WaterSurface};
use crate::water::WaterDraw;

const W: u32 = 320;
const H: u32 = 180;

/// Rows (180 high, 50° fov, eye 4 m up looking level) just below the horizon (~150-300 m
/// away) and at the bottom of the view (~10 m away).
const FAR_ROWS: std::ops::Range<u32> = 92..96;
const NEAR_ROWS: std::ops::Range<u32> = 168..178;

fn camera() -> Camera {
    Camera {
        position: Vec3::new(0.0, 4.0, 0.0),
        rotation: Quat::IDENTITY,
        projection: Projection::Perspective { fov_y: 50.0 },
        near: 0.1,
        far: 2000.0,
    }
}

fn calm_ocean() -> WaterDraw {
    let mut s = WaterSurface::default();
    s.settings.crest_foam = 0.0;
    s.settings.screen_space_reflections = false;
    WaterDraw::new(&s, Mat4::IDENTITY, 0.0)
}

fn classic_fog(density: f32) -> Environment {
    let mut e = Environment::default();
    let f = &mut e.settings.fog;
    f.enabled = true;
    f.mode = FogMode::Exponential;
    f.density = density;
    f.color = [0.9, 0.55, 0.3];
    e
}

/// Uniform volumetric fog (base height above the view) with `attenuation` metres.
fn volume_fog(attenuation: f32) -> FogSettings {
    FogSettings::from_override(&FogOverride {
        active: true,
        max_fog_distance: 1000.0,
        volumetric_distance: 64.0,
        attenuation_distance: attenuation,
        base_height: 50.0,
        maximum_height: 100.0,
        volumetric: true,
        quality: FogQuality::Medium,
        denoising: false,
        ..Default::default()
    })
}

type Gpu = (wgpu::Device, wgpu::Queue);

fn render(
    (device, queue): &Gpu,
    env: &Environment,
    fog: FogSettings,
    camera: &Camera,
    water: &[WaterDraw],
    meshes: &[MeshDraw],
) -> Vec<u8> {
    let mut renderer = Renderer::new(device);
    let frame = Frame {
        camera,
        grid: None,
        skybox: true,
        background: [0.0; 3],
        environment: env,
        post: PostSettings {
            fog,
            bloom_intensity: 0.0,
            ..Default::default()
        },
        lines: &[],
        overlay: &[],
        meshes,
        particles: &[],
        terrains: &[],
        water,
        shaded: true,
        lights: &[],
    };
    let target = renderer.create_target(device, W, H);
    // The same wave time for every render: wall-clock time moves the waves between the
    // clear / fogged / reference images, by more when the machine is busy (parallel tests).
    crate::clock::set_play_time(0.0);
    renderer.render(device, queue, &target, &frame);
    let image = read_back(device, queue, &target);
    if let Some(dir) = std::env::var_os("MISTY_FOG_TEST_DIR") {
        static COUNT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = COUNT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let path = std::path::Path::new(&dir).join(format!("water_fog_{n:02}.png"));
        image::RgbaImage::from_raw(W, H, image.clone())
            .expect("size")
            .save(path)
            .expect("png");
    }
    image
}

/// Mean RGB (0..255) of a band of rows (middle half of the width).
fn band(image: &[u8], rows: std::ops::Range<u32>) -> Vec3 {
    let (mut sum, mut n) = (Vec3::ZERO, 0.0);
    for y in rows {
        for x in W / 4..3 * W / 4 {
            let i = ((y * W + x) * 4) as usize;
            sum += Vec3::new(image[i] as f32, image[i + 1] as f32, image[i + 2] as f32);
            n += 1.0;
        }
    }
    sum / n
}

/// Largest channel difference between two colours.
fn diff(a: Vec3, b: Vec3) -> f32 {
    (a - b).abs().max_element()
}

/// Far water matches the fully fogged reference, near water does not.
fn check(name: &str, fogged: &[u8], reference: &[u8], clear: &[u8]) {
    let far = diff(band(fogged, FAR_ROWS), band(reference, FAR_ROWS));
    let near = diff(band(fogged, NEAR_ROWS), band(reference, NEAR_ROWS));
    let near_clear = diff(band(fogged, NEAR_ROWS), band(clear, NEAR_ROWS));
    let far_clear = diff(band(clear, FAR_ROWS), band(reference, FAR_ROWS));
    eprintln!(
        "{name}: far {far:.1}, near {near:.1}, near vs clear {near_clear:.1}, far clear {far_clear:.1}"
    );
    assert!(
        far_clear > 20.0,
        "{name}: the test needs fog to change far water"
    );
    assert!(far < 6.0, "{name}: far water is the fog colour ({far})");
    assert!(
        near > 4.0 * far.max(1.0),
        "{name}: near water is not all fog ({near})"
    );
    assert!(near_clear < near, "{name}: near water stays mostly clear");
}

#[test]
fn distant_water_fades_into_lighting_fog() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let water = [calm_ocean()];
    let cam = camera();
    let off = FogSettings::default();
    let clear = render(&gpu, &Environment::default(), off, &cam, &water, &[]);
    let fogged = render(&gpu, &classic_fog(0.03), off, &cam, &water, &[]);
    let reference = render(&gpu, &classic_fog(5.0), off, &cam, &water, &[]);
    check("lighting fog", &fogged, &reference, &clear);
}

#[test]
fn distant_water_fades_into_volumetric_fog() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let water = [calm_ocean()];
    let cam = camera();
    let env = Environment::default();
    let clear = render(&gpu, &env, FogSettings::default(), &cam, &water, &[]);
    let fogged = render(&gpu, &env, volume_fog(25.0), &cam, &water, &[]);
    let reference = render(&gpu, &env, volume_fog(0.05), &cam, &water, &[]);
    check("volumetric fog", &fogged, &reference, &clear);
}

/// The camera under the ocean: the underwater pass is the fog, air fog must not change it.
#[test]
fn air_fog_is_off_under_water() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let water = [calm_ocean()];
    let mut cam = camera();
    cam.position.y = -3.0;
    // A rock 6 m ahead, under water with the camera.
    let rock = [MeshDraw {
        mesh: PrimitiveMesh::Cube.into(),
        model: Mat4::from_translation(Vec3::new(0.0, -3.0, 6.0)),
        material: MaterialParams::default(),
        cast_shadows: true,
        receive_shadows: true,
        outline: false,
        sorting: Default::default(),
        lod: None,
    }];
    let env = Environment::default();
    let off = FogSettings::default();
    let clear = render(&gpu, &env, off, &cam, &water, &rock);
    let lighting = render(&gpu, &classic_fog(0.3), off, &cam, &water, &rock);
    let volume = render(&gpu, &env, volume_fog(3.0), &cam, &water, &rock);
    assert!(clear == lighting, "Lighting fog applied under water");
    assert!(clear == volume, "volumetric fog applied under water");
}

/// WaterDemo looking out to sea with fog turned up: the horizon fades into it.
#[test]
fn water_demo_ocean_fades_into_fog() {
    let Some(gpu) = headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let (scene, _) = crate::scene::Scene::load(&root.join("Assets/Scenes/WaterDemo.scene"))
        .expect("WaterDemo.scene");
    let materials = crate::assets::AssetCache::open(&root);
    let view = crate::render_scene::GameView::new(&scene, &materials).expect("camera");
    // Away from the island, level, 4 m above the sea.
    let mut cam = view.camera;
    cam.position = Vec3::new(0.0, 4.0, -60.0);
    cam.rotation = Quat::from_rotation_y(std::f32::consts::PI);
    cam.projection = Projection::Perspective { fov_y: 50.0 };
    let mut env = view.environment.clone();
    let off = FogSettings::default();
    let clear = render(&gpu, &env, off, &cam, &view.water, &[]);
    let f = &mut env.settings.fog;
    f.enabled = true;
    f.mode = FogMode::Exponential;
    f.color = [0.8, 0.6, 0.5];
    f.density = 0.03;
    let fogged = render(&gpu, &env, off, &cam, &view.water, &[]);
    env.settings.fog.density = 5.0;
    let reference = render(&gpu, &env, off, &cam, &view.water, &[]);
    check("WaterDemo", &fogged, &reference, &clear);
}
