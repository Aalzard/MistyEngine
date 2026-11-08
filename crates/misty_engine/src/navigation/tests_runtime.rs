//! Runtime tests: carving and tile rebuilds, off-mesh link traversal, crowds with ORCA, agent
//! types and runtime surface rebuilds.

use std::sync::Arc;

use glam::{Vec3, Vec3Swizzles};
use hecs::Entity;

use super::agent::NavEventKind;
use super::tests::{block, floor};
use super::*;
use crate::scene::{Scene, Transform};

fn agent(s: &mut Scene, name: &str, at: Vec3, a: NavMeshAgent) -> Entity {
    let e = s.spawn(name, None);
    s.set_transform(e, Transform::from_position(at + Vec3::Y * a.base_offset));
    s.add(e, a);
    e
}

fn run(s: &mut Scene, nav: &mut NavRuntime, seconds: f32) {
    for _ in 0..(seconds * 60.0) as usize {
        nav.update(s, 1.0 / 60.0);
    }
}

fn runtime(s: &Scene) -> NavRuntime {
    let mut nav = NavRuntime::new(Some(Arc::new(bake::bake_scene(s))));
    nav.sync_rebuilds = true;
    nav
}

/// A carving obstacle that settles in an agent's way cuts a hole: only the tiles it touches
/// are rebuilt, the agent plans round it; when it goes the hole closes again.
#[test]
fn carving_rebuilds_only_the_tiles_it_touches() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 40.0, 40.0);
    let walker = agent(
        &mut s,
        "Walker",
        Vec3::new(-15.0, 0.0, 1.0),
        NavMeshAgent::default(),
    );
    let mut nav = runtime(&s);
    let tiles = nav.mesh().unwrap().tiles().len();
    assert!(tiles >= 16, "{tiles}");
    let rock = s.spawn("Rock", None);
    s.set_transform(rock, {
        let mut t = Transform::from_position(Vec3::new(0.0, 1.0, 1.0));
        t.scale = Vec3::new(3.0, 2.0, 3.0);
        t
    });
    s.add(
        rock,
        NavMeshObstacle {
            carve: true,
            ..Default::default()
        },
    );
    // Not stationary long enough yet: no carving.
    nav.update(&mut s, 0.1);
    assert_eq!(nav.stats.tiles_rebuilt, 0);
    run(&mut s, &mut nav, 0.6);
    nav.finish_rebuilds(&s);
    nav.update(&mut s, 1.0 / 60.0);
    let rebuilt = nav.stats.tiles_rebuilt;
    assert!(
        rebuilt >= 1 && rebuilt <= 4,
        "{rebuilt} of {tiles} tiles rebuilt"
    );
    let m = nav.mesh().unwrap();
    assert!(m.locate(Vec3::new(0.0, 0.0, 1.0), 0.3).is_none(), "carved");
    assert!(
        m.locate(Vec3::new(0.0, 0.0, 1.0 + 2.2), 0.3).is_some(),
        "only the obstacle + radius"
    );
    // The agent plans round the hole.
    assert!(nav.set_destination(&s, walker, Vec3::new(15.0, 0.0, 1.0)));
    let st = nav.state(walker).unwrap();
    assert!(st.corners.len() >= 2, "{:?}", st.corners);
    run(&mut s, &mut nav, 12.0);
    let p = s.world_position(walker);
    assert!(p.distance(Vec3::new(15.0, 0.0, 1.0)) < 0.3, "arrived: {p}");
    // Taken away: the hole closes.
    s.world.get::<&mut NavMeshObstacle>(rock).unwrap().enabled = false;
    nav.update(&mut s, 1.0 / 60.0);
    nav.finish_rebuilds(&s);
    nav.update(&mut s, 1.0 / 60.0);
    assert!(
        nav.mesh()
            .unwrap()
            .locate(Vec3::new(0.0, 0.0, 1.0), 0.3)
            .is_some()
    );
    assert!(nav.stats.tiles_rebuilt <= rebuilt * 2);
}

