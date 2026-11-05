//! Scene-view spline editing through the real Scene panel (egui_kittest).

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::splines::{Knot, Spline, SplineContainer, TangentMode};

use crate::project::ProjectBrowser;
use crate::state::EditorState;

fn harness() -> Harness<'static, EditorState> {
    let root = std::env::temp_dir().join("misty_spline_tools_test_project");
    let state = EditorState::new(ProjectBrowser::open(root));
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::scene::show(ui, s, None),
        state,
    );
    h.run_steps(2);
    h
}

/// A straight two-knot spline along X through the origin, selected.
fn setup(h: &mut Harness<'_, EditorState>) -> Entity {
    let s = h.state_mut();
    let e = s.scene.spawn("Spline", None);
    s.scene.add(
        e,
        SplineContainer::with(Spline::new(
            vec![
                Knot::at(Vec3::new(-2.0, 0.0, 0.0)),
                Knot::at(Vec3::new(2.0, 0.0, 0.0)),
            ],
            false,
        )),
    );
    s.selection.set(e);
    h.run_steps(2);
    e
}

fn screen(h: &Harness<'_, EditorState>, p: Vec3) -> Pos2 {
    let cam = h.state().scene_view.camera.camera();
    let rect = h.state().scene_view.view_rect;
    crate::picking::to_screen(&cam, rect, p).expect("on screen")
}

fn press(h: &mut Harness<'_, EditorState>, pos: Pos2, pressed: bool, modifiers: Modifiers) {
    h.event(Event::ModifiersChanged(modifiers));
    h.event(Event::PointerMoved(pos));
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers,
    });
    h.step();
}

fn click(h: &mut Harness<'_, EditorState>, pos: Pos2, modifiers: Modifiers) {
    press(h, pos, true, modifiers);
    press(h, pos, false, modifiers);
    h.event(Event::ModifiersChanged(Modifiers::NONE));
    h.run_steps(2);
}

