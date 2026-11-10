//! MoonDemo sample scene in `SampleProject`: a moonlit valley at night. Rolling meadows under
//! dark ridges, a ring of standing stones on a knoll casting moon shadows, a cabin with a warm
//! window, a few dead trees, a little ground haze and the stars. The sun is 24° below the
//! horizon; the moon (Manual, a big disc above the stones) lights the scene through the main
//! Directional Light. `MoonCycle.lua` runs the phases round (new → crescent → quarter →
//! gibbous → full → waning) every 40 seconds so the terminator, the earthshine and the
//! moonlight's strength can be watched. The terrain reuses TerrainDemo2's textures.
//! Regenerate with `cargo test --release -p misty_editor -- --ignored write_moon_demo`; the
//! other tests check the committed files and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::moon::MoonMode;
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, ScriptRef,
    Scripts, Shadows, Terrain, TerrainCollider, TonemapMode, Transform,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::{TerrainData, TerrainLayer, noise};

const DATA: &str = "Assets/Terrains/MoonDemo.terrain";
const SCENE: &str = "Assets/Scenes/MoonDemo.scene";
const SCRIPT: &str = "Assets/Scripts/MoonCycle.lua";
// Written by `sample_terrain2`.
const GRASS: &str = "Assets/Textures/Terrain/Meadow.png";
const SCREE: &str = "Assets/Textures/Terrain/Scree.png";
const ROCK: &str = "Assets/Textures/Terrain/Cliff.png";
// Existing sample materials.
const STONE: &str = "Assets/Materials/Ruin Stone.mat";
const WALL: &str = "Assets/Materials/Night Plaster.mat";
const ROOF: &str = "Assets/Materials/Night Brick.mat";
const BARK: &str = "Assets/Materials/Bark.mat";
const WINDOW: &str = "Assets/Materials/Lantern Glow.mat";

/// The files this generator writes and the GUIDs of their metas.
const METAS: [(&str, &str); 4] = [
    (DATA, "6d6f6f6e64656d6f7465727261696e31"),
    (SCENE, "6d6f6f6e64656d6f7363656e65303031"),
    (SCRIPT, "6d6f6f6e6379636c656c756173637231"),
    (NIGHT_SCENE, "6e69676874736b7964656d6f73636e31"),
];

/// The same valley looking north under a dark, moonless sky: the Milky Way, glowing stars that
/// twinkle, an animated aurora, meteors, a faint town glow.
const NIGHT_SCENE: &str = "Assets/Scenes/NightSkyDemo.scene";

const SIZE: Vec3 = Vec3::new(1600.0, 260.0, 1600.0);
const CORNER: Vec3 = Vec3::new(-800.0, 0.0, -800.0);

/// Where the moon hangs (Manual): south-east, above the stones.
const MOON_AZIMUTH: f32 = 150.0;
const MOON_ALTITUDE: f32 = 21.0;

const CYCLE_LUA: &str = r#"-- MoonCycle: runs the moon through its phases (new, waxing crescent, first quarter,
-- gibbous, full, waning) so you can watch the terminator, the earthshine on the dark side
-- and the moonlight dimming and brightening with it.
local MoonCycle = {
    period = 40,      -- seconds for a whole lunar month
    start = 0.12,     -- phase at Start (0 new, 0.25 first quarter, 0.5 full, 0.75 last quarter)
}

function MoonCycle:Start()
    RenderSettings.moonPhase = self.start
end

function MoonCycle:Update()
    local phase = self.start + Time.time / self.period
    RenderSettings.moonPhase = phase - math.floor(phase)
end

