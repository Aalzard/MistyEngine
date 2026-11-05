//! MeshBuilderDemo sample scene in `SampleProject`: a small plastered house modelled with the
//! Mesh Builder operations — walls with a doorway and windows cut by loop cuts, inset, extrude,
//! delete and weld; a gabled roof and a platform with beveled edges; stairs, an arch gate and
//! two columns — using two materials (plaster and stone, set per face). Regenerate with
//! `cargo test -p misty_editor -- --ignored write_mesh_builder_demo`; the other test checks the
//! committed files still match.

use std::path::PathBuf;

use glam::Vec3;
use misty_engine::assets::Material;
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::ops;
use misty_engine::mesh_builder::shapes;
use misty_engine::mesh_builder::{EditableMesh, edge_key};
use misty_engine::scene::{
    Light, MeshCollider, MeshFilter, MeshRenderer, Scene, Shadows, Transform,
};

const SCENE: &str = "Assets/Scenes/MeshBuilderDemo.scene";
const PLASTER: &str = "Assets/Materials/Builder Plaster.mat";
const STONE: &str = "Assets/Materials/Builder Stone.mat";
const GRASS: &str = "Assets/Materials/Builder Grass.mat";

/// Materials elements of the building parts.
const PLASTER_ELEMENT: u32 = 0;
const STONE_ELEMENT: u32 = 1;

/// Wall thickness, wall height, platform height.
const WALL: f32 = 0.25;
const HEIGHT: f32 = 3.0;
const PLATFORM: f32 = 0.9;

fn project() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject")
}

