use glam::{Mat4, Vec2, Vec3};

use super::ops::{self, Selection};
use super::shapes::{self, Shape};
use super::*;
use crate::scene::{MeshFilter, MeshRenderer, Scene};

fn cube() -> EditableMesh {
    shapes::cube(Vec3::ONE)
}

/// Face whose normal is closest to `dir`.
fn face_towards(m: &EditableMesh, dir: Vec3) -> usize {
    (0..m.faces.len())
        .max_by(|a, b| {
            m.face_normal(*a)
                .dot(dir)
                .total_cmp(&m.face_normal(*b).dot(dir))
        })
        .unwrap()
}

fn counts(m: &EditableMesh) -> (usize, usize, usize) {
    (m.positions.len(), m.edges().len(), m.faces.len())
}

fn assert_solid(m: &EditableMesh, what: &str) {
    assert!(m.is_valid(), "{what}: invalid faces");
    assert!(
        m.is_manifold(),
        "{what}: not manifold / inconsistent winding"
    );
    assert!(m.is_closed(), "{what}: not closed");
    assert!(m.volume() > 0.0, "{what}: normals point inward");
}

/// Sum of a face's triangle areas equals its polygon (Newell) area.
fn triangulation_covers(m: &EditableMesh, f: usize) -> bool {
    let pts: Vec<Vec3> = m.faces[f].verts.iter().map(|v| m.position(*v)).collect();
    let mut newell = Vec3::ZERO;
    for i in 0..pts.len() {
        newell += pts[i].cross(pts[(i + 1) % pts.len()]);
    }
    let poly = newell.length() * 0.5;
    let tris: f32 = m
        .triangles_of(f)
        .iter()
        .map(|t| {
            let (a, b, c) = (m.position(t[0]), m.position(t[1]), m.position(t[2]));
            let n = (b - a).cross(c - a);
            // Each triangle must face the same way as the polygon.
            assert!(n.dot(newell) > -1e-6, "flipped triangle");
            n.length() * 0.5
        })
        .sum();
    (poly - tris).abs() < 1e-4 * poly.max(1.0)
}

#[test]
fn every_shape_is_a_valid_closed_outward_solid() {
    for shape in Shape::ALL {
        let m = shape.build();
        let name = shape.name();
        if name == "Plane" {
            assert!(m.is_valid() && m.is_manifold() && !m.is_closed());
            assert!(
                m.faces
                    .iter()
                    .enumerate()
                    .all(|(f, _)| m.face_normal(f).y > 0.99)
            );
            continue;
        }
        assert_solid(&m, name);
        for f in 0..m.faces.len() {
            assert!(triangulation_covers(&m, f), "{name} face {f}");
        }
        // Sits on the ground.
        let (lo, _) = m.bounds();
        assert!(lo.y.abs() < 1e-5, "{name} base at {}", lo.y);
    }
}

#[test]
fn shape_volumes_match_their_formulas() {
    assert!((shapes::cube(Vec3::new(2.0, 3.0, 4.0)).volume() - 24.0).abs() < 1e-4);
    assert!((shapes::prism(2.0, 1.0, 3.0).volume() - 3.0).abs() < 1e-4);
    // 4 steps of 1 × 1 deep (total depth 4, height 4), width 1: 1 + 2 + 3 + 4 = 10.
    assert!((shapes::stairs(1.0, 4.0, 4.0, 4).volume() - 10.0).abs() < 1e-3);
    let c = shapes::cylinder(1.0, 2.0, 64).volume();
    assert!((c - std::f32::consts::PI * 2.0).abs() < 0.02, "{c}");
}