return MoonCycle
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Normalised height at local (x, z) metres: a valley opening towards the moon (south-east of
/// the camera) with a knoll for the stones and ridges around.
fn height(x: f32, z: f32) -> f32 {
    let (cx, cz) = (x - 800.0, z - 800.0);
    let r = (cx * cx + cz * cz).sqrt();
    let rim = ((r - 380.0) / 380.0).clamp(0.0, 1.0);
    let rolling = noise::fbm(x, z, 140.0, 4, 7) * 0.5 + 0.5;
    let ridges = noise::ridged(x, z, 420.0, 5, 13);
    // The knoll under the stones, 70 m south-east of the camera.
    let (kx, kz) = (cx - 40.0, cz + 60.0);
    let knoll = (-(kx * kx + kz * kz) / (2.0 * 26.0 * 26.0)).exp();
    (0.03 + 0.03 * rolling + knoll * 0.022 + rim * rim * (0.18 + 0.7 * ridges * ridges))
        .clamp(0.0, 1.0)
}

fn layer(name: &str, diffuse: &str, tile: f32, slope: [f32; 2], priority: i32) -> TerrainLayer {
    TerrainLayer {
        name: name.into(),
        diffuse: Some(diffuse.into()),
        tile_size: [tile, tile],
        rules: AutoPaintRules {
            enabled: true,
            slope,
            spread: 0.25,
            breakup: 0.6,
            breakup_scale: 40.0,
            priority,
            ..Default::default()
        },
        ..Default::default()
    }
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(257, SIZE, 256);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![
        layer("Meadow", GRASS, 9.0, [0.0, 22.0], 0),
        layer("Scree", SCREE, 7.0, [20.0, 34.0], 1),
        layer("Cliff", ROCK, 30.0, [34.0, 90.0], 2),
    ];
    d.rule_seed = 9;
    auto_paint::generate(&mut d);
    d
}

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Aces;
    v.bloom.active = true;
    v.bloom.intensity = 0.6;
    v.bloom.threshold = 0.9;
    // Night is dark: open the exposure like a night photo.
    v.color_adjustments.active = true;
    v.color_adjustments.post_exposure = 2.0;
    let f = &mut v.fog;
    f.active = true;
    f.max_fog_distance = 1200.0;
    f.attenuation_distance = 2500.0;
    f.base_height = 0.0;
    f.maximum_height = 40.0;
    f.anisotropy = 0.5;
    f.volumetric_distance = 120.0;
    v
}

fn mesh(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

fn at(p: Vec3, scale: Vec3, yaw: f32) -> Transform {
    let mut t = Transform::from_position(p).with_euler(Vec3::new(0.0, yaw, 0.0));
    t.scale = scale;
    t
}

fn stones(s: &mut Scene, d: &TerrainData) {
    let centre = (40.0, -60.0);
    for i in 0..9 {
        let a = i as f32 / 9.0 * std::f32::consts::TAU;
        let (x, z) = (centre.0 + a.cos() * 9.0, centre.1 + a.sin() * 9.0);
        let tall = 2.6 + (i % 3) as f32 * 0.5;
        let y = ground(d, x, z) + tall * 0.5 - 0.3;
        let t = at(
            Vec3::new(x, y, z),
            Vec3::new(1.1, tall, 0.6),
            -a.to_degrees() + 90.0,
        );
        mesh(
            s,
            &format!("Standing Stone {}", i + 1),
            PrimitiveMesh::Cube,
            t,
            STONE,
        );
    }
    let y = ground(d, centre.0, centre.1);
    mesh(
        s,
        "Altar Stone",
        PrimitiveMesh::Cube,
        at(
            Vec3::new(centre.0, y + 0.3, centre.1),
            Vec3::new(2.4, 0.7, 1.2),
            20.0,
        ),
        STONE,
    );
}

fn cabin(s: &mut Scene, d: &TerrainData) {
    const YAW: f32 = 188.0;
    let (x, z) = (14.0, -62.0);
    let y = ground(d, x, z);
    mesh(
        s,
        "Cabin",
        PrimitiveMesh::Cube,
        at(Vec3::new(x, y + 1.6, z), Vec3::new(6.0, 3.6, 4.5), YAW),
        WALL,
    );
    mesh(
        s,
        "Cabin Roof",
        PrimitiveMesh::Cube,
        at(Vec3::new(x, y + 3.8, z), Vec3::new(6.6, 0.8, 5.1), YAW),
        ROOF,
    );
    // The window faces the camera (towards -Z rotated by the cabin's yaw).
    let yaw = YAW.to_radians();
    let out = Vec3::new(-yaw.sin(), 0.0, -yaw.cos());
    let w = Vec3::new(x, y + 1.7, z) + out * 2.26;
    mesh(
        s,
        "Cabin Window",
        PrimitiveMesh::Cube,
        at(w, Vec3::new(1.2, 0.9, 0.05), YAW),
        WINDOW,
    );
    let lamp = s.spawn("Window Light", None);
    s.set_transform(lamp, Transform::from_position(w + out * 0.6));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.62, 0.3],
            intensity: 2.5,
            range: 9.0,
            shadows: Shadows::None,
            ..Default::default()
        },
    );
}

