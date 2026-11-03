//! Procedural TerrainDemo scene in `SampleProject`: a 120 m terrain with a big hill, a ridge and
//! a sandy shore, painted with grass, rock (on slopes) and sand (low ground) from generated
//! tileable textures; balls and crates on the hilltop roll down when Play starts, and a marker
//! circles the hill at a fixed height above the ground (`TerrainHover.lua`, `SampleHeight`).
//! Regenerate with `cargo test -p misty_editor -- --ignored write_terrain_demo`; the other tests
//! check the committed files still match and that the scene plays.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::scene::{
    BoxCollider, MeshFilter, MeshRenderer, PrimitiveMesh, Rigidbody, Scene, ScriptRef, Scripts,
    SphereCollider, Terrain, TerrainCollider, Transform,
};
use misty_engine::terrain::{TerrainData, TerrainLayer};

const DATA: &str = "Assets/Terrains/TerrainDemo.terrain";
const SCENE: &str = "Assets/Scenes/TerrainDemo.scene";
const HOVER: &str = "Assets/Scripts/TerrainHover.lua";
const GRASS: &str = "Assets/Textures/TerrainGrass.png";
const ROCK: &str = "Assets/Textures/TerrainRock.png";
const ROCK_NORMAL: &str = "Assets/Textures/TerrainRockNormal.png";
const SAND: &str = "Assets/Textures/TerrainSand.png";
const RED: &str = "Assets/Materials/Red.mat";
const WOOD: &str = "Assets/Materials/Wood.mat";
const LASER: &str = "Assets/Materials/Laser.mat";

/// Terrain size and its corner in the world (centred on the origin).
const SIZE: Vec3 = Vec3::new(120.0, 30.0, 120.0);
const CORNER: Vec3 = Vec3::new(-60.0, 0.0, -60.0);

const TERRAIN_HOVER: &str = r#"-- TerrainHover: circles around its start point, keeping a fixed height above the terrain
-- (Terrain.activeTerrain:SampleHeight).
local TerrainHover = {
    radius = 20,
    speed = 0.35,
    height = 1.5,
}

function TerrainHover:Start()
    self._terrain = Terrain.activeTerrain
    self._center = self.transform.position
    self._angle = 0
end

function TerrainHover:Update()
    self._angle = self._angle + self.speed * Time.deltaTime
    local offset = Vector3.new(math.cos(self._angle), 0, math.sin(self._angle)) * self.radius
    local p = self._center + offset
    local ground = self._terrain:SampleHeight(p) + self._terrain:GetPosition().y
    self.transform.position = Vector3.new(p.x, ground + self.height, p.z)
end

return TerrainHover
"#;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- tileable noise ----------------------------------------------------------------------

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(374_761_393)
        ^ (y as u32).wrapping_mul(668_265_263)
        ^ seed.wrapping_mul(2_246_822_519);
    h = (h ^ (h >> 13)).wrapping_mul(1_274_126_177);
    h ^= h >> 16;
    (h & 0xffff) as f32 / 65535.0
}

/// Smooth value noise on a lattice of `period` cells that wraps (tileable), 0..1.
fn value_noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (s(fx), s(fy));
    let at = |dx: i32, dy: i32| {
        let ix = (x0 as i32 + dx).rem_euclid(period);
        let iy = (y0 as i32 + dy).rem_euclid(period);
        hash(ix, iy, seed)
    };
    let a = at(0, 0) + (at(1, 0) - at(0, 0)) * u;
    let b = at(0, 1) + (at(1, 1) - at(0, 1)) * u;
    a + (b - a) * v
}

/// Tileable fractal noise over a `size` px image (0..1).
fn fbm(px: u32, py: u32, size: u32, base_period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let period = base_period << o;
        let scale = period as f32 / size as f32;
        sum += amp * value_noise(px as f32 * scale, py as f32 * scale, period, seed + o);
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn shade(base: [f32; 3], k: f32) -> Rgba<u8> {
    let c = base.map(|c| (c * k).clamp(0.0, 255.0).round() as u8);
    Rgba([c[0], c[1], c[2], 255])
}

fn grass(size: u32) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let n = fbm(x, y, size, 4, 5, 11);
        let blades = hash(x as i32, y as i32, 3);
        let dry = fbm(x, y, size, 2, 2, 29);
        let base = [70.0 + 40.0 * dry, 112.0 + 10.0 * dry, 38.0];
        shade(base, 0.75 + 0.45 * n + 0.12 * (blades - 0.5))
    })
}

fn rock_height(x: u32, y: u32, size: u32) -> f32 {
    let n = fbm(x, y, size, 4, 5, 51);
    // Ridged layer for cracks.
    let r = 1.0 - (fbm(x, y, size, 8, 3, 77) * 2.0 - 1.0).abs();
    0.6 * n + 0.4 * r * r
}

fn rock(size: u32) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let h = rock_height(x, y, size);
        let tint = fbm(x, y, size, 2, 2, 91);
        shade(
            [118.0 + 14.0 * tint, 114.0 + 6.0 * tint, 106.0],
            0.55 + 0.7 * h,
        )
    })
}

