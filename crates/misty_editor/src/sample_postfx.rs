//! PostFxDemo sample scene in `SampleProject`: a courtyard at golden hour showing the round 17
//! camera effects and decals together. A concrete yard with a brick wall, pillars and lamps
//! reaching into the distance (Bokeh depth of field focused on the middle, bright lamps for
//! bokeh shapes), an orbiting ball and a spinning fan (motion blur), a dim archway the camera
//! looks out of (auto exposure), and Decal Projectors: dirt, moss, cracks and a wet puddle on
//! the concrete, the wall and a small grass hill (terrain).
//!
//! The decal textures in `Assets/Decals` are procedural and CC0 (made by this file).
//! Regenerate with `cargo test -p misty_editor -- --ignored write_postfx_demo`; the other test
//! checks the committed files still match.

use std::path::{Path, PathBuf};

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::image::{self, Rgba, RgbaImage};
use misty_engine::scene::{
    AdaptationMode, DecalProjector, DofMode, Light, MeshFilter, MeshRenderer, PostFxQuality,
    PostProcessVolume, PrimitiveMesh, Scene, ScriptRef, ScriptValue, Scripts, Shadows, TonemapMode,
    Transform,
};
use misty_engine::terrain::{Terrain, TerrainData, TerrainLayer};

const SCENE: &str = "Assets/Scenes/PostFxDemo.scene";
const TERRAIN: &str = "Assets/Terrains/PostFxDemo.terrain";
const GRASS: &str = "Assets/Textures/TerrainGrass.png";
const ORBITER: &str = "Assets/Scripts/Orbiter.lua";
const ROTATOR: &str = "Assets/Scripts/Rotator.lua";

const CONCRETE: &str = "Assets/Materials/Yard Concrete.mat";
const BRICK: &str = "Assets/Materials/Yard Brick.mat";
const STONE: &str = "Assets/Materials/Yard Stone.mat";
const BALL: &str = "Assets/Materials/Yard Ball.mat";
const LAMP: &str = "Assets/Materials/Yard Lamp.mat";

const DIRT: &str = "Assets/Decals/Dirt.png";
const MOSS: &str = "Assets/Decals/Moss.png";
const MOSS_NORMAL: &str = "Assets/Decals/Moss_Normal.png";
const MOSS_MASK: &str = "Assets/Decals/Moss_Mask.png";
const CRACKS: &str = "Assets/Decals/Cracks.png";
const CRACKS_NORMAL: &str = "Assets/Decals/Cracks_Normal.png";
const PUDDLE: &str = "Assets/Decals/Puddle.png";

/// Side of each decal texture.
const SIZE: u32 = 256;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

// ---- Procedural decal textures ---------------------------------------------------------------

fn hash(x: i32, y: i32, seed: u32) -> f32 {
    let mut n = (x as u32).wrapping_mul(0x27d4_eb2d) ^ (y as u32).wrapping_mul(0x1656_67b1) ^ seed;
    n = (n ^ (n >> 15)).wrapping_mul(0x2c1b_3c6d);
    n = (n ^ (n >> 12)).wrapping_mul(0x297a_2d39);
    (n ^ (n >> 15)) as f32 / u32::MAX as f32
}

/// Smooth value noise, tiling every `period` cells.
fn noise(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - xi as f32, y - yi as f32);
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let h = |i: i32, j: i32| hash(i.rem_euclid(period), j.rem_euclid(period), seed);
    let top = h(xi, yi) + (h(xi + 1, yi) - h(xi, yi)) * s(fx);
    let bottom = h(xi, yi + 1) + (h(xi + 1, yi + 1) - h(xi, yi + 1)) * s(fx);
    top + (bottom - top) * s(fy)
}

