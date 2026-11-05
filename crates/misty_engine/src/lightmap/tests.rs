//! Lightmapper checks on tiny analytic scenes.

use glam::{Mat4, Quat, Vec2, Vec3};

use super::bake::{self, BakeInput, BakeLight, BakeMaterial, LightShape, Progress};
use super::uv::MARGIN;
use crate::render::mesh;
use crate::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Scene};

fn white() -> BakeMaterial {
    BakeMaterial {
        albedo: Vec3::splat(0.8),
        emission: Vec3::ZERO,
        double_sided: false,
    }
}

fn point(at: Vec3, color: Vec3, shadows: bool) -> BakeLight {
    BakeLight {
        shape: LightShape::Point,
        position: at,
        direction: Vec3::Z,
        color,
        range: 20.0,
        radius: 0.0,
        shadows,
        bake_direct: true,
    }
}

fn settings(samples: u32, bounces: u32) -> crate::scene::LightmapSettings {
    crate::scene::LightmapSettings {
        resolution: 6.0,
        samples,
        bounces,
        ..Default::default()
    }
}

/// Lightmap value at a uv2 point of receiver `k`.
fn lookup(out: &bake::BakeOutput, k: usize, uv2: Vec2) -> Vec3 {
    let r = &out.receivers[k];
    let st = r.scale_offset;
    let uv = uv2 * Vec2::new(st[0], st[1]) + Vec2::new(st[2], st[3]);
    out.maps[r.lightmap].sample(uv)
}

/// The Plane primitive's uv2 at a local point (x, z).
fn plane_uv2(x: f32, z: f32) -> Vec2 {
    let uv = Vec2::new((5.0 - x) / 10.0, (5.0 - z) / 10.0);
    Vec2::splat(MARGIN * 0.5) + uv * (1.0 - MARGIN)
}

#[test]
fn point_light_on_a_plane_follows_the_realtime_falloff() {
    let mut input = BakeInput {
        settings: settings(4, 0),
        ..Default::default()
    };
    input.add_mesh(
        &mesh::generate(PrimitiveMesh::Plane),
        Mat4::IDENTITY,
        white(),
        Some((1, 1.0)),
    );
    let h = 2.0;
    input
        .lights
        .push(point(Vec3::new(0.0, h, 0.0), Vec3::ONE, false));
    let out = bake::bake(&input, &Progress::default()).expect("not cancelled");
    assert_eq!(out.maps.len(), 1);
    let expected = |x: f32| {
        let d = (x * x + h * h).sqrt();
        let r = d / 20.0;
        let w = (1.0 - r.powi(4)).clamp(0.0, 1.0);
        (h / d) * w * w / (1.0 + 25.0 * r * r)
    };
    for x in [0.0, 1.0, 2.0, 3.0] {
        let got = lookup(&out, 0, plane_uv2(x, 0.0)).x;
        let want = expected(x);
        assert!(
            (got - want).abs() < want * 0.06 + 0.005,
            "x = {x}: baked {got}, expected {want}"
        );
    }

    // Seams: the padding ring around the renderer's square is dilated (not black).
    let st = out.receivers[0].scale_offset;
    let size = out.size as f32;
    let (x0, y0, side) = (st[2] * size, st[3] * size, st[0] * size);
    let ring = out.maps[0].get(x0 as u32 - 1, (y0 + side * 0.5) as u32);
    let inside = out.maps[0].get(x0 as u32 + 1, (y0 + side * 0.5) as u32);
    assert!(inside.x > 0.0);
    assert!(
        (ring - inside).abs().max_element() < inside.x * 0.5,
        "{ring} next to {inside}"
    );
}

