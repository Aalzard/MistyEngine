//! Bake pipeline tests: voxels, contours, tiles, detail heights, areas and agent types.

use glam::{Vec2, Vec3, Vec3Swizzles};

use super::geometry::{self, BakeGeometry, BuildInput};
use super::navmesh::BuildSettings;
use super::tests::{block, floor};
use super::tiles::{self, TileBuilder};
use super::*;
use crate::scene::{Scene, Transform};

fn settings(radius: f32, cell: f32, tile_size: u32) -> BuildSettings {
    BuildSettings {
        agent: AgentSettings {
            radius,
            ..Default::default()
        },
        agent_type: 0,
        cell_size: cell,
        tile_size,
        origin: Vec2::ZERO,
        min_region_area: 0.0,
    }
}

fn static_input(s: &Scene) -> BuildInput {
    geometry::collect_static(s)
}

/// A static 3.8 m floor inside one 4 m patch of 0.25 m voxels gives one cell per column, then
/// erosion by a 0.5 m radius removes two cells from each side.
#[test]
fn voxelising_a_box_gives_one_cell_per_column() {
    let mut s = Scene::new("T");
    floor(&mut s, 2.0, 2.0, 3.8, 3.8);
    let b = TileBuilder::new(static_input(&s), settings(0.0, 0.25, 64));
    assert_eq!(b.tiles(), vec![(0, 0)]);
    let (live, regions, contours) = tiles::debug_tile(&b, 0, 0);
    assert_eq!(live, 16 * 16);
    assert_eq!(regions, 1);
    assert_eq!(contours.len(), 1);
    // The outline: the four corners of the covered cells, at the top surface.
    let c = &contours[0];
    let mut corners: Vec<Vec3> = c
        .verts
        .iter()
        .map(|v| tiles::corner_world(&b, 0, 0, v))
        .collect();
    corners.sort_by(|a, b| (a.x, a.z).partial_cmp(&(b.x, b.z)).unwrap());
    let expect = [
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(0.0, 0.0, 4.0),
        Vec3::new(4.0, 0.0, 0.0),
        Vec3::new(4.0, 0.0, 4.0),
    ];
    assert_eq!(corners.len(), 4, "{corners:?}");
    for (a, e) in corners.iter().zip(expect) {
        assert!(a.distance(e) < 1e-4, "{a} vs {e}");
    }
    // The raw outline walks every boundary cell edge: 4 × 16.
    assert_eq!(c.raw.len(), 64);
    let eroded = TileBuilder::new(static_input(&s), settings(0.5, 0.25, 64));
    let (live, _, _) = tiles::debug_tile(&eroded, 0, 0);
    assert_eq!(live, 12 * 12);
}

/// An L of two floors is one monotone region whose simplified outline has the L's six corners.
#[test]
fn contours_trace_an_l_shape() {
    let mut s = Scene::new("T");
    // Foot: x 0..6, z 0..2; upright: x 0..2, z 2..6 (inset 0.1 so cells don't spill over).
    floor(&mut s, 3.0, 1.0, 5.8, 1.8);
    floor(&mut s, 1.0, 4.05, 1.8, 3.7);
    let b = TileBuilder::new(static_input(&s), settings(0.0, 0.25, 64));
    let (live, regions, contours) = tiles::debug_tile(&b, 0, 0);
    assert_eq!(live, 24 * 8 + 8 * 16);
    assert_eq!(regions, 1, "an L is monotone along z");
    let mut corners: Vec<(i32, i32)> = contours[0]
        .verts
        .iter()
        .map(|v| {
            let p = tiles::corner_world(&b, 0, 0, v);
            ((p.x * 4.0).round() as i32, (p.z * 4.0).round() as i32)
        })
        .collect();
    corners.sort();
    assert_eq!(
        corners,
        vec![(0, 0), (0, 24), (8, 8), (8, 24), (24, 0), (24, 8)],
        "corners in quarter metres"
    );
    // The polygons cover the L exactly (6×2 + 2×4 = 20 m²) and are convex.
    let m = bake::bake(static_input(&s), settings(0.0, 0.25, 64), None, &|_, _| {});
    assert!((m.area() - 20.0).abs() < 1e-3, "{}", m.area());
    for (_, p) in m.polys() {
        let v = &p.vertices;
        for i in 0..v.len() {
            let (a, b, c) = (
                v[i].xz(),
                v[(i + 1) % v.len()].xz(),
                v[(i + 2) % v.len()].xz(),
            );
            assert!(
                (b - a).perp_dot(c - b) > 0.0,
                "convex, counter-clockwise: {v:?}"
            );
        }
        assert!(v.len() <= 6);
    }
}