fn trees(s: &mut Scene, d: &TerrainData) {
    let spots = [
        (-48.0, -10.0),
        (-58.0, -52.0),
        (30.0, -34.0),
        (70.0, -30.0),
        (-8.0, -78.0),
    ];
    for (i, (x, z)) in spots.into_iter().enumerate() {
        let tall = 5.0 + (i % 2) as f32 * 2.0;
        let y = ground(d, x, z);
        let name = format!("Dead Tree {}", i + 1);
        mesh(
            s,
            &name,
            PrimitiveMesh::Cylinder,
            at(
                Vec3::new(x, y + tall * 0.5, z),
                Vec3::new(0.35, tall * 0.5, 0.35),
                0.0,
            ),
            BARK,
        );
        // Two bare branches.
        for (k, lean) in [(0, 38.0f32), (1, -46.0)] {
            let h = y + tall * (0.62 + 0.14 * k as f32);
            let mut t = Transform::from_position(Vec3::new(x, h, z)).with_euler(Vec3::new(
                0.0,
                60.0 * i as f32 + 120.0 * k as f32,
                lean,
            ));
            t.scale = Vec3::new(0.14, 1.3, 0.14);
            mesh(
                s,
                &format!("{name} Branch {}", k + 1),
                PrimitiveMesh::Cylinder,
                t,
                BARK,
            );
        }
    }
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("MoonDemo");
    let eye = (18.0, -22.0);
    if let Some(cam) = s.find_by_name("Main Camera") {
        let y = ground(d, eye.0, eye.1) + 2.2;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(eye.0, y, eye.1))
                .with_euler(Vec3::new(-7.0, 158.0, 0.0)),
        );
        let mut c = s
            .world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap();
        c.far = 3000.0;
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        // The sun 24° below the northern horizon: night. The moon takes over this light.
        let towards = misty_engine::moon::from_az_alt(340.0, -24.0);
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 50.0, 0.0))
                .with_euler(misty_engine::hdri::sun_euler(towards)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.color = [1.0, 0.95, 0.88];
        l.intensity = 1.6;
        l.shadows = Shadows::Soft;
    }
    let m = &mut s.render_settings.moon;
    m.mode = MoonMode::Manual;
    m.azimuth = MOON_AZIMUTH;
    m.altitude = MOON_ALTITUDE;
    m.phase = 0.12;
    m.size = 5.0;
    m.light_intensity = 0.45;
    let n = &mut s.render_settings.night_sky;
    n.star_brightness = 1.5;
    s.render_settings.skybox.atmosphere_thickness = 0.9;

    let terrain = s.spawn("Terrain", None);
    s.set_transform(terrain, Transform::from_position(CORNER));
    s.add(
        terrain,
        Terrain {
            terrain_data: DATA.into(),
            ..Default::default()
        },
    );
    s.add(terrain, TerrainCollider::default());
    stones(&mut s, d);
    cabin(&mut s, d);
    trees(&mut s, d);
    let runner = s.spawn("Moon Cycle", None);
    s.add(
        runner,
        Scripts {
            list: vec![ScriptRef {
                path: SCRIPT.into(),
                ..Default::default()
            }],
        },
    );
    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