fn key(h: &mut Harness<'_, EditorState>, key: Key) {
    h.event(Event::Key {
        key,
        physical_key: None,
        pressed: true,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.step();
    h.event(Event::Key {
        key,
        physical_key: None,
        pressed: false,
        repeat: false,
        modifiers: Modifiers::NONE,
    });
    h.run_steps(2);
}

fn knots(h: &Harness<'_, EditorState>, e: Entity) -> Vec<Knot> {
    h.state()
        .scene
        .world
        .get::<&SplineContainer>(e)
        .unwrap()
        .splines[0]
        .knots
        .clone()
}

const CTRL: Modifiers = Modifiers {
    alt: false,
    ctrl: true,
    shift: false,
    mac_cmd: false,
    command: true,
};

#[test]
fn ctrl_click_inserts_and_delete_removes_knots() {
    let mut h = harness();
    let e = setup(&mut h);
    let mid = screen(&h, Vec3::new(0.5, 0.0, 0.0));
    click(&mut h, mid, CTRL);
    let k = knots(&h, e);
    assert_eq!(k.len(), 3, "Ctrl+click on the curve inserts a knot");
    assert!(
        (k[1].position - Vec3::new(0.5, 0.0, 0.0)).length() < 0.1,
        "{}",
        k[1].position
    );
    assert_eq!(h.state().splines.selected.len(), 1);
    assert_eq!(h.state().splines.selected[0].knot, 1);
    assert_eq!(h.state().undo.undo_label(), Some("Insert Knot"));
    assert_eq!(
        h.state().selection.active(),
        Some(e),
        "object stays selected"
    );

    // Delete with the pointer over the view removes the selected knot.
    key(&mut h, Key::Delete);
    assert_eq!(knots(&h, e).len(), 2);
    assert!(h.state().splines.selected.is_empty());
    assert_eq!(h.state().undo.undo_label(), Some("Delete Knot"));
    // Each action is its own undo step.
    h.state_mut().undo_step();
    assert_eq!(knots(&h, e).len(), 3);
    h.state_mut().undo_step();
    assert_eq!(knots(&h, e).len(), 2);
}

#[test]
fn clicking_selects_knots_and_dragging_moves_them() {
    let mut h = harness();
    let e = setup(&mut h);
    let a = screen(&h, Vec3::new(2.0, 0.0, 0.0));
    click(&mut h, a, Modifiers::NONE);
    assert_eq!(h.state().splines.selected.len(), 1);
    assert_eq!(h.state().splines.selected[0].knot, 1);
    // Shift+click adds the other knot.
    let b = screen(&h, Vec3::new(-2.0, 0.0, 0.0));
    click(&mut h, b, Modifiers::SHIFT);
    assert_eq!(h.state().splines.selected.len(), 2);
    // Plain click on one knot keeps it selected; drag it upward on screen.
    click(&mut h, a, Modifiers::NONE);
    assert_eq!(
        h.state().splines.selected.len(),
        2,
        "clicking a selected knot keeps the set"
    );
    let before = knots(&h, e);
    let start = screen(&h, Vec3::new(2.0, 0.0, 0.0));
    press(&mut h, start, true, Modifiers::NONE);
    for i in 1..=5 {
        h.event(Event::PointerMoved(
            start + egui::vec2(0.0, -8.0 * i as f32),
        ));
        h.step();
    }
    press(
        &mut h,
        start + egui::vec2(0.0, -40.0),
        false,
        Modifiers::NONE,
    );
    h.run_steps(2);
    let after = knots(&h, e);
    for (a, b) in before.iter().zip(&after) {
        assert!(
            b.position.y > a.position.y + 0.1,
            "both selected knots moved up"
        );
    }
    assert_eq!(h.state().selection.active(), Some(e));
    // Clicking empty space clears the knot selection.
    let empty = screen(&h, Vec3::new(0.0, 0.0, 6.0));
    click(&mut h, empty, Modifiers::NONE);
    assert!(h.state().splines.selected.is_empty());
}

#[test]
fn draw_tool_places_knots_on_the_ground_and_closes_loops() {
    let mut h = harness();
    let e = setup(&mut h);
    h.state_mut().splines.set_drawing(true);
    let pts = [
        Vec3::new(0.0, 0.0, 2.0),
        Vec3::new(3.0, 0.0, 4.0),
        Vec3::new(-2.0, 0.0, 5.0),
    ];
    for p in pts {
        let at = screen(&h, p);
        click(&mut h, at, Modifiers::NONE);
    }
    let c = (*h.state().scene.world.get::<&SplineContainer>(e).unwrap()).clone();
    assert_eq!(c.splines.len(), 2, "drawing starts a new spline");
    let drawn = &c.splines[1];
    assert_eq!(drawn.knots.len(), 3);
    for (k, p) in drawn.knots.iter().zip(pts) {
        assert!((k.position - p).length() < 0.05, "{} vs {p}", k.position);
        assert_eq!(k.mode, TangentMode::AutoSmooth);
    }
    // Clicking the first knot closes the loop and ends drawing.
    let first = screen(&h, pts[0]);
    click(&mut h, first, Modifiers::NONE);
    let c = (*h.state().scene.world.get::<&SplineContainer>(e).unwrap()).clone();
    assert!(c.splines[1].closed);
    assert!(!h.state().splines.drawing);
    assert_eq!(h.state().undo.undo_label(), Some("Close Spline"));
}

#[test]
fn dragging_while_drawing_makes_a_bezier_knot() {
    let mut h = harness();
    let e = setup(&mut h);
    h.state_mut().splines.set_drawing(true);
    let at = screen(&h, Vec3::new(0.0, 0.0, 3.0));
    press(&mut h, at, true, Modifiers::NONE);
    for i in 1..=5 {
        h.event(Event::PointerMoved(at + egui::vec2(10.0 * i as f32, 0.0)));
        h.step();
    }
    press(&mut h, at + egui::vec2(50.0, 0.0), false, Modifiers::NONE);
    let c = (*h.state().scene.world.get::<&SplineContainer>(e).unwrap()).clone();
    let k = c.splines[1].knots[0];
    assert_eq!(k.mode, TangentMode::Mirrored);
    let out = k.rotation * k.tangent_out;
    assert!(out.length() > 0.2 && out.y.abs() < 1e-3, "{out}");
    key(&mut h, Key::Escape);
    assert!(!h.state().splines.drawing);
}

#[test]
fn move_tool_handle_moves_a_knot_along_one_axis() {
    let mut h = harness();
    let e = setup(&mut h);
    h.state_mut().tool = crate::state::Tool::Move;
    h.state_mut().global_space = true;
    let knot = Vec3::new(-2.0, 0.0, 0.0);
    let at = screen(&h, knot);
    click(&mut h, at, Modifiers::NONE);
    assert_eq!(h.state().splines.selected.len(), 1);
    let cam = h.state().scene_view.camera.camera();
    let rect = h.state().scene_view.view_rect;
    let len = cam.world_per_pixel(knot, rect.height()) * super::AXIS_PX;
    let tip = screen(&h, knot + Vec3::Y * len);
    let grab = at + (tip - at) * 0.6;
    let dir = (tip - at).normalized();
    press(&mut h, grab, true, Modifiers::NONE);
    for i in 1..=5 {
        h.event(Event::PointerMoved(grab + dir * (8.0 * i as f32)));
        h.step();
    }
    press(&mut h, grab + dir * 40.0, false, Modifiers::NONE);
    h.run_steps(2);
    let p = knots(&h, e)[0].position;
    assert!(p.y > 0.2, "moved up the Y axis: {p}");
    assert!((p.x + 2.0).abs() < 1e-3 && p.z.abs() < 1e-3, "{p}");
}