/// A floor spanning several tiles: tiles meet on shared edges, link across them, and a path
/// across the whole floor is one straight line.
#[test]
fn tiles_link_across_their_borders() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 30.0, 30.0);
    let m = bake::bake(static_input(&s), settings(0.5, 0.25, 32), None, &|_, _| {});
    assert!(m.tiles().len() >= 16, "{} tiles", m.tiles().len());
    let area = m.area();
    assert!((area - 29.0 * 29.0).abs() < 29.0 * 29.0 * 0.03, "{area}");
    // Every polygon touching an inner tile edge has a link to the next tile.
    let cross = m
        .polys()
        .flat_map(|(r, p)| p.links.iter().map(move |l| (r, l.to)))
        .filter(|(a, b)| navmesh::tile_of(*a) != navmesh::tile_of(*b))
        .count();
    assert!(cross >= 2 * 24, "{cross} links between tiles");
    let a = Vec3::new(-13.0, 0.0, -12.0);
    let b = Vec3::new(13.5, 0.0, 12.5);
    let path = m.find_path(a, b);
    assert_eq!(path.status, PathStatus::Complete);
    assert_eq!(path.corners.len(), 2, "{:?}", path.corners);
    assert!(path.polys.len() > 4, "crosses several tiles");
}

/// Rebaking after moving one box rebuilds only the tiles near it.
#[test]
fn partial_rebake_reuses_untouched_tiles() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 40.0, 40.0);
    let crate_box = block(&mut s, Vec3::new(12.0, 0.5, 12.0), Vec3::ONE, Vec3::ZERO);
    let cfg = |s: &Scene| bake::scene_settings(&static_input(s), &s.navigation);
    let first = bake::bake(static_input(&s), cfg(&s), None, &|_, _| {});
    let again = bake::bake(static_input(&s), cfg(&s), Some(&first), &|_, _| {});
    assert_eq!(bake::reused_tiles(&first, &again), first.tiles().len());
    s.set_transform(crate_box, {
        let mut t = Transform::from_position(Vec3::new(13.0, 0.5, 12.0));
        t.scale = Vec3::ONE;
        t
    });
    let moved = bake::bake(static_input(&s), cfg(&s), Some(&first), &|_, _| {});
    let rebuilt = moved.tiles().len() - bake::reused_tiles(&first, &moved);
    assert!(
        rebuilt >= 1 && rebuilt <= 4,
        "{rebuilt} of {} tiles",
        moved.tiles().len()
    );
    assert!(moved.locate(Vec3::new(13.0, 0.0, 12.0), 0.3).is_none());
    assert!(moved.locate(Vec3::new(11.6, 0.0, 12.0), 0.3).is_some());
}

/// Detail triangles follow a bumpy surface inside big polygons.
#[test]
fn detail_heights_follow_bumpy_ground() {
    let mut g = BakeGeometry::default();
    let h = |x: f32, z: f32| 0.6 * (x * 0.5).sin() * (z * 0.4).cos();
    let n = 60;
    let step = 0.25;
    for iz in 0..n {
        for ix in 0..n {
            let p = |i: usize, j: usize| {
                let (x, z) = (i as f32 * step, j as f32 * step);
                Vec3::new(x, h(x, z), z)
            };
            let (a, b, c, d) = (p(ix, iz), p(ix + 1, iz), p(ix, iz + 1), p(ix + 1, iz + 1));
            g.push([a, c, b], Vec3::Y);
            g.push([b, c, d], Vec3::Y);
        }
    }
    let input = BuildInput {
        geometry: g,
        ..Default::default()
    };
    let mut cfg = settings(0.3, 0.1, 64);
    cfg.agent.max_slope = 60.0;
    let m = bake::bake(input, cfg, None, &|_, _| {});
    assert!(
        m.polys().any(|(_, p)| !p.detail_verts.is_empty()),
        "detail points added"
    );
    let mut worst: f32 = 0.0;
    for iz in 2..14 {
        for ix in 2..14 {
            let (x, z) = (ix as f32 + 0.37, iz as f32 + 0.61);
            let p = Vec3::new(x, h(x, z), z);
            let Some(r) = m.locate(p, 1.0) else {
                continue;
            };
            worst = worst.max((m.height(r, x, z) - h(x, z)).abs());
        }
    }
    assert!(worst < 0.2, "detail height error {worst}");
}

