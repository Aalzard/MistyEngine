//! Navigation tests: baking (walkable area, slopes, steps, holes), paths, agents, files.

use std::sync::Arc;

use glam::{Vec3, Vec3Swizzles};
use hecs::Entity;

use super::*;
use crate::scene::{MeshFilter, MeshRenderer, PrimitiveMesh, Scene, Transform};

/// A static cube of `size` centred at `pos`, rotated by `euler` degrees.
pub(crate) fn block(scene: &mut Scene, pos: Vec3, size: Vec3, euler: Vec3) -> Entity {
    let e = scene.spawn("Block", None);
    let mut t = Transform::from_position(pos).with_euler(euler);
    t.scale = size;
    scene.set_transform(e, t);
    scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    scene.add(e, MeshRenderer::default());
    scene.game_object_mut(e).is_static = true;
    e
}

/// A static floor `w` × `d` whose top is at y = 0, centred on (x, z).
pub(crate) fn floor(scene: &mut Scene, x: f32, z: f32, w: f32, d: f32) -> Entity {
    block(
        scene,
        Vec3::new(x, -0.5, z),
        Vec3::new(w, 1.0, d),
        Vec3::ZERO,
    )
}

fn baked(scene: &Scene) -> NavMesh {
    bake::bake_scene(scene)
}

fn on_mesh(m: &NavMesh, p: Vec3) -> bool {
    m.locate(p, 0.3).is_some()
}

#[test]
fn flat_floor_is_walkable_inside_the_agent_radius() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 10.0, 10.0);
    let m = baked(&s);
    // 10 m square eroded by the 0.5 m radius on every side.
    let area = m.area();
    assert!((area - 81.0).abs() < 81.0 * 0.04, "area {area}");
    assert!(
        m.polys.len() <= 4,
        "a flat square merges: {}",
        m.polys.len()
    );
    assert!(on_mesh(&m, Vec3::new(4.3, 0.0, 0.0)));
    assert!(
        !on_mesh(&m, Vec3::new(4.8, 0.0, 0.0)),
        "within the radius of the edge"
    );
    for p in &m.polys {
        assert!(p.vertices.iter().all(|v| v.y.abs() < 1e-4));
    }
    // Non-static objects are ignored.
    let mut s2 = Scene::new("T");
    let f = floor(&mut s2, 0.0, 0.0, 10.0, 10.0);
    s2.game_object_mut(f).is_static = false;
    assert!(baked(&s2).is_empty());
}

#[test]
fn slopes_steeper_than_the_limit_are_not_walkable() {
    let ramp = |deg: f32| {
        let mut s = Scene::new("T");
        block(
            &mut s,
            Vec3::new(0.0, 0.0, 0.0),
            Vec3::new(4.0, 0.2, 10.0),
            Vec3::new(-deg, 0.0, 0.0),
        );
        baked(&s)
    };
    let gentle = ramp(30.0);
    assert!(!gentle.is_empty());
    // The surface follows the slope: rising towards +z (tilted by -30° about x).
    let hit = gentle.nearest(Vec3::new(0.0, 5.0, 3.0), 10.0).unwrap();
    let expected = 3.0 * 30f32.to_radians().tan() + 0.1 / 30f32.to_radians().cos();
    assert!(
        (hit.position.y - expected).abs() < 0.1,
        "{hit:?} vs {expected}"
    );
    assert!(ramp(60.0).is_empty(), "60° is above the 45° limit");
    // Raising the limit makes it walkable.
    let mut s = Scene::new("T");
    block(
        &mut s,
        Vec3::ZERO,
        Vec3::new(4.0, 0.2, 10.0),
        Vec3::new(-60.0, 0.0, 0.0),
    );
    s.navigation.agent.max_slope = 70.0;
    assert!(!baked(&s).is_empty());
}

#[test]
fn obstacles_cut_holes_and_small_tops_are_dropped() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 12.0, 12.0);
    block(
        &mut s,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(2.0, 2.0, 2.0),
        Vec3::ZERO,
    );
    let m = baked(&s);
    assert!(!on_mesh(&m, Vec3::ZERO), "under the box");
    assert!(
        !on_mesh(&m, Vec3::new(1.3, 0.0, 0.0)),
        "closer than the radius"
    );
    assert!(on_mesh(&m, Vec3::new(1.7, 0.0, 0.0)));
    assert!(
        m.locate(Vec3::new(0.0, 2.0, 0.0), 0.5).is_none(),
        "box top < min area"
    );
    let area = m.area();
    let expected = 11.0 * 11.0 - 3.0 * 3.0;
    assert!(
        (area - expected).abs() < expected * 0.05,
        "{area} vs {expected}"
    );
}