/// Normal map of the rock height (OpenGL convention: green points up the image).
fn rock_normal(size: u32) -> RgbaImage {
    let h = |x: i32, y: i32| {
        rock_height(
            x.rem_euclid(size as i32) as u32,
            y.rem_euclid(size as i32) as u32,
            size,
        )
    };
    RgbaImage::from_fn(size, size, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let dx = (h(x + 1, y) - h(x - 1, y)) * 3.0;
        // Image y grows downwards; "up" in the image is -y.
        let dy = (h(x, y - 1) - h(x, y + 1)) * 3.0;
        let n = Vec3::new(-dx, -dy, 1.0).normalize();
        let enc = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgba([enc(n.x), enc(n.y), enc(n.z), 255])
    })
}

fn sand(size: u32) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let n = fbm(x, y, size, 8, 4, 131);
        let grain = hash(x as i32, y as i32, 7);
        // Ripples: a whole number of waves across the tile so it still tiles.
        let t = (x as f32 + 6.0 * fbm(x, y, size, 2, 2, 17)) / size as f32;
        let ripple = (t * std::f32::consts::TAU * 6.0).sin();
        shade(
            [196.0, 178.0, 132.0],
            0.82 + 0.12 * n + 0.06 * ripple + 0.08 * (grain - 0.5),
        )
    })
}

// ---- terrain -----------------------------------------------------------------------------

fn gauss(x: f32, z: f32, cx: f32, cz: f32, sx: f32, sz: f32) -> f32 {
    (-((x - cx) / sx).powi(2) * 0.5 - ((z - cz) / sz).powi(2) * 0.5).exp()
}

/// Normalised height at local (x, z) metres.
fn height(x: f32, z: f32) -> f32 {
    // Rolling base that dips towards the southern shore.
    let shore = ((z - 8.0) / 30.0).clamp(0.0, 1.0);
    let base = 0.03 + 0.09 * shore * shore * (3.0 - 2.0 * shore);
    let rolling = 0.035 * ((x * 0.11).sin() * (z * 0.09).cos() + 0.5 * (x * 0.23 + z * 0.17).sin());
    let hill = 0.62 * gauss(x, z, 60.0, 84.0, 15.0, 13.0);
    let ridge = 0.4 * gauss(x, z, 18.0, 74.0, 9.0, 26.0);
    let knoll = 0.28 * gauss(x, z, 98.0, 46.0, 10.0, 9.0);
    (base + rolling * shore + hill + ridge + knoll).clamp(0.0, 1.0)
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(129, SIZE, 128);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, height(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![
        TerrainLayer {
            name: "Grass".into(),
            diffuse: Some(GRASS.into()),
            tile_size: [6.0, 6.0],
            ..Default::default()
        },
        TerrainLayer {
            name: "Rock".into(),
            diffuse: Some(ROCK.into()),
            normal_map: Some(ROCK_NORMAL.into()),
            tile_size: [8.0, 8.0],
            smoothness: 0.15,
            ..Default::default()
        },
        TerrainLayer {
            name: "Sand".into(),
            diffuse: Some(SAND.into()),
            tile_size: [5.0, 5.0],
            ..Default::default()
        },
    ];
    // Paint: sand on low ground, rock on steep slopes, grass elsewhere (soft transitions).
    let n = d.alphamap_resolution;
    for tz in 0..n {
        for tx in 0..n {
            let x = (tx as f32 + 0.5) / n as f32 * SIZE.x;
            let z = (tz as f32 + 0.5) / n as f32 * SIZE.z;
            let h = d.sample_normalized(x, z);
            let step = 0.5;
            let dhx = (d.sample_height(x + step, z) - d.sample_height(x - step, z)) / (2.0 * step);
            let dhz = (d.sample_height(x, z + step) - d.sample_height(x, z - step)) / (2.0 * step);
            let slope = (dhx * dhx + dhz * dhz).sqrt();
            let smooth = |a: f32, b: f32, v: f32| {
                let t = ((v - a) / (b - a)).clamp(0.0, 1.0);
                t * t * (3.0 - 2.0 * t)
            };
            let rock = smooth(0.45, 0.8, slope);
            let sand = (1.0 - smooth(0.06, 0.1, h)) * (1.0 - rock);
            let q = |v: f32| (v * 255.0).round() as u8;
            let (r, s) = (q(rock), q(sand));
            let g = 255u8.saturating_sub(r).saturating_sub(s);
            d.alphamap[tz * n + tx] = [g, r, s, 0];
        }
    }
    d
}

// ---- scene -------------------------------------------------------------------------------

fn shape(
    s: &mut Scene,
    name: &str,
    mesh: PrimitiveMesh,
    t: Transform,
    material: &str,
) -> misty_engine::hecs::Entity {
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
    e
}

/// World height of the generated terrain at world (x, z).
fn ground(d: &TerrainData, x: f32, z: f32) -> f32 {
    CORNER.y + d.sample_height(x - CORNER.x, z - CORNER.z)
}

fn demo_scene() -> Scene {
    let d = terrain_data();
    let mut s = Scene::default_3d("TerrainDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 16.0, -52.0))
                .with_euler(Vec3::new(14.0, 0.0, 0.0)),
        );
    }
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 30.0, 0.0))
                .with_euler(Vec3::new(38.0, -40.0, 0.0)),
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

    // Balls and crates just south of the hilltop: they roll down towards the camera.
    let top = Vec3::new(0.0, 0.0, 24.0);
    for i in 0..6 {
        let x = top.x - 7.5 + i as f32 * 3.0;
        let z = top.z - 4.0 - (i % 2) as f32 * 2.0;
        let pos = Vec3::new(x, ground(&d, x, z) + 0.8, z);
        let e = shape(
            &mut s,
            "Ball",
            PrimitiveMesh::Sphere,
            Transform::from_position(pos),
            RED,
        );
        s.add(e, SphereCollider::default());
        s.add(e, Rigidbody::default());
    }
    for i in 0..3 {
        let x = top.x - 4.0 + i as f32 * 4.0;
        let z = top.z - 9.0;
        let pos = Vec3::new(x, ground(&d, x, z) + 0.8, z);
        let e = shape(
            &mut s,
            "Crate",
            PrimitiveMesh::Cube,
            Transform::from_position(pos),
            WOOD,
        );
        s.add(e, BoxCollider::default());
        s.add(e, Rigidbody::default());
    }
    // Hovering marker circling the hill (no collider).
    let marker_center = Vec3::new(0.0, ground(&d, 0.0, 24.0), 24.0);
    let marker = shape(
        &mut s,
        "Hover Marker",
        PrimitiveMesh::Sphere,
        Transform::from_position(marker_center),
        LASER,
    );
    s.add(
        marker,
        Scripts {
            list: vec![ScriptRef {
                path: HOVER.into(),
                ..Default::default()
            }],
        },
    );
    s
}

