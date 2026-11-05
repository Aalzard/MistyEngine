//! Occlusion bake and runtime tests on small walled scenes.

use std::sync::Arc;

use glam::{UVec3, Vec3};

use super::bake::{BakeInput, Progress, bake};
use super::data::{INVALID, OUTSIDE};
use super::*;
use crate::render::culling::Aabb;
use crate::scene::{
    Camera, MeshFilter, MeshRenderer, OcclusionArea, PrimitiveMesh, Scene, StaticFlags, Transform,
};

/// A static box (occluder and occludee).
fn block(s: &mut Scene, name: &str, center: Vec3, size: Vec3) -> hecs::Entity {
    let e = s.spawn(name, None);
    s.set_transform(
        e,
        Transform {
            position: center,
            scale: size,
            ..Default::default()
        },
    );
    s.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.add(e, MeshRenderer::default());
    s.game_object_mut(e).is_static = true;
    e
}

/// A 1 m crate: Occludee Static only.
fn crate_at(s: &mut Scene, name: &str, at: Vec3) -> hecs::Entity {
    let e = block(s, name, at + Vec3::Y * 0.5, Vec3::ONE);
    s.game_object_mut(e)
        .static_flags
        .set(StaticFlags::OCCLUDER, false);
    e
}

/// 30 × 30 m yard: a closed room (crate "Locked" inside), a room with a doorway (crate "Peek"
/// inside), a long wall with a crate "Behind" on its far side, and crates in the open.
fn yard() -> Scene {
    let mut s = Scene::new("Yard");
    let floor = s.spawn("Floor", None);
    s.set_transform(
        floor,
        Transform {
            scale: Vec3::new(3.0, 1.0, 3.0),
            ..Default::default()
        },
    );
    s.add(floor, MeshFilter::primitive(PrimitiveMesh::Plane));
    s.add(floor, MeshRenderer::default());
    s.game_object_mut(floor).is_static = true;
    // Closed room centred at (-9, -9), 8 m square, 4 m tall walls 0.5 m thick.
    room(&mut s, Vec3::new(-9.0, 0.0, -9.0), false);
    crate_at(&mut s, "Locked", Vec3::new(-9.0, 0.0, -9.0));
    // Room with a 2 m doorway in its south wall at (9, -9).
    room(&mut s, Vec3::new(9.0, 0.0, -9.0), true);
    crate_at(&mut s, "Peek", Vec3::new(9.0, 0.0, -10.5));
    // A long wall along z = 6 from x = -14 to 4, crate behind it.
    block(
        &mut s,
        "Long Wall",
        Vec3::new(-5.0, 2.0, 6.0),
        Vec3::new(18.0, 4.0, 0.5),
    );
    crate_at(&mut s, "Behind", Vec3::new(-6.0, 0.0, 8.0));
    crate_at(&mut s, "Open A", Vec3::new(0.0, 0.0, 0.0));
    crate_at(&mut s, "Open B", Vec3::new(10.0, 0.0, 10.0));
    let area = s.spawn("Occlusion Area", None);
    s.set_transform(area, Transform::from_position(Vec3::new(0.0, 1.5, 0.0)));
    s.add(
        area,
        OcclusionArea {
            size: [30.0, 3.0, 30.0],
            ..Default::default()
        },
    );
    s.render_settings.occlusion.smallest_occluder = 3.0;
    s.render_settings.occlusion.smallest_hole = 1.0;
    s
}

fn room(s: &mut Scene, c: Vec3, door: bool) {
    let (h, t, w) = (4.0, 0.5, 8.0);
    let y = Vec3::Y * h * 0.5;
    block(
        s,
        "North",
        c + y + Vec3::Z * w * 0.5,
        Vec3::new(w + t, h, t),
    );
    block(s, "East", c + y + Vec3::X * w * 0.5, Vec3::new(t, h, w + t));
    block(s, "West", c + y - Vec3::X * w * 0.5, Vec3::new(t, h, w + t));
    let south = c + y - Vec3::Z * w * 0.5;
    if door {
        // Two pieces leaving a 2 m gap in the middle.
        let piece = (w + t - 2.0) * 0.5;
        let off = Vec3::X * (1.0 + piece * 0.5);
        block(s, "South L", south - off, Vec3::new(piece, h, t));
        block(s, "South R", south + off, Vec3::new(piece, h, t));
    } else {
        block(s, "South", south, Vec3::new(w + t, h, t));
    }
}

fn baked(s: &Scene) -> OcclusionData {
    bake(&BakeInput::from_scene(s), &Progress::default()).expect("not cancelled")
}

fn index_of(s: &Scene, name: &str) -> usize {
    let e = s.find_by_name(name).unwrap();
    bake::occludees(s)
        .iter()
        .position(|(o, _)| *o == e)
        .unwrap()
}

