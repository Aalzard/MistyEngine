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
    h.run_steps(3);
    h.get_by_label("Window").click();
    h.run_steps(3);
    h.get_by_label("Terrain ⏵").hover();
    h.run_steps(3);
    h.get_by_label("World Builder").click();
    h.run_steps(3);
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
    h.run_steps(3);
    h.get_by_label("Create World Builder (with a new Terrain)")
        .click();
    h.run_steps(3);
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
    job::start(&mut s, b, Stages::All);
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
    s.undo_step();
    assert_eq!(*store::get(&path).unwrap().0, *flat);
    assert!(
        s.scene
            .children(b)
            .into_iter()
            .all(|c| s.scene.name(c) != world_gen::GENERATED)
    );
}

/// Run the preview until it has settled (coarse, then sharp).
fn settle_preview(s: &mut EditorState, b: Entity, ctx: &egui::Context) {
    for _ in 0..300 {
        preview::tick(s, b, ctx);
        preview::finish(s, ctx);
        if preview::sharp(s) {
            preview::tick(s, b, ctx);
            if !s.world_builder.preview.busy() {
                return;
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    panic!("the preview never settled");
}

#[test]
fn preview_follows_edits_coarse_then_sharp() {
    let mut s = state("misty_wb_preview_test");
    let b = small_builder(&mut s);
    with_settings(&mut s, b, |w| {
        w.heightmap_resolution = 513;
        w.alphamap_resolution = 256;
        true
    });
    let ctx = egui::Context::default();
    let path = s
        .scene
        .world
        .get::<&WorldBuilder>(b)
        .unwrap()
        .terrain_data
        .clone();
    let before = store::get(&path).unwrap().0;
    // First look: a coarse pass right away.
    preview::tick(&mut s, b, &ctx);
    assert!(s.world_builder.preview.busy());
    preview::finish(&mut s, &ctx);
    assert!(s.world_builder.preview.has_image());
    assert_eq!(
        preview::last_data(&s).unwrap().heightmap_resolution,
        preview::COARSE
    );
    assert!(!preview::sharp(&s));
    // After a quiet moment: the sharp pass.
    settle_preview(&mut s, b, &ctx);
    assert_eq!(
        preview::last_data(&s).unwrap().heightmap_resolution,
        preview::SHARP
    );
    let sharp = preview::last_data(&s).unwrap();
    // An edit starts a coarse pass again, and the result changes.
    with_settings(&mut s, b, |w| {
        w.base.height = 0.95;
        true
    });
    preview::tick(&mut s, b, &ctx);
    assert!(s.world_builder.preview.busy());
    preview::finish(&mut s, &ctx);
    assert_eq!(
        preview::last_data(&s).unwrap().heightmap_resolution,
        preview::COARSE
    );
    settle_preview(&mut s, b, &ctx);
    assert_ne!(preview::last_data(&s).unwrap().heights, sharp.heights);
    // Live in Scene view is off: the terrain itself was never touched.
    assert_eq!(*store::get(&path).unwrap().0, *before);
}

#[test]
fn live_in_scene_view_pushes_the_preview_and_reverts() {
    let mut s = state("misty_wb_live_test");
    let b = small_builder(&mut s);
    with_settings(&mut s, b, |w| {
        w.live_preview = true;
        true
    });
    let ctx = egui::Context::default();
    let path = s
        .scene
        .world
        .get::<&WorldBuilder>(b)
        .unwrap()
        .terrain_data
        .clone();
    let before = store::get(&path).unwrap().0;
    with_settings(&mut s, b, |w| {
        w.base.height = 0.9;
        true
    });
    settle_preview(&mut s, b, &ctx);
    let (d, _) = store::get(&path).unwrap();
    assert_ne!(*d, *before, "the terrain shows the preview");
    assert_eq!(Some(&*d), preview::last_data(&s).as_ref());
    job::revert_preview(&mut s);
    assert_eq!(*store::get(&path).unwrap().0, *before);
}

#[test]
fn preview_card_shows_modes_and_live_toggle() {
    let mut s = state("misty_wb_preview_card_test");
    let b = small_builder(&mut s);
    let mut h = Harness::builder()
        .with_size([420.0, 900.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run_steps(3);
    for _ in 0..200 {
        h.run_steps(3);
        if h.state().world_builder.preview.has_image() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(h.state().world_builder.preview.has_image());
    h.get_by_label("Preview");
    h.get_all_by_label("Height").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(
        h.state().world_builder.preview.mode,
        world_gen::preview::PreviewMode::Height
    );
    h.get_all_by_label("Biomes").next().unwrap().click();
    h.run_steps(3);
    assert_eq!(
        h.state().world_builder.preview.mode,
        world_gen::preview::PreviewMode::Biomes
    );
    h.get_by_label("Live in Scene view").click();
    h.run_steps(3);
    let st = h.state();
    assert!(st.scene.world.get::<&WorldBuilder>(b).unwrap().live_preview);
}

#[test]
fn template_picker_applies_a_template_as_one_undo_step() {
    use world_gen::templates::Template;
    let mut s = state("misty_wb_template_test");
    let b = small_builder(&mut s);
    // Back to the default landform (small_builder trims erosion and the height).
    with_settings(&mut s, b, |w| {
        w.erosion = Default::default();
        w.size[1] = WorldBuilder::default().size[1];
        true
    });
    // A fresh builder: applying needs no confirmation.
    assert!(!templates::is_edited(&s, b));
    let mut h = Harness::builder()
        .with_size([420.0, 1400.0])
        .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
    h.run_steps(3);
    h.get_by_label("Templates");
    for t in Template::ALL {
        h.get_by_label(&format!("{} template", t.label()));
    }
    h.get_by_label("Desert template").click();
    h.run_steps(3);
    let st = h.state();
    let w = (*st.scene.world.get::<&WorldBuilder>(b).unwrap()).clone();
    assert_eq!(w.base.generator, world_gen::Generator::Dunes);
    assert_eq!(w.default_biome, world_gen::Preset::Desert.path());
    assert_eq!(w.heightmap_resolution, 65, "resolution is kept");
    let stamps = world_gen::stamps_of(&st.scene, b);
    assert_eq!(stamps.len(), Template::Desert.setup(&w).stamps.len());
    assert!(
        st.project
            .root
            .join(world_gen::Preset::Volcanic.path())
            .is_file()
    );
    // Switching between untouched templates does not ask either.
    assert!(!templates::is_edited(st, b));
    h.get_by_label("Forest template").click();
    h.run_steps(3);
    assert_eq!(
        h.state()
            .scene
            .world
            .get::<&WorldBuilder>(b)
            .unwrap()
            .base
            .generator,
        world_gen::Generator::RollingHills
    );
    // Edit something: now the picker asks first.
    let st = h.state_mut();
    with_settings(st, b, |w| {
        w.base.seed = 4321;
        true
    });
    st.undo.end_frame(false);
    assert!(templates::is_edited(st, b));
    h.run_steps(3);
    h.get_by_label("Volcanic template").click();
    h.run_steps(3);
    assert_eq!(
        h.state().world_builder.confirm_template,
        Some(Template::Volcanic)
    );
    h.get_by_label("Cancel").click();
    h.run_steps(3);
    assert_eq!(
        h.state()
            .scene
            .world
            .get::<&WorldBuilder>(b)
            .unwrap()
            .base
            .seed,
        4321,
        "Cancel keeps the edits"
    );
    h.get_by_label("Volcanic template").click();
    h.run_steps(3);
    h.get_by_label("Replace").click();
    h.run_steps(3);
    let st = h.state_mut();
    assert_eq!(
        st.scene
            .world
            .get::<&WorldBuilder>(b)
            .unwrap()
            .default_biome,
        world_gen::Preset::Volcanic.path()
    );
    // One undo step brings the edited setup back.
    st.undo_step();
    let w = (*st.scene.world.get::<&WorldBuilder>(b).unwrap()).clone();
    assert_eq!(w.base.seed, 4321);
    assert_eq!(w.base.generator, world_gen::Generator::RollingHills);
}

#[test]
fn template_thumbnails_are_committed() {
    for t in world_gen::templates::Template::ALL {
        let img = misty_engine::image::load_from_memory(templates::thumbnail_png(t))
            .unwrap()
            .into_rgba8();
        assert_eq!(
            img.dimensions(),
            (templates::THUMB, templates::THUMB),
            "{t:?}"
        );
    }
}

#[test]
fn world_menu_creates_terrain_and_builder_from_a_template() {
    let mut h = Harness::builder().with_size([900.0, 700.0]).build_ui_state(
        |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
        state("misty_wb_world_menu_test"),
    );
    h.run_steps(3);
    h.get_by_label("GameObject").click();
    h.run_steps(3);
    h.get_by_label("3D Object ⏵").hover();
    h.run_steps(3);
    h.get_by_label("World ⏵").hover();
    h.run_steps(3);
    h.get_by_label("Islands").click();
    h.run_steps(3);
    let s = h.state_mut();
    job::finish(s);
    let b = current_builder(s).expect("a builder");
    assert_eq!(s.scene.name(b), "Islands World");
    let terrain = world_gen::terrain_of(&s.scene, b).expect("its terrain");
    assert!(
        s.scene
            .world
            .get::<&misty_engine::scene::Terrain>(terrain)
            .is_ok()
    );
    assert!(
        world_gen::stamps_of(&s.scene, b)
            .iter()
            .any(|e| s.scene.name(*e) == "Sea")
    );
    assert!(
        s.world_builder.status.contains("done"),
        "{}",
        s.world_builder.status
    );
    let actions = std::mem::take(&mut s.actions);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::FocusTab(Tab::WorldBuilder))),
        "{actions:?}"
    );
}

#[test]
fn terrain_inspector_button_opens_the_world_builder() {
    let mut s = state("misty_wb_terrain_button_test");
    crate::commands::apply(
        &mut s,
        Action::Create {
            what: crate::actions::Create::Terrain,
            parent: None,
        },
    );
    let terrain = s.selection.active().expect("terrain selected");
    let mut h = Harness::builder().with_size([620.0, 700.0]).build_ui_state(
        move |ui, s: &mut EditorState| {
            crate::panels::inspector::terrain::edit(ui, s, terrain, "Terrain");
        },
        s,
    );
    h.run_steps(3);
    h.get_by_label("World Builder").click();
    h.run_steps(3);
    let s = h.state_mut();
    let b = current_builder(s).expect("a builder for the terrain");
    assert_eq!(world_gen::terrain_of(&s.scene, b), Some(terrain));
    assert_eq!(s.selection.active(), Some(terrain));
    let actions = std::mem::take(&mut s.actions);
    assert!(
        actions
            .iter()
            .any(|a| matches!(a, Action::FocusTab(Tab::WorldBuilder)))
    );
    // A second click reuses that builder.
    h.get_by_label("World Builder").click();
    h.run_steps(3);
    assert_eq!(world_gen::builders(&h.state().scene).len(), 1);
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
    h.run_steps(3);
    h.get_by_label("+ Add").click();
    h.run_steps(3);
    let s = h.state();
    let st = s.scene.world.get::<&WorldStamp>(e).unwrap();
    let StampKind::River(r) = &st.kind else {
        panic!("river")
    };
    assert_eq!(r.points.len(), 4);
}