/// NightSkyDemo: MoonDemo's valley looking north-north-east from the stones' knoll under a thin
/// crescent setting in the west: a dark sky with the summer Milky Way (Sky Time 20 h) falling
/// to the horizon, varied glowing stars that twinkle, a green and purple aurora over the
/// northern ridges, shooting stars, and a faint town glow.
fn night_sky_scene(d: &TerrainData) -> Scene {
    let mut s = demo_scene(d);
    s.name = "NightSkyDemo".into();
    if let Some(cam) = s.find_by_name("Main Camera") {
        let (x, z) = (40.0, -60.0);
        let y = ground(d, x, z) + 2.0;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(x, y, z)).with_euler(Vec3::new(-14.0, 12.0, 0.0)),
        );
        s.world
            .get::<&mut misty_engine::scene::Camera>(cam)
            .unwrap()
            .fov = 70.0;
    }
    // No phase cycling here.
    if let Some(e) = s.find_by_name("Moon Cycle") {
        s.despawn(e);
    }
    let m = &mut s.render_settings.moon;
    m.azimuth = 255.0;
    m.altitude = 9.0;
    m.phase = 0.07;
    m.size = 3.0;
    let n = &mut s.render_settings.night_sky;
    n.darkness = 0.35;
    n.star_brightness = 1.3;
    n.star_density = 0.8;
    n.star_magnitude_spread = 0.95;
    n.star_glow = 1.5;
    n.star_color_variation = 0.85;
    n.star_twinkle = 0.6;
    n.sky_time = 20.0;
    n.milky_way = 1.8;
    n.milky_way_core = 1.3;
    n.milky_way_dust = 1.1;
    n.light_pollution = 0.12;
    n.aurora = 0.8;
    n.aurora_azimuth = 5.0;
    n.aurora_distance = 520.0;
    n.aurora_curtains = 3;
    n.aurora_waviness = 1.2;
    n.aurora_pulse = 0.7;
    n.meteor_rate = 8.0;
    n.meteor_brightness = 1.2;
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_moon_demo() {
    let root = project();
    let d = terrain_data();
    d.save(&root.join(DATA)).unwrap();
    std::fs::write(root.join(SCRIPT), CYCLE_LUA).unwrap();
    demo_scene(&d).save(&root.join(SCENE)).unwrap();
    night_sky_scene(&d).save(&root.join(NIGHT_SCENE)).unwrap();
    // Metas for this generator's own files only.
    for (rel, guid) in METAS {
        let meta = root.join(format!("{rel}.meta"));
        if !meta.exists() {
            std::fs::write(meta, format!("{{\n  \"guid\": \"{guid}\"\n}}\n")).unwrap();
        }
    }
}