/// Deterministic 0..1 values.
struct Lcg(u64);
impl Lcg {
    fn next(&mut self) -> f32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 40) as f32 / (1u64 << 24) as f32
    }
    fn in_box(&mut self, b: &Aabb) -> Vec3 {
        b.min + (b.max - b.min) * Vec3::new(self.next(), self.next(), self.next())
    }
}

/// Is the open segment a→b blocked by any triangle (brute force, no BVH)?
fn blocked(tris: &[[Vec3; 3]], a: Vec3, b: Vec3) -> bool {
    let d = b - a;
    let len = d.length();
    let dir = d / len;
    tris.iter().any(|t| {
        let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
        let p = dir.cross(e2);
        let det = e1.dot(p);
        if det.abs() < 1e-9 {
            return false;
        }
        let s = a - t[0];
        let u = s.dot(p) / det;
        let q = s.cross(e1);
        let v = dir.dot(q) / det;
        let dist = e2.dot(q) / det;
        (0.0..=1.0).contains(&u) && v >= 0.0 && u + v <= 1.0 && dist > 1e-3 && dist < len - 1e-3
    })
}

#[test]
fn cells_are_found_by_position() {
    let data = OcclusionData {
        origin: Vec3::new(-10.0, 0.0, -10.0),
        cell_size: 5.0,
        dims: UVec3::new(4, 2, 4),
        words: 1,
        sets: vec![0b1, 0b10],
        cells: (0..32)
            .map(|i| match i {
                0 => OUTSIDE,
                1 => INVALID,
                i => i % 2,
            })
            .collect(),
        ..Default::default()
    };
    // x fastest, then z, then y.
    let i = data.index(UVec3::new(2, 1, 3));
    assert_eq!(i, 2 + 4 * (3 + 4));
    assert_eq!(data.coords(i), UVec3::new(2, 1, 3));
    let p = Vec3::new(-10.0 + 2.5 * 5.0, 7.0, -10.0 + 3.5 * 5.0);
    assert_eq!(data.lookup(p).cell(), Some(i));
    let b = data.cell_bounds(i);
    assert!(b.min.cmple(p).all() && p.cmple(b.max).all());
    assert_eq!(data.lookup(Vec3::new(-10.5, 1.0, 0.0)), CellLookup::Outside);
    assert_eq!(data.lookup(Vec3::new(0.0, 10.5, 0.0)), CellLookup::Outside);
    assert_eq!(data.lookup(Vec3::new(-9.0, 1.0, -9.0)), CellLookup::Outside);
    assert_eq!(
        data.lookup(Vec3::new(-4.0, 1.0, -9.0)),
        CellLookup::Invalid { cell: 1 }
    );
    assert_eq!(
        data.lookup(Vec3::new(1.0, 1.0, -9.0)),
        CellLookup::Cell { cell: 2, set: 0 }
    );
    assert!(data.visible(0, 0) && !data.visible(0, 1) && data.visible(1, 1));
}

#[test]
fn hidden_objects_are_culled_and_visible_ones_kept() {
    let s = yard();
    let data = baked(&s);
    let at = |p: Vec3| match data.lookup(p) {
        CellLookup::Cell { set, .. } => set,
        other => panic!("{p} is {other:?}"),
    };
    let (locked, behind, peek) = (
        index_of(&s, "Locked"),
        index_of(&s, "Behind"),
        index_of(&s, "Peek"),
    );
    let (open_a, open_b) = (index_of(&s, "Open A"), index_of(&s, "Open B"));
    // South of the long wall, near the middle of the yard.
    let south = at(Vec3::new(-5.0, 1.7, 1.0));
    assert!(!data.visible(south, locked), "closed room hides its crate");
    assert!(
        !data.visible(south, behind),
        "the long wall hides the crate"
    );
    assert!(data.visible(south, open_a) && data.visible(south, open_b));
    // Looking through the doorway from straight in front of it.
    let door = at(Vec3::new(9.0, 1.7, -14.0));
    assert!(data.visible(door, peek), "seen through the doorway");
    assert!(!data.visible(door, locked));
    // Beside the long wall's far side the crate is in view again.
    let north = at(Vec3::new(-6.0, 1.7, 12.0));
    assert!(data.visible(north, behind));
    // Fewer sets than cells: equal results are shared.
    assert!(data.set_count() < data.cell_count());
}

