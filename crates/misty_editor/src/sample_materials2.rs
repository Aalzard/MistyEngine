//! MaterialDemo2 sample scene in `SampleProject`: the Standard shader's optional features
//! (Standard Lit v2) side by side. A car-paint body with a clear coat over metallic flakes,
//! brushed metal with anisotropic highlights (a radial disc and a horizontal pipe), a brick
//! wall with parallax occlusion mapping, a detail-mapped rock and translucent leaves lit from
//! behind by a warm lamp. Every texture is generated here (procedural, so free of any licence).
//! Regenerate with `cargo test -p misty_editor -- --ignored write_material_demo2`; the other
//! tests check the committed files still match and that each material compiles its features.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::material_features::{
    Anisotropy, AnisotropyDirection, ClearCoat, DetailMaps, Features, Parallax, Translucency,
};
use misty_engine::render::RenderingMode;
use misty_engine::scene::{
    Light, LightType, MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/MaterialDemo2.scene";
const FLOOR: &str = "Assets/Materials/Checker.mat";

const T: &str = "Assets/Textures/MaterialDemo2";
const BRICK_ALBEDO: &str = "Assets/Textures/MaterialDemo2/Brick Albedo.png";
const BRICK_NORMAL: &str = "Assets/Textures/MaterialDemo2/Brick Normal.png";
const BRICK_HEIGHT: &str = "Assets/Textures/MaterialDemo2/Brick Height.png";
const ROCK_ALBEDO: &str = "Assets/Textures/MaterialDemo2/Rock Albedo.png";
const ROCK_NORMAL: &str = "Assets/Textures/MaterialDemo2/Rock Normal.png";
const ROCK_DETAIL: &str = "Assets/Textures/MaterialDemo2/Rock Detail.png";
const ROCK_DETAIL_NORMAL: &str = "Assets/Textures/MaterialDemo2/Rock Detail Normal.png";
const ROCK_MASK: &str = "Assets/Textures/MaterialDemo2/Rock Detail Mask.png";
const FLAKES: &str = "Assets/Textures/MaterialDemo2/Paint Flakes Normal.png";
const BRUSHED: &str = "Assets/Textures/MaterialDemo2/Brushed Normal.png";
const LEAF: &str = "Assets/Textures/MaterialDemo2/Leaf.png";
const LEAF_THICKNESS: &str = "Assets/Textures/MaterialDemo2/Leaf Thickness.png";

const M: &str = "Assets/Materials/MaterialDemo2";
const CAR_PAINT: &str = "Assets/Materials/MaterialDemo2/Clear Coat Paint.mat";
const DISC: &str = "Assets/Materials/MaterialDemo2/Brushed Disc.mat";
const PIPE: &str = "Assets/Materials/MaterialDemo2/Brushed Pipe.mat";
const BRICKS: &str = "Assets/Materials/MaterialDemo2/Parallax Bricks.mat";
const ROCK: &str = "Assets/Materials/MaterialDemo2/Detail Rock.mat";
const LEAVES: &str = "Assets/Materials/MaterialDemo2/Translucent Leaves.mat";
const STEM: &str = "Assets/Materials/MaterialDemo2/Stem.mat";

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- procedural textures ------------------------------------------------------------------

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut h = (x as u32).wrapping_mul(0x8da6_b343)
        ^ (y as u32).wrapping_mul(0xd816_3841)
        ^ seed.wrapping_mul(0xcb1a_b31f);
    h ^= h >> 13;
    h = h.wrapping_mul(0x5bd1_e995);
    h ^= h >> 15;
    (h & 0xffff) as f32 / 65535.0
}

/// Tileable value noise with `cells` cells across the tile (coordinates 0..1).
fn value_noise(u: f32, v: f32, cells: i32, seed: u32) -> f32 {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (x0, y0) = (x.floor(), y.floor());
    let (fx, fy) = (x - x0, y - y0);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (sx, sy) = (s(fx), s(fy));
    let w = |i: i32| i.rem_euclid(cells);
    let (xi, yi) = (x0 as i32, y0 as i32);
    let a = hash(w(xi), w(yi), seed);
    let b = hash(w(xi + 1), w(yi), seed);
    let c = hash(w(xi), w(yi + 1), seed);
    let d = hash(w(xi + 1), w(yi + 1), seed);
    let top = a + (b - a) * sx;
    let bottom = c + (d - c) * sx;
    top + (bottom - top) * sy
}