#[test]
fn moon_demo_matches_its_generator() {
    let root = project();
    for rel in [GRASS, SCREE, ROCK, STONE, WALL, ROOF, BARK, WINDOW] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.size, generated.size);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let lua = std::fs::read_to_string(root.join(SCRIPT)).unwrap();
    assert_eq!(lua.replace("\r\n", "\n"), CYCLE_LUA);
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(
        &scene.to_json(),
        &demo_scene(&generated).to_json(),
        "regenerate with write_moon_demo",
    );
    for (rel, _) in METAS {
        assert!(root.join(format!("{rel}.meta")).exists(), "{rel}.meta");
    }
    let (night, report) = Scene::load(&root.join(NIGHT_SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    crate::test_util::assert_json_close(
        &night.to_json(),
        &night_sky_scene(&generated).to_json(),
        "regenerate with write_moon_demo",
    );
    let n = &night.render_settings.night_sky;
    assert!(n.milky_way > 1.0 && n.aurora > 1.0 && n.meteor_rate > 0.0);
    // Night: the sun is down, the moon up in front of the camera and lighting the scene.
    let lights = misty_engine::render_scene::lights(&scene);
    let env = misty_engine::render_scene::environment(&scene, &lights);
    assert!(env.sun.unwrap().direction.y < -0.3);
    let moon = misty_engine::render::moon::moon_state(&env).unwrap();
    assert!(moon.direction.y > 0.3);
    let moonlit = misty_engine::render::moon::night_lights(&lights, &env).unwrap();
    let main = moonlit
        .iter()
        .find(|l| l.kind == LightType::Directional)
        .unwrap();
    assert!(
        (main.direction + moon.direction).length() < 1e-4,
        "lit from the moon"
    );
    let cam = misty_engine::render_scene::GameView::new(&scene, &NoMaterials)
        .unwrap()
        .camera;
    assert!(cam.forward().dot(moon.direction) > 0.8, "moon in view");
}

struct NoMaterials;
impl misty_engine::render_scene::MaterialSource for NoMaterials {
    fn params(&self, _: Option<&str>) -> misty_engine::render::MaterialParams {
        Default::default()
    }
}

/// Playing works: the script runs the phase round without errors.
#[test]
fn moon_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, ScriptHost};
    let root = project();
    let data = TerrainData::load(&root.join(DATA)).unwrap();
    let mut scene = demo_scene(&data);
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    let mut phases = Vec::new();
    for _ in 0..600 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
        let logs = host.take_logs();
        assert!(logs.is_empty(), "{logs:?}");
        phases.push(scene.render_settings.moon.phase);
    }
    // Ten seconds = a quarter of the cycle.
    let (first, last) = (phases[0], *phases.last().unwrap());
    assert!((last - first - 0.25).abs() < 0.01, "{first} → {last}");
}

// ---- screenshots ---------------------------------------------------------------------------

fn shots_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/screenshots/r27")
}

/// MoonDemo with its camera turned: `yaw` / `pitch` degrees, vertical field of view `fov`.
fn aimed(d: &TerrainData, yaw: f32, pitch: f32, fov: f32) -> Scene {
    let s = demo_scene(d);
    let cam = s.find_by_name("Main Camera").unwrap();
    let mut t = s.transform(cam);
    t = Transform::from_position(t.position).with_euler(Vec3::new(pitch, yaw, 0.0));
    let mut s = s;
    s.set_transform(cam, t);
    s.world
        .get::<&mut misty_engine::scene::Camera>(cam)
        .unwrap()
        .fov = fov;
    s
}

fn shoot(
    gpu: &(misty_engine::wgpu::Device, misty_engine::wgpu::Queue),
    assets: &misty_engine::assets::AssetCache,
    scene: &Scene,
    size: (u32, u32),
) -> misty_engine::image::RgbaImage {
    shoot_at(gpu, assets, scene, size, 12.0)
}

/// [`shoot`] with the night sky's clock fixed at `time` seconds (aurora, twinkle, meteors).
fn shoot_at(
    gpu: &(misty_engine::wgpu::Device, misty_engine::wgpu::Queue),
    assets: &misty_engine::assets::AssetCache,
    scene: &Scene,
    size: (u32, u32),
    time: f32,
) -> misty_engine::image::RgbaImage {
    let (device, queue) = gpu;
    let mut view = misty_engine::render_scene::GameView::new(scene, assets).unwrap();
    view.environment.grass_time = Some(time);
    let mut renderer = misty_engine::render::Renderer::new(device);
    renderer.set_quality(device, &misty_engine::quality::QualityLevel::high());
    let target = renderer.create_target(device, size.0, size.1);
    for _ in 0..48 {
        renderer.render(device, queue, &target, &view.frame());
    }
    let px = crate::project_thumbnail::read_pixels(device, queue, &target);
    misty_engine::image::RgbaImage::from_raw(size.0, size.1, px).unwrap()
}