#[test]
fn steps_up_to_the_step_height_connect() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 10.0, 10.0);
    // A low step and a tall platform, each 4 m deep, on the floor.
    let step = block(
        &mut s,
        Vec3::new(-2.5, 0.15, 0.0),
        Vec3::new(4.0, 0.3, 4.0),
        Vec3::ZERO,
    );
    let tall = block(
        &mut s,
        Vec3::new(2.5, 0.6, 0.0),
        Vec3::new(4.0, 1.2, 4.0),
        Vec3::ZERO,
    );
    let m = baked(&s);
    let from = Vec3::new(0.0, 0.0, -4.0);
    let up_step = m.find_path(from, Vec3::new(-2.5, 0.3, 0.0));
    assert_eq!(up_step.status, PathStatus::Complete);
    let up_tall = m.find_path(from, Vec3::new(2.5, 1.2, 0.0));
    assert_eq!(
        up_tall.status,
        PathStatus::Partial,
        "1.2 m is above the 0.75 m step"
    );
    let _ = (step, tall);
}

#[test]
fn paths_go_around_walls_and_stay_on_the_mesh() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    // Wall across z = 0 from x = -10 to x = 4.
    block(
        &mut s,
        Vec3::new(-3.0, 1.0, 0.0),
        Vec3::new(14.0, 2.0, 0.5),
        Vec3::ZERO,
    );
    let m = baked(&s);
    let (a, b) = (Vec3::new(-5.0, 0.0, -5.0), Vec3::new(-5.0, 0.0, 5.0));
    let path = m.find_path(a, b);
    assert_eq!(path.status, PathStatus::Complete);
    assert!(path.corners.len() >= 4, "{:?}", path.corners);
    assert!((path.corners[0] - a).length() < 1e-3);
    assert!((path.corners.last().unwrap().xz() - b.xz()).length() < 1e-3);
    // It rounds the wall's end (x = 4) at about the agent radius.
    let max_x = path.corners.iter().map(|c| c.x).fold(f32::MIN, f32::max);
    assert!(max_x > 4.2 && max_x < 5.0, "{max_x}");
    // Shortest: about 2 × √(9.5² + 5²) plus a little for the corner.
    let len = path.length();
    assert!(len > 20.0 && len < 23.0, "{len}");
    for w in path.corners.windows(2) {
        for i in 0..=20 {
            let p = w[0].lerp(w[1], i as f32 / 20.0);
            assert!(m.nearest(p, 0.05).is_some(), "{p} is off the mesh");
        }
    }
    // A straight line in the open has just its two ends.
    let open = m.find_path(Vec3::new(-5.0, 0.0, 3.0), Vec3::new(5.0, 0.0, 8.0));
    assert_eq!(open.corners.len(), 2, "{:?}", open.corners);
}

#[test]
fn unreachable_targets_give_a_partial_path() {
    let mut s = Scene::new("T");
    floor(&mut s, -5.0, 0.0, 6.0, 6.0);
    floor(&mut s, 5.0, 0.0, 6.0, 6.0);
    let m = baked(&s);
    let p = m.find_path(Vec3::new(-6.0, 0.0, 0.0), Vec3::new(6.0, 0.0, 0.0));
    assert_eq!(p.status, PathStatus::Partial);
    let end = *p.corners.last().unwrap();
    assert!(
        (end.x - -2.5).abs() < 0.2,
        "ends at the near island's edge: {end}"
    );
    assert_eq!(
        m.find_path(Vec3::new(0.0, 50.0, 0.0), Vec3::ZERO).status,
        PathStatus::Invalid
    );
}

