//! Mesh Builder editor tests: undo per operation, element picking and handles through the real
//! Scene panel, the floating strip, shortcuts, New Shape, convert and export.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::mesh_builder::shapes::{self, Shape};
use misty_engine::mesh_builder::{EditableMesh, Selection};
use misty_engine::scene::{MeshFilter, MeshRenderer, PrimitiveMesh};

use super::{EditMode, Op, run};
use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn project(name: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!("misty_mesh_builder_{name}"))
}

/// A fresh editor with an editable 1 m cube at the origin, selected.
fn state_with_cube(name: &str) -> (EditorState, Entity) {
    let mut s = EditorState::new(ProjectBrowser::open(project(name)));
    let e = s.scene.spawn("Box", None);
    s.scene.add(e, MeshFilter::default());
    s.scene.add(e, MeshRenderer::default());
    s.scene.add(e, shapes::cube(Vec3::ONE));
    misty_engine::mesh_builder::sync(&s.scene);
    s.select(e);
    super::sync_frame(&mut s);
    (s, e)
}

fn mesh(s: &EditorState, e: Entity) -> EditableMesh {
    (*s.scene.world.get::<&EditableMesh>(e).unwrap()).clone()
}

fn top_face(m: &EditableMesh) -> usize {
    (0..m.faces.len())
        .max_by(|a, b| m.face_normal(*a).y.total_cmp(&m.face_normal(*b).y))
        .unwrap()
}

/// Every operation is one undo step that restores the mesh exactly, and redo re-applies it.
#[test]
fn each_operation_undoes_in_one_step() {
    let cases: [(Op, EditMode); 12] = [
        (Op::Extrude, EditMode::Face),
        (Op::Extrude, EditMode::Edge),
        (Op::Inset, EditMode::Face),
        (Op::Bevel, EditMode::Edge),
        (Op::Subdivide, EditMode::Face),
        (Op::Connect, EditMode::Edge),
        (Op::Merge, EditMode::Vertex),
        (Op::Detach, EditMode::Face),
        (Op::FlipNormals, EditMode::Face),
        (Op::SetMaterial, EditMode::Face),
        (Op::Delete, EditMode::Face),
        (Op::CenterPivot, EditMode::Object),
    ];
    for (op, mode) in cases {
        let (mut s, e) = state_with_cube("undo");
        // Open edges for edge extrude: remove the top first (its own undo step).
        if op == Op::Extrude && mode == EditMode::Edge {
            s.mesh_builder.mode = EditMode::Face;
            s.mesh_builder.selection = Selection::faces(vec![top_face(&mesh(&s, e))]);
            run(&mut s, Op::Delete);
        }
        let before = mesh(&s, e);
        let transform = s.scene.transform(e);
        s.mesh_builder.mode = mode;
        s.mesh_builder.settings.material = 1;
        s.mesh_builder.selection = match mode {
            EditMode::Face => Selection::faces(vec![top_face(&before)]),
            EditMode::Edge => {
                let ef = before.edge_faces();
                let want = if op == Op::Extrude { 1 } else { 2 };
                let e = before
                    .edges()
                    .into_iter()
                    .find(|e| ef[e].len() == want)
                    .unwrap();
                Selection::edges(vec![e])
            }
            EditMode::Vertex => Selection::verts(vec![0, 1]),
            EditMode::Object => Selection::default(),
        };
        assert!(op.enabled(&s), "{op:?} enabled");
        let steps = s.undo.position();
        run(&mut s, op);
        assert_eq!(s.undo.position(), steps + 1, "{op:?}: one step");
        let after = mesh(&s, e);
        assert!(
            after != before || s.scene.transform(e) != transform,
            "{op:?} changed something"
        );
        s.undo_step();
        assert_eq!(mesh(&s, e), before, "{op:?} undone");
        assert_eq!(s.scene.transform(e), transform);
        s.redo_step();
        assert_eq!(mesh(&s, e), after, "{op:?} redone");
    }
}

#[test]
fn set_material_grows_the_materials_list() {
    let (mut s, e) = state_with_cube("material");
    s.mesh_builder.mode = EditMode::Face;
    s.mesh_builder.selection = Selection::faces(vec![0]);
    s.mesh_builder.settings.material = 2;
    run(&mut s, Op::SetMaterial);
    let r = s.scene.world.get::<&MeshRenderer>(e).unwrap();
    assert_eq!(r.material_count(), 3);
}