#[test]
fn cube_counts_and_flat_bake() {
    let m = cube();
    assert_eq!(counts(&m), (8, 12, 6));
    let baked = bake(&m, None);
    assert_eq!(baked.len(), 2, "all faces + material 0");
    assert_eq!(baked[0].vertices.len(), 24);
    assert_eq!(baked[0].indices.len(), 36);
    assert_eq!(baked[1].indices.len(), 36);
    // Flat: every vertex normal is its face normal (an axis).
    for v in &baked[0].vertices {
        let n = Vec3::from(v.normal);
        assert!((n.abs().max_element() - 1.0).abs() < 1e-5, "{n:?}");
    }
    // Baked triangles wind outward like the faces.
    for t in baked[0].indices.chunks(3) {
        let p = |i: u32| Vec3::from(baked[0].vertices[i as usize].position);
        let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        let normal = Vec3::from(baked[0].vertices[t[0] as usize].normal);
        assert!(n.dot(normal) > 0.0);
    }
}

#[test]
fn smoothing_groups_average_normals() {
    let m = shapes::cylinder(1.0, 2.0, 8);
    let baked = bake(&m, None);
    // Side vertices point straight out (no Y), caps straight up/down.
    let sides = baked[0]
        .vertices
        .iter()
        .filter(|v| v.normal[1].abs() < 1e-4)
        .count();
    assert_eq!(sides, 8 * 4);
    for v in baked[0]
        .vertices
        .iter()
        .filter(|v| v.normal[1].abs() < 1e-4)
    {
        let p = Vec2::new(v.position[0], v.position[2]).normalize();
        let n = Vec2::new(v.normal[0], v.normal[2]);
        assert!(p.dot(n) > 0.999, "smooth normal follows the radius");
    }
}

#[test]
fn materials_bake_into_submeshes() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    ops::set_material(&mut m, &[top], 1);
    assert_eq!(m.material_count(), 2);
    let baked = bake(&m, None);
    assert_eq!(baked.len(), 3);
    assert_eq!(baked[1].indices.len(), 30);
    assert_eq!(baked[2].indices.len(), 6);
}

#[test]
fn uvs_tile_per_metre_and_world_space_ignores_the_face_corner() {
    let m = shapes::cube(Vec3::new(4.0, 2.0, 1.0));
    let front = face_towards(&m, -Vec3::Z);
    let pts: Vec<Vec3> = m.faces[front]
        .verts
        .iter()
        .map(|v| m.position(*v))
        .collect();
    let n = m.face_normal(front);
    let local = bake::face_uvs(&pts, n, &FaceUv::default(), None);
    let (lo, hi) = local
        .iter()
        .fold((Vec2::MAX, Vec2::MIN), |(a, b), p| (a.min(*p), b.max(*p)));
    assert!(lo.abs().max_element() < 1e-5, "anchored at the corner");
    assert!(
        (hi - Vec2::new(4.0, 2.0)).abs().max_element() < 1e-5,
        "{hi:?}"
    );
    // World space: moving the object by 0.5 shifts the UVs by 0.5 (continuous tiling).
    let uv = FaceUv {
        world: true,
        ..Default::default()
    };
    let at = |x: f32| {
        let w = Mat4::from_translation(Vec3::new(x, 0.0, 0.0));
        bake::face_uvs(&pts, n, &uv, Some(&w))
    };
    let (a, b) = (at(0.0), at(0.5));
    for (p, q) in a.iter().zip(&b) {
        assert!(((q.x - p.x).abs() - 0.5).abs() < 1e-5);
    }
    // Tiling scales.
    let tiled = FaceUv {
        scale: Vec2::splat(2.0),
        ..Default::default()
    };
    let t = bake::face_uvs(&pts, n, &tiled, None);
    let max = t.iter().fold(Vec2::MIN, |a, p| a.max(*p));
    assert!((max - Vec2::new(8.0, 4.0)).abs().max_element() < 1e-4);
}

#[test]
fn extrude_one_face() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let out = ops::extrude_faces(&mut m, &[top], 1.0);
    assert_eq!(out, vec![top]);
    assert_eq!(counts(&m), (12, 20, 10));
    assert_solid(&m, "extruded");
    assert!((m.volume() - 2.0).abs() < 1e-4);
    assert!((m.face_center(top).y - 2.0).abs() < 1e-5);
}

