//! HdriSkyDemo sample in `SampleProject`: a procedural sunset HDRI written by our own Radiance
//! writer (`Assets/HDRI/Sunset Clouds.hdr`: gradient sky, a low sun disc with its glow, fbm
//! clouds lit from the sun's side, dark ground) and a scene lit only by it plus a sun aligned
//! to its brightest point: chrome → rough metal spheres, glossy → matte white spheres and a gold
//! one on a grey floor. Regenerate with `cargo test --release -p misty_editor -- --ignored
//! write_hdri_sky_demo`; the other test checks the committed files.

use std::path::PathBuf;

use glam::{Vec2, Vec3};
use misty_engine::assets::Material;
use misty_engine::hdri::{self, HdrImage};
use misty_engine::scene::render_settings::SkyboxKind;
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/HdriSkyDemo.scene";
pub const HDRI: &str = "Assets/HDRI/Sunset Clouds.hdr";
const FOLDER: &str = "Assets/Materials/HDRI";
const WIDTH: u32 = 2048;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Towards the sun: 7° up, 28° right of +Z (in front of the demo camera).
fn sun() -> Vec3 {
    let (el, az) = (7f32.to_radians(), 28f32.to_radians());
    Vec3::new(az.sin() * el.cos(), el.sin(), az.cos() * el.cos())
}

fn hash(p: Vec2) -> f32 {
    let h = p.dot(Vec2::new(127.1, 311.7)).sin() * 43_758.547;
    h - h.floor()
}

/// Smooth value noise in 0..1.
fn noise(p: Vec2) -> f32 {
    let i = p.floor();
    let f = p - i;
    let u = f * f * (Vec2::splat(3.0) - 2.0 * f);
    let a = hash(i);
    let b = hash(i + Vec2::X);
    let c = hash(i + Vec2::Y);
    let d = hash(i + Vec2::ONE);
    let top = a + (b - a) * u.x;
    let bottom = c + (d - c) * u.x;
    top + (bottom - top) * u.y
}

fn fbm(mut p: Vec2) -> f32 {
    let (mut sum, mut amp) = (0.0, 0.5);
    for _ in 0..5 {
        sum += noise(p) * amp;
        p = Vec2::new(p.x * 1.6 - p.y * 1.2, p.x * 1.2 + p.y * 1.6) + Vec2::splat(3.1);
        amp *= 0.5;
    }
    sum
}