#[test]
fn bridge_needs_two_border_edges_and_selects_the_new_face() {
    let (mut s, e) = state_with_cube("bridge");
    let m = mesh(&s, e);
    let top = top_face(&m);
    let v = m.faces[top].verts.clone();
    s.mesh_builder.mode = EditMode::Face;
    s.mesh_builder.selection = Selection::faces(vec![top]);
    run(&mut s, Op::Delete);
    s.mesh_builder.mode = EditMode::Edge;
    let key = misty_engine::mesh_builder::edge_key;
    s.mesh_builder.selection = Selection::edges(vec![key(v[0], v[1])]);
    assert!(!Op::Bridge.enabled(&s), "one edge is not enough");
    s.mesh_builder.selection = Selection::edges(vec![key(v[0], v[1]), key(v[2], v[3])]);
    run(&mut s, Op::Bridge);
    assert_eq!(s.mesh_builder.mode, EditMode::Face);
    assert_eq!(s.mesh_builder.selection.faces.len(), 1);
    assert!(mesh(&s, e).is_closed());
}

#[test]
fn freeze_transform_bakes_scale_into_vertices() {
    let (mut s, e) = state_with_cube("freeze");
    let mut t = s.scene.transform(e);
    t.scale = Vec3::new(2.0, 1.0, 3.0);
    t.position = Vec3::new(1.0, 0.0, 0.0);
    s.scene.set_transform(e, t);
    run(&mut s, Op::FreezeTransform);
    assert_eq!(s.scene.transform(e).scale, Vec3::ONE);
    let (lo, hi) = mesh(&s, e).bounds();
    assert!((hi - lo - Vec3::new(2.0, 1.0, 3.0)).length() < 1e-5);
    assert!((lo.x - 0.0).abs() < 1e-5, "moved with the position: {lo:?}");
    assert!(mesh(&s, e).volume() > 0.0);
}

#[test]
fn undo_keeps_the_element_selection_valid() {
    let (mut s, e) = state_with_cube("selection");
    s.mesh_builder.mode = EditMode::Face;
    s.mesh_builder.selection = Selection::faces(vec![top_face(&mesh(&s, e))]);
    run(&mut s, Op::Subdivide);
    s.mesh_builder.selection = Selection::faces((0..9).collect());
    s.undo_step();
    super::sync_frame(&mut s);
    assert!(s.mesh_builder.selection.faces.iter().all(|f| *f < 6));
    // Another object: the element selection is cleared.
    let other = s.scene.spawn("Other", None);
    s.select(other);
    super::sync_frame(&mut s);
    assert!(s.mesh_builder.selection.is_empty());
    assert_eq!(super::target(&s), None);
}

#[test]
fn new_shape_creates_an_editable_object_in_one_step() {
    let (mut s, _) = state_with_cube("shape");
    let steps = s.undo.position();
    let stairs = Shape::ALL[3];
    let e = super::create_shape(&mut s, stairs);
    assert_eq!(s.scene.name(e), "Stairs");
    assert_eq!(s.selection.active(), Some(e));
    assert!(s.scene.world.get::<&EditableMesh>(e).is_ok());
    assert!(
        s.scene
            .world
            .get::<&misty_engine::scene::MeshCollider>(e)
            .is_ok()
    );
    let key = s.scene.world.get::<&MeshFilter>(e).unwrap().mesh.clone();
    assert!(key.is_some_and(|k| misty_engine::mesh_builder::is_baked_key(&k)));
    assert_eq!(s.undo.position(), steps + 1);
    s.undo_step();
    assert!(!s.scene.contains(e));
}

#[test]
fn convert_a_primitive_to_an_editable_mesh() {
    let mut s = EditorState::new(ProjectBrowser::open(project("convert")));
    let e = s.scene.spawn("Cube", None);
    s.scene.add(e, MeshFilter::primitive(PrimitiveMesh::Cube));
    s.scene.add(e, MeshRenderer::default());
    assert!(super::convert_to_editable(&mut s, e));
    let m = mesh(&s, e);
    assert_eq!(m.faces.len(), 6);
    let key = s
        .scene
        .world
        .get::<&MeshFilter>(e)
        .unwrap()
        .mesh
        .clone()
        .unwrap();
    assert!(misty_engine::mesh_builder::is_baked_key(&key));
    s.undo_step();
    assert!(s.scene.world.get::<&EditableMesh>(e).is_err());
}

