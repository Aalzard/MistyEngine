//! Procedural TerrainDemo2 scene in `SampleProject`: 512 m of fBm mountains with terraced
//! cliffs around a meadow, painted entirely by Auto Paint Rules (grass on flat ground, scree
//! on medium slopes, rock on cliffs, snow on the high band), grass scattered on flat meadow
//! only, triplanar cliffs, height blending and Color Harmony. Textures are generated tileable
//! PNGs under `Assets/Textures/Terrain`.
//! Regenerate with `cargo test -p misty_editor -- --ignored write_terrain_demo2`; the other
//! tests check the committed files still match, the rules hold and the scene plays.
//! `write_terrain_demo2_variants` writes before/after variants (for screenshots) into
//! `MISTY_TD2_OUT/<variant>/`.

use std::path::{Path, PathBuf};

use glam::Vec3;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::scene::{
    MeshFilter, MeshRenderer, PrimitiveMesh, Rigidbody, Scene, SphereCollider, Terrain,
    TerrainCollider, Transform,
};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::{TerrainData, TerrainLayer, details, noise};

const DATA: &str = "Assets/Terrains/TerrainDemo2.terrain";
const SCENE: &str = "Assets/Scenes/TerrainDemo2.scene";
const GRASS: &str = "Assets/Textures/Terrain/Meadow.png";
const SCREE: &str = "Assets/Textures/Terrain/Scree.png";
const ROCK: &str = "Assets/Textures/Terrain/Cliff.png";
const ROCK_NORMAL: &str = "Assets/Textures/Terrain/CliffNormal.png";
const ROCK_HEIGHT: &str = "Assets/Textures/Terrain/CliffHeight.png";
const SNOW: &str = "Assets/Textures/Terrain/Snow.png";
const RED: &str = "Assets/Materials/Red.mat";

const SIZE: Vec3 = Vec3::new(512.0, 170.0, 512.0);
const CORNER: Vec3 = Vec3::new(-256.0, 0.0, -256.0);
/// Meadow centre (terrain-local x, z): low, flat ground in front of the camera.
const MEADOW: (f32, f32) = (256.0, 150.0);

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- tileable texture noise ------------------------------------------------------------

/// Smooth value noise on a wrapping lattice of `period` cells, 0..1.
fn tile_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (s(x - x0), s(y - y0));
    let at = |dx: i32, dy: i32| {
        let ix = (x0 as i32 + dx).rem_euclid(period);
        let iy = (y0 as i32 + dy).rem_euclid(period);
        noise::hash(ix, iy, seed)
    };
    let a = at(0, 0) + (at(1, 0) - at(0, 0)) * u;
    let b = at(0, 1) + (at(1, 1) - at(0, 1)) * u;
    a + (b - a) * v
}

/// Tileable fBm over a `size` px image, 0..1.
fn tile_fbm(px: u32, py: u32, size: u32, period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let p = period << o;
        let k = p as f32 / size as f32;
        sum += amp * tile_noise(px as f32 * k, py as f32 * k, p, seed + o);
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn rgb(c: [f32; 3], k: f32) -> Rgba<u8> {
    let c = c.map(|v| (v * k).clamp(0.0, 255.0).round() as u8);
    Rgba([c[0], c[1], c[2], 255])
}

fn meadow(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let patches = tile_fbm(x, y, n, 3, 4, 5);
        let fine = tile_fbm(x, y, n, 32, 2, 9);
        let blade = noise::hash(x as i32, y as i32, 13);
        // Yellow-green, drier patches: deliberately warmer than the other layers.
        let base = [
            96.0 + 50.0 * patches,
            118.0 + 12.0 * patches,
            34.0 + 6.0 * patches,
        ];
        rgb(base, 0.72 + 0.3 * fine + 0.14 * (blade - 0.5))
    })
}

fn scree(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let pebbles = tile_fbm(x, y, n, 24, 3, 21);
        let big = tile_fbm(x, y, n, 4, 3, 23);
        let grain = noise::hash(x as i32, y as i32, 27);
        let k = 0.6 + 0.55 * (pebbles * pebbles) + 0.1 * (grain - 0.5);
        rgb([132.0 + 20.0 * big, 108.0 + 10.0 * big, 82.0], k)
    })
}