/// A white floor beside a wall lit by a Baked point light: the floor next to a red wall turns
/// red, next to a white wall it stays neutral.
#[test]
fn red_wall_bleeds_onto_the_floor() {
    let ratio = |wall: Vec3| {
        let mut input = BakeInput {
            settings: settings(48, 2),
            ..Default::default()
        };
        input.add_mesh(
            &mesh::generate(PrimitiveMesh::Plane),
            Mat4::IDENTITY,
            white(),
            Some((1, 1.0)),
        );
        // Wall at x = 1 facing −X (the Quad faces −Z; turn it a quarter about +Y).
        let model = Mat4::from_scale_rotation_translation(
            Vec3::new(10.0, 4.0, 1.0),
            Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
            Vec3::new(1.0, 2.0, 0.0),
        );
        input.add_mesh(
            &mesh::generate(PrimitiveMesh::Quad),
            model,
            BakeMaterial {
                albedo: wall,
                emission: Vec3::ZERO,
                double_sided: false,
            },
            None,
        );
        input
            .lights
            .push(point(Vec3::new(-0.5, 1.5, 0.0), Vec3::splat(3.0), true));
        let out = bake::bake(&input, &Progress::default()).expect("not cancelled");
        // Floor point close to the wall.
        let c = lookup(&out, 0, plane_uv2(0.7, 0.0));
        c.x / c.y
    };
    let red = ratio(Vec3::new(0.9, 0.05, 0.05));
    let neutral = ratio(Vec3::splat(0.6));
    assert!((neutral - 1.0).abs() < 0.05, "white wall: r/g = {neutral}");
    assert!(red > neutral * 1.1, "red wall: r/g = {red}");
}

#[test]
fn emissive_surfaces_and_sky_light_receivers() {
    // A floor under a glowing ceiling panel, no lights at all.
    let mut input = BakeInput {
        settings: settings(32, 1),
        ..Default::default()
    };
    input.add_mesh(
        &mesh::generate(PrimitiveMesh::Plane),
        Mat4::IDENTITY,
        white(),
        Some((1, 1.0)),
    );
    let panel = Mat4::from_scale_rotation_translation(
        Vec3::splat(4.0),
        Quat::from_rotation_x(-std::f32::consts::FRAC_PI_2),
        Vec3::new(0.0, 1.0, 0.0),
    );
    input.add_mesh(
        &mesh::generate(PrimitiveMesh::Quad),
        panel,
        BakeMaterial {
            albedo: Vec3::ZERO,
            emission: Vec3::new(0.0, 0.0, 4.0),
            double_sided: true,
        },
        None,
    );
    input.sky.up = Vec3::new(0.2, 0.0, 0.0);
    input.sky.equator = Vec3::new(0.2, 0.0, 0.0);
    let out = bake::bake(&input, &Progress::default()).expect("not cancelled");
    let under = lookup(&out, 0, plane_uv2(0.0, 0.0));
    let far = lookup(&out, 0, plane_uv2(4.5, 4.5));
    assert!(under.z > 1.0, "panel light under it: {under}");
    assert!(far.z < under.z * 0.5);
    // The sky reaches the open corner far more than the covered middle.
    assert!(far.x > under.x, "{far} vs {under}");
}

#[test]
fn cancelling_stops_the_bake() {
    let mut input = BakeInput {
        settings: settings(4, 0),
        ..Default::default()
    };
    input.add_mesh(
        &mesh::generate(PrimitiveMesh::Plane),
        Mat4::IDENTITY,
        white(),
        Some((1, 1.0)),
    );
    let p = Progress::default();
    p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(bake::bake(&input, &p).is_none());
}

#[test]
fn lightmap_indices_round_trip_through_scene_files() {
    let mut s = Scene::new("Lit");
    let e = s.spawn("Floor", None);
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(
        e,
        MeshRenderer {
            lightmap_index: 2,
            lightmap_scale_offset: [0.25, 0.25, 0.5, 0.125],
            scale_in_lightmap: 2.0,
            contribute_gi: false,
            ..Default::default()
        },
    );
    s.render_settings.lightmapping.lightmaps = vec!["Assets/Scenes/Lit/Lightmap-0.lightmap".into()];
    s.render_settings.lightmapping.bounces = 3;
    let (back, _) = Scene::from_json("Lit", &s.to_json()).unwrap();
    let e = back.find_by_name("Floor").unwrap();
    let r = back.world.get::<&MeshRenderer>(e).unwrap();
    assert_eq!(r.lightmap_index, 2);
    assert_eq!(r.lightmap_scale_offset, [0.25, 0.25, 0.5, 0.125]);
    assert_eq!(r.scale_in_lightmap, 2.0);
    assert!(!r.contribute_gi);
    assert_eq!(
        back.render_settings.lightmapping,
        s.render_settings.lightmapping
    );
    // Defaults stay out of files.
    let plain = Scene::new("Plain").to_json();
    assert!(!plain.contains("lightmap"));
    assert!(
        super::asset_path_for("Assets/Scenes/Lit.scene", 1).ends_with("Lit/Lightmap-1.lightmap")
    );
}