#[test]
fn export_to_model_writes_an_obj_and_uses_it() {
    let root = project("export");
    let _ = std::fs::remove_dir_all(root.join("Assets/Models"));
    let (mut s, e) = state_with_cube("export");
    s.mesh_builder.mode = EditMode::Face;
    s.mesh_builder.selection = Selection::faces(vec![0]);
    s.mesh_builder.settings.material = 1;
    run(&mut s, Op::SetMaterial);
    run(&mut s, Op::ExportModel);
    assert!(root.join("Assets/Models/Box.obj").is_file());
    assert!(s.scene.world.get::<&EditableMesh>(e).is_err());
    let mesh = s
        .scene
        .world
        .get::<&MeshFilter>(e)
        .unwrap()
        .mesh
        .clone()
        .unwrap();
    assert_eq!(mesh.split_asset(), Some(("Assets/Models/Box.obj", 0)));
    // The second material's faces went to a child.
    let children = s.scene.children(e);
    assert_eq!(children.len(), 1);
    let child_mesh = s
        .scene
        .world
        .get::<&MeshFilter>(children[0])
        .unwrap()
        .mesh
        .clone();
    assert_eq!(
        child_mesh.unwrap().split_asset(),
        Some(("Assets/Models/Box.obj", 1))
    );
    s.undo_step();
    assert!(s.scene.world.get::<&EditableMesh>(e).is_ok());
}

// ---- Scene panel (kittest) -------------------------------------------------------------------

fn harness(name: &str) -> (Harness<'static, EditorState>, Entity) {
    let (state, e) = state_with_cube(name);
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| {
            let typing = ui.ctx().egui_wants_keyboard_input();
            super::shortcuts(ui.ctx(), s, typing);
            crate::panels::scene::show(ui, s, None);
        },
        state,
    );
    // Look at the cube from the front, slightly above.
    h.state_mut().scene_view.camera.align_to(
        Vec3::new(1.6, 2.2, -3.0),
        misty_engine::camera::euler_to_quat(Vec3::new(30.0, -25.0, 0.0)),
    );
    h.run_steps(2);
    (h, e)
}

fn click(h: &mut Harness<'_, EditorState>, at: Pos2, mods: Modifiers) {
    h.event(Event::ModifiersChanged(mods));
    h.event(Event::PointerMoved(at));
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos: at,
            button: PointerButton::Primary,
            pressed,
            modifiers: mods,
        });
        h.step();
    }
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn screen(h: &Harness<'_, EditorState>, p: Vec3) -> Pos2 {
    let cam = h.state().scene_view.camera.camera();
    crate::picking::to_screen(&cam, h.state().scene_view.view_rect, p).unwrap()
}

/// Screen rect of the strip's mode button `i` (0 = Object … 3 = Face).
fn mode_button(h: &Harness<'_, EditorState>, i: usize) -> Pos2 {
    let r = h.state().scene_view.view_rect;
    pos2(
        r.left() + 8.0 + 3.0 + i as f32 * 30.0 + 14.0,
        r.top() + super::strip::TOP + 3.0 + 13.0,
    )
}

#[test]
fn strip_appears_with_an_editable_mesh_and_switches_modes() {
    let (mut h, _) = harness("strip");
    assert!(h.query_by_label("Edit ⏷").is_some(), "strip shown");
    for (i, mode) in EditMode::ALL.into_iter().enumerate().rev() {
        let at = mode_button(&h, i);
        click(&mut h, at, Modifiers::NONE);
        assert_eq!(h.state().mesh_builder.mode, mode);
    }
    // Hidden without an Editable Mesh selected.
    h.state_mut().selection.clear();
    h.run_steps(2);
    assert!(h.query_by_label("Edit ⏷").is_none());
}

#[test]
fn strip_menu_runs_an_operation() {
    let (mut h, e) = harness("strip_menu");
    h.state_mut().mesh_builder.mode = EditMode::Face;
    let top = top_face(&mesh(h.state(), e));
    h.state_mut().mesh_builder.selection = Selection::faces(vec![top]);
    h.run_steps(2);
    h.get_by_label("Edit ⏷").click();
    h.run_steps(3);
    h.get_by_label_contains("Extrude").click();
    h.run_steps(3);
    assert_eq!(mesh(h.state(), e).faces.len(), 10);
}