#[test]
fn nav_mesh_files_round_trip() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 8.0, 8.0);
    block(
        &mut s,
        Vec3::new(0.0, 1.0, 0.0),
        Vec3::new(1.0, 2.0, 3.0),
        Vec3::ZERO,
    );
    let m = baked(&s);
    let text = m.to_json();
    assert_eq!(
        text.lines().count(),
        m.polys.len() + 2,
        "one polygon per line"
    );
    let back = NavMesh::from_json(&text).unwrap();
    assert_eq!(back.polys, m.polys);
    assert_eq!(back.agent, m.agent);
    let dir = std::env::temp_dir().join(format!("misty_navmesh_{}", std::process::id()));
    let path = dir.join("Level/NavMesh.navmesh");
    m.save(&path).unwrap();
    let loaded = store::load(&path).unwrap();
    assert_eq!(loaded.polys, m.polys);
    assert!(NavMesh::from_json("{\"version\":1}").is_err());
    let broken = text.replacen("\"to\":", "\"to\":9999", 1);
    if broken != text {
        assert!(NavMesh::from_json(&broken).is_err());
    }
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn scene_files_keep_navigation_settings() {
    let mut s = Scene::new("T");
    assert!(!s.to_json().contains("navigation"), "defaults are left out");
    s.navigation.agent.radius = 0.3;
    s.navigation.nav_mesh = Some("Assets/Scenes/T/NavMesh.navmesh".into());
    let (back, report) = Scene::from_json("T", &s.to_json()).unwrap();
    assert!(report.warnings.is_empty());
    assert_eq!(back.navigation, s.navigation);
    assert_eq!(
        NavigationSettings::asset_path_for("Assets/Scenes/Level 1.scene"),
        "Assets/Scenes/Level 1/NavMesh.navmesh"
    );
}

/// Scene with an agent at `start`, the runtime built from its bake.
fn agent_scene(mut s: Scene, start: Vec3) -> (Scene, Entity, NavRuntime) {
    let e = s.spawn("Agent", None);
    s.set_transform(e, Transform::from_position(start + Vec3::Y));
    s.add(
        e,
        NavMeshAgent {
            base_offset: 1.0,
            ..Default::default()
        },
    );
    let nav = NavRuntime::new(Some(Arc::new(baked(&s))));
    (s, e, nav)
}

fn run(s: &mut Scene, nav: &mut NavRuntime, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        nav.update(s, 1.0 / 60.0);
    }
}

#[test]
fn agents_walk_around_walls_to_their_destination() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    block(
        &mut s,
        Vec3::new(-3.0, 1.0, 0.0),
        Vec3::new(14.0, 2.0, 0.5),
        Vec3::ZERO,
    );
    let (mut s, e, mut nav) = agent_scene(s, Vec3::new(-5.0, 0.0, -5.0));
    let goal = Vec3::new(-5.0, 0.0, 5.0);
    assert!(nav.set_destination(&s, e, goal));
    let st = nav.state(e).unwrap();
    assert!(st.has_path() && st.remaining_distance() > 20.0);
    let mut max_speed: f32 = 0.0;
    for _ in 0..(12 * 60) {
        nav.update(&mut s, 1.0 / 60.0);
        max_speed = max_speed.max(nav.state(e).unwrap().velocity.length());
        let p = s.world_position(e);
        assert!(
            p.z.abs() > 0.5 || p.x > 4.2,
            "walked through the wall at {p}"
        );
    }
    let p = s.world_position(e);
    assert!((p.xz() - goal.xz()).length() < 0.05, "{p}");
    assert!((p.y - 1.0).abs() < 1e-3, "base offset above the floor: {p}");
    assert!(max_speed <= 3.5 + 1e-3 && max_speed > 3.0, "{max_speed}");
    let st = nav.state(e).unwrap();
    assert!(!st.has_path(), "arriving clears the path");
    assert_eq!(st.remaining_distance(), 0.0);
}

#[test]
fn agents_follow_ramps_up_to_platforms() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 30.0, 30.0);
    // 2 m high platform at z = 8..14, reached by a 20° ramp from the floor.
    block(
        &mut s,
        Vec3::new(0.0, 1.0, 11.0),
        Vec3::new(6.0, 2.0, 6.0),
        Vec3::ZERO,
    );
    let len = 2.0 / 20f32.to_radians().sin();
    let run_z = len * 20f32.to_radians().cos();
    block(
        &mut s,
        Vec3::new(0.0, 1.0 - 0.1, 8.0 - run_z * 0.5),
        Vec3::new(3.0, 0.2, len + 0.3),
        Vec3::new(-20.0, 0.0, 0.0),
    );
    let (mut s, e, mut nav) = agent_scene(s, Vec3::new(0.0, 0.0, -6.0));
    assert!(nav.set_destination(&s, e, Vec3::new(0.0, 2.0, 12.0)));
    assert_eq!(nav.state(e).unwrap().status, PathStatus::Complete);
    let mut max_y: f32 = 0.0;
    for _ in 0..(10 * 60) {
        nav.update(&mut s, 1.0 / 60.0);
        let p = s.world_position(e);
        max_y = max_y.max(p.y);
        // On the ramp the height follows the slope.
        if p.z > 8.0 - run_z + 0.5 && p.z < 7.5 {
            let expected = (p.z - (8.0 - run_z)) * 20f32.to_radians().tan() + 1.0;
            assert!((p.y - expected).abs() < 0.25, "{p} vs {expected}");
        }
    }
    let p = s.world_position(e);
    assert!((p - Vec3::new(0.0, 3.0, 12.0)).length() < 0.1, "{p}");
}