/// A moving obstacle that carves while moving keeps its hole following it.
#[test]
fn moving_carvers_carve_as_they_go() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 30.0, 30.0);
    let door = s.spawn("Door", None);
    s.set_transform(door, Transform::from_position(Vec3::new(-5.0, 1.0, 0.0)));
    s.add(
        door,
        NavMeshObstacle {
            carve: true,
            carve_only_stationary: false,
            move_threshold: 0.5,
            ..Default::default()
        },
    );
    let mut nav = runtime(&s);
    nav.update(&mut s, 1.0 / 60.0);
    nav.finish_rebuilds(&s);
    nav.update(&mut s, 1.0 / 60.0);
    assert!(
        nav.mesh()
            .unwrap()
            .locate(Vec3::new(-5.0, 0.0, 0.0), 0.3)
            .is_none()
    );
    for i in 0..60 {
        let x = -5.0 + i as f32 * 0.2;
        s.set_world_position(door, Vec3::new(x, 1.0, 0.0));
        nav.update(&mut s, 1.0 / 60.0);
    }
    nav.finish_rebuilds(&s);
    nav.update(&mut s, 1.0 / 60.0);
    let m = nav.mesh().unwrap();
    assert!(
        m.locate(Vec3::new(-5.0, 0.0, 0.0), 0.3).is_some(),
        "old spot open again"
    );
    assert!(
        m.locate(Vec3::new(6.8, 0.0, 0.0), 0.3).is_none(),
        "hole at the new spot"
    );
    assert!(nav.stats.rebuilds >= 3);
}

/// Agents cross an off-mesh link (jumping), firing start and end events; with automatic
/// traversal off they wait at the link until `CompleteOffMeshLink`.
#[test]
fn agents_cross_off_mesh_links() {
    let mut s = Scene::new("T");
    floor(&mut s, -5.0, 0.0, 6.0, 6.0);
    floor(&mut s, 5.0, 0.0, 6.0, 6.0);
    let link = s.spawn("Jump", None);
    s.add(
        link,
        NavMeshLink {
            start: Vec3::new(-2.6, 0.0, 0.0),
            end: Vec3::new(2.6, 0.0, 0.0),
            ..Default::default()
        },
    );
    let jumper = agent(
        &mut s,
        "Jumper",
        Vec3::new(-6.0, 0.0, 1.0),
        NavMeshAgent::default(),
    );
    let waiter = agent(
        &mut s,
        "Waiter",
        Vec3::new(-6.0, 0.0, -1.5),
        NavMeshAgent {
            auto_traverse_off_mesh_link: false,
            ..Default::default()
        },
    );
    let mut nav = runtime(&s);
    nav.update(&mut s, 1.0 / 60.0);
    let goal = Vec3::new(6.0, 0.0, 0.5);
    assert!(nav.set_destination(&s, jumper, goal));
    assert_eq!(nav.state(jumper).unwrap().status, PathStatus::Complete);
    assert!(nav.set_destination(&s, waiter, goal + Vec3::Z * -2.0));
    let mut events = Vec::new();
    let mut highest: f32 = 0.0;
    let mut on_link = false;
    for _ in 0..(6 * 60) {
        nav.update(&mut s, 1.0 / 60.0);
        events.extend(nav.take_events());
        let st = nav.state(jumper).unwrap();
        on_link |= st.is_on_off_mesh_link();
        if st.is_on_off_mesh_link() {
            highest = highest.max(s.world_position(jumper).y);
        }
    }
    assert!(on_link);
    assert!(highest > 0.5, "jump links arc: {highest}");
    assert!(
        s.world_position(jumper).distance(goal) < 0.3,
        "{}",
        s.world_position(jumper)
    );
    let mine: Vec<_> = events
        .iter()
        .filter(|e| e.entity == jumper)
        .map(|e| e.kind)
        .collect();
    assert_eq!(mine, [NavEventKind::LinkStart, NavEventKind::LinkEnd]);
    // The waiter sits at the link start until told.
    let w = nav.state(waiter).unwrap();
    assert!(w.is_on_off_mesh_link());
    assert!(s.world_position(waiter).x < -2.0);
    assert!(nav.complete_off_mesh_link(&mut s, waiter));
    assert!(s.world_position(waiter).x > 2.0);
    run(&mut s, &mut nav, 3.0);
    assert!(s.world_position(waiter).distance(goal + Vec3::Z * -2.0) < 0.3);
}

/// Two groups walk through each other: ORCA keeps every pair apart all the way.
#[test]
fn crowds_cross_without_overlapping() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 30.0, 30.0);
    let mut agents = Vec::new();
    let n = 10;
    for i in 0..n {
        let a = i as f32 / n as f32 * std::f32::consts::TAU;
        let p = Vec3::new(a.cos(), 0.0, a.sin()) * 7.0;
        let e = agent(
            &mut s,
            &format!("A{i}"),
            p,
            NavMeshAgent {
                radius: 0.4,
                speed: 2.5,
                ..Default::default()
            },
        );
        agents.push((e, -p));
    }
    let mut nav = runtime(&s);
    nav.update(&mut s, 1.0 / 60.0);
    for (e, goal) in &agents {
        assert!(nav.set_destination(&s, *e, *goal));
    }
    let mut min_gap = f32::MAX;
    for _ in 0..(20 * 60) {
        nav.update(&mut s, 1.0 / 60.0);
        for i in 0..n {
            for j in i + 1..n {
                let (a, b) = (s.world_position(agents[i].0), s.world_position(agents[j].0));
                min_gap = min_gap.min(a.xz().distance(b.xz()) - 0.8);
            }
        }
    }
    assert!(min_gap > -0.08, "agents overlapped by {}", -min_gap);
    for (e, goal) in &agents {
        let p = s.world_position(*e);
        assert!(p.xz().distance(goal.xz()) < 0.6, "{p} vs {goal}");
    }
}

