//! World Builder window tests: menu, create, stamps, generate + undo, preview.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::terrain::store;
use misty_engine::world_gen::{self, StampKind, WorldBuilder, WorldStamp};

use super::*;
use crate::actions::Action;
use crate::layout::Tab;
use crate::project::ProjectBrowser;

fn state(name: &str) -> EditorState {
    let root = std::env::temp_dir().join(name);
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    misty_engine::assets::set_project_root(&root);
    EditorState::new(ProjectBrowser::open(root))
}

/// A builder with a small terrain (fast to generate).
fn small_builder(s: &mut EditorState) -> Entity {
    let b = create_builder(s, None).unwrap();
    // Absolute terrain paths: other tests running in parallel move the process-wide project
    // root, and the generation job outlives many of them.
    let rel = s
        .scene
        .world
        .get::<&WorldBuilder>(b)
        .unwrap()
        .terrain_data
        .clone();
    let abs = s
        .project
        .root
        .join(&rel)
        .to_string_lossy()
        .replace('\\', "/");
    for t in s
        .scene
        .world
        .query_mut::<&mut misty_engine::scene::Terrain>()
    {
        if t.terrain_data == rel {
            t.terrain_data = abs.clone();
        }
    }
    s.scene
        .world
        .get::<&mut WorldBuilder>(b)
        .unwrap()
        .terrain_data = abs;
    with_settings(s, b, |w| {
        w.heightmap_resolution = 65;
        w.alphamap_resolution = 64;
        w.size = [120.0, 40.0, 120.0];
        w.erosion.hydraulic_iterations = 1;
        w.erosion.thermal_iterations = 2;
        true
    });
    b
}

#[test]
fn window_menu_opens_the_world_builder() {
    let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
        state("misty_wb_menu_test"),
    );
    h.run();
    h.get_by_label("Window").click();
    h.run();
    h.get_by_label("Terrain ⏵").hover();
    h.run();
    h.get_by_label("World Builder").click();
    h.run();
    let actions = std::mem::take(&mut h.state_mut().actions);
    assert!(
        matches!(actions.as_slice(), [Action::FocusTab(Tab::WorldBuilder)]),
        "{actions:?}"
    );
}

#[test]
fn create_button_makes_a_builder_terrain_and_presets() {
    let mut h = Harness::builder().with_size([520.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| show(ui, s),
        state("misty_wb_create_test"),
    );
    h.run();
    h.get_by_label("Create World Builder (with a new Terrain)")
        .click();
    h.run();
    let s = h.state();
    let b = current_builder(s).expect("created");
    assert!(world_gen::terrain_of(&s.scene, b).is_some());
    for p in world_gen::Preset::ALL {
        assert!(s.project.root.join(p.path()).is_file(), "{}", p.path());
    }
    h.get_by_label("1  Base Heightmap");
    h.get_by_label("4  Generate");
}

#[test]
fn generate_writes_the_terrain_and_undo_restores_it() {
    let mut s = state("misty_wb_generate_test");
    let b = small_builder(&mut s);
    stamp_ui::add(&mut s, b, "Mountain", stamp_ui::kinds()[0].1.clone());
    let stamp = s.selection.active().unwrap();
    assert!(s.scene.world.get::<&WorldStamp>(stamp).is_ok());
    let path = s
        .scene
        .world
        .get::<&WorldBuilder>(b)
        .unwrap()
        .terrain_data
        .clone();
    let flat = store::get(&path).unwrap().0;
    job::start(&mut s, b, Stages::All, false);
    job::finish(&mut s);
    assert!(
        s.world_builder.status.contains("done"),
        "{}",
        s.world_builder.status
    );
    let (d, _) = store::get(&path).unwrap();
    assert_eq!(d.heightmap_resolution, 65);
    assert!(d.height_range().1 > 0.1, "the mountain rose");
    assert!(!d.layers.is_empty());
    // Saved, with generated objects under the builder.
    assert!(s.project.root.join(&path).is_file());
    let generated = s
        .scene
        .children(b)
        .into_iter()
        .find(|c| s.scene.name(*c) == world_gen::GENERATED)
        .expect("generated objects");
    assert!(!s.scene.children(generated).is_empty());
    // Undo puts the flat terrain and the scene back.
    let s = &mut s;
    s.undo
        .undo(&mut s.scene, &mut s.selection, &mut s.anim_clips);
    assert_eq!(*store::get(&path).unwrap().0, *flat);
    assert!(
        s.scene
            .children(b)
            .into_iter()
            .all(|c| s.scene.name(c) != world_gen::GENERATED)
    );
}

#[test]
fn preview_regenerates_after_edits_and_reverts() {
    let mut s = state("misty_wb_preview_test");
    let b = small_builder(&mut s);
    let path = s
        .scene
        .world
        .get::<&WorldBuilder>(b)
        .unwrap()
        .terrain_data
        .clone();
    let before = store::get(&path).unwrap().0;
    job::preview_tick(&mut s, b);
    assert!(s.world_builder.job.is_none(), "nothing changed yet");
    with_settings(&mut s, b, |w| {
        w.base.height = 0.9;
        true
    });
    job::preview_tick(&mut s, b);
    std::thread::sleep(std::time::Duration::from_millis(400));
    job::preview_tick(&mut s, b);
    assert!(s.world_builder.job.as_ref().is_some_and(|j| j.preview()));
    job::finish(&mut s);
    let (d, _) = store::get(&path).unwrap();
    assert!(d.heightmap_resolution <= world_gen::generate::PREVIEW_RESOLUTION);
    assert_ne!(*d, *before);
    job::revert_preview(&mut s);
    assert_eq!(*store::get(&path).unwrap().0, *before);
}

#[test]
fn stamp_inspector_edits_the_component() {
    let mut s = state("misty_wb_inspector_test");
    let b = small_builder(&mut s);
    stamp_ui::add(&mut s, b, "River", stamp_ui::kinds()[2].1.clone());
    let e = s.selection.active().unwrap();
    let mut h = Harness::builder().with_size([520.0, 700.0]).build_ui_state(
        move |ui, s: &mut EditorState| {
            inspector(ui, s, e, "World Stamp");
        },
        s,
    );
    h.run();
    h.get_by_label("+ Add").click();
    h.run();
    let s = h.state();
    let st = s.scene.world.get::<&WorldStamp>(e).unwrap();
    let StampKind::River(r) = &st.kind else {
        panic!("river")
    };
    assert_eq!(r.points.len(), 4);
}