#[test]
fn extrude_two_adjacent_faces_as_one_region() {
    let mut m = cube();
    let (top, right) = (face_towards(&m, Vec3::Y), face_towards(&m, Vec3::X));
    ops::extrude_faces(&mut m, &[top, right], 0.5);
    // 6 shared corners moved; 6 border edges → 6 side quads.
    assert_eq!(counts(&m), (14, 24, 12));
    assert_solid(&m, "region extrude");
    // Each face moved by the full distance along its own normal.
    assert!((m.face_center(top).y - 1.5).abs() < 1e-5);
    assert!((m.face_center(right).x - 1.0).abs() < 1e-5);
}

#[test]
fn extrude_inward_then_delete_cuts_a_hole() {
    // A wall slab: inset both sides, extrude the front inset through the wall, delete the
    // cap and the back inset, weld: a closed slab with a square window.
    let mut m = shapes::cube(Vec3::new(1.0, 1.0, 0.2));
    let front = face_towards(&m, -Vec3::Z);
    let back = face_towards(&m, Vec3::Z);
    ops::inset_faces(&mut m, &[front, back], 0.3);
    ops::extrude_faces(&mut m, &[front], -0.2);
    ops::delete_faces(&mut m, &[front, back]);
    let welded = ops::weld_vertices(&mut m, &[], 1e-4);
    assert_eq!(welded, 4);
    assert_solid(&m, "window cut");
    assert!(
        (m.volume() - 0.2 * (1.0 - 0.16)).abs() < 1e-4,
        "{}",
        m.volume()
    );
}

#[test]
fn extrude_border_edge() {
    let mut m = shapes::plane(1.0, 1.0, 1, 1);
    let e = m.edges()[0];
    let out = ops::extrude_edges(&mut m, &[e], 1.0);
    assert_eq!(out.len(), 1);
    assert_eq!(counts(&m), (6, 7, 2));
    assert!(m.is_manifold());
    // Extends the plane in its own plane.
    assert!(m.positions.iter().all(|p| p.y.abs() < 1e-6));
    // Interior edges are not extruded.
    let mut c = cube();
    let e = c.edges()[0];
    assert!(ops::extrude_edges(&mut c, &[e], 1.0).is_empty());
}

#[test]
fn inset_face() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let out = ops::inset_faces(&mut m, &[top], 0.2);
    assert_eq!(out, vec![top]);
    assert_eq!(counts(&m), (12, 20, 10));
    assert_solid(&m, "inset");
    assert!((m.volume() - 1.0).abs() < 1e-4);
    // Inner face is 0.6 × 0.6.
    let (lo, hi) = m.faces[top]
        .verts
        .iter()
        .fold((Vec3::MAX, Vec3::MIN), |(a, b), v| {
            (a.min(m.position(*v)), b.max(m.position(*v)))
        });
    assert!(((hi - lo).x - 0.6).abs() < 1e-5 && ((hi - lo).z - 0.6).abs() < 1e-5);
    // A huge inset stops short of the centre.
    let mut m = cube();
    ops::inset_faces(&mut m, &[top], 5.0);
    assert_solid(&m, "big inset");
}

#[test]
fn bevel_edge_of_a_cube() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let right = face_towards(&m, Vec3::X);
    let shared = m.faces[top]
        .edges()
        .map(|(a, b)| edge_key(a, b))
        .find(|e| m.faces[right].edges().any(|(a, b)| edge_key(a, b) == *e))
        .unwrap();
    let strips = ops::bevel_edges(&mut m, &[shared], 0.2);
    assert_eq!(strips.len(), 1);
    assert_eq!(counts(&m), (10, 15, 7));
    assert_solid(&m, "bevel");
    // A 0.2 chamfer removes a triangular prism of 0.02.
    assert!((m.volume() - 0.98).abs() < 1e-4, "{}", m.volume());
}