fn textures() -> [(&'static str, RgbaImage); 4] {
    [
        (GRASS, grass(256)),
        (ROCK, rock(256)),
        (ROCK_NORMAL, rock_normal(256)),
        (SAND, sand(256)),
    ]
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_terrain_demo() {
    let root = project();
    for (rel, img) in textures() {
        img.save(root.join(rel)).unwrap();
    }
    terrain_data().save(&root.join(DATA)).unwrap();
    std::fs::write(root.join(HOVER), TERRAIN_HOVER).unwrap();
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn terrain_demo_matches_its_generator() {
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
    assert_eq!(on_disk.heightmap_resolution, generated.heightmap_resolution);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    for (a, b) in on_disk.alphamap.iter().zip(&generated.alphamap) {
        assert!(
            a.iter().zip(b).all(|(x, y)| x.abs_diff(*y) <= 2),
            "{a:?} {b:?}"
        );
    }
    let script = std::fs::read_to_string(root.join(HOVER)).unwrap();
    assert_eq!(script.replace("\r\n", "\n"), TERRAIN_HOVER);
    let (scene, report) = Scene::load(&root.join(SCENE)).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(scene.to_json(), demo_scene().to_json());
}

/// Playing: the balls roll down the hill and stay on the terrain; the marker follows the ground.
#[test]
fn terrain_demo_plays() {
    use misty_engine::runtime::Time;
    use misty_engine::scripting::{Input, LogKind, ScriptHost};
    let root = project();
    let data = TerrainData::load(&root.join(DATA)).unwrap();
    let mut scene = demo_scene();
    // An absolute data path, so the test doesn't depend on the global project root.
    let terrain = scene.find_by_name("Terrain").unwrap();
    scene
        .world
        .get::<&mut Terrain>(terrain)
        .unwrap()
        .terrain_data = root.join(DATA).to_string_lossy().into_owned();
    let physics = misty_engine::physics::Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    let balls: Vec<_> = scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.name(*e).starts_with("Ball"))
        .collect();
    let start: Vec<Vec3> = balls.iter().map(|e| scene.world_position(*e)).collect();
    let mut host = ScriptHost::start(&mut scene, physics, root);
    let mut time = Time::default();
    for _ in 0..180 {
        time.advance(1.0 / 60.0);
        host.update(&mut scene, &time, Input::default());
    }
    for (kind, text) in host.take_logs() {
        assert_ne!(kind, LogKind::Error, "{text}");
    }
    let mut rolled = 0;
    for (e, s) in balls.iter().zip(&start) {
        let p = scene.world_position(*e);
        assert!(
            p.y > ground(&data, p.x, p.z) - 0.1,
            "ball stays above the ground: {p}"
        );
        if s.z - p.z > 2.0 && p.y < s.y - 1.0 {
            rolled += 1;
        }
    }
    assert!(rolled >= 4, "most balls rolled down the hill ({rolled})");
    let marker = scene.find_by_name("Hover Marker").unwrap();
    let m = scene.world_position(marker);
    assert!(
        (m.y - ground(&data, m.x, m.z) - 1.5).abs() < 0.05,
        "marker hovers 1.5 m above the ground: {m}"
    );
}