/// Cliff height: layered strata plus ridged cracks.
fn cliff_height(x: u32, y: u32, n: u32) -> f32 {
    let warp = tile_fbm(x, y, n, 4, 3, 41) * 0.25;
    let strata = ((y as f32 / n as f32 + warp) * std::f32::consts::TAU * 6.0).sin() * 0.5 + 0.5;
    let cracks = 1.0 - (tile_fbm(x, y, n, 8, 4, 43) * 2.0 - 1.0).abs();
    let bumps = tile_fbm(x, y, n, 6, 5, 47);
    (0.35 * strata + 0.4 * bumps + 0.25 * cracks.powi(3)).clamp(0.0, 1.0)
}

fn cliff(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let h = cliff_height(x, y, n);
        let tint = tile_fbm(x, y, n, 2, 3, 49);
        // Cool blue-grey stone.
        rgb(
            [104.0 + 10.0 * tint, 110.0 + 8.0 * tint, 120.0],
            0.5 + 0.75 * h,
        )
    })
}

fn cliff_height_map(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let v = (cliff_height(x, y, n) * 255.0).round() as u8;
        Rgba([v, v, v, 255])
    })
}

/// OpenGL-convention normal map of the cliff height.
fn cliff_normal(n: u32) -> RgbaImage {
    let h = |x: i32, y: i32| {
        cliff_height(
            x.rem_euclid(n as i32) as u32,
            y.rem_euclid(n as i32) as u32,
            n,
        )
    };
    RgbaImage::from_fn(n, n, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let dx = (h(x + 1, y) - h(x - 1, y)) * 4.0;
        let dy = (h(x, y - 1) - h(x, y + 1)) * 4.0;
        let v = Vec3::new(-dx, -dy, 1.0).normalize();
        let enc = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        Rgba([enc(v.x), enc(v.y), enc(v.z), 255])
    })
}

fn snow(n: u32) -> RgbaImage {
    RgbaImage::from_fn(n, n, |x, y| {
        let drifts = tile_fbm(x, y, n, 4, 4, 61);
        let sparkle = noise::hash(x as i32, y as i32, 67);
        rgb(
            [228.0, 236.0, 248.0],
            0.86 + 0.12 * drifts + 0.04 * (sparkle - 0.5),
        )
    })
}

fn textures() -> Vec<(&'static str, RgbaImage)> {
    vec![
        (GRASS, meadow(512)),
        (SCREE, scree(512)),
        (ROCK, cliff(512)),
        (ROCK_NORMAL, cliff_normal(512)),
        (ROCK_HEIGHT, cliff_height_map(512)),
        (SNOW, snow(256)),
    ]
}

// ---- terrain -----------------------------------------------------------------------------

/// Normalised height at local (x, z) metres.
fn height(x: f32, z: f32) -> f32 {
    let (mx, mz) = MEADOW;
    // 0 in the meadow, 1 among the mountains.
    let r = ((x - mx) / 170.0).powi(2) + ((z - mz) / 120.0).powi(2);
    let mountains = ((r.sqrt() - 0.55) / 0.8).clamp(0.0, 1.0);
    let mountains = mountains * mountains * (3.0 - 2.0 * mountains);
    let ridges = noise::ridged(x, z, 180.0, 6, 3);
    let rolling = noise::fbm(x, z, 70.0, 4, 8) * 0.5 + 0.5;
    let mut h = 0.04 + 0.05 * rolling + mountains * (0.15 + 0.75 * ridges * ridges);
    // Terraces above the meadow: flat steps joined by steep cliffs.
    if h > 0.2 {
        let steps = 7.0;
        let t = h * steps;
        let f = t.fract();
        let stepped = (t.floor() + (f / 0.35).clamp(0.0, 1.0).powf(0.6)) / steps;
        h += (stepped - h) * 0.55 * mountains;
    }
    h.clamp(0.0, 1.0)
}

fn rules(slope: [f32; 2], priority: i32, spread: f32, breakup: f32) -> AutoPaintRules {
    AutoPaintRules {
        enabled: true,
        slope,
        spread,
        breakup,
        breakup_scale: 14.0,
        priority,
        ..Default::default()
    }
}