fn smoothstep(e0: f32, e1: f32, x: f32) -> f32 {
    let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Linear radiance of the sunset sky in direction `d`.
pub fn sunset(d: Vec3) -> Vec3 {
    let s = sun();
    let g = d.dot(s).max(0.0);
    // How much this azimuth faces the sun (1 = towards it).
    let towards = (Vec2::new(d.x, d.z)
        .normalize_or_zero()
        .dot(Vec2::new(s.x, s.z).normalize())
        * 0.5
        + 0.5)
        .powi(3);
    let horizon = Vec3::new(0.3, 0.2, 0.3).lerp(Vec3::new(1.3, 0.55, 0.22), towards);
    let zenith = Vec3::new(0.03, 0.07, 0.24);
    let glow = Vec3::new(1.6, 0.75, 0.32) * (g.powi(8) * 0.8 + g.powi(90) * 6.0);
    if d.y < 0.0 {
        // Dark ground, lit a little towards the sun near the horizon.
        let ground = Vec3::new(0.08, 0.06, 0.05);
        return ground.lerp(horizon * 0.35, (1.0 - smoothstep(0.0, 0.12, -d.y)) * 0.8);
    }
    let mut c = horizon.lerp(zenith, smoothstep(0.0, 0.55, d.y).powf(0.7)) + glow;
    // Clouds on a plane above the camera, thinning out at the horizon and the zenith.
    let p = Vec2::new(d.x, d.z) / (d.y + 0.12) * 1.8;
    let density = smoothstep(0.42, 0.66, fbm(p))
        * smoothstep(0.02, 0.12, d.y)
        * (1.0 - smoothstep(0.55, 0.95, d.y));
    let lit = (d.dot(s) * 0.5 + 0.5).powi(4);
    let cloud = Vec3::new(0.1, 0.07, 0.1) + Vec3::new(1.8, 0.75, 0.3) * lit * 1.4;
    c = c.lerp(cloud, density * 0.85);
    // The sun disc (0.55° radius, reddened), in front of the clouds' thin edges.
    let disc = smoothstep(
        0.55f32.to_radians().cos(),
        0.45f32.to_radians().cos(),
        d.dot(s),
    );
    c + Vec3::new(1.0, 0.62, 0.32) * 4000.0 * disc * (1.0 - density * 0.7)
}

pub fn sunset_image() -> HdrImage {
    HdrImage::from_fn(WIDTH, WIDTH / 2, sunset)
}

fn mat(color: [f32; 3], metallic: f32, smoothness: f32) -> Material {
    Material {
        color: [color[0], color[1], color[2], 1.0],
        metallic,
        smoothness,
        ..Default::default()
    }
}

/// The demo's materials by name.
fn materials() -> Vec<(&'static str, Material)> {
    let steel = [0.92, 0.92, 0.94];
    let white = [0.9, 0.9, 0.9];
    vec![
        ("Chrome", mat(steel, 1.0, 1.0)),
        ("Brushed Steel", mat(steel, 1.0, 0.75)),
        ("Satin Steel", mat(steel, 1.0, 0.5)),
        ("Rough Steel", mat(steel, 1.0, 0.2)),
        ("Glossy White", mat(white, 0.0, 0.9)),
        ("Satin White", mat(white, 0.0, 0.5)),
        ("Matte White", mat(white, 0.0, 0.0)),
        ("Gold", mat([1.0, 0.78, 0.34], 1.0, 0.85)),
        ("Floor", mat([0.32, 0.32, 0.33], 0.0, 0.55)),
    ]
}

fn path(name: &str) -> String {
    format!("{FOLDER}/{name}.mat")
}

fn add(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(mesh));
    s.add(
        e,
        MeshRenderer {
            material: Some(path(material)),
            ..Default::default()
        },
    );
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("HdriSkyDemo");
    let sky = &mut s.render_settings.skybox;
    sky.kind = SkyboxKind::Hdri;
    sky.hdri.texture = Some(HDRI.into());
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.9, -4.2))
                .with_euler(Vec3::new(12.0, 0.0, 0.0)),
        );
    }
    // The sun shines from the HDRI's brightest point (Lighting ▸ Align Sun to HDRI).
    if let Some(light) = s.find_by_name("Directional Light") {
        s.set_transform(
            light,
            Transform::from_position(Vec3::new(0.0, 3.0, 0.0)).with_euler(hdri::sun_euler(sun())),
        );
        let mut l = s.world.get::<&mut Light>(light).unwrap();
        l.color = [1.0, 0.72, 0.48];
        l.intensity = 1.4;
        l.shadows = Shadows::Soft;
    }
    let mut floor = Transform::from_position(Vec3::ZERO);
    floor.scale = Vec3::new(10.0, 1.0, 10.0);
    add(&mut s, "Floor", PrimitiveMesh::Plane, floor, "Floor");
    let ball = |x: f32, y: f32, z: f32| {
        let mut t = Transform::from_position(Vec3::new(x, y, z));
        t.scale = Vec3::splat(1.0);
        t
    };
    let back = ["Chrome", "Brushed Steel", "Satin Steel", "Rough Steel"];
    for (i, m) in back.iter().enumerate() {
        let x = -2.25 + i as f32 * 1.5;
        add(&mut s, m, PrimitiveMesh::Sphere, ball(x, 0.5, 1.4), m);
    }
    let front = ["Glossy White", "Satin White", "Matte White"];
    for (i, m) in front.iter().enumerate() {
        let x = -1.5 + i as f32 * 1.5;
        add(&mut s, m, PrimitiveMesh::Sphere, ball(x, 0.5, -0.8), m);
    }
    let mut big = ball(3.6, 1.0, 2.6);
    big.scale = Vec3::splat(2.0);
    add(&mut s, "Gold", PrimitiveMesh::Sphere, big, "Gold");
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_hdri_sky_demo() {
    let root = project();
    hdri::rgbe::save(&root.join(HDRI), &sunset_image()).unwrap();
    std::fs::create_dir_all(root.join(FOLDER)).unwrap();
    for (name, m) in materials() {
        m.save(&root.join(path(name))).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn hdri_sky_demo_matches_its_generator() {
    let root = project();
    for (name, m) in materials() {
        assert_eq!(Material::load(&root.join(path(name))).unwrap(), m, "{name}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    assert_eq!(
        text.replace("\r\n", "\n"),
        demo_scene().to_json(),
        "regenerate with write_hdri_sky_demo"
    );
    let (scene, report) = Scene::from_json("HdriSkyDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert_eq!(
        scene.render_settings.skybox.hdri.texture.as_deref(),
        Some(HDRI)
    );

    // The committed image is the generator's (RGBE precision), and its sun is where the
    // scene's light comes from.
    let img = HdrImage::load(&root.join(HDRI)).unwrap();
    assert_eq!((img.width, img.height), (WIDTH, WIDTH / 2));
    for i in 0..400u32 {
        let (x, y) = ((i * 7919) % img.width, (i * 104_729) % img.height);
        let want = sunset(img.texel_direction(x, y));
        let got = img.get(x, y);
        assert!(
            (got - want).abs().max_element() <= want.max_element() * 0.02 + 1e-3,
            "texel {x},{y}: {got} vs {want}"
        );
    }
    let sky = hdri::HdriSky::new(img);
    assert!(sky.brightest.dot(sun()) > 0.998, "{}", sky.brightest);
}
