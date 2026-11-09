//! Timeline window: creating a timeline, adding tracks and clips, trimming and moving clips
//! with the mouse, bindings, scrubbing preview and undo.

use egui::{Event, Modifiers, PointerButton, Pos2, pos2};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::scene::PlayableDirector;
use misty_engine::timeline::{TimelineAsset, TrackKind};

use super::*;
use crate::commands;
use crate::project::ProjectBrowser;

fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
    let root =
        std::env::temp_dir().join(format!("misty_timeline_win_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    (EditorState::new(ProjectBrowser::open(root.clone())), root)
}

/// The window as the editor runs it: preview before the UI.
fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1000.0, 360.0])
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

/// Drag with the left button from `a` to `b` in small steps.
fn drag(h: &mut Harness<'_, EditorState>, a: Pos2, b: Pos2) {
    h.event(Event::PointerMoved(a));
    h.step();
    press(h, a, PointerButton::Primary, true);
    for i in 1..=8 {
        let p = a + (b - a) * (i as f32 / 8.0);
        h.event(Event::PointerMoved(p));
        h.step();
    }
    press(h, b, PointerButton::Primary, false);
    h.run();
}

fn asset(s: &mut EditorState) -> TimelineAsset {
    let (_, path) = current(s).unwrap();
    s.anim_clips.timelines.get(&path).unwrap().clone()
}

/// Lane centre y and the x of time `t` from last frame's layout.
fn at(s: &EditorState, track: &str, t: f32) -> Pos2 {
    let g = &s.timeline.geometry;
    let lane = g.lanes.iter().find(|(id, _)| id == track).unwrap().1;
    pos2(g.axis.x(t), lane.center().y)
}