/// Fractal noise (0..1) at texture uv (0..1), tiling.
fn fbm(u: f32, v: f32, cells: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 0.5, 0.0);
    for o in 0..octaves {
        let c = cells << o;
        sum += noise(u * c as f32, v * c as f32, c, seed + o) * amp;
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn smoothstep(a: f32, b: f32, x: f32) -> f32 {
    let t = ((x - a) / (b - a)).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

fn byte(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

/// Each texel's uv (pixel centres).
fn image(f: impl Fn(f32, f32) -> [f32; 4]) -> RgbaImage {
    RgbaImage::from_fn(SIZE, SIZE, |x, y| {
        let c = f(
            (x as f32 + 0.5) / SIZE as f32,
            (y as f32 + 0.5) / SIZE as f32,
        );
        Rgba(c.map(byte))
    })
}

/// A blotch: 1 in the middle, ragged edge (noise) fading out before the border.
fn blotch(u: f32, v: f32, seed: u32, edge: f32) -> f32 {
    let r = ((u - 0.5).powi(2) + (v - 0.5).powi(2)).sqrt() * 2.0;
    let n = fbm(u, v, 4, 5, seed);
    // (Always gone at the border, so the box edges never show.)
    let square = (u - 0.5).abs().max((v - 0.5).abs()) * 2.0;
    smoothstep(edge, edge - 0.25, r + (n - 0.5) * 0.7) * smoothstep(1.0, 0.85, square)
}

/// Tangent-space normal map of a height field (OpenGL convention: green up the texture).
fn normal_map(height: impl Fn(f32, f32) -> f32, strength: f32) -> RgbaImage {
    let d = 1.0 / SIZE as f32;
    image(|u, v| {
        let dx = (height(u + d, v) - height(u - d, v)) * strength;
        // v grows down the image; up the texture is -v.
        let dy = (height(u, v - d) - height(u, v + d)) * strength;
        let n = Vec3::new(-dx, -dy, 1.0).normalize();
        [n.x * 0.5 + 0.5, n.y * 0.5 + 0.5, n.z * 0.5 + 0.5, 1.0]
    })
}

/// Distance to the nearest edge between two cells of a tiling Voronoi pattern.
fn cell_edge(u: f32, v: f32, cells: i32, seed: u32) -> f32 {
    let (x, y) = (u * cells as f32, v * cells as f32);
    let (xi, yi) = (x.floor() as i32, y.floor() as i32);
    let mut first = (f32::MAX, glam::Vec2::ZERO);
    let mut points = Vec::with_capacity(9);
    for j in -1..=1 {
        for i in -1..=1 {
            let (cx, cy) = (xi + i, yi + j);
            let p = glam::Vec2::new(
                cx as f32 + hash(cx.rem_euclid(cells), cy.rem_euclid(cells), seed),
                cy as f32 + hash(cx.rem_euclid(cells), cy.rem_euclid(cells), seed + 7),
            );
            points.push(p);
        }
    }
    let at = glam::Vec2::new(x, y);
    for p in &points {
        let d = p.distance(at);
        if d < first.0 {
            first = (d, *p);
        }
    }
    // Distance to the bisector between the nearest point and every other one.
    let mut edge = f32::MAX;
    for p in &points {
        if *p == first.1 {
            continue;
        }
        let mid = (*p + first.1) * 0.5;
        let dir = (*p - first.1).normalize();
        edge = edge.min((mid - at).dot(dir).abs());
    }
    edge / cells as f32
}

fn crack_depth(u: f32, v: f32) -> f32 {
    // Ragged lines between cells, wider where noise says, only inside a blotch.
    let wobble = (fbm(u, v, 8, 3, 31) - 0.5) * 0.02;
    let e = cell_edge(u + wobble, v - wobble, 5, 17);
    let width = 0.004 + 0.006 * fbm(u, v, 6, 2, 41);
    smoothstep(width, 0.0, e) * blotch(u, v, 23, 1.0)
}

fn moss_height(u: f32, v: f32) -> f32 {
    fbm(u, v, 16, 4, 51) * blotch(u, v, 3, 0.95)
}

/// Every decal texture: (project path, image).
fn decal_textures() -> Vec<(&'static str, RgbaImage)> {
    let dirt = image(|u, v| {
        let n = fbm(u, v, 8, 4, 11);
        let grime = 0.6 + 0.4 * fbm(u, v, 32, 2, 13);
        let a = blotch(u, v, 5, 0.95) * (0.35 + 0.65 * n);
        [0.30 * grime, 0.23 * grime, 0.16 * grime, a]
    });
    let moss = image(|u, v| {
        let h = moss_height(u, v);
        let tone = fbm(u, v, 24, 3, 53);
        let a = smoothstep(0.08, 0.3, h);
        [0.16 + 0.12 * tone, 0.28 + 0.2 * tone, 0.07 + 0.05 * tone, a]
    });
    let moss_mask = image(|u, v| {
        // R metallic, G occlusion (deeper between clumps), A smoothness (damp, low).
        let h = moss_height(u, v);
        [0.0, 0.55 + 0.45 * h, 0.0, 0.15 + 0.15 * h]
    });
    let cracks = image(|u, v| {
        let d = crack_depth(u, v);
        let stain = 0.08 + 0.05 * fbm(u, v, 16, 2, 61);
        [stain, stain * 0.95, stain * 0.9, d]
    });
    let puddle = image(|u, v| {
        let a = blotch(u, v, 71, 0.9);
        [0.16, 0.15, 0.14, a * 0.7]
    });
    vec![
        (DIRT, dirt),
        (MOSS, moss),
        (MOSS_NORMAL, normal_map(moss_height, 6.0)),
        (MOSS_MASK, moss_mask),
        (CRACKS, cracks),
        (CRACKS_NORMAL, normal_map(|u, v| -crack_depth(u, v), 1.0)),
        (PUDDLE, puddle),
    ]
}

// ---- Scene ------------------------------------------------------------------------------------

fn materials() -> [(&'static str, Material); 5] {
    let m = |color: [f32; 4], metallic, smoothness| Material {
        color,
        metallic,
        smoothness,
        ..Default::default()
    };
    [
        (CONCRETE, m([0.6, 0.57, 0.52, 1.0], 0.0, 0.15)),
        (BRICK, m([0.5, 0.26, 0.19, 1.0], 0.0, 0.15)),
        (STONE, m([0.62, 0.6, 0.56, 1.0], 0.0, 0.3)),
        (BALL, m([0.85, 0.2, 0.12, 1.0], 0.2, 0.7)),
        (
            LAMP,
            Material {
                emission_enabled: true,
                emission: [1.0, 0.8, 0.55],
                emission_intensity: 8.0,
                ..m([1.0, 0.85, 0.6, 1.0], 0.0, 0.5)
            },
        ),
    ]
}

/// Normalised height of the grass hill (left of the yard).
fn hill(x: f32, z: f32) -> f32 {
    let r = ((x - 12.0).powi(2) + (z - 16.0).powi(2)).sqrt() / 14.0;
    (1.0 - r * r).max(0.0) * 0.8 + 0.04 * fbm(x / 24.0, z / 24.0, 3, 3, 91)
}

fn terrain_data() -> TerrainData {
    let mut d = TerrainData::new(65, Vec3::new(24.0, 3.0, 32.0), 64);
    let s = d.spacing();
    for iz in 0..d.heightmap_resolution {
        for ix in 0..d.heightmap_resolution {
            d.set_height(ix, iz, hill(ix as f32 * s.x, iz as f32 * s.y));
        }
    }
    d.layers = vec![TerrainLayer {
        name: "Grass".into(),
        diffuse: Some(GRASS.into()),
        tile_size: [4.0, 4.0],
        ..Default::default()
    }];
    d
}

fn at(position: Vec3, scale: Vec3) -> Transform {
    Transform {
        position,
        scale,
        ..Default::default()
    }
}

fn mesh(s: &mut Scene, name: &str, kind: PrimitiveMesh, t: Transform, material: &str) {
    let e = s.spawn(name, None);
    s.set_transform(e, t);
    s.add(e, MeshFilter::primitive(kind));
    s.add(
        e,
        MeshRenderer {
            material: Some(material.into()),
            ..Default::default()
        },
    );
}

fn script(path: &str, fields: &[(&str, ScriptValue)]) -> Scripts {
    Scripts {
        list: vec![ScriptRef {
            path: path.into(),
            fields: fields
                .iter()
                .map(|(k, v)| ((*k).to_owned(), v.clone()))
                .collect(),
            ..Default::default()
        }],
    }
}

/// A projector at `position` facing along `euler` (degrees), `size` = width, height, depth.
fn decal(s: &mut Scene, name: &str, position: Vec3, euler: Vec3, d: DecalProjector) {
    let e = s.spawn(name, None);
    let mut t = Transform::from_position(position);
    t.set_euler(euler);
    s.set_transform(e, t);
    s.add(e, d);
}

/// A projector 0.5 m above the ground at (x, z) pointing down.
fn ground_decal(s: &mut Scene, name: &str, x: f32, z: f32, yaw: f32, d: DecalProjector) {
    decal(s, name, Vec3::new(x, 0.5, z), Vec3::new(90.0, yaw, 0.0), d);
}

fn volume() -> PostProcessVolume {
    let mut v = PostProcessVolume::default();
    v.tonemapping.active = true;
    v.tonemapping.mode = TonemapMode::Agx;
    v.bloom.active = true;
    v.bloom.intensity = 0.6;
    v.bloom.threshold = 1.0;
    let d = &mut v.depth_of_field;
    d.active = true;
    d.mode = DofMode::Bokeh;
    d.focus_distance = 11.0;
    d.focal_length = 50.0;
    d.aperture = 2.0;
    d.blade_count = 6;
    d.blade_curvature = 0.3;
    d.quality = PostFxQuality::High;
    let m = &mut v.motion_blur;
    m.active = true;
    m.intensity = 0.6;
    m.sample_count = 12;
    let a = &mut v.auto_exposure;
    a.active = true;
    a.adaptation = AdaptationMode::Progressive;
    a.limit_min = -2.0;
    a.limit_max = 2.0;
    v
}

fn demo_scene() -> Scene {
    let mut s = Scene::new("PostFxDemo");
    let cam = s.spawn("Main Camera", None);
    let mut t = Transform::from_position(Vec3::new(0.0, 1.6, -6.0));
    t.set_euler(Vec3::new(6.0, 0.0, 0.0));
    s.set_transform(cam, t);
    s.add(
        cam,
        misty_engine::scene::Camera {
            fov: 50.0,
            far: 300.0,
            ..Default::default()
        },
    );
    let sun = s.spawn("Sun", None);
    let mut t = Transform::default();
    t.set_euler(Vec3::new(25.0, -70.0, 0.0));
    s.set_transform(sun, t);
    s.add(
        sun,
        Light {
            kind: misty_engine::scene::LightType::Directional,
            color: [1.0, 0.86, 0.68],
            intensity: 1.3,
            shadows: Shadows::Soft,
            ..Default::default()
        },
    );

    // The yard: concrete floor, the brick wall across the back, a dim archway behind the camera.
    mesh(
        &mut s,
        "Yard",
        PrimitiveMesh::Plane,
        at(Vec3::new(0.0, 0.0, 15.0), Vec3::new(1.4, 1.0, 6.0)),
        CONCRETE,
    );
    mesh(
        &mut s,
        "Wall",
        PrimitiveMesh::Cube,
        at(Vec3::new(-3.0, 1.5, 5.0), Vec3::new(5.0, 3.0, 0.4)),
        BRICK,
    );
    for (i, x) in [-2.5f32, 2.5].into_iter().enumerate() {
        mesh(
            &mut s,
            &format!("Arch Side {i}"),
            PrimitiveMesh::Cube,
            at(Vec3::new(x, 1.8, -6.5), Vec3::new(1.2, 3.6, 3.0)),
            STONE,
        );
    }
    mesh(
        &mut s,
        "Arch Top",
        PrimitiveMesh::Cube,
        at(Vec3::new(0.0, 3.9, -6.5), Vec3::new(6.2, 0.6, 3.0)),
        STONE,
    );
    // Pillars and lamps down the right side into the distance (depth of field, bokeh).
    for i in 0..9 {
        let z = 2.0 + i as f32 * 5.0;
        mesh(
            &mut s,
            &format!("Pillar {i}"),
            PrimitiveMesh::Cube,
            at(Vec3::new(4.0, 1.4, z), Vec3::new(0.5, 2.8, 0.5)),
            STONE,
        );
        mesh(
            &mut s,
            &format!("Lamp {i}"),
            PrimitiveMesh::Sphere,
            at(Vec3::new(4.0, 3.0, z), Vec3::splat(0.3)),
            LAMP,
        );
    }
    // Motion: a ball orbiting in the yard and a spinning fan on the wall.
    let ball = s.spawn("Orbiting Ball", None);
    s.set_transform(ball, at(Vec3::new(0.0, 0.5, 3.0), Vec3::splat(0.8)));
    s.add(ball, MeshFilter::primitive(PrimitiveMesh::Sphere));
    s.add(
        ball,
        MeshRenderer {
            material: Some(BALL.into()),
            ..Default::default()
        },
    );
    s.add(
        ball,
        script(
            ORBITER,
            &[
                ("center", ScriptValue::Vector3([0.5, 0.5, 4.0])),
                ("radiusX", ScriptValue::Number(2.5)),
                ("radiusZ", ScriptValue::Number(1.5)),
                ("speed", ScriptValue::Number(2.0)),
            ],
        ),
    );
    let fan = s.spawn("Fan", None);
    s.set_transform(
        fan,
        at(Vec3::new(-3.0, 2.0, 4.7), Vec3::new(1.6, 0.18, 0.05)),
    );
    s.add(fan, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(
        fan,
        MeshRenderer {
            material: Some(STONE.into()),
            ..Default::default()
        },
    );
    s.add(
        fan,
        script(
            ROTATOR,
            &[
                ("speed", ScriptValue::Number(540.0)),
                ("axis", ScriptValue::Vector3([0.0, 0.0, 1.0])),
            ],
        ),
    );
    // The grass hill on the left (terrain).
    let hill = s.spawn("Hill", None);
    s.set_transform(hill, Transform::from_position(Vec3::new(-26.0, -0.4, 2.0)));
    s.add(
        hill,
        Terrain {
            terrain_data: TERRAIN.into(),
            ..Default::default()
        },
    );

    // Decals.
    let tex = |p: &str| Some(p.to_owned());
    let dirt = DecalProjector {
        size: [2.4, 2.4, 1.0],
        base_map: tex(DIRT),
        affects_normal: false,
        smoothness: 0.1,
        ..Default::default()
    };
    ground_decal(&mut s, "Dirt 1", -1.2, -2.0, 20.0, dirt.clone());
    ground_decal(&mut s, "Dirt 2", 1.6, 1.5, 130.0, dirt.clone());
    ground_decal(
        &mut s,
        "Dirt 3",
        -0.4,
        8.0,
        250.0,
        DecalProjector {
            size: [3.5, 3.5, 1.0],
            ..dirt
        },
    );
    let moss = DecalProjector {
        size: [3.0, 3.0, 4.0],
        base_map: tex(MOSS),
        normal_map: tex(MOSS_NORMAL),
        mask_map: tex(MOSS_MASK),
        smoothness: 1.0,
        priority: 1,
        ..Default::default()
    };
    ground_decal(&mut s, "Moss (yard edge)", -4.2, 2.5, 60.0, moss.clone());
    decal(
        &mut s,
        "Moss (hill)",
        Vec3::new(-7.5, 3.0, 10.0),
        Vec3::new(90.0, 10.0, 0.0),
        DecalProjector {
            size: [4.0, 4.0, 6.0],
            ..moss.clone()
        },
    );
    decal(
        &mut s,
        "Moss (wall foot)",
        Vec3::new(-3.8, 0.4, 4.0),
        Vec3::ZERO,
        DecalProjector {
            size: [2.4, 1.2, 1.5],
            pivot: [0.0, 0.0, 0.5],
            angle_fade: [40.0, 70.0],
            ..moss
        },
    );
    let cracks = DecalProjector {
        size: [2.5, 2.5, 1.0],
        base_map: tex(CRACKS),
        normal_map: tex(CRACKS_NORMAL),
        normal_strength: 0.6,
        smoothness: 0.1,
        ..Default::default()
    };
    decal(
        &mut s,
        "Cracks (wall)",
        Vec3::new(-2.0, 1.6, 4.2),
        Vec3::ZERO,
        cracks.clone(),
    );
    ground_decal(&mut s, "Cracks (yard)", 0.8, -0.5, 75.0, cracks);
    ground_decal(
        &mut s,
        "Puddle",
        1.2,
        5.0,
        0.0,
        DecalProjector {
            size: [3.0, 2.2, 1.0],
            base_map: tex(PUDDLE),
            affects_normal: false,
            metallic: 0.0,
            smoothness: 0.95,
            priority: 2,
            ..Default::default()
        },
    );

    let fx = s.spawn("Global Volume", None);
    s.add(fx, volume());
    s
}

/// A `.meta` with a new GUID next to `asset`, unless it has one.
fn ensure_meta(asset: &Path) {
    if misty_engine::asset_db::meta::read_guid(asset).is_none() {
        let guid = misty_engine::asset_db::Guid::new();
        misty_engine::asset_db::meta::write_guid(asset, guid).unwrap();
    }
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_postfx_demo() {
    let root = project();
    std::fs::create_dir_all(root.join("Assets/Decals")).unwrap();
    ensure_meta(&root.join("Assets/Decals"));
    for (path, img) in decal_textures() {
        img.save(root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
        ensure_meta(&root.join(path));
    }
    terrain_data().save(&root.join(TERRAIN)).unwrap();
    ensure_meta(&root.join(TERRAIN));
    demo_scene().save(&root.join(SCENE)).unwrap();
    ensure_meta(&root.join(SCENE));
}

#[test]
fn postfx_demo_matches_its_generator() {
    let root = project();
    for (path, generated) in decal_textures() {
        let on_disk = image::open(root.join(path)).unwrap().to_rgba8();
        assert!(
            on_disk == generated,
            "{path}: regenerate with write_postfx_demo"
        );
    }
    for (path, m) in materials() {
        crate::test_util::assert_close(&Material::load(&root.join(path)).unwrap(), &m, path);
    }
    for rel in [GRASS, ORBITER, ROTATOR] {
        assert!(root.join(rel).exists(), "{rel}");
    }
    let on_disk = TerrainData::load(&root.join(TERRAIN)).unwrap();
    let generated = terrain_data();
    assert_eq!(on_disk.layers, generated.layers);
    for (a, b) in on_disk.heights.iter().zip(&generated.heights) {
        assert!((a - b).abs() < 3.0 / 65535.0);
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    crate::test_util::assert_json_close(
        &text,
        &demo_scene().to_json(),
        "regenerate with write_postfx_demo",
    );
    let (scene, report) = Scene::from_json("PostFxDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // All four features are on.
    let post = misty_engine::render_scene::post_settings(&scene);
    assert!(post.depth_of_field.active && post.depth_of_field.mode == DofMode::Bokeh);
    assert!(post.motion_blur.active && post.auto_exposure.active);
    let sun = misty_engine::render_scene::lights(&scene)[0];
    assert_eq!(sun.kind, misty_engine::scene::LightType::Directional);
    let decals = misty_engine::render_scene::decal_draws(&scene);
    assert_eq!(decals.len(), 9);
    for d in &decals {
        for path in d.decal.textures().into_iter().flatten() {
            assert!(root.join(path).exists(), "{path}");
        }
    }
}

#[test]
fn decal_textures_tile_and_fade_out_at_their_edges() {
    for (path, img) in decal_textures() {
        if path.ends_with("_Normal.png") || path.ends_with("_Mask.png") {
            continue;
        }
        // Base maps: transparent along the border, something opaque inside.
        let border = (0..SIZE).all(|i| img.get_pixel(i, 0)[3] < 8 && img.get_pixel(0, i)[3] < 8);
        assert!(border, "{path}");
        assert!(img.pixels().any(|p| p[3] > 150), "{path}");
    }
}
