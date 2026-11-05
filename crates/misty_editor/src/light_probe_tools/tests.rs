//! Scene-view light probe editing through the real Scene panel (egui_kittest), Fill Box and the
//! bake's preconditions.

use egui::{Event, Key, Modifiers, PointerButton, Pos2};
use egui_kittest::Harness;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::light_probes::LightProbeGroup;

use super::*;
use crate::project::ProjectBrowser;

fn harness(name: &str) -> Harness<'static, EditorState> {
    let root = std::env::temp_dir().join(name);
    let state = EditorState::new(ProjectBrowser::open(root));
    let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::scene::show(ui, s, None),
        state,
    );
    h.run_steps(2);
    h
}

/// A group with two probes on the X axis, selected and in edit mode.
fn setup(h: &mut Harness<'_, EditorState>) -> Entity {
    let s = h.state_mut();
    let e = s.scene.spawn("Light Probe Group", None);
    s.scene.add(
        e,
        LightProbeGroup {
            enabled: true,
            probes: vec![Vec3::new(-2.0, 0.0, 0.0), Vec3::new(2.0, 0.0, 0.0)],
        },
    );
    s.selection.set(e);
    s.light_probes.set_editing(true);
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

fn key(h: &mut Harness<'_, EditorState>, key: Key, modifiers: Modifiers) {
    for pressed in [true, false] {
        h.event(Event::Key {
            key,
            physical_key: None,
            pressed,
            repeat: false,
            modifiers,
        });
        h.step();
    }
    h.run_steps(2);
}

fn probes(h: &Harness<'_, EditorState>, e: Entity) -> Vec<Vec3> {
    h.state()
        .scene
        .world
        .get::<&LightProbeGroup>(e)
        .unwrap()
        .probes
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
fn clicking_selects_probes_and_dragging_moves_them() {
    let mut h = harness("misty_light_probe_drag_test");
    let e = setup(&mut h);
    let a = screen(&h, Vec3::new(2.0, 0.0, 0.0));
    click(&mut h, a, Modifiers::NONE);
    assert_eq!(h.state().light_probes.selected, vec![1]);
    let b = screen(&h, Vec3::new(-2.0, 0.0, 0.0));
    click(&mut h, b, Modifiers::SHIFT);
    assert_eq!(h.state().light_probes.selected.len(), 2);

    let before = probes(&h, e);
    press(&mut h, a, true, Modifiers::NONE);
    for i in 1..=5 {
        h.event(Event::PointerMoved(a + egui::vec2(0.0, -8.0 * i as f32)));
        h.step();
    }
    press(&mut h, a + egui::vec2(0.0, -40.0), false, Modifiers::NONE);
    h.run_steps(2);
    let after = probes(&h, e);
    for (p, q) in before.iter().zip(&after) {
        assert!(q.y > p.y + 0.1, "both selected probes moved up: {p} → {q}");
    }
    assert_eq!(
        h.state().selection.active(),
        Some(e),
        "group stays selected"
    );

    // Empty space clears the probe selection but keeps the group selected and editing.
    let empty = screen(&h, Vec3::new(0.0, 0.0, 6.0));
    click(&mut h, empty, Modifiers::NONE);
    assert!(h.state().light_probes.selected.is_empty());
    assert_eq!(h.state().selection.active(), Some(e));
    assert!(h.state().light_probes.editing);
}

#[test]
fn delete_and_duplicate_act_on_the_selected_probes_with_undo() {
    let mut h = harness("misty_light_probe_keys_test");
    let e = setup(&mut h);
    let a = screen(&h, Vec3::new(2.0, 0.0, 0.0));
    click(&mut h, a, Modifiers::NONE);
    let objects = h.state().scene.walk().len();
    // Ctrl+D copies the probe in place and selects the copy.
    key(&mut h, Key::D, CTRL);
    assert_eq!(probes(&h, e).len(), 3);
    assert_eq!(h.state().light_probes.selected, vec![2]);
    assert_eq!(h.state().undo.undo_label(), Some("Duplicate Light Probe"));
    assert_eq!(
        h.state().scene.walk().len(),
        objects,
        "no object was duplicated"
    );
    key(&mut h, Key::Delete, Modifiers::NONE);
    assert_eq!(probes(&h, e).len(), 2);
    assert_eq!(h.state().undo.undo_label(), Some("Delete Light Probe"));
    assert!(h.state().scene.contains(e), "the group itself stays");
    h.state_mut().undo_step();
    assert_eq!(probes(&h, e).len(), 3);
    h.state_mut().undo_step();
    assert_eq!(probes(&h, e).len(), 2);
}

#[test]
fn fill_box_and_add_probe_are_undoable() {
    let root = std::env::temp_dir().join("misty_light_probe_fill_test");
    let mut s = EditorState::new(ProjectBrowser::open(root));
    create_group(&mut s, None);
    let e = s.selection.active().unwrap();
    assert_eq!(s.scene.name(e), "Light Probe Group");
    s.light_probes.fill_size = Vec3::new(4.0, 2.0, 4.0);
    s.light_probes.fill_spacing = 2.0;
    fill_box(&mut s, e);
    let n = s
        .scene
        .world
        .get::<&LightProbeGroup>(e)
        .unwrap()
        .probes
        .len();
    assert_eq!(n, 3 * 2 * 3);
    add_probe_at_view_center(&mut s, e);
    assert_eq!(
        s.scene
            .world
            .get::<&LightProbeGroup>(e)
            .unwrap()
            .probes
            .len(),
        n + 1
    );
    assert_eq!(s.light_probes.selected, vec![n]);
    s.undo_step();
    s.undo_step();
    assert_eq!(
        s.scene
            .world
            .get::<&LightProbeGroup>(e)
            .unwrap()
            .probes
            .len(),
        8
    );
    // Eight cube corners: the 12 box edges plus diagonals.
    let edges = tetra_edges(&LightProbeGroup::default().probes);
    assert!((17..=28).contains(&edges.len()), "{}", edges.len());
}

#[test]
fn baking_needs_a_saved_scene_with_probes() {
    let root = std::env::temp_dir().join("misty_light_probe_bake_test");
    let mut s = EditorState::new(ProjectBrowser::open(root));
    s.scene.forget_scene_path(0);
    start_bake(&mut s);
    assert!(!s.light_probes.baking());
    assert!(s.light_probes.status.contains("Save the scene"));
    s.scene.set_scene_path(0, "Assets/Scenes/Probe.scene");
    start_bake(&mut s);
    assert!(!s.light_probes.baking());
    assert!(s.light_probes.status.contains("No light probes"));
    create_group(&mut s, None);
    start_bake(&mut s);
    assert!(s.light_probes.baking());
    assert_eq!(s.light_probes.progress().unwrap().0, 0.0);
    cancel_bake(&mut s);
    assert!(!s.light_probes.baking());
}