#[test]
fn bevel_edge_with_four_faces_at_a_corner_adds_triangles() {
    // Subdivide the top so the beveled edge's ends meet four faces.
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let first = m.faces[top]
        .edges()
        .map(|(a, b)| edge_key(a, b))
        .next()
        .unwrap();
    ops::loop_cut(&mut m, first, 0.5);
    let before = m.faces.len();
    let e = m.edges()[0];
    ops::bevel_edges(&mut m, &[e], 0.1);
    assert!(m.faces.len() > before);
    assert_solid(&m, "bevel valence 4");
}

#[test]
fn subdivide_all_and_one() {
    let mut m = cube();
    let all: Vec<usize> = (0..6).collect();
    let out = ops::subdivide_faces(&mut m, &all);
    assert_eq!(out.len(), 24);
    assert_eq!(counts(&m), (26, 48, 24));
    assert_solid(&m, "subdivided");

    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    ops::subdivide_faces(&mut m, &[top]);
    assert_eq!(counts(&m), (13, 20, 9));
    assert_solid(&m, "one face subdivided (neighbours keep the midpoints)");
}

#[test]
fn loop_cut_goes_all_round() {
    let mut m = cube();
    let e = m.edges()[0];
    let new = ops::loop_cut(&mut m, e, 0.25);
    assert_eq!(new.len(), 4);
    assert_eq!(counts(&m), (12, 20, 10));
    assert_solid(&m, "loop cut");
    // The cut vertices sit a quarter of the way along, measured from e[0].
    let (a, b) = (m.position(e[0]), m.position(e[1]));
    let want = a.lerp(b, 0.25);
    assert!(m.positions.iter().any(|p| p.distance(want) < 1e-5));
}

#[test]
fn connect_two_edges_splits_a_face() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let v = m.faces[top].verts.clone();
    let opposite = [edge_key(v[0], v[1]), edge_key(v[2], v[3])];
    let new = ops::connect_edges(&mut m, &opposite);
    assert_eq!(new.len(), 1);
    assert_eq!(counts(&m), (10, 15, 7));
    assert_solid(&m, "connect");
    // One edge: a loop cut.
    let mut m = cube();
    let e = m.edges()[0];
    assert_eq!(ops::connect_edges(&mut m, &[e]).len(), 4);
}

#[test]
fn merge_top_corners_into_a_pyramid() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let v = m.faces[top].verts.clone();
    let merged = ops::merge_vertices(&mut m, &v).unwrap();
    assert_eq!(counts(&m), (5, 8, 5));
    assert_solid(&m, "pyramid");
    assert!((m.position(merged) - Vec3::new(0.0, 1.0, 0.0)).length() < 1e-5);
    assert!((m.volume() - 1.0 / 3.0).abs() < 1e-4);
}

#[test]
fn weld_rejoins_detached_faces() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    ops::detach_faces(&mut m, &[top]);
    assert_eq!(counts(&m).0, 12);
    assert!(!m.is_closed());
    assert_eq!(ops::weld_vertices(&mut m, &[], 1e-4), 4);
    assert_eq!(counts(&m), (8, 12, 6));
    assert_solid(&m, "welded");
    // Only selected vertices weld.
    let mut m = cube();
    ops::detach_faces(&mut m, &[top]);
    assert_eq!(ops::weld_vertices(&mut m, &[0, 1], 1e-4), 0);
}

#[test]
fn delete_flip_and_bridge() {
    let mut m = cube();
    let top = face_towards(&m, Vec3::Y);
    let v = m.faces[top].verts.clone();
    ops::delete_faces(&mut m, &[top]);
    assert_eq!(counts(&m), (8, 12, 5));
    assert!(m.is_manifold() && !m.is_closed());
    // Bridging two opposite border edges closes the box again.
    let (e1, e2) = (edge_key(v[0], v[1]), edge_key(v[2], v[3]));
    let f = ops::bridge_edges(&mut m, e1, e2).unwrap();
    assert_eq!(f, 5);
    assert_solid(&m, "bridged");
    assert!((m.volume() - 1.0).abs() < 1e-4);
    // Interior edges cannot be bridged.
    assert!(ops::bridge_edges(&mut m, e1, e2).is_none());

    let all: Vec<usize> = (0..m.faces.len()).collect();
    ops::flip_faces(&mut m, &all);
    assert!(m.volume() < 0.0 && m.is_manifold());
}