#[test]
fn visible_objects_are_never_culled() {
    let s = yard();
    let input = BakeInput::from_scene(&s);
    let data = bake(&input, &Progress::default()).unwrap();
    let tris = &input.occluders.triangles;
    let mut rng = Lcg(7);
    let area = Aabb::new(Vec3::new(-15.0, 0.05, -15.0), Vec3::new(15.0, 3.0, 15.0));
    let (mut checked, mut seen) = (0, 0);
    while checked < 400 {
        let p = rng.in_box(&area);
        let CellLookup::Cell { set, .. } = data.lookup(p) else {
            continue;
        };
        // Points inside walls see nothing; skip them like a camera can't be there.
        if input
            .occludees
            .iter()
            .zip(&data.occludees)
            .any(|(b, _)| b.min.cmple(p).all() && p.cmple(b.max).all())
        {
            continue;
        }
        checked += 1;
        for (i, b) in input.occludees.iter().enumerate() {
            for _ in 0..6 {
                let q = rng.in_box(b);
                if !blocked(tris, p, q) {
                    seen += 1;
                    assert!(
                        data.visible(set, i),
                        "occludee {i} is visible from {p} (target {q}) but culled"
                    );
                    break;
                }
            }
        }
    }
    assert!(seen > 400, "the check saw things: {seen}");
}

#[test]
fn bakes_are_deterministic_and_files_round_trip() {
    let s = yard();
    let a = baked(&s);
    let b = baked(&s);
    assert_eq!(a.to_bytes(), b.to_bytes());
    let back = OcclusionData::from_bytes(&a.to_bytes()).unwrap();
    assert_eq!(back, a);
    let dir = std::env::temp_dir().join("misty_occlusion_round_trip");
    let path = dir.join("Level/OcclusionData.occ");
    a.save(&path).unwrap();
    assert_eq!(OcclusionData::load(&path).unwrap(), a);
    // Damaged files are refused, not misread.
    let bytes = a.to_bytes();
    assert!(OcclusionData::from_bytes(&bytes[..bytes.len() - 3]).is_err());
    assert!(OcclusionData::from_bytes(b"nope").is_err());
    assert_eq!(
        OcclusionSettings::asset_path_for("Assets/Scenes/Level.scene"),
        "Assets/Scenes/Level/OcclusionData.occ"
    );
}

#[test]
fn cancelled_bakes_stop_and_progress_counts_points() {
    let s = yard();
    let input = BakeInput::from_scene(&s);
    let p = Progress::default();
    p.cancel.store(true, std::sync::atomic::Ordering::Relaxed);
    assert!(bake(&input, &p).is_none());
    let p = Progress::default();
    bake(&input, &p).unwrap();
    assert_eq!(p.fraction(), 1.0);
}

#[test]
fn flags_and_volumes_choose_what_is_baked() {
    let mut s = yard();
    let input = BakeInput::from_scene(&s);
    // Crates are smaller than the smallest occluder and not Occluder Static anyway; the floor
    // and walls occlude. Every static renderer is an occludee.
    assert_eq!(input.occludees.len(), bake::occludees(&s).len());
    assert_eq!(input.volumes.len(), 1);
    let tris = input.occluders.triangles.len();
    let wall = s.find_by_name("Long Wall").unwrap();
    s.game_object_mut(wall)
        .static_flags
        .set(StaticFlags::OCCLUDER, false);
    assert_eq!(
        BakeInput::from_scene(&s).occluders.triangles.len(),
        tris - 12
    );
    s.game_object_mut(wall).is_static = false;
    assert_eq!(
        BakeInput::from_scene(&s).occludees.len(),
        input.occludees.len() - 1
    );
    // Without view volumes the grid covers the static geometry.
    let area = s.find_by_name("Occlusion Area").unwrap();
    s.world
        .get::<&mut OcclusionArea>(area)
        .unwrap()
        .is_view_volume = false;
    let input = BakeInput::from_scene(&s);
    assert!(input.volumes.is_empty());
    let (origin, size, dims) = bake::grid(&input).unwrap();
    assert!(origin.x <= -15.0 && origin.x + size * dims.x as f32 >= 15.0);
}

#[test]
fn backface_threshold_marks_cells_inside_geometry() {
    let mut s = Scene::new("Solid");
    // A 12 m cube the view volume sits inside: every sample sees back faces.
    block(&mut s, "Solid", Vec3::ZERO, Vec3::splat(12.0));
    let a = s.spawn("Area", None);
    s.add(
        a,
        OcclusionArea {
            size: [6.0, 6.0, 6.0],
            ..Default::default()
        },
    );
    s.render_settings.occlusion.smallest_occluder = 3.0;
    s.render_settings.occlusion.smallest_hole = 4.0;
    let all_inside = baked(&s);
    assert!(
        all_inside.cells.iter().all(|c| *c != INVALID),
        "100 never removes"
    );
    s.render_settings.occlusion.backface_threshold = 50.0;
    let removed = baked(&s);
    assert!(removed.cells.iter().all(|c| *c == INVALID));
    assert!(matches!(
        removed.lookup(Vec3::ZERO),
        CellLookup::Invalid { .. }
    ));
}