/// Modifiers give objects areas or leave them out; volumes paint areas and cut holes.
#[test]
fn modifiers_and_volumes_set_areas() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    // A raised mud patch (a static slab) with an area override.
    let mud = block(
        &mut s,
        Vec3::new(-5.0, 0.05, 0.0),
        Vec3::new(4.0, 0.1, 4.0),
        Vec3::ZERO,
    );
    s.add(
        mud,
        NavMeshModifier {
            override_area: true,
            area: 3,
            ..Default::default()
        },
    );
    // A box left out of the bake entirely.
    let ghost = block(
        &mut s,
        Vec3::new(5.0, 1.0, 5.0),
        Vec3::splat(2.0),
        Vec3::ZERO,
    );
    s.add(
        ghost,
        NavMeshModifier {
            ignore_from_build: true,
            ..Default::default()
        },
    );
    // A Not Walkable volume and a "Jump" painted volume.
    let hole = s.spawn("Hole", None);
    s.set_transform(hole, Transform::from_position(Vec3::new(5.0, 0.0, -5.0)));
    s.add(
        hole,
        NavMeshModifierVolume {
            size: Vec3::new(3.0, 2.0, 3.0),
            area: AREA_NOT_WALKABLE,
            ..Default::default()
        },
    );
    let paint = s.spawn("Paint", None);
    s.set_transform(paint, Transform::from_position(Vec3::new(0.0, 0.0, 6.0)));
    s.add(
        paint,
        NavMeshModifierVolume {
            size: Vec3::new(4.0, 2.0, 2.0),
            area: 4,
            ..Default::default()
        },
    );
    let m = bake::bake_scene(&s);
    let area_at = |p: Vec3| m.locate(p, 0.3).map(|r| m.area_of(r));
    assert_eq!(area_at(Vec3::new(-5.0, 0.1, 0.0)), Some(3));
    assert_eq!(
        area_at(Vec3::new(5.0, 0.0, 5.0)),
        Some(AREA_WALKABLE),
        "ignored box"
    );
    assert_eq!(area_at(Vec3::new(5.0, 0.0, -5.0)), None, "cut out");
    assert_eq!(area_at(Vec3::new(0.0, 0.0, 6.0)), Some(4));
    assert_eq!(area_at(Vec3::new(0.0, 0.0, -6.0)), Some(AREA_WALKABLE));
    // Hole grown by the radius? No: volumes cut exactly, erosion only follows real edges.
    assert!(m.locate(Vec3::new(5.0, 0.0, -7.0), 0.3).is_some());
}

/// Each agent type bakes its own mesh: a big agent doesn't fit through a gap a small one does.
#[test]
fn agent_types_bake_their_own_meshes() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    // A wall across z = 0 with a 1.6 m gap at x = 0.
    block(
        &mut s,
        Vec3::new(-5.4, 1.0, 0.0),
        Vec3::new(9.2, 2.0, 0.4),
        Vec3::ZERO,
    );
    block(
        &mut s,
        Vec3::new(5.4, 1.0, 0.0),
        Vec3::new(9.2, 2.0, 0.4),
        Vec3::ZERO,
    );
    let big = s.navigation.add_agent_type("Ogre");
    s.navigation.agent_types[0].settings.radius = 1.0;
    let surface = s.spawn("Ogre Surface", None);
    s.add(
        surface,
        NavMeshSurface {
            agent_type: big,
            ..Default::default()
        },
    );
    let small = bake::bake_scene(&s);
    let large = bake::bake_surface(&s, surface).unwrap();
    assert_eq!(large.settings.agent_type, big);
    assert_eq!(large.agent.radius, 1.0);
    let (a, b) = (Vec3::new(0.0, 0.0, -5.0), Vec3::new(0.0, 0.0, 5.0));
    assert_eq!(small.find_path(a, b).status, PathStatus::Complete);
    assert_eq!(
        large.find_path(a, b).status,
        PathStatus::Partial,
        "too wide for the gap"
    );
    // The surface took the walls even though nothing is Static.
    for (e, _) in s.walk() {
        s.game_object_mut(e).is_static = false;
    }
    assert!(bake::bake_scene(&s).is_empty());
    assert!(!bake::bake_surface(&s, surface).unwrap().is_empty());
}
