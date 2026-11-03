//! Animation window: creating a clip, adding properties, keys, Record mode, preview and undo.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::animation::{AnimationClip, Property};
use misty_engine::scene::Animation;

use super::*;
use crate::commands;
use crate::project::ProjectBrowser;

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root = std::env::temp_dir().join(format!("misty_anim_win_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    (EditorState::new(ProjectBrowser::open(root.clone())), root)
}

/// The window as the editor runs it: preview before the UI, recording after it.
fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1000.0, 360.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                preview::pre_frame(s, 0.0);
                show(ui, s);
                preview::post_frame(s, false);
            },
            s,
        );
    h.run();
    h
}

fn y_curve(s: &mut EditorState) -> misty_engine::animation::Curve {
    let path = s.animation.clip.clone().unwrap();
    let clip = s.anim_clips.get(&path).unwrap();
    clip.curve("", Property::Position(1)).unwrap().clone()
}

#[test]
fn create_clip_add_property_record_and_undo() {
    let (mut s, root) = project("create");
    let cube = s.scene.spawn("Cube", None);
    s.selection.set(cube);
    let mut h = window(s);
    h.get_by_label("To begin animating Cube, create an Animation Clip.");
    h.get_by_label("Create").click();
    h.run();
    let file = root.join("Assets/Animations/Cube.anim");
    assert!(file.exists(), "clip file written");
    let anim = (*h.state().scene.world.get::<&Animation>(cube).unwrap()).clone();
    assert_eq!(anim.clip.as_deref(), Some("Assets/Animations/Cube.anim"));
    assert!(
        h.state()
            .assets
            .anims
            .contains("Assets/Animations/Cube.anim")
    );

    h.get_by_label("Add Property").click();
    h.run();
    h.get_by_label("Transform ▸ Position").click();
    h.run();
    h.get_by_label("Cube : Position");
    h.get_by_label("Position.y");
    let s = h.state_mut();
    let y = y_curve(s);
    assert_eq!(y.keys.len(), 2, "keys at the start and end of the clip");
    assert!(s.animation.preview);

    // Record mode: an edit at 0.5 s becomes a key there.
    s.animation.recording = true;
    s.animation.time = 0.5;
    preview::pre_frame(s, 0.0);
    let mut t = s.scene.transform(cube);
    t.position.y = 2.0;
    s.scene.set_transform(cube, t);
    preview::post_frame(s, false);
    let y = y_curve(s);
    assert_eq!(y.keys.len(), 3);
    assert_eq!(y.evaluate(0.5), 2.0);
    let saved = AnimationClip::load(&file).unwrap();
    assert_eq!(
        saved.curve("", Property::Position(1)).unwrap().keys.len(),
        3
    );

    // Scrubbing elsewhere poses the cube; leaving the preview restores it.
    s.animation.time = 0.0;
    preview::pre_frame(s, 0.0);
    assert_eq!(s.scene.transform(cube).position.y, 0.0);
    s.animation.time = 0.5;
    preview::pre_frame(s, 0.0);
    assert_eq!(s.scene.transform(cube).position.y, 2.0);
    preview::stop_preview(s);
    assert_eq!(s.scene.transform(cube).position.y, 0.0);

    // Undo removes the recorded key (and the clip file follows).
    s.push(crate::actions::Action::Undo);
    for a in std::mem::take(&mut s.actions) {
        commands::apply(s, a);
    }
    preview::post_frame(s, false);
    assert_eq!(y_curve(s).keys.len(), 2);
    let saved = AnimationClip::load(&file).unwrap();
    assert_eq!(
        saved.curve("", Property::Position(1)).unwrap().keys.len(),
        2
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn add_key_button_select_and_delete_keys() {
    let (mut s, root) = project("keys");
    let cube = s.scene.spawn("Cube", None);
    s.selection.set(cube);
    preview::create_clip(&mut s, cube);
    preview::add_property(&mut s, cube, "", &Property::Position(0).siblings());
    s.animation.time = 0.25;
    let mut h = window(s);
    h.get_by_label("◆+").click();
    h.run();
    let s = h.state_mut();
    assert_eq!(y_curve(s).keys.len(), 3, "keyed at the playhead");
    assert!(y_curve(s).keys.find(0.25).is_some());

    // Select the keys at 0.25 s on all three curves and delete them.
    for c in 0..3 {
        s.animation.selected.insert((c, 0.25f32.to_bits()));
    }
    s.animation.focused = true;
    assert!(s.animation.wants_delete());
    preview::delete_selected_keys(s);
    assert_eq!(y_curve(s).keys.len(), 2);
    assert!(s.animation.selected.is_empty());
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn dopesheet_and_curves_views_draw() {
    let (mut s, root) = project("views");
    let cube = s.scene.spawn("Cube", None);
    s.selection.set(cube);
    preview::create_clip(&mut s, cube);
    preview::add_property(&mut s, cube, "", &[Property::Active]);
    let mut h = window(s);
    h.get_by_label("Cube : Is Active");
    h.get_by_label("Curves").click();
    h.run();
    assert!(h.state().animation.curves_view);
    h.get_by_label("Dopesheet").click();
    h.run();
    assert!(!h.state().animation.curves_view);
    // Ruler labels use seconds:frames.
    assert_eq!(timeline::time_label(1.5, 60.0), "1:30");
    std::fs::remove_dir_all(root).ok();
}
