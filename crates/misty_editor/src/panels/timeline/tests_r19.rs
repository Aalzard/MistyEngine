//! Timeline window, round 19: Camera tracks (adding shots from the lane menu, the edit-mode
//! preview posing the Game camera and putting it back), Control clips in the preview, the
//! record button writing keys, and Ripple editing.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use glam::Vec3;
use misty_engine::animation::Property;
use misty_engine::scene::{Camera, Transform};
use misty_engine::timeline::{TimelineAsset, TrackKind};
use misty_engine::vcam::{AimKind, BodyKind, CameraBrain, VirtualCamera};

use super::*;
use crate::project::ProjectBrowser;

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("misty_timeline_r19_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    (EditorState::new(ProjectBrowser::open(root.clone())), root)
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1100.0, 380.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                preview::pre_frame(s, 0.0);
                show(ui, s);
            },
            s,
        );
    h.run();
    h
}

fn press(h: &mut Harness<'_, EditorState>, pos: Pos2, button: PointerButton, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button,
        pressed,
        modifiers: Modifiers::NONE,
    });
    h.step();
}

fn click(h: &mut Harness<'_, EditorState>, pos: Pos2, button: PointerButton) {
    h.event(Event::PointerMoved(pos));
    h.step();
    press(h, pos, button, true);
    press(h, pos, button, false);
    h.run();
}

fn drag(h: &mut Harness<'_, EditorState>, a: Pos2, b: Pos2) {
    h.event(Event::PointerMoved(a));
    h.step();
    press(h, a, PointerButton::Primary, true);
    for i in 1..=8 {
        h.event(Event::PointerMoved(a + (b - a) * (i as f32 / 8.0)));
        h.step();
    }
    press(h, b, PointerButton::Primary, false);
    h.run();
}

fn asset(s: &mut EditorState) -> TimelineAsset {
    let (_, path) = current(s).unwrap();
    s.anim_clips.timelines.get(&path).unwrap().clone()
}

fn at(s: &EditorState, track: &str, t: f32) -> Pos2 {
    let g = &s.timeline.geometry;
    let lane = g.lanes.iter().find(|(id, _)| id == track).unwrap().1;
    pos2(g.axis.x(t), lane.center().y)
}

fn scrub(h: &mut Harness<'_, EditorState>, t: f32) {
    let ruler = h.state().timeline.geometry.ruler;
    let x = h.state().timeline.geometry.axis.x(t);
    click(h, pos2(x, ruler.center().y), PointerButton::Primary);
}

fn shot_cam(s: &mut EditorState, name: &str, at: Vec3, fov: f32) -> misty_engine::hecs::Entity {
    let e = s.scene.spawn(name, None);
    s.scene.set_transform(e, Transform::from_position(at));
    s.scene.add(
        e,
        VirtualCamera {
            priority: 1,
            body: BodyKind::DoNothing,
            aim: AimKind::DoNothing,
            lens: misty_engine::vcam::Lens {
                fov,
                ..Default::default()
            },
            ..Default::default()
        },
    );
    e
}