fn materials() -> [(&'static str, Material); 3] {
    let textured = |color: [f32; 4], albedo: &str, normal: Option<&str>, tiling: f32| Material {
        color,
        smoothness: 0.15,
        albedo_map: Some(format!("Assets/Textures/{albedo}")),
        normal_map: normal.map(|n| format!("Assets/Textures/{n}")),
        tiling: [tiling, tiling],
        ..Default::default()
    };
    [
        (
            PLASTER,
            Material {
                color: [0.93, 0.87, 0.77, 1.0],
                smoothness: 0.1,
                ..Default::default()
            },
        ),
        (
            STONE,
            textured(
                [0.7, 0.68, 0.65, 1.0],
                "TerrainRock.png",
                Some("TerrainRockNormal.png"),
                0.5,
            ),
        ),
        (
            GRASS,
            textured([0.75, 0.85, 0.6, 1.0], "TerrainGrass.png", None, 0.25),
        ),
    ]
}

/// Face whose centre is nearest `center` among those facing `normal`.
fn face_at(m: &EditableMesh, center: Vec3, normal: Vec3) -> usize {
    (0..m.faces.len())
        .filter(|f| m.face_normal(*f).dot(normal) > 0.95)
        .min_by(|a, b| {
            let d = |f: usize| m.face_center(f).distance(center);
            d(*a).total_cmp(&d(*b))
        })
        .expect("face")
}

/// Vertex at `p`.
fn vertex_at(m: &EditableMesh, p: Vec3) -> u32 {
    m.positions
        .iter()
        .position(|q| q.distance(p) < 1e-4)
        .expect("vertex") as u32
}

/// Loop cut through edge `a`–`b` at the point `at` on it.
fn cut_at(m: &mut EditableMesh, a: Vec3, b: Vec3, at: Vec3) {
    let e = edge_key(vertex_at(m, a), vertex_at(m, b));
    let start = m.position(e[0]);
    let end = m.position(e[1]);
    let t = (at - start).length() / (end - start).length();
    ops::loop_cut(m, e, t);
}

/// Cut a hole through a slab: inset the outer and inner faces alike, extrude the outer inset
/// through the wall, delete both ends and weld. The opening's sides get `frame`.
fn window(m: &mut EditableMesh, outer: (Vec3, Vec3), inner: (Vec3, Vec3), inset: f32, frame: u32) {
    let (fo, fi) = (face_at(m, outer.0, outer.1), face_at(m, inner.0, inner.1));
    ops::inset_faces(m, &[fo, fi], inset);
    ops::extrude_faces(m, &[fo], -WALL);
    ops::delete_faces(m, &[fo, fi]);
    ops::weld_vertices(m, &[], 1e-4);
    let normal = outer.1;
    let sides: Vec<usize> = (0..m.faces.len())
        .filter(|f| {
            let c = m.face_center(*f);
            m.face_normal(*f).dot(normal).abs() < 0.1
                && (c - outer.0).dot(normal).abs() < WALL
                && (c - outer.0).reject_from(normal).length() < 1.2
        })
        .collect();
    ops::set_material(m, &sides, frame);
}

/// Every face projects world-space UVs, so the textures run on across faces and parts.
fn world_uvs(mut m: EditableMesh) -> EditableMesh {
    for f in &mut m.faces {
        f.uv.world = true;
    }
    m
}

/// Front wall (local: 6 m wide along X, outer face −Z): a doorway 1.2 × 2.2 m in the middle.
/// Two vertical loop cuts and one horizontal give the door face; it is extruded through the
/// wall and removed with the matching back face and the strips under it, then welded.
pub fn front_wall() -> EditableMesh {
    let mut m = shapes::cube(Vec3::new(6.0, HEIGHT, WALL));
    let z = -WALL * 0.5;
    let (bl, br) = (Vec3::new(-3.0, 0.0, z), Vec3::new(3.0, 0.0, z));
    cut_at(&mut m, bl, br, Vec3::new(-0.6, 0.0, z));
    cut_at(&mut m, Vec3::new(-0.6, 0.0, z), br, Vec3::new(0.6, 0.0, z));
    cut_at(
        &mut m,
        bl,
        Vec3::new(-3.0, HEIGHT, z),
        Vec3::new(-3.0, 2.2, z),
    );
    let door = face_at(&m, Vec3::new(0.0, 1.1, z), -Vec3::Z);
    let back = face_at(&m, Vec3::new(0.0, 1.1, -z), Vec3::Z);
    let under = face_at(&m, Vec3::new(0.0, 0.0, 0.0), -Vec3::Y);
    ops::extrude_faces(&mut m, &[door], -WALL);
    let floor = face_at(&m, Vec3::new(0.0, 0.0, 0.0), Vec3::Y);
    ops::delete_faces(&mut m, &[door, back, under, floor]);
    ops::weld_vertices(&mut m, &[], 1e-4);
    // Stone door frame: the jambs and the lintel's underside.
    let frame: Vec<usize> = (0..m.faces.len())
        .filter(|f| {
            let c = m.face_center(*f);
            c.x.abs() < 0.61 && c.y < 2.21 && c.z.abs() < WALL && m.face_normal(*f).z.abs() < 0.1
        })
        .collect();
    ops::set_material(&mut m, &frame, STONE_ELEMENT);
    world_uvs(m)
}

/// Back wall: two square windows (1.8 m) with stone reveals.
pub fn back_wall() -> EditableMesh {
    let mut m = shapes::cube(Vec3::new(6.0, HEIGHT, WALL));
    let z = WALL * 0.5;
    cut_at(
        &mut m,
        Vec3::new(-3.0, 0.0, z),
        Vec3::new(3.0, 0.0, z),
        Vec3::new(0.0, 0.0, z),
    );
    for x in [-1.5, 1.5] {
        let c = Vec3::new(x, HEIGHT * 0.5, z);
        window(
            &mut m,
            (c, Vec3::Z),
            (c - Vec3::Z * WALL, -Vec3::Z),
            0.6,
            STONE_ELEMENT,
        );
    }
    world_uvs(m)
}

/// Side wall (local: along Z, outer face +X): one wide window.
pub fn side_wall() -> EditableMesh {
    let depth = 4.0 - WALL * 2.0;
    let mut m = shapes::cube(Vec3::new(WALL, HEIGHT, depth));
    let c = Vec3::new(WALL * 0.5, HEIGHT * 0.5, 0.0);
    window(
        &mut m,
        (c, Vec3::X),
        (c - Vec3::X * WALL, -Vec3::X),
        0.9,
        STONE_ELEMENT,
    );
    world_uvs(m)
}

/// Gabled roof: stone slopes, plaster gable ends (ridge along local Z).
pub fn roof() -> EditableMesh {
    let mut m = shapes::prism(4.8, 1.5, 6.8);
    let slopes: Vec<usize> = (0..m.faces.len())
        .filter(|f| m.face_normal(*f).y > 0.1)
        .collect();
    let all: Vec<usize> = (0..m.faces.len()).collect();
    ops::set_material(&mut m, &all, PLASTER_ELEMENT);
    ops::set_material(&mut m, &slopes, STONE_ELEMENT);
    // Overhanging eaves: the underside is stone too.
    let under = face_at(&m, Vec3::ZERO, -Vec3::Y);
    ops::set_material(&mut m, &[under], STONE_ELEMENT);
    world_uvs(m)
}

/// Stone platform with beveled front and back top edges.
pub fn platform() -> EditableMesh {
    let mut m = shapes::cube(Vec3::new(8.0, PLATFORM, 6.0));
    let edge = |m: &EditableMesh, z: f32| {
        edge_key(
            vertex_at(m, Vec3::new(-4.0, PLATFORM, z)),
            vertex_at(m, Vec3::new(4.0, PLATFORM, z)),
        )
    };
    let (front, back) = (edge(&m, -3.0), edge(&m, 3.0));
    ops::bevel_edges(&mut m, &[front, back], 0.12);
    let all: Vec<usize> = (0..m.faces.len()).collect();
    ops::set_material(&mut m, &all, STONE_ELEMENT);
    world_uvs(m)
}

fn stone_part(m: EditableMesh) -> EditableMesh {
    let mut m = world_uvs(m);
    let all: Vec<usize> = (0..m.faces.len()).collect();
    ops::set_material(&mut m, &all, STONE_ELEMENT);
    m
}

/// One Mesh Builder object: Editable Mesh, Mesh Filter, Mesh Renderer (plaster, stone) and a
/// Mesh Collider.
fn part(
    s: &mut Scene,
    name: &str,
    parent: Option<Entity>,
    mesh: EditableMesh,
    t: Transform,
    materials: &[&str],
) -> Entity {
    let e = s.spawn(name, parent);
    s.set_transform(e, t);
    s.add(e, MeshFilter::default());
    s.add(
        e,
        MeshRenderer {
            material: materials.first().map(|m| (*m).to_owned()),
            extra_materials: materials[1..]
                .iter()
                .map(|m| Some((*m).to_owned()))
                .collect(),
            ..Default::default()
        },
    );
    s.add(e, mesh);
    s.add(e, MeshCollider::default());
    e
}

pub fn demo_scene() -> Scene {
    let mut s = Scene::default_3d("MeshBuilderDemo");
    if let Some(cam) = s.find_by_name("Main Camera") {
        s.set_transform(
            cam,
            Transform::from_position(Vec3::new(-7.5, 4.6, -11.5))
                .with_euler(Vec3::new(14.0, 33.0, 0.0)),
        );
    }
    if let Some(sun) = s.find_by_name("Directional Light") {
        s.set_transform(
            sun,
            Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
                .with_euler(Vec3::new(42.0, -40.0, 0.0)),
        );
        let mut l = s.world.get::<&mut Light>(sun).unwrap();
        l.intensity = 1.15;
        l.shadows = Shadows::Soft;
    }
    let both = [PLASTER, STONE];
    let at = Transform::from_position;
    part(
        &mut s,
        "Ground",
        None,
        world_uvs(shapes::plane(30.0, 30.0, 1, 1)),
        at(Vec3::ZERO),
        &[GRASS],
    );
    let house = s.spawn("House", None);
    part(
        &mut s,
        "Platform",
        Some(house),
        platform(),
        at(Vec3::ZERO),
        &both,
    );
    let floor = PLATFORM;
    part(
        &mut s,
        "Front Wall",
        Some(house),
        front_wall(),
        at(Vec3::new(0.0, floor, -2.0 + WALL * 0.5)),
        &both,
    );
    part(
        &mut s,
        "Back Wall",
        Some(house),
        back_wall(),
        at(Vec3::new(0.0, floor, 2.0 - WALL * 0.5)),
        &both,
    );
    for (name, x, yaw) in [("Left Wall", -3.0f32, 180.0), ("Right Wall", 3.0, 0.0)] {
        part(
            &mut s,
            name,
            Some(house),
            side_wall(),
            Transform::from_position(Vec3::new(x - x.signum() * WALL * 0.5, floor, 0.0))
                .with_euler(Vec3::new(0.0, yaw, 0.0)),
            &both,
        );
    }
    part(
        &mut s,
        "Roof",
        Some(house),
        roof(),
        Transform::from_position(Vec3::new(0.0, floor + HEIGHT, 0.0))
            .with_euler(Vec3::new(0.0, 90.0, 0.0)),
        &both,
    );
    for (name, x) in [("Column Left", -1.15), ("Column Right", 1.15)] {
        part(
            &mut s,
            name,
            Some(house),
            stone_part(shapes::cylinder(0.14, HEIGHT, 14)),
            at(Vec3::new(x, floor, -2.55)),
            &both,
        );
    }
    part(
        &mut s,
        "Stairs",
        None,
        stone_part(shapes::stairs(2.0, PLATFORM, 1.2, 3)),
        at(Vec3::new(0.0, 0.0, -3.6)),
        &both,
    );
    part(
        &mut s,
        "Arch Gate",
        None,
        stone_part(shapes::arch(1.7, 0.32, 0.6, 16)),
        at(Vec3::new(0.0, 0.0, -6.0)),
        &both,
    );
    s
}

#[test]
#[ignore = "writes into SampleProject; run on purpose to regenerate"]
fn write_mesh_builder_demo() {
    let root = project();
    for (path, m) in materials() {
        m.save(&root.join(path)).unwrap();
    }
    let scene = demo_scene();
    misty_engine::mesh_builder::sync(&scene);
    scene.save(&root.join(SCENE)).unwrap();
}

#[test]
fn mesh_builder_demo_matches_its_generator() {
    let root = project();
    for (path, m) in materials() {
        assert_eq!(Material::load(&root.join(path)).unwrap(), m, "{path}");
    }
    let text = std::fs::read_to_string(root.join(SCENE)).unwrap();
    let fresh = demo_scene();
    misty_engine::mesh_builder::sync(&fresh);
    assert!(
        text.replace("\r\n", "\n") == fresh.to_json(),
        "regenerate with write_mesh_builder_demo"
    );
    let (scene, report) = Scene::from_json("MeshBuilderDemo", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    // Every part is a valid, consistently wound mesh; all but the ground are closed solids.
    let mut materials_used = std::collections::BTreeSet::new();
    for (e, _) in scene.walk() {
        let Ok(m) = scene.world.get::<&EditableMesh>(e) else {
            continue;
        };
        let name = scene.name(e);
        assert!(m.is_valid() && m.is_manifold(), "{name}");
        if name != "Ground" {
            assert!(m.is_closed(), "{name} closed");
            assert!(m.volume() > 0.0, "{name} faces outward");
            materials_used.extend(m.faces.iter().map(|f| f.material));
        }
        // Loading baked it.
        let key = scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
        assert!(
            key.is_some_and(|k| misty_engine::mesh_builder::is_baked_key(&k)),
            "{name}"
        );
    }
    assert_eq!(
        materials_used.into_iter().collect::<Vec<_>>(),
        vec![PLASTER_ELEMENT, STONE_ELEMENT]
    );
}

/// The doorway and windows are real openings: rays through them miss the walls.
#[test]
fn walls_have_openings() {
    let hits = |m: &EditableMesh, o: Vec3, d: Vec3| {
        (0..m.faces.len()).any(|f| {
            m.triangles_of(f).iter().any(|t| {
                let p = t.map(|v| m.position(v));
                ray_hits(o, d, &p)
            })
        })
    };
    let front = front_wall();
    assert!(!hits(&front, Vec3::new(0.0, 1.0, -2.0), Vec3::Z), "doorway");
    assert!(
        hits(&front, Vec3::new(2.0, 1.0, -2.0), Vec3::Z),
        "wall beside it"
    );
    assert!(
        !hits(&front, Vec3::new(0.0, 0.05, -2.0), Vec3::Z),
        "open to the floor"
    );
    let back = back_wall();
    assert!(
        !hits(&back, Vec3::new(-1.5, 1.5, -2.0), Vec3::Z),
        "left window"
    );
    assert!(
        !hits(&back, Vec3::new(1.5, 1.5, -2.0), Vec3::Z),
        "right window"
    );
    assert!(
        hits(&back, Vec3::new(0.0, 1.5, -2.0), Vec3::Z),
        "pier between"
    );
    let side = side_wall();
    assert!(
        !hits(&side, Vec3::new(-2.0, 1.5, 0.0), Vec3::X),
        "side window"
    );
    // The door frame and window reveals are stone.
    assert!(front.faces.iter().any(|f| f.material == STONE_ELEMENT));
}

fn ray_hits(o: Vec3, d: Vec3, t: &[Vec3; 3]) -> bool {
    let (e1, e2) = (t[1] - t[0], t[2] - t[0]);
    let p = d.cross(e2);
    let det = e1.dot(p);
    if det.abs() < 1e-9 {
        return false;
    }
    let s = o - t[0];
    let u = s.dot(p) / det;
    let q = s.cross(e1);
    let v = d.dot(q) / det;
    u >= 0.0 && v >= 0.0 && u + v <= 1.0 && e2.dot(q) / det > 0.0
}