fn layers() -> Vec<TerrainLayer> {
    vec![
        TerrainLayer {
            name: "Meadow".into(),
            diffuse: Some(GRASS.into()),
            tile_size: [7.0, 7.0],
            rules: rules([0.0, 22.0], 0, 0.2, 0.5),
            ..Default::default()
        },
        TerrainLayer {
            name: "Scree".into(),
            diffuse: Some(SCREE.into()),
            tile_size: [5.0, 5.0],
            rules: rules([20.0, 36.0], 1, 0.3, 0.7),
            ..Default::default()
        },
        TerrainLayer {
            name: "Cliff".into(),
            diffuse: Some(ROCK.into()),
            normal_map: Some(ROCK_NORMAL.into()),
            height_map: Some(ROCK_HEIGHT.into()),
            normal_scale: 1.2,
            tile_size: [14.0, 14.0],
            smoothness: 0.2,
            rules: rules([36.0, 90.0], 2, 0.15, 0.6),
            ..Default::default()
        },
        TerrainLayer {
            name: "Snow".into(),
            diffuse: Some(SNOW.into()),
            tile_size: [9.0, 9.0],
            smoothness: 0.35,
            rules: AutoPaintRules {
                use_height: true,
                height: [118.0, 170.0],
                breakup_scale: 22.0,
                ..rules([0.0, 48.0], 3, 0.25, 0.8)
            },
            ..Default::default()
        },
    ]
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(513, SIZE, 512);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = layers();
    d.rule_seed = 7;
    d.details.enabled = true;
    d.details.on_layer = Some(0);
    d.details.density = 10.0;
    d.details.draw_distance = 110.0;
    d.details.rules.slope = [0.0, 24.0];
    d.harmony.enabled = true;
    d.harmony.strength = 0.45;
    auto_paint::generate(&mut d);
    details::generate(&mut d);
    d
}

// ---- scene -------------------------------------------------------------------------------

fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

fn demo_scene(d: &TerrainData) -> Scene {
    let mut s = Scene::default_3d("TerrainDemo2");
    let eye = (0.0, -150.0);
    if let Some(cam) = s.find_by_name("Main Camera") {
        let y = ground(d, eye.0, eye.1) + 7.0;
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(eye.0, y, eye.1))
                .with_euler(Vec3::new(6.0, 0.0, 0.0)),
        );
    }
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 80.0, 0.0))
                .with_euler(Vec3::new(32.0, -35.0, 0.0)),
        );
    }
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
    // A ball on the meadow slope rolls when Play starts.
    let (bx, bz) = (12.0, -112.0);
    let ball = s.spawn("Ball", None);
    s.set_transform(
        ball,
        Transform::from_position(Vec3::new(bx, ground(d, bx, bz) + 1.0, bz)),
    );
    s.add(ball, MeshFilter::primitive(PrimitiveMesh::Sphere));
    s.add(
        ball,
        MeshRenderer {
            material: Some(RED.into()),
            ..Default::default()
        },
    );
    s.add(ball, SphereCollider::default());
    s.add(ball, Rigidbody::default());
    s
}