/// The runtime: the game camera's cell hides draws of culled occludees; moved objects and
/// cameras with Occlusion Culling off draw everything.
#[test]
fn game_view_hides_culled_draws() {
    struct NoMaterials;
    impl crate::render_scene::MaterialSource for NoMaterials {
        fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
            Default::default()
        }
    }
    let mut s = yard();
    let cam = s.spawn("Camera", None);
    s.set_transform(cam, Transform::from_position(Vec3::new(-5.0, 1.7, 1.0)));
    s.add(cam, Camera::default());
    let data = Arc::new(baked(&s));
    let dir = std::env::temp_dir().join("misty_occlusion_game_view");
    let path = dir.join("Yard/OcclusionData.occ");
    data.save(&path).unwrap();
    store::insert(&path, data.clone());
    s.render_settings.occlusion.data = Some(path.to_string_lossy().into_owned());

    let mut cache = crate::draw_cache::DrawCache::new();
    cache.set_static_batching(true);
    let view = crate::render_scene::GameView::cached(&s, &NoMaterials, &mut cache).unwrap();
    let o = view.occlusion.as_ref().expect("occlusion on");
    assert!(matches!(o.lookup, CellLookup::Cell { .. }));
    let mask = view.environment.occlusion.as_ref().unwrap();
    assert_eq!(mask.hidden.len(), view.meshes.len());
    let owners = cache.draw_owners();
    let locked = s.find_by_name("Locked").unwrap();
    let open = s.find_by_name("Open A").unwrap();
    let hidden_of = |e| {
        owners
            .iter()
            .zip(mask.hidden.iter())
            .find(|(o, _)| **o == Some(e))
            .map(|(_, h)| *h)
    };
    assert_eq!(
        hidden_of(locked),
        Some(true),
        "occludees are kept out of batches"
    );
    assert_eq!(hidden_of(open), Some(false));
    assert!(o.culled >= 2 && o.culled < o.occludees);

    // A moved occludee no longer matches the bake: always drawn.
    s.world.get::<&mut Transform>(locked).unwrap().position.y += 1.0;
    let view = crate::render_scene::GameView::cached(&s, &NoMaterials, &mut cache).unwrap();
    let o = view.occlusion.as_ref().unwrap();
    assert_eq!(o.stale, 1);
    let owners = cache.draw_owners();
    let mask = view.environment.occlusion.as_ref().unwrap();
    let i = owners.iter().position(|o| *o == Some(locked)).unwrap();
    assert!(!mask.hidden[i]);

    // Camera toggle off: no mask.
    s.world.get::<&mut Camera>(cam).unwrap().occlusion_culling = false;
    let view = crate::render_scene::GameView::cached(&s, &NoMaterials, &mut cache).unwrap();
    assert!(view.occlusion.is_none() && view.environment.occlusion.is_none());
}

#[test]
fn lua_toggles_camera_occlusion_culling() {
    use crate::runtime::Time;
    use crate::scripting::{Input, LogKind, ScriptHost};
    let root = std::env::temp_dir().join("misty_occlusion_lua");
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Toggle.lua"),
        r#"local T = {}
function T:Start()
    local cam = Camera.main
    Debug.Log("before " .. tostring(cam.useOcclusionCulling))
    cam.useOcclusionCulling = false
    Debug.Log("after " .. tostring(cam.useOcclusionCulling))
end
return T
"#,
    )
    .unwrap();
    let mut s = Scene::new("Lua");
    let cam = s.spawn("Main Camera", None);
    s.game_object_mut(cam).tag = "MainCamera".into();
    s.add(cam, Camera::default());
    s.add(
        cam,
        crate::scene::Scripts {
            list: vec![crate::scene::ScriptRef {
                path: "Assets/Scripts/Toggle.lua".into(),
                enabled: true,
                ..Default::default()
            }],
        },
    );
    let physics = crate::physics::Physics::build(&s, Vec3::ZERO);
    let mut host = ScriptHost::start(&mut s, physics, root);
    let mut time = Time::default();
    time.advance(1.0 / 60.0);
    host.update(&mut s, &time, Input::default());
    let logs = host.take_logs();
    let text: Vec<&str> = logs
        .iter()
        .inspect(|(k, t)| assert_ne!(*k, LogKind::Error, "{t}"))
        .map(|(_, t)| t.as_str())
        .collect();
    assert!(
        text.contains(&"before true") && text.contains(&"after false"),
        "{text:?}"
    );
    assert!(!s.world.get::<&Camera>(cam).unwrap().occlusion_culling);
}