fn save_jpeg(name: &str, img: &misty_engine::image::RgbaImage) {
    let dir = shots_dir();
    std::fs::create_dir_all(&dir).unwrap();
    let rgb = misty_engine::image::DynamicImage::ImageRgba8(img.clone()).into_rgb8();
    let mut file = std::fs::File::create(dir.join(format!("{name}.jpg"))).unwrap();
    misty_engine::image::codecs::jpeg::JpegEncoder::new_with_quality(&mut file, 90)
        .encode_image(&rgb)
        .unwrap();
    eprintln!("wrote docs/screenshots/r27/{name}.jpg");
}

/// Renders `docs/screenshots/r27/`: the moon close up at six phases (and a strip of them), the
/// moonlit valley under a full and a crescent moon, and twilight with a low rising moon.
/// `MISTY_MOON_SHOTS=phases,night,twilight` renders only some.
#[test]
#[ignore = "writes screenshots into docs/screenshots/r27; run on purpose"]
fn render_moon_screenshots() {
    let Some(gpu) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let assets = misty_engine::assets::AssetCache::open(&root);
    let d = TerrainData::load(&root.join(DATA)).unwrap();
    let only = std::env::var("MISTY_MOON_SHOTS").unwrap_or_default();
    let wants = |k: &str| only.is_empty() || only.split(',').any(|o| o.trim() == k);
    if wants("phases") {
        let phases = [
            ("waxing_crescent", 0.12),
            ("first_quarter", 0.25),
            ("waxing_gibbous", 0.38),
            ("full", 0.5),
            ("waning_gibbous", 0.63),
            ("last_quarter", 0.75),
        ];
        let tile = 360u32;
        let mut strip = misty_engine::image::RgbaImage::new(tile * phases.len() as u32, tile);
        for (i, (name, phase)) in phases.into_iter().enumerate() {
            // Telephoto straight at the moon.
            let mut s = aimed(&d, MOON_AZIMUTH, -MOON_ALTITUDE, 7.0);
            s.render_settings.moon.phase = phase;
            let img = shoot(&gpu, &assets, &s, (tile, tile));
            misty_engine::image::imageops::replace(&mut strip, &img, i as i64 * tile as i64, 0);
            save_jpeg(&format!("moon_{name}"), &img);
        }
        save_jpeg("moon_phases", &strip);
    }
    if wants("night") {
        for (name, phase) in [
            ("moonlit_night_full", 0.5),
            ("moonlit_night_crescent", 0.15),
        ] {
            let mut s = demo_scene(&d);
            s.render_settings.moon.phase = phase;
            save_jpeg(name, &shoot(&gpu, &assets, &s, (1600, 900)));
        }
    }
    if wants("twilight") {
        // The sun 4° below the western horizon, a nearly full moon rising low in the east.
        let mut s = aimed(&d, 100.0, -4.0, 55.0);
        let sun = s.find_by_name("Directional Light").unwrap();
        let towards = misty_engine::moon::from_az_alt(280.0, -4.0);
        let pos = s.transform(sun).position;
        s.set_transform(
            sun,
            Transform::from_position(pos).with_euler(misty_engine::hdri::sun_euler(towards)),
        );
        let m = &mut s.render_settings.moon;
        m.azimuth = 104.0;
        m.altitude = 7.0;
        m.phase = 0.47;
        save_jpeg("twilight_moonrise", &shoot(&gpu, &assets, &s, (1600, 900)));
    }
    if wants("night") {
        let full = (1600, 900);
        let base = night_sky_scene(&d);
        save_jpeg(
            "night_sky_demo",
            &shoot_at(&gpu, &assets, &base, full, 40.0),
        );
        // The Milky Way alone (no aurora, no meteors), the camera up towards Cygnus.
        let mut s = base.clone_scene();
        s.render_settings.night_sky.aurora = 0.0;
        s.render_settings.night_sky.meteor_rate = 0.0;
        aim(&mut s, 200.0, -38.0, 80.0);
        save_jpeg("milky_way", &shoot(&gpu, &assets, &s, full));
        // Stars: flat and white versus spread, glow and colour.
        let mut flat = base.clone_scene();
        let n = &mut flat.render_settings.night_sky;
        (n.milky_way, n.aurora, n.meteor_rate) = (0.0, 0.0, 0.0);
        (n.star_magnitude_spread, n.star_glow, n.star_color_variation) = (0.0, 0.0, 0.0);
        aim(&mut flat, 60.0, -40.0, 50.0);
        let mut varied = flat.clone_scene();
        let n = &mut varied.render_settings.night_sky;
        (n.star_magnitude_spread, n.star_glow, n.star_color_variation) = (1.0, 2.5, 1.0);
        let a = shoot(&gpu, &assets, &flat, (800, 900));
        let b = shoot(&gpu, &assets, &varied, (800, 900));
        let mut pair = misty_engine::image::RgbaImage::new(1600, 900);
        misty_engine::image::imageops::replace(&mut pair, &a, 0, 0);
        misty_engine::image::imageops::replace(&mut pair, &b, 800, 0);
        save_jpeg("stars_flat_vs_varied", &pair);
        // Aurora settings: the default green arcs, red-topped tall curtains, a quiet single arc.
        let looks: [(
            &str,
            fn(&mut misty_engine::moon::night_sky::NightSkySettings),
        ); 3] = [
            ("aurora_green", |_| {}),
            ("aurora_red_top_tall", |n| {
                n.aurora_top_color = [1.0, 0.12, 0.2];
                n.aurora_thickness = 320.0;
                n.aurora_curtains = 5;
                n.aurora_waviness = 2.0;
                n.aurora = 2.2;
            }),
            ("aurora_single_arc", |n| {
                n.aurora_curtains = 1;
                n.aurora_waviness = 0.3;
                n.aurora_rays = 2.5;
                n.aurora_color = [0.4, 0.75, 1.0];
                n.aurora_top_color = [0.6, 0.3, 1.0];
            }),
        ];
        for (name, look) in looks {
            let mut s = base.clone_scene();
            s.render_settings.night_sky.meteor_rate = 0.0;
            look(&mut s.render_settings.night_sky);
            save_jpeg(name, &shoot_at(&gpu, &assets, &s, full, 25.0));
        }
        // Meteors: a shower (many per minute) over the Milky Way.
        let mut s = base.clone_scene();
        s.render_settings.night_sky.aurora = 0.0;
        s.render_settings.night_sky.meteor_rate = 240.0;
        s.render_settings.night_sky.meteor_brightness = 2.0;
        aim(&mut s, 200.0, -38.0, 80.0);
        let mut quiet = s.clone_scene();
        quiet.render_settings.night_sky.meteor_rate = 0.0;
        let still = shoot_at(&gpu, &assets, &quiet, full, 31.0);
        // The moment (of a few) with the most meteor in view.
        let best = (0..10)
            .map(|k| shoot_at(&gpu, &assets, &s, full, 31.0 + k as f32 * 0.17))
            .max_by_key(|img| {
                img.pixels()
                    .zip(still.pixels())
                    .filter(|(a, b)| a[0].abs_diff(b[0]) > 40)
                    .count()
            })
            .unwrap();
        save_jpeg("meteors", &best);
    }
}

/// Turn the scene's camera (degrees) and set its field of view.
fn aim(s: &mut Scene, yaw: f32, pitch: f32, fov: f32) {
    let cam = s.find_by_name("Main Camera").unwrap();
    let pos = s.transform(cam).position;
    s.set_transform(
        cam,
        Transform::from_position(pos).with_euler(Vec3::new(pitch, yaw, 0.0)),
    );
    s.world
        .get::<&mut misty_engine::scene::Camera>(cam)
        .unwrap()
        .fov = fov;
}

trait CloneScene {
    fn clone_scene(&self) -> Scene;
}

impl CloneScene for Scene {
    /// A copy through the scene file (scenes do not implement Clone).
    fn clone_scene(&self) -> Scene {
        Scene::from_json(&self.name, &self.to_json()).unwrap().0
    }
}