#[test]
fn center_pivot_and_transform() {
    let mut m = cube();
    let c = ops::center_pivot(&mut m);
    assert!((c - Vec3::new(0.0, 0.5, 0.0)).length() < 1e-6);
    let (lo, hi) = m.bounds();
    assert!((lo + hi).length() < 1e-6);
    let top = face_towards(&m, Vec3::Y);
    let verts = Selection::faces(vec![top]).all_verts(&m);
    ops::transform_vertices(&mut m, &verts, &Mat4::from_translation(Vec3::Y));
    assert!((m.face_center(top).y - 1.5).abs() < 1e-6);
    assert_solid(&m, "moved top");
}

#[test]
fn selection_retain_valid_after_topology_change() {
    let mut m = cube();
    let mut sel = Selection {
        verts: vec![0, 7, 99],
        edges: m.edges(),
        faces: vec![0, 5, 6],
    };
    sel.retain_valid(&m);
    assert_eq!(sel.verts, vec![0, 7]);
    assert_eq!(sel.faces, vec![0, 5]);
    ops::delete_faces(&mut m, &[0]);
    sel.retain_valid(&m);
    assert!(sel.faces.iter().all(|f| *f < 5));
}

#[test]
fn convert_primitives_and_triangle_meshes() {
    use crate::scene::{MeshRef, PrimitiveMesh};
    let c = convert::from_mesh_ref(&MeshRef::Primitive(PrimitiveMesh::Cube)).unwrap();
    assert_solid(&c, "converted cube");
    assert_eq!(c.bounds(), (Vec3::splat(-0.5), Vec3::splat(0.5)));
    let q = convert::from_mesh_ref(&MeshRef::Primitive(PrimitiveMesh::Quad)).unwrap();
    assert!(q.face_normal(0).z < -0.99, "quad faces −Z");
    // Generic path: the primitive sphere welds into a closed smooth solid.
    let s = convert::from_mesh_data(crate::render::mesh::primitive(PrimitiveMesh::Sphere));
    assert!(s.is_valid() && s.is_manifold(), "sphere");
    assert!(s.faces.iter().all(|f| f.smoothing == 1));
    // The primitive cube's triangles merge back into 6 quads.
    let c = convert::from_mesh_data(crate::render::mesh::primitive(PrimitiveMesh::Cube));
    assert_eq!(counts(&c), (8, 12, 6));
    assert_solid(&c, "welded cube");
}

#[test]
fn scene_round_trip_bakes_on_load() {
    let mut scene = Scene::new("Mesh");
    let e = scene.spawn("Box", None);
    let mut mesh = shapes::stairs(1.0, 1.0, 1.0, 3);
    ops::set_material(&mut mesh, &[0], 1);
    scene.add(e, MeshFilter::default());
    scene.add(e, MeshRenderer::default());
    scene.add(e, mesh.clone());
    sync(&scene);
    let text = scene.to_json();
    let (loaded, report) = Scene::from_json("Mesh", &text).unwrap();
    assert!(report.warnings.is_empty(), "{:?}", report.warnings);
    let e2 = loaded.find_by_name("Box").unwrap();
    assert_eq!(*loaded.world.get::<&EditableMesh>(e2).unwrap(), mesh);
    // Loading baked it and pointed the Mesh Filter at the result.
    let key = loaded
        .world
        .get::<&MeshFilter>(e2)
        .unwrap()
        .mesh
        .clone()
        .unwrap();
    assert!(is_baked_key(&key));
    let data = crate::models::mesh(&key).unwrap();
    let tris: usize = (0..mesh.faces.len())
        .map(|f| mesh.triangles_of(f).len())
        .sum();
    assert_eq!(data.indices.len(), tris * 3);
    // Element 1 draws the material-1 face only.
    let one = crate::models::mesh(&submesh(&key, 1)).unwrap();
    assert_eq!(one.indices.len(), mesh.triangles_of(0).len() * 3);
    // Same content, same key: saving again is byte-identical.
    assert_eq!(loaded.to_json(), text);
}