fn write_all(root: &Path, d: &TerrainData) {
    std::fs::create_dir_all(root.join("Assets/Textures/Terrain")).unwrap();
    for (rel, img) in textures() {
        img.save(root.join(rel)).unwrap();
    }
    d.save(&root.join(DATA)).unwrap();
    demo_scene(d).save(&root.join(SCENE)).unwrap();
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_terrain_demo2() {
    write_all(&project(), &terrain_data());
}

/// Before/after variants for screenshots: `MISTY_TD2_OUT=<dir>` gets one project per variant.
#[test]
#[ignore = "writes screenshot variants"]
fn write_terrain_demo2_variants() {
    let Some(out) = std::env::var_os("MISTY_TD2_OUT") else {
        return;
    };
    let out = PathBuf::from(out);
    let base = terrain_data();
    let mut no_breakup = base.clone();
    for l in &mut no_breakup.layers {
        l.rules.breakup = 0.0;
    }
    no_breakup.details.rules.breakup = 0.0;
    auto_paint::generate(&mut no_breakup);
    details::generate(&mut no_breakup);
    let mut no_harmony = base.clone();
    no_harmony.harmony.enabled = false;
    let mut planar = base.clone();
    planar.cliffs.triplanar = false;
    planar.cliffs.height_blend = 0.0;
    for (name, d) in [
        ("final", base),
        ("no_breakup", no_breakup),
        ("no_harmony", no_harmony),
        ("planar", planar),
    ] {
        let root = out.join(name);
        // Materials used by the scene.
        std::fs::create_dir_all(root.join("Assets/Materials")).unwrap();
        std::fs::copy(project().join(RED), root.join(RED)).unwrap();
        write_all(&root, &d);
    }
}

#[test]
fn terrain_demo2_matches_its_generator() {
    let root = project();
    for (rel, img) in textures() {
        let on_disk = misty_engine::image::open(root.join(rel))
            .unwrap()
            .into_rgba8();
        assert_eq!(on_disk, img, "{rel}");
    }
    let on_disk = TerrainData::load(&root.join(DATA)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    assert_eq!(on_disk.size, generated.size);
    assert_eq!(on_disk.details, generated.details);
    assert_eq!(on_disk.harmony, generated.harmony);
    assert_eq!(on_disk.cliffs, generated.cliffs);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let close = on_disk
        .alphamap
        .iter()
        .zip(&generated.alphamap)
        .filter(|(a, b)| a.iter().zip(*b).all(|(x, y)| x.abs_diff(*y) <= 3))
        .count();
    assert!(
        close as f32 > on_disk.alphamap.len() as f32 * 0.999,
        "splat map matches"
    );
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(scene.to_json(), demo_scene(&generated).to_json());
}

/// The rules hold on the generated data: rock on cliffs, meadow and grass on flat ground,
/// snow up high.
#[test]
fn terrain_demo2_follows_its_rules() {
    let d = terrain_data();
    let n = d.alphamap_resolution;
    let (mut steep, mut rock_on_steep, mut flat, mut meadow_on_flat) = (0, 0, 0, 0);
    for tz in (0..n).step_by(3) {
        for tx in (0..n).step_by(3) {
            let x = (tx as f32 + 0.5) / n as f32 * SIZE.x;
            let z = (tz as f32 + 0.5) / n as f32 * SIZE.z;
            let (slope, h) = auto_paint::surface_at(&d, x, z);
            let w = d.alphamap[tz * n + tx];
            if slope > 50.0 && h < 110.0 {
                steep += 1;
                rock_on_steep += usize::from(w[2] > 200);
            }
            if slope < 8.0 && h < 60.0 {
                flat += 1;
                meadow_on_flat += usize::from(w[0] > 200);
            }
            if slope > 32.0 {
                assert_eq!(d.detail_map[tz * n + tx], 0, "no grass on a {slope}° slope");
            }
        }
    }
    assert!(
        steep > 100 && flat > 1000,
        "{steep} steep, {flat} flat texels"
    );
    assert!(
        rock_on_steep as f32 > steep as f32 * 0.9,
        "{rock_on_steep}/{steep}"
    );
    assert!(
        meadow_on_flat as f32 > flat as f32 * 0.85,
        "{meadow_on_flat}/{flat}"
    );
    assert!(
        d.weights_at(MEADOW.0, 40.0)[3] < 0.01,
        "no snow in the meadow"
    );
    let top = (0..n * n).filter(|i| d.alphamap[*i][3] > 200).count();
    assert!(top > 100, "a snow band ({top} texels)");
    let grass = details::scatter(
        &d,
        glam::Vec2::new(200.0, 80.0),
        glam::Vec2::new(300.0, 180.0),
    );
    assert!(grass.len() > 20_000, "meadow grass ({})", grass.len());
}

/// Playing: the ball lands on the terrain and stays above it.
#[test]
fn terrain_demo2_plays() {
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
    let ball = scene.find_by_name("Ball").unwrap();
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..120 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    let p = scene.world_position(ball);
    assert!(
        p.y > ground(&data, p.x, p.z) - 0.1,
        "ball above the ground: {p}"
    );
    assert!(p.y < ground(&data, p.x, p.z) + 1.5, "ball landed: {p}");
}
