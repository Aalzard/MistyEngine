//! Terrain Inspector UI tests (egui harness): the tool bar, palette cards, adding and removing
//! grass types, live previews refreshing after a change, reset buttons, confirmed Clear, and
//! the Inspector's draw time with previews.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::scene::Terrain;
use misty_engine::terrain::TerrainData;
use misty_engine::terrain::detail_layers::DetailLayer;
use misty_engine::terrain::store;

use crate::project::ProjectBrowser;
use crate::state::EditorState;
use crate::terrain_previews::TuftParams;
use crate::terrain_tools::{self, TerrainTool};

/// An editor with a selected terrain whose data (absolute path, in its own folder) has one
/// grass species "Tall"; the Inspector open on `tool`.
fn setup(name: &str, tool: TerrainTool) -> (EditorState, String) {
    let root = std::env::temp_dir().join(format!("misty_terrain_ui_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    let path = root
        .join("Assets/T.terrain")
        .to_string_lossy()
        .replace('\\', "/");
    let mut d = TerrainData::new(65, glam::Vec3::new(64.0, 20.0, 64.0), 64);
    d.details.enabled = true;
    d.layers = vec![
        misty_engine::terrain::TerrainLayer {
            name: "Grass".into(),
            ..Default::default()
        },
        misty_engine::terrain::TerrainLayer {
            name: "Rock".into(),
            ..Default::default()
        },
    ];
    d.fit_splat_maps();
    d.detail_layers.push(DetailLayer::grass("Tall"));
    d.save(std::path::Path::new(&path)).unwrap();
    store::insert(&path, d);
    let mut s = EditorState::new(ProjectBrowser::open(root));
    let e = s.scene.spawn("Terrain", None);
    s.scene.add(
        e,
        Terrain {
            terrain_data: path.clone(),
            ..Default::default()
        },
    );
    s.selection.set(e);
    s.terrain_tools.tool = tool;
    (s, path)
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    let mut h = Harness::builder()
        .with_size([420.0, 2600.0])
        .build_ui_state(
            |ui, s: &mut EditorState| {
                crate::panels::inspector::show(ui, s);
                // The app closes edits once the mouse is up (one undo step each).
                let down = ui.input(|i| i.pointer.any_down());
                terrain_tools::end_frame(s, down);
            },
            s,
        );
    h.run();
    h
}

fn data(path: &str) -> std::sync::Arc<TerrainData> {
    store::get(path).unwrap().0
}

#[test]
fn tool_bar_switches_tools_by_icon() {
    let (s, _) = setup("toolbar", TerrainTool::RaiseLower);
    let mut h = harness(s);
    h.get_by_label("Paint Trees").click();
    h.run();
    assert_eq!(h.state().terrain_tools.tool, TerrainTool::Trees);
    // The Trees palette's empty state and its "+" card.
    h.get_by_label("No trees yet: add a tree asset to start planting.");
    h.get_by_label("Add Tree");
    h.get_by_label("Paint Texture").click();
    h.run();
    assert_eq!(h.state().terrain_tools.tool, TerrainTool::Paint);
    h.get_by_label("Rock").click();
    h.run();
    assert_eq!(h.state().terrain_tools.layer, 1);
}

#[test]
fn selecting_a_palette_card_picks_what_the_brush_paints() {
    let (s, _) = setup("select", TerrainTool::Details);
    let mut h = harness(s);
    assert_eq!(h.state().terrain_tools.detail, 0, "terrain grass first");
    h.get_by_label("Tall").click();
    h.run();
    assert_eq!(h.state().terrain_tools.detail, 1);
    // Its settings show, in sections.
    for label in [
        "Shape",
        "Colour",
        "Blade Width",
        "Healthy ↔ Dry",
        "Brightness",
        "Seed Heads",
    ] {
        h.get_by_label(label);
    }
    h.get_by_label("Terrain Grass").click();
    h.run();
    assert_eq!(h.state().terrain_tools.detail, 0);
    h.get_by_label("Rules, Wind and Patches…");
}

#[test]
fn grass_types_are_added_and_removed_with_undo() {
    let (s, path) = setup("addremove", TerrainTool::Details);
    let mut h = harness(s);
    h.get_by_label("Add").click();
    h.run();
    h.get_by_label("Grass Species").click();
    h.run();
    h.run();
    assert_eq!(data(&path).detail_layers.len(), 2);
    assert_eq!(
        h.state().terrain_tools.detail,
        2,
        "the new type is selected"
    );
    assert_eq!(h.state().undo.undo_label(), Some("Add Detail Grass"));
    // Remove "Tall" from its card's hover action.
    h.get_by_label("Tall").hover();
    h.run();
    h.get_by_label("Remove").click();
    h.run();
    h.run();
    let d = data(&path);
    assert_eq!(d.detail_layers.len(), 1);
    assert_eq!(d.detail_layers[0].name, "Grass 2");
    assert_eq!(h.state().undo.undo_label(), Some("Remove Detail Layer"));
    h.state_mut().undo_step();
    assert_eq!(data(&path).detail_layers.len(), 2, "undo brings it back");
}

#[test]
fn previews_are_requested_and_refreshed_after_a_setting_change() {
    let (s, path) = setup("preview", TerrainTool::Details);
    let mut h = harness(s);
    h.get_by_label("Tall").click();
    h.run();
    let p = |d: &TerrainData| TuftParams::species(&d.detail_layers[0], &d.details);
    let before = h.state().project.thumbnails.terrain.stats;
    assert!(
        before.tufts_built >= 2,
        "card and large preview built: {before:?}"
    );
    assert!(
        h.state()
            .project
            .thumbnails
            .terrain
            .has_tuft(&p(&data(&path)))
    );
    // A frame without changes builds nothing new.
    h.run();
    assert_eq!(
        h.state().project.thumbnails.terrain.stats.tufts_built,
        before.tufts_built
    );
    // Taller, drier grass: the next frame shows it.
    store::modify_settings(&path, |d| {
        d.detail_layers[0].height = [1.0, 1.6];
        d.detail_layers[0].dryness = 0.9;
    });
    h.run();
    let after = h.state().project.thumbnails.terrain.stats;
    assert!(after.tufts_built >= before.tufts_built + 2, "{after:?}");
    assert!(
        h.state()
            .project
            .thumbnails
            .terrain
            .has_tuft(&p(&data(&path)))
    );
    // The terrain grass's colour is in its species' previews too.
    store::modify_settings(&path, |d| d.details.healthy_color = [0.1, 0.8, 0.1]);
    h.run();
    assert!(
        h.state()
            .project
            .thumbnails
            .terrain
            .has_tuft(&p(&data(&path)))
    );
}

#[test]
fn reset_buttons_restore_defaults_as_one_undo_step() {
    let (s, path) = setup("reset", TerrainTool::Details);
    store::modify_settings(&path, |d| d.detail_layers[0].bend = 1.2);
    let mut h = harness(s);
    h.get_by_label("Tall").click();
    h.run();
    // Only the changed field offers a reset.
    let resets = h.get_all_by_label("Reset").count();
    assert_eq!(resets, 1);
    h.get_by_label("Reset").click();
    h.run();
    h.run();
    assert_eq!(
        data(&path).detail_layers[0].bend,
        DetailLayer::default().bend
    );
    assert_eq!(h.state().undo.undo_label(), Some("Change Detail Layer"));
    assert!(h.query_by_label("Reset").is_none());
    // Brush fields reset too (editor settings, no undo step).
    h.state_mut().terrain_tools.size = 80.0;
    h.run();
    h.get_by_label("Reset").click();
    h.run();
    assert_eq!(
        h.state().terrain_tools.size,
        terrain_tools::TerrainTools::default().size
    );
    h.state_mut().undo_step();
    assert_eq!(data(&path).detail_layers[0].bend, 1.2);
}

#[test]
fn clear_asks_first() {
    let (s, path) = setup("clear", TerrainTool::Details);
    store::modify(&path, |d| {
        misty_engine::terrain::detail_layers::fill(
            d,
            misty_engine::terrain::detail_layers::DetailTarget::Layer(0),
            1.0,
        )
    });
    let mut h = harness(s);
    h.get_by_label("Tall").click();
    h.run();
    h.get_by_label("Clear").click();
    h.run();
    assert!(
        data(&path).detail_layers[0].map.iter().any(|v| *v > 0),
        "not yet"
    );
    h.get_by_label("Erase Tall everywhere on this terrain?");
    // The confirmation's own Clear button (the last one).
    h.get_all_by_label("Clear").last().unwrap().click();
    h.run();
    h.run();
    assert!(data(&path).detail_layers[0].map.iter().all(|v| *v == 0));
    assert_eq!(h.state().undo.undo_label(), Some("Clear Details"));
}

#[test]
fn bracket_keys_change_the_brush_size() {
    let (s, _) = setup("brackets", TerrainTool::RaiseLower);
    let mut h = harness(s);
    let size = h.state().terrain_tools.size;
    h.key_press(egui::Key::CloseBracket);
    h.run();
    assert!(h.state().terrain_tools.size > size);
    h.key_press(egui::Key::OpenBracket);
    h.key_press(egui::Key::OpenBracket);
    h.run();
    assert!(h.state().terrain_tools.size < size);
}

/// The Inspector's draw time on the Paint Details tool with live previews (release build:
/// `cargo test --release -p misty_editor terrain_ui_tests::inspector_draw_time -- --nocapture`).
#[test]
fn inspector_draw_time_with_previews() {
    let (s, path) = setup("timing", TerrainTool::Details);
    store::modify_settings(&path, |d| {
        for i in 0..6 {
            let mut l = DetailLayer::grass(&format!("Species {i}"));
            l.height = [0.2 + i as f32 * 0.1, 0.5 + i as f32 * 0.15];
            l.seed_heads = i as f32 * 0.15;
            d.detail_layers.push(l);
        }
    });
    let times = std::rc::Rc::new(std::cell::RefCell::new(Vec::new()));
    let t2 = times.clone();
    let mut h = Harness::builder()
        .with_size([420.0, 2600.0])
        .build_ui_state(
            move |ui, s: &mut EditorState| {
                let t = std::time::Instant::now();
                crate::panels::inspector::show(ui, s);
                t2.borrow_mut().push(t.elapsed().as_secs_f64() * 1000.0);
            },
            s,
        );
    h.run();
    h.get_by_label("Species 3").click();
    h.run();
    times.borrow_mut().clear();
    // Steady frames (everything cached), then frames that each change a setting (one card and
    // the large preview rebuilt per frame, like dragging a slider).
    for _ in 0..30 {
        h.step();
    }
    let steady: Vec<f64> = times.borrow_mut().drain(..).collect();
    for i in 0..30 {
        store::modify_settings(&path, |d| d.detail_layers[3].bend = 0.1 + i as f32 * 0.04);
        h.step();
    }
    let dragging: Vec<f64> = times.borrow().clone();
    eprintln!("dragging frames (ms): {dragging:.1?}");
    let avg = |v: &[f64]| v.iter().sum::<f64>() / v.len().max(1) as f64;
    let max = |v: &[f64]| v.iter().copied().fold(0.0, f64::max);
    eprintln!(
        "Terrain Inspector (Paint Details, 7 grass cards + large preview): steady {:.2} ms avg / {:.2} ms max; while dragging a setting {:.2} ms avg / {:.2} ms max",
        avg(&steady),
        max(&steady),
        avg(&dragging),
        max(&dragging)
    );
    // Loose bound (debug builds and loaded machines are slower).
    assert!(avg(&dragging) < 60.0, "{dragging:?}");
}
