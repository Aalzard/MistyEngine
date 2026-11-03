//! Procedural sample assets: the checker and bump textures, two textured materials and the
//! TexturesDemo scene in `SampleProject`. Regenerate with
//! `cargo test -p misty_editor -- --ignored write_sample_assets`; the other tests check the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::image::{Rgba, RgbaImage};
use misty_engine::scene::{MeshFilter, MeshRef, MeshRenderer, PrimitiveMesh, Scene, Transform};

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

/// Light/dark grey checkerboard, `cells` squares per side.
fn checker(size: u32, cells: u32) -> RgbaImage {
    let cell = size / cells;
    RgbaImage::from_fn(size, size, |x, y| {
        let v = if (x / cell + y / cell).is_multiple_of(2) {
            214
        } else {
            96
        };
        Rgba([v, v, v, 255])
    })
}

/// Tangent-space normal map of round bumps (OpenGL convention: green points up the image).
fn bumps(size: u32, cells: u32) -> RgbaImage {
    let cell = size as f32 / cells as f32;
    RgbaImage::from_fn(size, size, |x, y| {
        // Cell-local coordinates in -1..1.
        let cx = ((x as f32 + 0.5) % cell) / cell * 2.0 - 1.0;
        let cy = ((y as f32 + 0.5) % cell) / cell * 2.0 - 1.0;
        let inside = cx * cx + cy * cy < 0.8;
        // Height 1 - r²: slope points away from the centre; image y grows downwards.
        let n = if inside {
            Vec3::new(1.2 * cx, -1.2 * cy, 1.0).normalize()
        } else {
            Vec3::Z
        };
        let enc = |v: f32| ((v * 0.5 + 0.5) * 255.0).round() as u8;
        Rgba([enc(n.x), enc(n.y), enc(n.z), 255])
    })
}

fn materials() -> [(&'static str, Material); 2] {
    [
        (
            "Assets/Materials/Checker.mat",
            Material {
                albedo_map: Some("Assets/Textures/Checker.png".into()),
                tiling: [4.0, 4.0],
                smoothness: 0.2,
                ..Default::default()
            },
        ),
        (
            "Assets/Materials/Bumpy Tiles.mat",
            Material {
                color: [0.55, 0.75, 1.0, 1.0],
                albedo_map: Some("Assets/Textures/Checker.png".into()),
                normal_map: Some("Assets/Textures/Bumps.png".into()),
                smoothness: 0.7,
                tiling: [2.0, 2.0],
                ..Default::default()
            },
        ),
    ]
}

fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("TexturesDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(0.0, 3.5, -8.0))
                .with_euler(Vec3::new(20.0, 0.0, 0.0)),
        );
    }
    let mut object = |name: &str, mesh: MeshRef, pos: Vec3, euler: Vec3, material: &str| {
        let e = s.spawn(name, None);
        s.set_transform(e, Transform::from_position(pos).with_euler(euler));
        s.add(e, MeshFilter { mesh: Some(mesh) });
        s.add(
            e,
            MeshRenderer {
                material: Some(material.into()),
                ..Default::default()
            },
        );
    };
    let (checker, tiles) = (
        "Assets/Materials/Checker.mat",
        "Assets/Materials/Bumpy Tiles.mat",
    );
    object(
        "Floor",
        PrimitiveMesh::Plane.into(),
        Vec3::ZERO,
        Vec3::ZERO,
        checker,
    );
    object(
        "Ramp",
        MeshRef::asset("Assets/Models/Ramp.obj", 0),
        Vec3::new(-2.2, 0.0, 0.5),
        Vec3::new(0.0, 30.0, 0.0),
        tiles,
    );
    object(
        "Cube",
        PrimitiveMesh::Cube.into(),
        Vec3::new(2.0, 0.75, 0.0),
        Vec3::new(0.0, -25.0, 0.0),
        tiles,
    );
    object(
        "Sphere",
        PrimitiveMesh::Sphere.into(),
        Vec3::new(0.3, 0.75, 1.5),
        Vec3::ZERO,
        tiles,
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_sample_assets() {
    let root = project();
    let textures = root.join("Assets/Textures");
    std::fs::create_dir_all(&textures).unwrap();
    checker(256, 8).save(textures.join("Checker.png")).unwrap();
    bumps(256, 4).save(textures.join("Bumps.png")).unwrap();
    for (rel, m) in materials() {
        m.save(&root.join(rel)).unwrap();
    }
    demo_scene()
        .save(&root.join("Assets/Scenes/TexturesDemo.scene"))
        .unwrap();
}

#[test]
fn sample_assets_match_their_generators() {
    let root = project();
    let load = |name: &str| {
        misty_engine::image::open(root.join("Assets/Textures").join(name))
            .unwrap()
            .into_rgba8()
    };
    assert_eq!(load("Checker.png"), checker(256, 8));
    assert_eq!(load("Bumps.png"), bumps(256, 4));
    for (rel, m) in materials() {
        assert_eq!(Material::load(&root.join(rel)).unwrap(), m, "{rel}");
    }
    let (scene, report) = Scene::load(&root.join("Assets/Scenes/TexturesDemo.scene")).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    assert!(scene.find_by_name("Ramp").is_some());
    let ramp = misty_engine::models::load(&root.join("Assets/Models/Ramp.obj")).unwrap();
    assert_eq!(
        ramp.meshes[0].data.indices.len(),
        3 * 8,
        "3 quads + 2 triangles"
    );
}