#[test]
fn draws_one_submesh_per_material_element() {
    struct Defaults;
    impl crate::render_scene::MaterialSource for Defaults {
        fn params(&self, _: Option<&str>) -> crate::render::MaterialParams {
            Default::default()
        }
    }
    let mut scene = Scene::new("Draws");
    let e = scene.spawn("Box", None);
    let mut mesh = cube();
    ops::set_material(&mut mesh, &[2], 1);
    scene.add(e, MeshFilter::default());
    scene.add(
        e,
        MeshRenderer {
            extra_materials: vec![None],
            ..Default::default()
        },
    );
    scene.add(e, mesh);
    let draws = crate::render_scene::mesh_draws(&scene, &Defaults, |_| false);
    assert_eq!(draws.len(), 2);
    let tris: Vec<usize> = draws
        .iter()
        .map(|d| crate::models::mesh(&d.mesh).unwrap().indices.len() / 3)
        .collect();
    assert_eq!(tris, vec![10, 2]);
    // Editing re-bakes under a new key.
    let before = scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
    scene.world.get::<&mut EditableMesh>(e).unwrap().positions[0].x -= 1.0;
    sync(&scene);
    assert_ne!(scene.world.get::<&MeshFilter>(e).unwrap().mesh, before);
}

#[test]
fn export_to_obj_imports_back_as_one_mesh_per_material() {
    let mut mesh = shapes::prism(2.0, 1.0, 1.0);
    let top = face_towards(&mesh, Vec3::Y);
    ops::set_material(&mut mesh, &[top], 2);
    let (text, materials) = export::to_obj(&mesh, "Roof Piece");
    assert_eq!(materials, vec![0, 2]);
    let dir = std::env::temp_dir().join("misty_mesh_builder_export");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("roof.obj");
    std::fs::write(&path, text).unwrap();
    let model = crate::models::load(&path).unwrap();
    assert_eq!(model.meshes.len(), 2);
    // Same shape back: bounds and outward winding survive the right-handed round trip.
    let (lo, hi) = model.meshes[0].bounds;
    let (lo1, hi1) = model.meshes[1].bounds;
    let (mlo, mhi) = mesh.bounds();
    assert!((lo.min(lo1) - mlo).length() < 1e-4 && (hi.max(hi1) - mhi).length() < 1e-4);
    let d = &model.meshes[0].data;
    for t in d.indices.chunks(3) {
        let p = |i: u32| Vec3::from(d.vertices[i as usize].position);
        let n = (p(t[1]) - p(t[0])).cross(p(t[2]) - p(t[0]));
        assert!(n.dot(Vec3::from(d.vertices[t[0] as usize].normal)) > 0.0);
    }
}

/// A scene whose Editable Mesh has a face past its positions (or a huge material element)
/// loads with that face dropped: loading baked the mesh and panicked on the index, or tried
/// to allocate one mesh per material up to 4 billion.
#[test]
fn damaged_editable_meshes_load_without_the_bad_faces() {
    let mut s = Scene::new("T");
    let e = s.spawn("M", None);
    let mut faces = vec![Face::new(vec![0, 1, 2]), Face::new(vec![0, 1, 9])];
    faces[0].material = 4_000_000_000;
    faces.push(Face::new(vec![0, 0, 1]));
    s.add(
        e,
        EditableMesh::new(vec![Vec3::ZERO, Vec3::X, Vec3::Z], faces),
    );
    s.add(e, MeshFilter::default());
    s.add(e, MeshRenderer::default());
    let (back, _) = Scene::from_json("T", &s.to_json()).unwrap();
    let e = back.find_by_name("M").unwrap();
    let m = back.world.get::<&EditableMesh>(e).unwrap();
    assert_eq!(m.faces.len(), 1);
    assert_eq!(m.faces[0].material, MAX_MATERIAL);
    assert!(m.is_valid());
}