/// Fractal noise (0..1), tileable.
fn fbm(u: f32, v: f32, cells: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut total, mut c) = (0.0, 0.5, 0.0, cells);
    for o in 0..octaves {
        sum += value_noise(u, v, c, seed + o) * amp;
        total += amp;
        amp *= 0.5;
        c *= 2;
    }
    sum / total
}

fn grey(size: u32, f: impl Fn(f32, f32) -> f32) -> Vec<f32> {
    let mut out = Vec::with_capacity((size * size) as usize);
    for y in 0..size {
        for x in 0..size {
            let (u, v) = (
                (x as f32 + 0.5) / size as f32,
                (y as f32 + 0.5) / size as f32,
            );
            out.push(f(u, v).clamp(0.0, 1.0));
        }
    }
    out
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn grey_image(size: u32, h: &[f32]) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let v = byte(h[(y * size + x) as usize]);
        Rgba([v, v, v, 255])
    })
}

/// Tangent-space normal map (OpenGL: green up) from a tileable height field; `strength` is the
/// slope per unit of height change across one texel.
fn normal_from_height(size: u32, h: &[f32], strength: f32) -> RgbaImage {
    let at = |x: i32, y: i32| {
        let s = size as i32;
        h[(y.rem_euclid(s) * s + x.rem_euclid(s)) as usize]
    };
    RgbaImage::from_fn(size, size, |x, y| {
        let (x, y) = (x as i32, y as i32);
        let dx = (at(x + 1, y) - at(x - 1, y)) * 0.5 * strength;
        // Image rows run down; green points up the image.
        let dy = (at(x, y - 1) - at(x, y + 1)) * 0.5 * strength;
        let n = Vec3::new(-dx, -dy, 1.0).normalize();
        Rgba([
            byte(n.x * 0.5 + 0.5),
            byte(n.y * 0.5 + 0.5),
            byte(n.z * 0.5 + 0.5),
            255,
        ])
    })
}

/// Bricks: (height, brick id noise) with rounded edges and a rough face; mortar is low.
fn brick_height(size: u32) -> Vec<f32> {
    grey(size, |u, v| {
        let rows = 8.0;
        let row = (v * rows).floor();
        let offset = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
        let bx = (u * 4.0 + offset).fract();
        let by = (v * rows).fract();
        // Distance to the brick's edge in brick-height units.
        let ex = bx.min(1.0 - bx) * 2.0;
        let ey = by.min(1.0 - by);
        let edge = (ex.min(ey) / 0.12).min(1.0);
        let bevel = edge * edge * (3.0 - 2.0 * edge);
        let face = 0.85 + 0.15 * fbm(u, v, 16, 3, 7);
        bevel * face
    })
}

fn brick_albedo(size: u32, h: &[f32]) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (
            (x as f32 + 0.5) / size as f32,
            (y as f32 + 0.5) / size as f32,
        );
        let height = h[(y * size + x) as usize];
        let rows = 8.0;
        let row = (v * rows).floor();
        let offset = if row as i32 % 2 == 0 { 0.0 } else { 0.5 };
        let col = (u * 4.0 + offset).floor();
        let tint = hash(col as i32, row as i32, 11);
        let grain = fbm(u, v, 32, 3, 3);
        let brick = Vec3::new(0.55 + 0.12 * tint, 0.22 + 0.06 * tint, 0.15) * (0.75 + 0.35 * grain);
        let mortar = Vec3::splat(0.62 + 0.1 * grain);
        let t = (height * 3.0).min(1.0);
        let c = mortar.lerp(brick, t);
        Rgba([byte(c.x), byte(c.y), byte(c.z), 255])
    })
}

fn rock_height(size: u32) -> Vec<f32> {
    grey(size, |u, v| fbm(u, v, 4, 5, 21))
}