#[test]
fn agents_keep_apart_and_stop_at_the_stopping_distance() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    let (mut s, a, _) = agent_scene(s, Vec3::new(-4.0, 0.0, 0.0));
    let b = s.spawn("Agent B", None);
    s.set_transform(b, Transform::from_position(Vec3::new(4.0, 1.0, 0.3)));
    s.add(
        b,
        NavMeshAgent {
            base_offset: 1.0,
            stopping_distance: 1.0,
            ..Default::default()
        },
    );
    let mut nav = NavRuntime::new(Some(Arc::new(baked(&s))));
    let goal = Vec3::new(0.0, 0.0, 0.0);
    assert!(nav.set_destination(&s, a, goal));
    assert!(nav.set_destination(&s, b, goal));
    run(&mut s, &mut nav, 6.0);
    let (pa, pb) = (s.world_position(a), s.world_position(b));
    assert!(pa.xz().distance(pb.xz()) > 0.9, "{pa} {pb}");
    let st = nav.state(b).unwrap();
    assert!(st.has_path(), "a stopping distance keeps the path");
    assert!(
        st.remaining_distance() <= 1.3,
        "{}",
        st.remaining_distance()
    );
    assert!(st.velocity.length() < 0.2);
}

#[test]
fn warp_reset_and_teleports() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 10.0, 10.0);
    let (mut s, e, mut nav) = agent_scene(s, Vec3::new(-3.0, 0.0, 0.0));
    assert!(nav.set_destination(&s, e, Vec3::new(3.0, 0.0, 0.0)));
    nav.reset_path(&s, e);
    assert!(!nav.state(e).unwrap().has_path());
    assert!(nav.warp(&mut s, e, Vec3::new(2.0, 0.5, 2.0)));
    assert!((s.world_position(e) - Vec3::new(2.0, 1.0, 2.0)).length() < 1e-3);
    assert!(
        !nav.warp(&mut s, e, Vec3::new(50.0, 0.0, 0.0)),
        "far from the mesh"
    );
    // Moving the Transform from a script re-places the agent there.
    assert!(nav.warp(&mut s, e, Vec3::new(0.0, 0.0, 0.0)));
    s.set_world_position(e, Vec3::new(-2.0, 1.0, -2.0));
    nav.update(&mut s, 1.0 / 60.0);
    let st = nav.state(e).unwrap();
    assert!((st.position - Vec3::new(-2.0, 0.0, -2.0)).length() < 1e-3);
    // Off-mesh agents can't take paths.
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 10.0, 10.0);
    let (s, e, mut nav) = agent_scene(s, Vec3::new(40.0, 0.0, 0.0));
    assert!(!nav.set_destination(&s, e, Vec3::ZERO));
    assert!(
        nav.take_logs()
            .iter()
            .any(|l| l.contains("not close enough"))
    );
}

#[test]
fn obstacles_push_agents_away() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
    let o = s.spawn("Obstacle", None);
    s.set_transform(o, Transform::from_position(Vec3::new(0.0, 1.0, 0.0)));
    s.add(o, NavMeshObstacle::default());
    let (mut s, e, mut nav) = agent_scene(s, Vec3::new(-5.0, 0.0, 0.0));
    assert!(nav.set_destination(&s, e, Vec3::new(5.0, 0.0, 0.0)));
    for _ in 0..(5 * 60) {
        nav.update(&mut s, 1.0 / 60.0);
        let p = s.world_position(e);
        // Never closer to the 1 m box than (about) the agent radius.
        let gap = (p.xz().abs() - glam::Vec2::splat(0.5)).max(glam::Vec2::ZERO);
        assert!(gap.length() > 0.45, "{p}");
    }
    assert!(s.world_position(e).x > 4.0);
}