#[test]
fn create_add_track_add_clip_trim_move_and_undo() {
    let (mut s, root) = project("edit");
    let hero = s.scene.spawn("Hero", None);
    s.selection.set(hero);
    let mut h = window(s);
    h.get_by_label(
        "To begin a new timeline with Hero, create a Director archetype and a Timeline asset.",
    );
    h.get_by_label("Create").click();
    h.run();
    let file = root.join("Assets/Timelines/HeroTimeline.timeline");
    assert!(file.exists(), "timeline file written");
    let d = (*h
        .state()
        .scene
        .world
        .get::<&PlayableDirector>(hero)
        .unwrap())
    .clone();
    assert_eq!(
        d.playable.as_deref(),
        Some("Assets/Timelines/HeroTimeline.timeline")
    );
    assert!(
        h.state()
            .assets
            .timelines
            .contains("Assets/Timelines/HeroTimeline.timeline")
    );

    // Add ▸ Activation Track.
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Activation Track").click();
    h.run();
    let a = asset(h.state_mut());
    assert_eq!(a.tracks.len(), 1);
    assert_eq!(a.tracks[0].kind, TrackKind::Activation);
    let id = a.tracks[0].id.clone();
    h.get_by_label("Activation Track");
    assert_eq!(TimelineAsset::load(&file).unwrap(), a, "saved right away");

    // Right-click the lane at 1 s ▸ Add Activation Clip.
    let p = at(h.state(), &id, 1.0);
    click(&mut h, p, PointerButton::Secondary);
    h.get_by_label("Add Activation Clip").click();
    h.run();
    let a = asset(h.state_mut());
    let c = &a.tracks[0].clips[0];
    assert!(
        (c.start - 1.0).abs() < 0.05,
        "added at the pointer: {}",
        c.start
    );
    assert!((c.duration - 2.0).abs() < 1e-4);
    let start = c.start;

    // Drag the right edge 1 s to the right: the clip gets longer (snapped to frames).
    let from = at(h.state(), &id, start + 2.0) - egui::vec2(2.0, 0.0);
    let to = at(h.state(), &id, start + 3.0) - egui::vec2(2.0, 0.0);
    drag(&mut h, from, to);
    let a = asset(h.state_mut());
    let c = &a.tracks[0].clips[0];
    assert!((c.start - start).abs() < 1e-4, "start kept");
    assert!((c.duration - 3.0).abs() < 0.05, "trimmed to {}", c.duration);
    let frames = c.end() * a.frame_rate;
    assert!((frames - frames.round()).abs() < 1e-2, "end on a frame");

    // Drag the left edge right by 0.5 s: the end stays.
    let end = c.end();
    let from = at(h.state(), &id, start) + egui::vec2(2.0, 0.0);
    let to = at(h.state(), &id, start + 0.5) + egui::vec2(2.0, 0.0);
    drag(&mut h, from, to);
    let a = asset(h.state_mut());
    let c = &a.tracks[0].clips[0];
    assert!((c.end() - end).abs() < 1e-3);
    assert!((c.start - (start + 0.5)).abs() < 0.05, "start {}", c.start);

    // Drag the body: it moves, keeping its length; one undo step per drag.
    let (s0, d0) = (c.start, c.duration);
    let mid = s0 + d0 / 2.0;
    let (a0, a1) = (at(h.state(), &id, mid), at(h.state(), &id, mid + 1.0));
    drag(&mut h, a0, a1);
    let a = asset(h.state_mut());
    let c = &a.tracks[0].clips[0];
    assert!((c.duration - d0).abs() < 1e-4);
    assert!((c.start - (s0 + 1.0)).abs() < 0.05, "moved to {}", c.start);

    let s = h.state_mut();
    s.push(crate::actions::Action::Undo);
    for act in std::mem::take(&mut s.actions) {
        commands::apply(s, act);
    }
    let a = asset(s);
    assert!(
        (a.tracks[0].clips[0].start - s0).abs() < 1e-4,
        "undo puts the move back"
    );
    assert_eq!(
        TimelineAsset::load(&file).unwrap(),
        a,
        "undo saves the file"
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn scrubbing_previews_activation_and_restores_the_scene() {
    let (mut s, root) = project("scrub");
    let dir = s.scene.spawn("Director", None);
    let lamp = s.scene.spawn("Lamp", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let id = ops::add_track(&mut s, &path, TrackKind::Activation, None).unwrap();
    ops::add_clip(&mut s, &path, &id, None, Some(2.0));
    ops::set_binding(&mut s, dir, &id, Some(lamp));
    let mut h = window(s);
    // Click the ruler at 1 s: preview on, lamp off (outside the clip).
    let ruler = h.state().timeline.geometry.ruler;
    let x = h.state().timeline.geometry.axis.x(1.0);
    click(&mut h, pos2(x, ruler.center().y), PointerButton::Primary);
    assert!(h.state().timeline.preview);
    assert!((h.state().timeline.time - 1.0).abs() < 0.05);
    assert!(!h.state().scene.game_object(lamp).active);
    let x = h.state().timeline.geometry.axis.x(3.0);
    click(&mut h, pos2(x, ruler.center().y), PointerButton::Primary);
    assert!(h.state().scene.game_object(lamp).active);
    // Leaving the preview puts the scene back.
    h.get_by_label("Preview").click();
    h.run();
    let s = h.state_mut();
    assert!(!s.timeline.preview);
    assert!(s.scene.game_object(lamp).active);
    s.timeline.time = 1.0;
    s.timeline.preview = true;
    preview::pre_frame(s, 0.0);
    assert!(!s.scene.game_object(lamp).active);
    preview::stop_preview(s);
    assert!(s.scene.game_object(lamp).active);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn groups_mute_lock_and_markers() {
    let (mut s, root) = project("groups");
    let dir = s.scene.spawn("Director", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let g = ops::add_track(&mut s, &path, TrackKind::Group, None).unwrap();
    // The selected group receives new tracks.
    let mut h = window(s);
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Signal Track").click();
    h.run();
    let a = asset(h.state_mut());
    let sig = a
        .tracks
        .iter()
        .find(|t| t.kind == TrackKind::Signal)
        .unwrap();
    assert_eq!(sig.group.as_deref(), Some(g.as_str()));
    let sig = sig.id.clone();

    // Right-click the signal lane ▸ Add Signal Marker; the markers row ▸ Add Marker.
    let p = at(h.state(), &sig, 1.5);
    click(&mut h, p, PointerButton::Secondary);
    h.get_by_label("Add Signal Marker").click();
    h.run();
    let row = h.state().timeline.geometry.markers;
    let x = h.state().timeline.geometry.axis.x(0.5);
    click(&mut h, pos2(x, row.center().y), PointerButton::Secondary);
    h.get_by_label("Add Marker").click();
    h.run();
    let a = asset(h.state_mut());
    assert_eq!(a.track(&sig).unwrap().markers.len(), 1);
    assert!((a.track(&sig).unwrap().markers[0].time - 1.5).abs() < 0.05);
    assert_eq!(a.markers.len(), 1);

    // Mute and lock buttons; a locked track ignores edits.
    h.get_all_by_label("🔇").next().unwrap().click();
    h.run();
    h.get_all_by_label("🔒").nth(1).unwrap().click();
    h.run();
    let a = asset(h.state_mut());
    assert!(a.track(&g).unwrap().muted, "group muted");
    assert!(
        a.is_muted(a.track(&sig).unwrap()),
        "muted through the group"
    );
    assert!(a.track(&sig).unwrap().locked);
    let p = at(h.state(), &sig, 1.5);
    let before = asset(h.state_mut());
    drag(&mut h, p, p + egui::vec2(60.0, 0.0));
    assert_eq!(asset(h.state_mut()), before, "locked: nothing moved");

    // Folding the group hides its tracks.
    h.get_by_label("⏷").click();
    h.run();
    assert_eq!(rows(&asset(h.state_mut())).len(), 1);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn inspector_edits_the_selected_clip_and_director_bindings() {
    let (mut s, root) = project("inspector");
    let dir = s.scene.spawn("Director", None);
    let lamp = s.scene.spawn("Lamp", None);
    s.selection.set(dir);
    let path = ops::create_for(&mut s, dir).unwrap();
    let id = ops::add_track(&mut s, &path, TrackKind::Activation, None).unwrap();
    // Director component: one object field per track.
    let mut h = Harness::builder().with_size([500.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::panels::inspector::show(ui, s),
        s,
    );
    h.run();
    h.get_by_label("Playable Director");
    h.get_by_label("Bindings");
    h.get_by_label("Activation Track");
    let s = h.state_mut();
    ops::set_binding(s, dir, &id, Some(lamp));
    ops::add_clip(s, &path, &id, None, Some(0.0));
    // Selecting the clip shows it in the Inspector.
    s.timeline.selected = Some(TimelineSel::Clip(id.clone(), 0));
    s.selection.clear();
    s.inspector_asset = Some(path.clone());
    h.run();
    h.get_by_label("Start");
    h.get_by_label("Duration");
    let bound = h
        .state()
        .scene
        .world
        .get::<&PlayableDirector>(dir)
        .unwrap()
        .binding(&id);
    assert_eq!(bound, Some(lamp));
    std::fs::remove_dir_all(root).ok();
}