fn rock_albedo(size: u32, h: &[f32]) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (
            (x as f32 + 0.5) / size as f32,
            (y as f32 + 0.5) / size as f32,
        );
        let height = h[(y * size + x) as usize];
        let streak = fbm(u * 0.5 + v, v, 4, 3, 5);
        let c = Vec3::new(0.42, 0.39, 0.35) * (0.6 + 0.6 * height) + Vec3::splat(0.06 * streak);
        Rgba([byte(c.x), byte(c.y), byte(c.z), 255])
    })
}

/// Fine grain around mid grey (a ×2 detail albedo: grey = no change).
fn rock_detail(size: u32, h: &[f32]) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let v = byte(0.5 + (h[(y * size + x) as usize] - 0.5) * 0.7);
        Rgba([v, v, v, 255])
    })
}

/// Where the detail shows: soft patches (moss-free rock faces).
fn rock_mask(size: u32) -> RgbaImage {
    let h = grey(size, |u, v| {
        let n = fbm(u, v, 3, 2, 41);
        ((n - 0.35) * 4.0).clamp(0.0, 1.0)
    });
    grey_image(size, &h)
}

/// Metallic flakes: a random tilt per small cell.
fn flakes_normal(size: u32) -> RgbaImage {
    RgbaImage::from_fn(size, size, |x, y| {
        let (cx, cy) = (x as i32 / 2, y as i32 / 2);
        let a = hash(cx, cy, 91) * std::f32::consts::TAU;
        let tilt = 0.35 * hash(cx, cy, 92);
        let n = Vec3::new(a.cos() * tilt, a.sin() * tilt, 1.0).normalize();
        Rgba([
            byte(n.x * 0.5 + 0.5),
            byte(n.y * 0.5 + 0.5),
            byte(n.z * 0.5 + 0.5),
            255,
        ])
    })
}

/// Brushing: fine streaks along U.
fn brushed_normal(size: u32) -> RgbaImage {
    let h = grey(size, |u, v| {
        let line = value_noise(u * 0.02, v, 96, 61) * 0.7 + value_noise(u * 0.05, v, 192, 62) * 0.3;
        line
    });
    normal_from_height(size, &h, 1.2)
}

/// A leaf with a stem: alpha = shape, green with lighter veins.
fn leaf(size: u32) -> (RgbaImage, RgbaImage) {
    let mut thick = RgbaImage::new(size, size);
    let img = RgbaImage::from_fn(size, size, |x, y| {
        let (u, v) = (
            (x as f32 + 0.5) / size as f32,
            (y as f32 + 0.5) / size as f32,
        );
        // Leaf along V: tip at the top, stem at the bottom.
        let t = 1.0 - v;
        let half_width = 0.42 * (std::f32::consts::PI * t.powf(0.8)).sin().max(0.0);
        let d = (u - 0.5).abs();
        let inside = d < half_width && t > 0.08 && t < 0.98;
        let stem = d < 0.012 && t <= 0.1;
        // Midrib and side veins.
        let midrib = d < 0.012;
        let side = ((t * 9.0 - d * 7.0).fract() - 0.5).abs() < 0.06 && d < half_width * 0.9;
        let vein = midrib || side;
        let shade = 0.85 + 0.15 * fbm(u, v, 8, 2, 77);
        let (a, c) = if inside || stem {
            let c = if vein {
                Vec3::new(0.52, 0.66, 0.22)
            } else {
                Vec3::new(0.24, 0.48, 0.1) * shade
            };
            (255, c)
        } else {
            (0, Vec3::new(0.24, 0.48, 0.1))
        };
        let th = if vein {
            0.85
        } else {
            0.25 * (1.0 - shade) + 0.1
        };
        let tv = byte(th);
        thick.put_pixel(x, y, Rgba([tv, tv, tv, 255]));
        Rgba([byte(c.x), byte(c.y), byte(c.z), a])
    });
    (img, thick)
}

