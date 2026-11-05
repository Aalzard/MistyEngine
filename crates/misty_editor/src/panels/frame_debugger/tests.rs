//! Frame Debugger window: the event tree (groups, collapsing, filter), keyboard stepping,
//! Enable pausing Play mode, and the window opening from the layout.

use std::collections::HashSet;

use egui::{Event, Key, Modifiers};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::render::{EventKind, EventTarget, FrameCapture, FrameEvent};

use super::tree::{Row, rows};
use super::*;
use crate::layout::{self, Tab};
use crate::project::ProjectBrowser;

fn ev(pass: &[&str], kind: EventKind, name: &str) -> FrameEvent {
    FrameEvent {
        pass: pass.iter().map(|s| (*s).to_owned()).collect(),
        display: EventTarget::Hdr,
        ..FrameEvent::new(kind, name)
    }
}

fn capture() -> FrameCapture {
    let mut cube = ev(
        &["Opaque"],
        EventKind::Draw,
        "Draw Mesh (instanced ×3) Cube A",
    );
    cube.instances = 3;
    cube.object = Some("Cube A".into());
    cube.mesh = Some("Cube".into());
    cube.material = Some("Assets/Materials/Red.mat".into());
    cube.batch = Some("GPU instanced: 3 objects share mesh, shader variant and textures".into());
    cube.shader = "Standard (Opaque) · mesh.wgsl vs_main / fs_main".into();
    cube.properties = vec![("Metallic".into(), "0.00".into())];
    FrameCapture {
        events: vec![
            ev(&["Shadows", "Cascade 0"], EventKind::Clear, "Clear (depth)"),
            ev(
                &["Shadows", "Cascade 0"],
                EventKind::Draw,
                "Draw Mesh Ground",
            ),
            ev(&["Shadows", "Cascade 1"], EventKind::Clear, "Clear (depth)"),
            ev(
                &["Shadows", "Cascade 1"],
                EventKind::Draw,
                "Draw Mesh Ground",
            ),
            ev(&["Sky"], EventKind::Clear, "Clear (color, depth)"),
            ev(&["Sky"], EventKind::Fullscreen, "Draw Sky"),
            ev(&["Opaque"], EventKind::Draw, "Draw Mesh Ground"),
            cube,
            ev(&["Post", "Bloom"], EventKind::Fullscreen, "Bloom Prefilter"),
            ev(&["Post", "Tonemap"], EventKind::Fullscreen, "Composite"),
        ],
        size: (320, 200),
        stopped_at: None,
    }
}

#[test]
fn rows_group_consecutive_passes_and_collapse() {
    let c = capture();
    let none = HashSet::new();
    let all = rows(&c.events, &none, "");
    // Shadows, Cascade 0 (+2), Cascade 1 (+2), Sky (+2), Opaque (+2), Post, Bloom (+1), Tonemap (+1).
    assert_eq!(all.len(), 10 + 8);
    assert_eq!(
        all[0],
        Row::Pass {
            key: "Shadows".into(),
            name: "Shadows".into(),
            depth: 0,
            events: 4,
            last: 3
        }
    );
    assert!(
        matches!(&all[1], Row::Pass { key, events: 2, depth: 1, .. } if key == "Shadows ▸ Cascade 0")
    );
    assert_eq!(all[2], Row::Event { index: 0, depth: 2 });
    let collapsed: HashSet<String> = ["Shadows".to_owned()].into();
    let folded = rows(&c.events, &collapsed, "");
    assert_eq!(
        folded.len(),
        all.len() - 6,
        "cascade groups and their events hidden"
    );
    assert!(matches!(&folded[1], Row::Pass { key, .. } if key == "Sky"));
    // The filter lists matching events flat.
    let found = rows(&c.events, &none, "cube");
    assert_eq!(found, vec![Row::Event { index: 7, depth: 0 }]);
    let found = rows(&c.events, &none, "red.mat");
    assert_eq!(found.len(), 1);
}

fn state() -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_framedbg_{}", std::process::id()));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

fn window(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([1000.0, 520.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run();
    h
}

#[test]
fn enable_pauses_play_mode_and_disable_resumes_it() {
    let mut s = state();
    s.play = PlayState::Playing;
    set_enabled(&mut s, true);
    assert_eq!(s.play, PlayState::Paused);
    set_enabled(&mut s, false);
    assert_eq!(s.play, PlayState::Playing);
    // In Edit mode Enable leaves the play state alone.
    s.play = PlayState::Edit;
    set_enabled(&mut s, true);
    assert_eq!(s.play, PlayState::Edit);
}

#[test]
fn window_lists_events_steps_with_arrows_and_shows_details() {
    let mut h = window(state());
    h.get_by_label("Enable").click();
    h.run();
    assert!(h.state().frame_debugger.enabled);
    assert!(h.state().frame_debugger.active(&h.ctx));
    // The Game view would hand over its capture now; the first one selects the last event.
    h.state_mut().frame_debugger.set_capture(capture());
    h.run();
    assert_eq!(h.state().frame_debugger.selected, 9);
    h.get_by_label("10 of 10");
    h.get_by_label("Shadows (4)");
    h.get_by_label("Cascade 1 (2)");
    // Click the instanced cube draw: its details appear.
    h.get_by_label("Draw Mesh (instanced ×3) Cube A").click();
    h.run();
    assert_eq!(h.state().frame_debugger.selected, 7);
    h.get_by_label("Assets/Materials/Red.mat");
    h.get_by_label("GPU instanced: 3 objects share mesh, shader variant and textures");
    h.get_by_label("Material Properties");
    // ← / → step while the pointer is over the window.
    h.event(Event::PointerMoved(egui::pos2(500.0, 300.0)));
    let key = |h: &mut Harness<'_, EditorState>, key| {
        h.event(Event::Key {
            key,
            physical_key: None,
            pressed: true,
            repeat: false,
            modifiers: Modifiers::NONE,
        });
        h.run();
    };
    key(&mut h, Key::ArrowLeft);
    assert_eq!(h.state().frame_debugger.selected, 6);
    key(&mut h, Key::ArrowRight);
    key(&mut h, Key::ArrowRight);
    assert_eq!(h.state().frame_debugger.selected, 8);
    // Collapse a group from its arrow; selecting a group picks its last event.
    h.get_by_label("Opaque (2)").click();
    h.run();
    assert_eq!(h.state().frame_debugger.selected, 7);
    h.get_by_label("● Disable").click();
    h.run();
    assert!(!h.state().frame_debugger.enabled);
}

#[test]
fn frame_debugger_opens_beside_the_project_window_and_in_the_menu() {
    let mut dock = layout::default_layout();
    layout::focus_or_open(&mut dock, Tab::FrameDebugger);
    let fd = dock.find_tab(&Tab::FrameDebugger).expect("opened");
    assert_eq!(fd.node, dock.find_tab(&Tab::Project).unwrap().node);
    assert!(Tab::ON_DEMAND.contains(&Tab::FrameDebugger));
    assert_eq!(Tab::FrameDebugger.title(), "Frame Debugger");
}
