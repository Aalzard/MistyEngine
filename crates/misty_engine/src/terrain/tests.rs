use glam::{Vec2, Vec3};

use super::brush::{self, Brush};
use super::file::{decode, encode, quantize};
use super::*;
use crate::scene::Scene;

/// 33×33 terrain, 32 m square and 10 m tall, with a slope along x.
fn sloped() -> TerrainData {
    let mut d = TerrainData::new(33, Vec3::new(32.0, 10.0, 32.0), 32);
    for iz in 0..33 {
        for ix in 0..33 {
            d.set_height(ix, iz, ix as f32 / 32.0);
        }
    }
    d
}

fn temp_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_terrain_{name}_{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn resolutions_snap_to_supported_values() {
    let d = TerrainData::new(100, Vec3::splat(10.0), 300);
    assert_eq!(d.heightmap_resolution, 129);
    assert_eq!(d.alphamap_resolution, 256);
    assert_eq!(d.heights.len(), 129 * 129);
    assert!(d.alphamap.iter().all(|p| *p == [255, 0, 0, 0]));
}

#[test]
fn sample_height_interpolates_and_clamps() {
    let d = sloped();
    // One metre per sample, height = x / 32 * 10.
    assert!((d.sample_height(16.0, 5.0) - 5.0).abs() < 1e-5);
    assert!((d.sample_height(16.5, 7.25) - 16.5 / 32.0 * 10.0).abs() < 1e-5);
    assert!(
        (d.sample_height(-5.0, 0.0)).abs() < 1e-6,
        "clamped to the edge"
    );
    assert!((d.sample_height(50.0, 50.0) - 10.0).abs() < 1e-5);

    // A single raised sample: triangle interpolation along the shared diagonal.
    let mut bump = TerrainData::new(33, Vec3::new(32.0, 10.0, 32.0), 32);
    bump.set_height(1, 0, 1.0);
    // (0.5, 0.5) lies on the diagonal from (0, 1) to (1, 0): half of sample (1, 0).
    assert!((bump.sample_height(0.5, 0.5) - 5.0).abs() < 1e-5);
    assert!((bump.sample_height(0.25, 0.25) - 2.5).abs() < 1e-5);
}

#[test]
fn normals_tilt_against_the_slope() {
    let d = sloped();
    let n = d.vertex_normal(10, 10);
    // Rises 10/32 per metre along +x: normal leans towards -x.
    let expected = Vec3::new(-10.0 / 32.0, 1.0, 0.0).normalize();
    assert!(n.distance(expected) < 1e-5, "{n}");
    let mesh = mesh::build(&d);
    assert_eq!(mesh.vertices.len(), 33 * 33);
    assert_eq!(mesh.indices.len(), 32 * 32 * 6);
    // Every triangle faces up (outward winding = cross(b - a, c - a) points up).
    for t in mesh.indices.chunks(3) {
        let p = |i: u32| Vec3::from(mesh.vertices[i as usize].position);
        let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        assert!(n.y > 0.0);
    }
}

#[test]
fn file_round_trip_keeps_heights_and_layers() {
    let mut d = sloped();
    d.layers.push(TerrainLayer {
        name: "Grass".into(),
        diffuse: Some("Assets/Textures/Grass.png".into()),
        tile_size: [4.0, 4.0],
        ..Default::default()
    });
    d.alphamap[5] = [10, 245, 0, 0];
    let dir = temp_dir("roundtrip");
    let path = dir.join("Test.terrain");
    d.save(&path).unwrap();
    let back = TerrainData::load(&path).unwrap();
    assert_eq!(back.heightmap_resolution, 33);
    assert_eq!(back.size, d.size);
    assert_eq!(back.layers, d.layers);
    assert_eq!(back.alphamap, d.alphamap);
    for (a, b) in back.heights.iter().zip(&d.heights) {
        assert!((a - b).abs() <= 1.0 / 65535.0);
    }
    // Saving again is stable (quantised values round-trip exactly).
    assert_eq!(
        back.to_file_string(),
        TerrainData::from_file_str(&back.to_file_string())
            .unwrap()
            .to_file_string()
    );
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn base64_round_trips_every_length() {
    for len in 0..10 {
        let bytes: Vec<u8> = (0..len).map(|i| (i * 37 + 3) as u8).collect();
        assert_eq!(decode(&encode(&bytes)).unwrap(), bytes);
    }
    assert_eq!(encode(b"Man"), "TWFu");
    assert_eq!(encode(b"Ma"), "TWE=");
    assert!(decode("T!==").is_err());
    assert_eq!(quantize(1.0), 65535);
    assert_eq!(quantize(-1.0), 0);
}

#[test]
fn broken_files_are_sanitised() {
    let d =
        TerrainData::from_file_str(r#"{"heightmap_resolution": 30, "size": [0, 5, 10]}"#).unwrap();
    assert_eq!(d.heightmap_resolution, 33);
    assert_eq!(d.heights.len(), 33 * 33);
    assert_eq!(d.size, Vec3::new(1.0, 5.0, 10.0));
    assert!(TerrainData::from_file_str("not json").is_err());
}

#[test]
fn brush_weight_falls_off_to_the_rim() {
    let b = Brush {
        center: Vec2::new(10.0, 10.0),
        size: 8.0,
        strength: 1.0,
        falloff: 0.5,
    };
    assert_eq!(b.weight(Vec2::new(10.0, 10.0)), 1.0);
    assert_eq!(b.weight(Vec2::new(11.9, 10.0)), 1.0, "inside the hard core");
    let mid = b.weight(Vec2::new(13.0, 10.0));
    assert!(mid > 0.0 && mid < 1.0);
    assert_eq!(b.weight(Vec2::new(14.0, 10.0)), 0.0);
}

#[test]
fn raise_lower_and_set_height_change_only_the_brush_area() {
    let mut d = TerrainData::new(33, Vec3::new(32.0, 10.0, 32.0), 32);
    let b = Brush {
        center: Vec2::new(16.0, 16.0),
        size: 6.0,
        strength: 1.0,
        falloff: 0.0,
    };
    brush::raise(&mut d, &b, 1.0, false);
    assert!((d.sample_height(16.0, 16.0) - brush::RAISE_RATE * 10.0).abs() < 1e-4);
    assert_eq!(d.sample_height(2.0, 2.0), 0.0);
    brush::raise(&mut d, &b, 2.0, true);
    assert_eq!(d.sample_height(16.0, 16.0), 0.0, "lowering stops at 0");

    for _ in 0..30 {
        brush::set_height(&mut d, &b, 0.1, 4.0);
    }
    assert!((d.sample_height(16.0, 16.0) - 4.0).abs() < 0.01);
    assert!((d.sample_height(17.0, 15.0) - 4.0).abs() < 0.01);
}

#[test]
fn smoothing_flattens_a_spike() {
    let mut d = TerrainData::new(33, Vec3::new(32.0, 10.0, 32.0), 32);
    d.set_height(16, 16, 1.0);
    let b = Brush {
        center: Vec2::new(16.0, 16.0),
        size: 6.0,
        strength: 1.0,
        falloff: 0.0,
    };
    brush::smooth(&mut d, &b, 1.0);
    let peak = d.height(16, 16);
    assert!(peak < 0.2, "{peak}");
    assert!(d.height(15, 16) > 0.0, "neighbours pick up some height");
}

#[test]
fn painting_moves_weight_to_the_layer_and_keeps_the_sum() {
    let mut d = TerrainData::new(33, Vec3::new(32.0, 10.0, 32.0), 32);
    let b = Brush {
        center: Vec2::new(8.0, 8.0),
        size: 4.0,
        strength: 1.0,
        falloff: 0.0,
    };
    brush::paint(&mut d, &b, 1.0, 2);
    let w = d.weights_at(8.0, 8.0);
    assert!(w[2] > 0.99, "{w:?}");
    assert_eq!(d.weights_at(20.0, 20.0), [1.0, 0.0, 0.0, 0.0]);
    for px in &d.alphamap {
        assert_eq!(px.iter().map(|v| *v as u32).sum::<u32>(), 255);
    }
    // A light stroke blends instead of replacing.
    let light = Brush {
        strength: 0.05,
        ..b
    };
    brush::paint(&mut d, &light, 0.5, 1);
    let w = d.weights_at(8.0, 8.0);
    assert!(w[1] > 0.0 && w[1] < 0.5 && w[2] > 0.5, "{w:?}");
}

#[test]
fn resizing_the_heightmap_keeps_its_shape() {
    let mut d = sloped();
    d.set_heightmap_resolution(65);
    assert_eq!(d.heights.len(), 65 * 65);
    assert!((d.sample_height(16.0, 3.0) - 5.0).abs() < 1e-4);
    d.set_alphamap_resolution(64);
    assert_eq!(d.alphamap.len(), 64 * 64);
}

#[test]
fn rays_hit_the_surface() {
    let d = sloped();
    let hit = raycast_data(&d, Vec3::new(16.0, 50.0, 8.0), Vec3::NEG_Y).unwrap();
    assert!(hit.distance(Vec3::new(16.0, 5.0, 8.0)) < 1e-3, "{hit}");
    // Slanted ray.
    let o = Vec3::new(0.0, 20.0, 16.0);
    let hit = raycast_data(&d, o, Vec3::new(1.0, -1.0, 0.0)).unwrap();
    assert!((hit.y - d.sample_height(hit.x, hit.z)).abs() < 1e-3);
    assert!(raycast_data(&d, o, Vec3::Y).is_none());
    assert!(raycast_data(&d, Vec3::new(100.0, 5.0, 0.0), Vec3::NEG_Y).is_none());
}

/// A scene with a terrain object at `pos` using data stored under a unique path.
fn terrain_scene(name: &str, pos: Vec3, data: TerrainData) -> (Scene, hecs::Entity, String) {
    let path = temp_dir(name)
        .join("T.terrain")
        .to_string_lossy()
        .into_owned();
    store::insert(&path, data);
    let mut scene = Scene::new("t");
    let e = scene.spawn("Terrain", None);
    scene.set_transform(e, crate::scene::Transform::from_position(pos));
    scene.add(
        e,
        Terrain {
            terrain_data: path.clone(),
            ..Default::default()
        },
    );
    scene.add(e, TerrainCollider::default());
    (scene, e, path)
}

#[test]
fn scene_helpers_use_the_object_position() {
    let (scene, e, path) = terrain_scene("helpers", Vec3::new(-16.0, 2.0, 0.0), sloped());
    assert_eq!(active_terrain(&scene), Some(e));
    // World x 0 = local x 16: 5 m above the terrain's base.
    let h = sample_height(&scene, e, Vec3::new(0.0, 100.0, 3.0)).unwrap();
    assert!((h - 5.0).abs() < 1e-4);
    let (hit_e, p) = raycast(&scene, Vec3::new(0.0, 40.0, 3.0), Vec3::NEG_Y).unwrap();
    assert_eq!(hit_e, e);
    assert!((p.y - 7.0).abs() < 1e-3, "{p}");
    let draws = draws(&scene);
    assert_eq!(draws.len(), 1);
    assert_eq!(draws[0].position, Vec3::new(-16.0, 2.0, 0.0));
    let v0 = draws[0].version;
    store::modify(&path, |d| d.set_height(0, 0, 0.5));
    assert!(
        super::draws(&scene)[0].version > v0,
        "edits bump the version"
    );
}

#[test]
fn rigidbody_rests_on_the_terrain_collider() {
    use crate::physics::Physics;
    use crate::scene::{Rigidbody, SphereCollider};
    let (mut scene, _, _) = terrain_scene("collider", Vec3::new(-16.0, 0.0, -16.0), sloped());
    let ball = scene.spawn("Ball", None);
    // Over local (24, 16): ground at 7.5 m.
    scene.set_transform(
        ball,
        crate::scene::Transform::from_position(Vec3::new(8.0, 12.0, 0.0)),
    );
    scene.add(ball, Rigidbody::default());
    scene.add(ball, SphereCollider::default());
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    assert!(physics.warnings.is_empty(), "{:?}", physics.warnings);
    // Straight down onto the slope it lands, then rolls towards -x (downhill).
    for _ in 0..70 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    let p = scene.world_position(ball);
    let ground = 7.5;
    assert!(
        p.y > ground - 0.1 && p.y < ground + 1.5,
        "landed on the slope: {p}"
    );
    for _ in 0..120 {
        physics.update(&mut scene, 1.0 / 60.0);
    }
    let q = scene.world_position(ball);
    assert!(q.x < p.x - 0.5, "rolled downhill: {p} -> {q}");
    let local_x = q.x + 16.0;
    let surface = local_x.clamp(0.0, 32.0) / 32.0 * 10.0;
    assert!(
        q.y > surface - 0.05,
        "stays above the surface: {q} vs {surface}"
    );
}

#[test]
fn rays_and_controllers_collide_with_the_terrain() {
    use crate::physics::{DEFAULT_RAYCAST_LAYERS, Physics};
    use crate::scene::CharacterController;
    let (mut scene, e, _) = terrain_scene("queries", Vec3::ZERO, sloped());
    let player = scene.spawn("Player", None);
    scene.set_transform(
        player,
        crate::scene::Transform::from_position(Vec3::new(8.0, 6.0, 16.0)),
    );
    scene.add(player, CharacterController::default());
    let mut physics = Physics::build(&scene, Vec3::new(0.0, -9.81, 0.0));
    let hit = physics
        .raycast(
            Vec3::new(16.0, 30.0, 16.0),
            Vec3::NEG_Y,
            100.0,
            DEFAULT_RAYCAST_LAYERS,
        )
        .unwrap();
    assert_eq!(hit.entity, e);
    assert!((hit.point.y - 5.0).abs() < 1e-3);
    // Falling controller lands on the ground (2.5 m at x = 8).
    for _ in 0..60 {
        physics.move_character(&mut scene, player, Vec3::new(0.0, -0.2, 0.0), 1.0 / 60.0);
        physics.update(&mut scene, 1.0 / 60.0);
    }
    let p = scene.world_position(player);
    assert!(p.y > 2.5 && p.y < 4.0, "standing on the terrain: {p}");
}