fn textures() -> Vec<(&'static str, RgbaImage)> {
    let brick_h = brick_height(256);
    let rock_h = rock_height(256);
    let detail_h = grey(128, |u, v| fbm(u, v, 16, 3, 33));
    let (leaf_img, leaf_thick) = leaf(256);
    vec![
        (BRICK_ALBEDO, brick_albedo(256, &brick_h)),
        (BRICK_NORMAL, normal_from_height(256, &brick_h, 4.0)),
        (BRICK_HEIGHT, grey_image(256, &brick_h)),
        (ROCK_ALBEDO, rock_albedo(256, &rock_h)),
        (ROCK_NORMAL, normal_from_height(256, &rock_h, 6.0)),
        (ROCK_DETAIL, rock_detail(128, &detail_h)),
        (ROCK_DETAIL_NORMAL, normal_from_height(128, &detail_h, 5.0)),
        (ROCK_MASK, rock_mask(128)),
        (FLAKES, flakes_normal(128)),
        (BRUSHED, brushed_normal(128)),
        (LEAF, leaf_img),
        (LEAF_THICKNESS, leaf_thick),
    ]
}

// ---- materials ----------------------------------------------------------------------------

fn materials() -> Vec<(&'static str, Material)> {
    let f = Features::new;
    vec![
        (
            CAR_PAINT,
            Material {
                color: [0.62, 0.04, 0.05, 1.0],
                metallic: 0.55,
                smoothness: 0.5,
                normal_map: Some(FLAKES.into()),
                normal_scale: 0.6,
                tiling: [6.0, 6.0],
                features: Features {
                    clear_coat: ClearCoat {
                        enabled: true,
                        strength: 1.0,
                        smoothness: 0.97,
                        ..Default::default()
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            DISC,
            Material {
                color: [0.86, 0.86, 0.88, 1.0],
                metallic: 1.0,
                smoothness: 0.78,
                features: Features {
                    anisotropy: Anisotropy {
                        enabled: true,
                        strength: 0.85,
                        direction: AnisotropyDirection::Radial,
                        rotation: 0.0,
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            PIPE,
            Material {
                color: [0.95, 0.78, 0.55, 1.0],
                metallic: 1.0,
                smoothness: 0.8,
                normal_map: Some(BRUSHED.into()),
                normal_scale: 0.25,
                tiling: [2.0, 1.0],
                features: Features {
                    anisotropy: Anisotropy {
                        enabled: true,
                        strength: 0.8,
                        direction: AnisotropyDirection::Vertical,
                        rotation: 0.0,
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            BRICKS,
            Material {
                albedo_map: Some(BRICK_ALBEDO.into()),
                normal_map: Some(BRICK_NORMAL.into()),
                smoothness: 0.15,
                tiling: [2.0, 1.5],
                features: Features {
                    parallax: Parallax {
                        enabled: true,
                        height_map: Some(BRICK_HEIGHT.into()),
                        scale: 0.05,
                        steps: 32,
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            ROCK,
            Material {
                albedo_map: Some(ROCK_ALBEDO.into()),
                normal_map: Some(ROCK_NORMAL.into()),
                smoothness: 0.25,
                features: Features {
                    detail: DetailMaps {
                        enabled: true,
                        albedo_map: Some(ROCK_DETAIL.into()),
                        normal_map: Some(ROCK_DETAIL_NORMAL.into()),
                        normal_scale: 1.0,
                        mask: Some(ROCK_MASK.into()),
                        tiling: [8.0, 8.0],
                        offset: [0.0, 0.0],
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            LEAVES,
            Material {
                albedo_map: Some(LEAF.into()),
                smoothness: 0.35,
                rendering_mode: RenderingMode::Cutout,
                alpha_cutoff: 0.5,
                double_sided: true,
                features: Features {
                    translucency: Translucency {
                        enabled: true,
                        color: [0.55, 0.85, 0.2],
                        strength: 0.6,
                        wrap: 0.5,
                        thickness_map: Some(LEAF_THICKNESS.into()),
                        thickness: 1.0,
                    },
                    ..f()
                },
                ..Default::default()
            },
        ),
        (
            STEM,
            Material {
                color: [0.3, 0.22, 0.12, 1.0],
                smoothness: 0.2,
                ..Default::default()
            },
        ),
    ]
}

// ---- scene --------------------------------------------------------------------------------

fn add_mesh(s: &mut Scene, name: &str, mesh: PrimitiveMesh, t: Transform, material: &str) {
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

fn at(p: Vec3, euler: Vec3, scale: Vec3) -> Transform {
    let mut t = Transform::from_position(p).with_euler(euler);
    t.scale = scale;
    t
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("MaterialDemo2");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 1.7, -5.4))
                .with_euler(Vec3::new(9.0, 0.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 4.0, 0.0))
                .with_euler(Vec3::new(38.0, 150.0, 0.0)),
        );
        s.world.get::<&mut Light>(sun).unwrap().shadows = Shadows::Soft;
    }
    add_mesh(
        &mut s,
        "Floor",
        PrimitiveMesh::Plane,
        at(
            Vec3::new(0.0, 0.0, 2.0),
            Vec3::ZERO,
            Vec3::new(2.0, 1.0, 1.4),
        ),
        FLOOR,
    );
    // Back: the parallax brick wall.
    add_mesh(
        &mut s,
        "Parallax Brick Wall",
        PrimitiveMesh::Quad,
        at(
            Vec3::new(0.0, 1.5, 3.2),
            Vec3::ZERO,
            Vec3::new(6.0, 3.0, 1.0),
        ),
        BRICKS,
    );
    // Left: the car-paint body (a capsule on its side) with a clear coat.
    add_mesh(
        &mut s,
        "Clear Coat Car Body",
        PrimitiveMesh::Capsule,
        at(
            Vec3::new(-3.2, 0.7, 0.6),
            Vec3::new(0.0, 0.0, 90.0),
            Vec3::new(1.1, 1.1, 1.1),
        ),
        CAR_PAINT,
    );
    add_mesh(
        &mut s,
        "Clear Coat Sphere",
        PrimitiveMesh::Sphere,
        at(Vec3::new(-1.7, 0.55, -0.6), Vec3::ZERO, Vec3::splat(1.1)),
        CAR_PAINT,
    );
    // Middle: brushed metal, a radial disc tilted to the camera and a horizontal pipe.
    add_mesh(
        &mut s,
        "Brushed Metal Disc",
        PrimitiveMesh::Cylinder,
        at(
            Vec3::new(0.0, 1.0, 0.8),
            Vec3::new(-62.0, 0.0, 0.0),
            Vec3::new(1.5, 0.04, 1.5),
        ),
        DISC,
    );
    add_mesh(
        &mut s,
        "Brushed Metal Pipe",
        PrimitiveMesh::Cylinder,
        at(
            Vec3::new(0.2, 0.3, -0.9),
            Vec3::new(0.0, 0.0, 90.0),
            Vec3::new(0.45, 0.9, 0.45),
        ),
        PIPE,
    );
    // Right: the detail-mapped rock and the leaves on a stem, back-lit by a lamp.
    add_mesh(
        &mut s,
        "Detail Rock",
        PrimitiveMesh::Sphere,
        at(
            Vec3::new(1.8, 0.45, -0.5),
            Vec3::new(10.0, 30.0, 0.0),
            Vec3::new(1.5, 0.9, 1.2),
        ),
        ROCK,
    );
    add_mesh(
        &mut s,
        "Leaf Stem",
        PrimitiveMesh::Cylinder,
        at(
            Vec3::new(3.3, 0.8, 0.9),
            Vec3::ZERO,
            Vec3::new(0.05, 0.8, 0.05),
        ),
        STEM,
    );
    for (i, (y, yaw, roll)) in [
        (0.9, 0.0, 35.0),
        (1.25, 140.0, -40.0),
        (1.55, 250.0, 30.0),
        (1.85, 40.0, -15.0),
    ]
    .into_iter()
    .enumerate()
    {
        add_mesh(
            &mut s,
            &format!("Translucent Leaf {}", i + 1),
            PrimitiveMesh::Quad,
            at(
                Vec3::new(3.3, y, 0.9),
                Vec3::new(-15.0, yaw, roll),
                Vec3::new(0.9, 1.15, 1.0),
            ),
            LEAVES,
        );
    }
    let lamp = s.spawn("Back Lamp", None);
    s.set_transform(lamp, Transform::from_position(Vec3::new(3.4, 1.4, 2.0)));
    s.add(
        lamp,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.82, 0.55],
            intensity: 1.6,
            range: 4.0,
            shadows: Shadows::None,
            ..Default::default()
        },
    );
    // A key light from the front left: highlights on the coat and the brushed metal.
    let key = s.spawn("Key Lamp", None);
    s.set_transform(key, Transform::from_position(Vec3::new(-1.2, 2.4, -2.2)));
    s.add(
        key,
        Light {
            kind: LightType::Point,
            color: [1.0, 0.97, 0.92],
            intensity: 2.0,
            range: 7.0,
            shadows: Shadows::None,
            ..Default::default()
        },
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_material_demo2() {
    let root = project();
    std::fs::create_dir_all(root.join(T)).unwrap();
    std::fs::create_dir_all(root.join(M)).unwrap();
    for (rel, img) in textures() {
        img.save(root.join(rel)).unwrap();
    }
    for (rel, m) in materials() {
        m.save(&root.join(rel)).unwrap();
    }
    demo_scene().save(&root.join(SCENE)).unwrap();
}

#[test]
fn material_demo2_matches_its_generator() {
    let root = project();
    for (rel, img) in textures() {
        let file = misty_engine::image::open(root.join(rel))
            .unwrap()
            .into_rgba8();
        crate::test_util::assert_image_close(&file, &img, rel);
    }
    for (rel, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(rel)).unwrap(), &m, rel);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_material_demo2",
    );
    let (_, report) = Scene::from_json("MaterialDemo2", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
}

/// Each demo material compiles the feature it shows, and only that one.
#[test]
fn material_demo2_materials_use_their_features() {
    use misty_engine::render::material_v2::feature;
    let root = project();
    let bits = |rel: &str| Material::load(&root.join(rel)).unwrap().params().features();
    assert_eq!(bits(CAR_PAINT), feature::CLEARCOAT);
    assert_eq!(bits(DISC), feature::ANISOTROPY);
    assert_eq!(bits(PIPE), feature::ANISOTROPY);
    assert_eq!(bits(BRICKS), feature::PARALLAX);
    assert_eq!(bits(ROCK), feature::DETAIL);
    assert_eq!(bits(LEAVES), feature::TRANSLUCENCY);
    assert_eq!(bits(STEM), 0, "plain Standard");
    // The textures the features read are all in the project.
    for (rel, _) in materials() {
        let m = Material::load(&root.join(rel)).unwrap();
        for t in m.features.textures() {
            assert!(root.join(t).is_file(), "{rel}: {t}");
        }
    }
}

/// The demo through its camera at 1280×720: `docs/screenshots/r21/material_demo2.png`, plus
/// crops of each feature (run alone: it sets the engine's project root).
#[test]
#[ignore = "writes a render into docs/screenshots/r21; run on purpose"]
fn render_material_demo2() {
    use misty_engine::render::Renderer;
    use misty_engine::render_scene::GameView;
    let Some((device, queue)) = crate::sample_render::headless() else {
        eprintln!("no GPU adapter: skipped");
        return;
    };
    let root = project();
    misty_engine::assets::set_project_root(&root);
    let (scene, _) = Scene::load(&root.join(SCENE)).unwrap();
    let assets = misty_engine::assets::AssetCache::open(&root);
    let view = GameView::new(&scene, &assets).unwrap();
    let (w, h) = (1280, 720);
    let mut renderer = Renderer::new(&device);
    let target = renderer.create_target(&device, w, h);
    for _ in 0..16 {
        renderer.render(&device, &queue, &target, &view.frame());
    }
    let pixels = crate::project_thumbnail::read_pixels(&device, &queue, &target);
    let out = root.join("../docs/screenshots/r21");
    std::fs::create_dir_all(&out).unwrap();
    let img = RgbaImage::from_raw(w, h, pixels).unwrap();
    img.save(out.join("material_demo2.png")).unwrap();
}