/// Agents of each type walk on their own type's mesh.
#[test]
fn agents_use_their_agent_types_mesh() {
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 20.0, 20.0);
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
    let root = std::env::temp_dir().join(format!("misty_nav_types_{}", std::process::id()));
    let small_rel = "Assets/T/NavMesh.navmesh";
    let big_rel = "Assets/T/NavMesh-Ogre.navmesh";
    bake::bake_scene(&s).save(&root.join(small_rel)).unwrap();
    bake::bake_surface(&s, surface)
        .unwrap()
        .save(&root.join(big_rel))
        .unwrap();
    s.navigation.nav_mesh = Some(small_rel.into());
    s.world
        .get::<&mut NavMeshSurface>(surface)
        .unwrap()
        .nav_mesh_data = Some(big_rel.into());
    let human = agent(
        &mut s,
        "Human",
        Vec3::new(1.0, 0.0, -5.0),
        NavMeshAgent::default(),
    );
    let ogre = agent(
        &mut s,
        "Ogre",
        Vec3::new(-1.0, 0.0, -5.0),
        NavMeshAgent {
            agent_type: big,
            radius: 1.0,
            ..Default::default()
        },
    );
    let mut nav = NavRuntime::load(&s, &root);
    assert_eq!(nav.meshes().count(), 2);
    nav.update(&mut s, 1.0 / 60.0);
    nav.set_destination(&s, human, Vec3::new(1.0, 0.0, 5.0));
    nav.set_destination(&s, ogre, Vec3::new(-1.0, 0.0, 5.0));
    assert_eq!(nav.state(human).unwrap().status, PathStatus::Complete);
    assert_eq!(nav.state(ogre).unwrap().status, PathStatus::Partial);
    run(&mut s, &mut nav, 8.0);
    assert!(s.world_position(human).z > 4.0);
    assert!(
        s.world_position(ogre).z < -0.5,
        "stopped at the gap: {}",
        s.world_position(ogre)
    );
    let _ = std::fs::remove_dir_all(root);
}

/// `UpdateNavMesh` while playing rebuilds only the tiles whose objects changed.
#[test]
fn runtime_surface_updates_rebuild_changed_tiles() {
    let mut s = Scene::new("T");
    let f = floor(&mut s, 0.0, 0.0, 40.0, 40.0);
    s.game_object_mut(f).is_static = false;
    let surface = s.spawn("Surface", None);
    s.add(surface, NavMeshSurface::default());
    let mut nav = NavRuntime::default();
    nav.sync_rebuilds = true;
    // No baked data: BuildNavMesh makes it.
    assert!(nav.rebuild_surface(&s, surface, true));
    nav.finish_rebuilds(&s);
    let tiles = nav.mesh().unwrap().tiles().len();
    assert!(tiles >= 16);
    let built = nav.stats.tiles_rebuilt;
    assert_eq!(built, tiles);
    block(
        &mut s,
        Vec3::new(10.0, 1.0, 10.0),
        Vec3::splat(2.0),
        Vec3::ZERO,
    );
    assert!(nav.rebuild_surface(&s, surface, false));
    nav.finish_rebuilds(&s);
    let changed = nav.stats.tiles_rebuilt - built;
    assert!(changed >= 1 && changed <= 4, "{changed} of {tiles}");
    assert!(
        nav.mesh()
            .unwrap()
            .locate(Vec3::new(10.0, 0.0, 10.0), 0.3)
            .is_none()
    );
}