#[test]
fn keys_switch_modes_and_delete_removes_faces_not_the_object() {
    let (mut h, e) = harness("keys");
    h.key_press(egui::Key::Num4);
    h.run_steps(2);
    assert_eq!(h.state().mesh_builder.mode, EditMode::Face);
    let top = top_face(&mesh(h.state(), e));
    h.state_mut().mesh_builder.selection = Selection::faces(vec![top]);
    h.key_press(egui::Key::Delete);
    h.run_steps(2);
    assert!(h.state().scene.contains(e));
    assert_eq!(mesh(h.state(), e).faces.len(), 5);
    // Ctrl+E extrudes.
    h.state_mut().mesh_builder.selection = Selection::faces(vec![0]);
    h.key_press_modifiers(Modifiers::COMMAND, egui::Key::E);
    h.run_steps(2);
    assert_eq!(mesh(h.state(), e).faces.len(), 9);
    h.key_press(egui::Key::Num1);
    h.run_steps(2);
    assert_eq!(h.state().mesh_builder.mode, EditMode::Object);
}

#[test]
fn clicking_selects_faces_and_shift_adds() {
    let (mut h, e) = harness("pick");
    h.state_mut().mesh_builder.mode = EditMode::Face;
    h.run_steps(2);
    let m = mesh(h.state(), e);
    let top = top_face(&m);
    let front = (0..m.faces.len())
        .max_by(|a, b| (-m.face_normal(*a).z).total_cmp(&(-m.face_normal(*b).z)))
        .unwrap();
    let at = screen(&h, m.face_center(top));
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().mesh_builder.selection.faces, vec![top]);
    let at = screen(&h, m.face_center(front));
    click(&mut h, at, Modifiers::SHIFT);
    assert_eq!(h.state().mesh_builder.selection.faces, vec![top, front]);
    // Empty space clears the elements, the object stays selected.
    click(&mut h, pos2(760.0, 560.0), Modifiers::NONE);
    assert!(h.state().mesh_builder.selection.is_empty());
    assert_eq!(h.state().selection.active(), Some(e));
}

#[test]
fn box_drag_selects_visible_vertices_only() {
    let (mut h, e) = harness("box");
    h.state_mut().mesh_builder.mode = EditMode::Vertex;
    h.run_steps(2);
    let r = h.state().scene_view.view_rect;
    let (a, b) = (
        r.left_top() + egui::vec2(120.0, 90.0),
        r.right_bottom() - egui::vec2(10.0, 10.0),
    );
    h.event(Event::PointerMoved(a));
    h.event(Event::PointerButton {
        pos: a,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=6 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 6.0)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: b,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let verts = &h.state().mesh_builder.selection.verts;
    // From above-front-right, 7 of the 8 corners are visible; the far bottom one is hidden.
    assert_eq!(verts.len(), 7, "{verts:?}");
    let m = mesh(h.state(), e);
    let hidden = (0..8u32).find(|v| !verts.contains(v)).unwrap();
    let p = m.position(hidden);
    assert!(p.y < 0.5 && p.z > 0.0 && p.x < 0.0, "hidden corner {p:?}");
}

#[test]
fn move_handle_drags_the_selected_face() {
    let (mut h, e) = harness("handle");
    h.state_mut().mesh_builder.mode = EditMode::Face;
    let top = top_face(&mesh(h.state(), e));
    h.state_mut().mesh_builder.selection = Selection::faces(vec![top]);
    h.run_steps(2);
    let center = Vec3::new(0.0, 1.0, 0.0);
    let cam = h.state().scene_view.camera.camera();
    let rect = h.state().scene_view.view_rect;
    let len = cam.world_per_pixel(center, rect.height()) * 90.0;
    let (o, tip) = (screen(&h, center), screen(&h, center + Vec3::Y * len));
    let grab = o + (tip - o) * 0.6;
    let dir = (tip - o).normalized();
    h.event(Event::PointerMoved(grab));
    h.event(Event::PointerButton {
        pos: grab,
        button: PointerButton::Primary,
        pressed: true,
        modifiers: Modifiers::NONE,
    });
    h.step();
    for i in 1..=5 {
        h.event(Event::PointerMoved(grab + dir * (10.0 * i as f32)));
        h.step();
    }
    h.event(Event::PointerButton {
        pos: grab + dir * 50.0,
        button: PointerButton::Primary,
        pressed: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
    let m = mesh(h.state(), e);
    assert!(
        m.face_center(top).y > 1.2,
        "top moved up: {}",
        m.face_center(top).y
    );
    // Only the top face's corners moved; the object did not.
    assert!(m.positions.iter().filter(|p| p.y.abs() < 1e-6).count() == 4);
    assert_eq!(h.state().scene.transform(e).position, Vec3::ZERO);
    assert_eq!(
        h.state().mesh_builder.selection.faces,
        vec![top],
        "selection kept"
    );
}