#[test]
fn camera_track_shots_preview_the_game_camera_and_put_it_back() {
    let (mut s, root) = project("camera");
    let cam = s.scene.spawn("Main Camera", None);
    s.scene.add(cam, Camera::default());
    s.scene.add(cam, CameraBrain::default());
    let home = Vec3::new(0.0, 1.0, -10.0);
    s.scene.set_transform(cam, Transform::from_position(home));
    let a = shot_cam(&mut s, "Shot A", Vec3::new(4.0, 2.0, 0.0), 40.0);
    let b = shot_cam(&mut s, "Shot B", Vec3::new(-4.0, 3.0, 0.0), 25.0);
    let fx = s.scene.spawn("Fireworks", None);
    s.scene.game_object_mut(fx).active = false;
    let dir = s.scene.spawn("Director", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let mut h = window(s);

    // Add ▸ Camera Track, bound to the brain camera.
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Camera Track").click();
    h.run();
    let id = asset(h.state_mut()).tracks[0].id.clone();
    assert_eq!(asset(h.state_mut()).tracks[0].kind, TrackKind::Camera);
    ops::set_binding(h.state_mut(), dir, &id, Some(cam));
    h.run();

    // Right-click the lane ▸ Add Camera Shot ▸ Shot A (at 0.5 s), then Shot B after it.
    let p = at(h.state(), &id, 0.5);
    click(&mut h, p, PointerButton::Secondary);
    h.get_by_label_contains("Add Camera Shot").hover();
    h.run();
    h.get_by_label("Shot A").click();
    h.run();
    let s = h.state_mut();
    ops::add_reference_clip(s, &path, &id, b, None);
    let t = asset(s);
    let clips = &t.track(&id).unwrap().clips;
    assert_eq!(clips.len(), 2);
    assert_eq!(clips[0].name, "Shot A");
    assert!((clips[0].start - 0.5).abs() < 0.05, "at the pointer");
    assert!((clips[1].start - clips[0].end()).abs() < 1e-4, "after it");
    let d = director(s, dir).unwrap();
    assert_eq!(d.reference(clips[0].reference.as_deref().unwrap()), Some(a));
    assert_eq!(d.reference(clips[1].reference.as_deref().unwrap()), Some(b));
    h.run();

    // A Control clip switching the fireworks on from 4 s.
    let ctl = ops::add_track(h.state_mut(), &path, TrackKind::Control, None).unwrap();
    ops::add_reference_clip(h.state_mut(), &path, &ctl, fx, Some(4.0));
    h.run();

    // Scrubbing: the brain camera (what the Game view shows) takes each shot's pose and lens.
    scrub(&mut h, 1.0);
    let s = h.state();
    assert!(s.timeline.preview);
    assert!((s.scene.world_position(cam) - Vec3::new(4.0, 2.0, 0.0)).length() < 1e-3);
    assert!((s.scene.world.get::<&Camera>(cam).unwrap().fov - 40.0).abs() < 1e-3);
    assert!(!s.scene.game_object(fx).active);
    scrub(&mut h, 4.5);
    let s = h.state();
    assert!((s.scene.world_position(cam) - Vec3::new(-4.0, 3.0, 0.0)).length() < 1e-3);
    assert!(
        s.scene.game_object(fx).active,
        "the Control clip activates it"
    );

    // Leaving the preview puts the camera, its lens and the object back.
    h.get_by_label("Preview").click();
    h.run();
    let s = h.state();
    assert!(!s.timeline.preview);
    assert!((s.scene.world_position(cam) - home).length() < 1e-4);
    assert!((s.scene.world.get::<&Camera>(cam).unwrap().fov - 60.0).abs() < 1e-3);
    assert!(!s.scene.game_object(fx).active);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn record_button_turns_scene_changes_into_keys() {
    let (mut s, root) = project("record");
    let cube = s.scene.spawn("Cube", None);
    let dir = s.scene.spawn("Director", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let id = ops::add_track(&mut s, &path, TrackKind::Animation, None).unwrap();
    ops::set_binding(&mut s, dir, &id, Some(cube));
    let mut h = window(s);

    // ⏺ arms the track: preview on, the toolbar says so.
    h.get_by_label("⏺").click();
    h.run();
    assert_eq!(h.state().timeline.recording.as_deref(), Some(id.as_str()));
    assert!(h.state().timeline.preview);
    h.get_by_label("⏺ Recording");

    // Move the cube at 1 s, then at 2 s (as a Scene view drag or Inspector edit would).
    let move_to = |h: &mut Harness<'_, EditorState>, t: f32, x: f32| {
        h.state_mut().timeline.time = t;
        h.run();
        let s = h.state_mut();
        let mut tr = s.scene.transform(cube);
        tr.position.x = x;
        s.scene.set_transform(cube, tr);
        h.run();
    };
    move_to(&mut h, 1.0, 3.0);
    move_to(&mut h, 2.0, 5.0);
    let t = asset(h.state_mut());
    let rec = t
        .track(&id)
        .unwrap()
        .recorded
        .clone()
        .expect("recorded keys");
    let curve = rec
        .curves
        .iter()
        .find(|c| c.path.is_empty() && c.property == Property::Position(0))
        .expect("an x curve");
    assert!((curve.evaluate(1.0) - 3.0).abs() < 1e-4);
    assert!((curve.evaluate(2.0) - 5.0).abs() < 1e-4);
    assert!(
        !rec.curves
            .iter()
            .any(|c| c.property == Property::Position(1)),
        "only what changed is keyed"
    );
    assert_eq!(TimelineAsset::load(&root.join(&path)).unwrap(), t, "saved");

    // Scrubbing plays the keys back; leaving the preview puts the cube where it was.
    scrub(&mut h, 1.0);
    assert!((h.state().scene.transform(cube).position.x - 3.0).abs() < 1e-3);
    h.get_by_label("Preview").click();
    h.run();
    let s = h.state();
    assert!(
        s.timeline.recording.is_none(),
        "leaving the preview stops recording"
    );
    assert!(s.scene.transform(cube).position.x.abs() < 1e-5);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn ripple_moves_and_deletes_shift_the_clips_after() {
    let (mut s, root) = project("ripple");
    let dir = s.scene.spawn("Director", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let id = ops::add_track(&mut s, &path, TrackKind::Activation, None).unwrap();
    for start in [0.0, 3.0, 6.0] {
        ops::add_clip(&mut s, &path, &id, None, Some(start));
    }
    let mut h = window(s);
    let starts = |h: &mut Harness<'_, EditorState>| -> Vec<f32> {
        let t = asset(h.state_mut());
        t.track(&id)
            .unwrap()
            .clips
            .iter()
            .map(|c| c.start)
            .collect()
    };

    // Mix (default): only the dragged clip moves.
    let (p0, p1) = (at(h.state(), &id, 4.0), at(h.state(), &id, 4.5));
    drag(&mut h, p0, p1);
    let v = starts(&mut h);
    assert!(
        (v[1] - 3.5).abs() < 0.05 && (v[2] - 6.0).abs() < 1e-4,
        "{v:?}"
    );

    // Ripple: the clips after it move by the same amount.
    h.get_by_label("Ripple").click();
    h.run();
    assert!(h.state().timeline.ripple);
    let (p0, p1) = (at(h.state(), &id, 4.5), at(h.state(), &id, 5.5));
    drag(&mut h, p0, p1);
    let v = starts(&mut h);
    assert!((v[1] - 4.5).abs() < 0.05, "{v:?}");
    assert!((v[2] - 7.0).abs() < 0.05, "shifted with it: {v:?}");
    assert!(v[0].abs() < 1e-4, "earlier clips stay");

    // Ripple delete closes the gap.
    let s = h.state_mut();
    s.timeline.selected = Some(TimelineSel::Clip(id.clone(), 1));
    ops::delete_selected(s, &path);
    let v = starts(&mut h);
    assert_eq!(v.len(), 2);
    assert!(
        (v[1] - 5.0).abs() < 0.05,
        "pulled back by the deleted clip's length: {v:?}"
    );
    std::fs::remove_dir_all(root).ok();
}