/// Bake and agent timings (printed): run with
/// `cargo test --release -p misty_engine -- --ignored nav_perf_report --nocapture`.
#[test]
#[ignore = "timings: run on purpose in release"]
fn nav_perf_report() {
    use std::time::Instant;
    // A 160 m bumpy heightfield with 80 boxes on it.
    let mut g = geometry::BakeGeometry::default();
    let h = |x: f32, z: f32| 1.5 * (x * 0.07).sin() * (z * 0.05).cos() + 0.3 * (x * 0.4).sin();
    let n = 320;
    let step = 0.5;
    for iz in 0..n {
        for ix in 0..n {
            let p = |i: usize, j: usize| {
                let (x, z) = (i as f32 * step - 80.0, j as f32 * step - 80.0);
                Vec3::new(x, h(x, z), z)
            };
            let (a, b, c, d) = (p(ix, iz), p(ix + 1, iz), p(ix, iz + 1), p(ix + 1, iz + 1));
            g.push([a, c, b], Vec3::Y);
            g.push([b, c, d], Vec3::Y);
        }
    }
    let mut s = Scene::new("T");
    for i in 0..80 {
        let (x, z) = ((i % 9) as f32 * 17.0 - 68.0, (i / 9) as f32 * 17.0 - 68.0);
        block(
            &mut s,
            Vec3::new(x, h(x, z), z),
            Vec3::new(2.0, 3.0, 2.0),
            Vec3::ZERO,
        );
    }
    let boxes = geometry::collect_static(&s);
    let base = g.triangles.len();
    g.triangles.extend(boxes.geometry.triangles);
    g.normals.extend(boxes.geometry.normals);
    g.solids.extend(
        boxes
            .geometry
            .solids
            .into_iter()
            .map(|r| r.start + base..r.end + base),
    );
    let input = geometry::BuildInput {
        geometry: g,
        ..Default::default()
    };
    let settings = bake::scene_settings(&input, &NavigationSettings::default());
    let t = Instant::now();
    let m = bake::bake(input.clone(), settings, None, &|_, _| {});
    let full = t.elapsed();
    let t = Instant::now();
    let again = bake::bake(input, settings, Some(&m), &|_, _| {});
    let partial = t.elapsed();
    eprintln!(
        "bake 160 m terrain ({} triangles, voxel {:.3} m): {} tiles, {} polygons, {:.1} m² in {:.2} s; \
         unchanged rebake (all {} tiles reused) {:.3} s",
        base + 80 * 12,
        settings.cell_size,
        m.tiles().len(),
        m.poly_count(),
        m.area(),
        full.as_secs_f32(),
        bake::reused_tiles(&m, &again),
        partial.as_secs_f32()
    );
    // Agents crossing a 60 m floor.
    for count in [100usize, 400] {
        let mut s = Scene::new("T");
        floor(&mut s, 0.0, 0.0, 60.0, 60.0);
        let mut ids = Vec::new();
        let side = (count as f32).sqrt().ceil() as usize;
        for i in 0..count {
            let (x, z) = (
                (i % side) as f32 * 1.4 - 14.0,
                (i / side) as f32 * 1.4 - 14.0,
            );
            let e = agent(
                &mut s,
                &format!("A{i}"),
                Vec3::new(x, 0.0, z),
                NavMeshAgent::default(),
            );
            ids.push((e, Vec3::new(-x * 1.5, 0.0, -z * 1.5)));
        }
        let mut nav = runtime(&s);
        nav.update(&mut s, 1.0 / 60.0);
        let t = Instant::now();
        for (e, goal) in &ids {
            nav.set_destination(&s, *e, *goal);
        }
        let plan = t.elapsed();
        let frames = 300;
        let t = Instant::now();
        for _ in 0..frames {
            nav.update(&mut s, 1.0 / 60.0);
        }
        let per = t.elapsed().as_secs_f32() * 1000.0 / frames as f32;
        eprintln!(
            "{count} agents (High Quality avoidance): SetDestination {:.3} ms each, update {per:.2} ms per frame",
            plan.as_secs_f32() * 1000.0 / count as f32
        );
    }
    // Carving: one obstacle settling on a 40 m floor.
    let mut s = Scene::new("T");
    floor(&mut s, 0.0, 0.0, 40.0, 40.0);
    let mut nav = runtime(&s);
    let rock = s.spawn("Rock", None);
    s.set_transform(rock, Transform::from_position(Vec3::new(3.0, 1.0, 3.0)));
    s.add(
        rock,
        NavMeshObstacle {
            carve: true,
            carve_only_stationary: false,
            ..Default::default()
        },
    );
    let t = Instant::now();
    nav.update(&mut s, 1.0 / 60.0);
    nav.finish_rebuilds(&s);
    eprintln!(
        "carve: {} tiles rebuilt in {:.2} ms (first carve includes collecting the scene)",
        nav.stats.tiles_rebuilt,
        t.elapsed().as_secs_f32() * 1000.0
    );
    s.set_world_position(rock, Vec3::new(4.0, 1.0, 3.0));
    let t = Instant::now();
    nav.update(&mut s, 1.0 / 60.0);
    nav.finish_rebuilds(&s);
    eprintln!(
        "carve again after moving: {} tiles in total, {:.2} ms",
        nav.stats.tiles_rebuilt,
        t.elapsed().as_secs_f32() * 1000.0
    );
}
