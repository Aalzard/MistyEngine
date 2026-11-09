//! Tutorial tests: the data, navigation, conditions, saved progress, and every tutorial run end
//! to end in the real editor UI (menu bar, toolbar and docked panels) with kittest.

use egui_dock::{DockArea, DockState};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::scene::Terrain;
use misty_engine::terrain::store;
use misty_engine::world_gen::{self, WorldBuilder};

use super::*;
use crate::actions::Action;
use crate::layout::{self, Tab};
use crate::panels::PanelViewer;
use crate::project::ProjectBrowser;

/// A fresh project in the temp folder with two terrain textures from the sample project.
fn state(name: &str) -> EditorState {
    let root = std::env::temp_dir().join(format!("misty_tut_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let textures = root.join("Assets/Textures");
    std::fs::create_dir_all(&textures).unwrap();
    let sample = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../SampleProject/Assets/Textures");
    for t in ["TerrainGrass.png", "TerrainRock.png"] {
        std::fs::copy(sample.join(t), textures.join(t)).unwrap();
    }
    let mut s = EditorState::new(ProjectBrowser::open(root));
    s.refresh_assets();
    s
}

/// The editor's frame without the GPU: menu bar, toolbar, dock, then the tutorial overlay.
struct App {
    s: EditorState,
    dock: DockState<Tab>,
}

fn frame(ui: &mut egui::Ui, app: &mut App) {
    let ctx = ui.ctx().clone();
    egui::Panel::top("menu_bar").show(ui, |ui| crate::menu_bar::show(ui, &mut app.s));
    egui::Panel::top("toolbar").show(ui, |ui| crate::toolbar::show(ui, &mut app.s));
    egui::CentralPanel::default().show(ui, |ui| {
        DockArea::new(&mut app.dock).show_inside(
            ui,
            &mut PanelViewer {
                state: &mut app.s,
                gpu: None,
            },
        );
    });
    super::frame(&ctx, &mut app.s);
    for a in std::mem::take(&mut app.s.actions) {
        match a {
            Action::FocusTab(tab) => layout::focus_or_open(&mut app.dock, tab),
            Action::TogglePlay => crate::play::start(&mut app.s),
            a => {
                crate::commands::apply(&mut app.s, a);
            }
        }
    }
    crate::terrain_tools::end_frame(&mut app.s, false);
}

fn harness(s: EditorState) -> Harness<'static, App> {
    let app = App {
        s,
        dock: layout::default_layout(),
    };
    Harness::builder()
        .with_size([1600.0, 1000.0])
        .with_step_dt(0.1)
        .build_ui_state(frame, app)
}

fn step_index(h: &Harness<App>) -> Option<usize> {
    h.state().s.tutorials.run.as_ref().map(|r| r.step)
}

/// Terrain paths made absolute: other tests use the same project-relative names in the
/// process-wide terrain store. World Builder terrains are made small (fast to generate).
fn isolate_terrains(s: &mut EditorState) {
    let root = s.project.root.clone();
    let abs = |rel: &str| root.join(rel).to_string_lossy().replace('\\', "/");
    for t in s.scene.world.query_mut::<&mut Terrain>() {
        if !t.terrain_data.is_empty() && !std::path::Path::new(&t.terrain_data).is_absolute() {
            if let Some((d, _)) = store::get(&t.terrain_data) {
                store::insert(&abs(&t.terrain_data), (*d).clone());
            }
            t.terrain_data = abs(&t.terrain_data);
        }
    }
    for w in s.scene.world.query_mut::<&mut WorldBuilder>() {
        if !std::path::Path::new(&w.terrain_data).is_absolute() {
            w.terrain_data = abs(&w.terrain_data);
        }
        w.heightmap_resolution = 65;
        w.alphamap_resolution = 64;
        w.size = [120.0, 40.0, 120.0];
    }
}

// ---- data ----------------------------------------------------------------------------------

#[test]
fn tutorial_data_loads_and_is_complete() {
    let all = data::all();
    assert_eq!(all.len(), 4);
    let mut ids: Vec<&str> = all.iter().map(|t| t.id.as_str()).collect();
    ids.dedup();
    assert_eq!(ids.len(), all.len(), "unique ids");
    let s = state("data");
    let ctx = egui::Context::default();
    for t in all {
        assert!(
            (6..=12).contains(&t.steps.len()),
            "{}: {} steps",
            t.id,
            t.steps.len()
        );
        assert!(scenes::NAMES.contains(&t.scene.as_str()), "{}: scene", t.id);
        assert!(t.minutes > 0 && !t.description.is_empty());
        for (i, step) in t.steps.iter().enumerate() {
            assert!(!step.title.is_empty() && !step.text.is_empty());
            assert!(!step.targets.is_empty(), "{} step {i}: no target", t.id);
            // Short texts: one to three sentences.
            let chars: Vec<char> = step.text.chars().collect();
            let sentences = (0..chars.len())
                .filter(|&i| {
                    matches!(chars[i], '.' | '!' | '?')
                        && chars.get(i + 1).is_none_or(|c| c.is_whitespace())
                })
                .count();
            assert!(
                (1..=3).contains(&sentences),
                "{} step {i}: {sentences}",
                t.id
            );
            if let Some(cond) = &step.done_when {
                let check = conditions::Check {
                    state: &s,
                    ctx: &ctx,
                    undo_before: &[],
                };
                assert!(
                    conditions::eval(cond, &check).is_some(),
                    "{} step {i}: unknown condition {cond}",
                    t.id
                );
                assert!(
                    step.demo.is_some(),
                    "{} step {i}: condition without demo",
                    t.id
                );
            }
        }
    }
}

#[test]
fn list_is_for_the_hub() {
    let list = list();
    assert_eq!(list[0].id, "post-processing");
    assert_eq!(list[0].title, "Make it beautiful: Post-processing");
    assert!(list.iter().all(|t| t.minutes > 0));
    assert_eq!(
        list.iter().map(|t| t.id).collect::<Vec<_>>(),
        ["post-processing", "god-rays", "terrain", "world-builder"]
    );
}

// ---- running -------------------------------------------------------------------------------

#[test]
fn start_opens_a_fresh_scene_and_navigates() {
    let mut s = state("nav");
    s.scene.spawn("Leftover", None);
    s.scene_dirty = true;
    // Unsaved changes: the save prompt comes first.
    assert!(start(&mut s, "post-processing"));
    assert!(s.tutorials.run.is_none() && s.files.prompt.is_some());
    s.scene_dirty = false;
    s.files.prompt = None;
    assert!(start(&mut s, "post-processing"));
    assert!(s.scene.find_by_name("Leftover").is_none());
    assert!(s.scene.find_by_name("Effects").is_some());
    assert!(!s.scene_dirty, "the starting scene is not an edit");
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 0);
    back(&mut s);
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 0);
    next(&mut s);
    next(&mut s);
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 2);
    back(&mut s);
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 1);
    exit(&mut s);
    assert!(s.tutorials.run.is_none());
    assert!(
        !s.tutorials.progress.is_done("post-processing"),
        "exit is not done"
    );
    start(&mut s, "god-rays");
    skip(&mut s);
    assert!(s.tutorials.run.is_none());
    assert!(
        s.tutorials.progress.is_done("god-rays"),
        "skip counts as done"
    );
    assert!(!start(&mut s, "no-such-tutorial"));
}

#[test]
fn conditions_advance_on_a_change_only() {
    let mut s = state("advance");
    start(&mut s, "post-processing");
    let ctx = egui::Context::default();
    // Step 1: select Effects. Not selected yet.
    update(&ctx, &mut s, 0.0);
    assert!(s.tutorials.run.as_ref().unwrap().done_at.is_none());
    demos::run(&mut s, "select:Effects");
    update(&ctx, &mut s, 1.0);
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 0, "waits a moment");
    update(&ctx, &mut s, 1.0 + ADVANCE_DELAY);
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 1);
    // Back to step 1 while its condition already holds: no jump forward.
    back(&mut s);
    for t in 0..20 {
        update(&ctx, &mut s, 5.0 + t as f64 * 0.2);
    }
    assert_eq!(s.tutorials.run.as_ref().unwrap().step, 0);
    // The undo condition counts only steps made during the step.
    next(&mut s);
    next(&mut s);
    let undo_before = s.tutorials.run.as_ref().unwrap().undo_before.clone();
    let check = |s: &EditorState| {
        conditions::eval(
            "undo:Create Cube",
            &conditions::Check {
                state: s,
                ctx: &ctx,
                undo_before: &undo_before,
            },
        )
    };
    assert_eq!(check(&s), Some(false));
    demos::run(&mut s, "create:Cube");
    assert_eq!(check(&s), Some(true));
}

#[test]
fn progress_is_saved_per_user_file() {
    let dir = std::env::temp_dir().join(format!("misty_tut_progress_{}", std::process::id()));
    let file = dir.join("tutorials.json");
    let _ = std::fs::remove_dir_all(&dir);
    let mut p = Progress::load(Some(file.clone()));
    assert!(!p.is_done("terrain"));
    p.complete("terrain").unwrap();
    let again = Progress::load(Some(file.clone()));
    assert!(again.is_done("terrain") && !again.is_done("god-rays"));
    std::fs::write(&file, "not json").unwrap();
    assert!(
        Progress::load(Some(file)).done.is_empty(),
        "broken file = nothing done"
    );
    std::fs::remove_dir_all(dir).ok();
    // Next to hub.json, in the one per-user Misty Engine folder.
    assert_eq!(
        progress_file(),
        crate::hub::config::config_dir().map(|d| d.join("tutorials.json"))
    );
}

#[test]
fn progress_falls_back_to_the_old_file_until_the_new_one_exists() {
    let dir = std::env::temp_dir().join(format!("misty_tut_legacy_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("old")).unwrap();
    let (new, old) = (
        dir.join("MistyEngine/tutorials.json"),
        dir.join("old/tutorials.json"),
    );
    std::fs::write(&old, r#"{"done":["terrain"]}"#).unwrap();
    let mut p = Progress::load_or(Some(new.clone()), Some(old.clone()));
    assert!(p.is_done("terrain"), "old progress is kept");
    // Finishing one writes the new file with both; the old file is left alone.
    p.complete("god-rays").unwrap();
    let again = Progress::load_or(Some(new.clone()), Some(old.clone()));
    assert!(again.is_done("terrain") && again.is_done("god-rays"));
    std::fs::write(&old, r#"{"done":["world-builder"]}"#).unwrap();
    let again = Progress::load_or(Some(new), Some(old));
    assert!(
        !again.is_done("world-builder"),
        "the new file wins once it exists"
    );
    std::fs::remove_dir_all(dir).ok();
}

// ---- end to end in the UI --------------------------------------------------------------------

/// Targets that only exist while a menu is open (checked in `menu_targets_exist`).
fn in_menu(id: &str) -> bool {
    id.starts_with("menu.") && id.contains('/')
}

/// Run tutorial `id` from the Tutorials window to the end: every step's targets must be drawn
/// (before or after the step is done), its demo must meet its condition and the step must then
/// advance by itself; steps without a condition are left with the Next button.
fn run_end_to_end(name: &str, id: &str) {
    let mut h = harness(state(name));
    h.run_steps(2);
    // Help ▸ Tutorials ▸ Start.
    h.state_mut().s.tutorials.window_open = true;
    h.run_steps(2);
    let t = data::find(id).unwrap();
    let index = data::all().iter().position(|x| x.id == id).unwrap();
    h.get_all_by_label("Start").nth(index).unwrap().click();
    h.run_steps(3);
    assert_eq!(step_index(&h), Some(0), "{id} started");
    assert!(!h.state().s.tutorials.window_open);
    for (i, step) in t.steps.iter().enumerate() {
        assert_eq!(step_index(&h), Some(i), "{id}: at step {i}");
        h.run_steps(3);
        let mut seen = targets::recorded(&h.ctx);
        if let Some(demo) = &step.demo {
            assert!(
                demos::run(&mut h.state_mut().s, demo),
                "{id} {i}: demo {demo}"
            );
            isolate_terrains(&mut h.state_mut().s);
        }
        // Wait for the condition (World Builder generates on a worker thread).
        let started = std::time::Instant::now();
        while step_index(&h) == Some(i) && step.done_when.is_some() {
            h.run_steps(2);
            seen.extend(targets::recorded(&h.ctx));
            assert!(
                started.elapsed().as_secs() < 120,
                "{id} step {i} ({}) did not advance",
                step.title
            );
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        for target in step.targets.iter().filter(|t| !in_menu(t)) {
            // Inside a section the user opens first: click its header, like the user would.
            if !seen.contains(target)
                && let Some((parent, _)) = target.rsplit_once('/')
                && seen.iter().any(|t| t == parent)
            {
                let header = parent.rsplit('/').next().unwrap();
                h.get_by_label(header).click();
                h.run_steps(3);
                seen.extend(targets::recorded(&h.ctx));
            }
            assert!(
                seen.contains(target),
                "{id} step {i}: target {target} never drawn"
            );
        }
        if step.done_when.is_none() {
            let label = if i + 1 == t.steps.len() {
                "Finish"
            } else {
                "Next ▶"
            };
            h.get_by_label(label).click();
            h.run_steps(2);
        }
    }
    assert!(h.state().s.tutorials.run.is_none(), "{id} finished");
    assert!(h.state().s.tutorials.progress.is_done(id));
    if h.state().s.play.in_play_mode() {
        crate::play::stop(&mut h.state_mut().s);
    }
}

#[test]
fn post_processing_runs_end_to_end() {
    run_end_to_end("e2e_post", "post-processing");
}

#[test]
fn god_rays_runs_end_to_end() {
    run_end_to_end("e2e_rays", "god-rays");
}

#[test]
fn terrain_runs_end_to_end() {
    run_end_to_end("e2e_terrain", "terrain");
}

#[test]
fn world_builder_runs_end_to_end() {
    run_end_to_end("e2e_world", "world-builder");
}

/// The first steps of the post-processing tutorial done with real clicks: clicks reach the
/// panels under the dim, and the steps advance on their own.
#[test]
fn clicking_the_real_ui_advances_steps() {
    let mut s = state("clicks");
    start(&mut s, "post-processing");
    let mut h = harness(s);
    h.run_steps(3);
    h.get_by_label("Effects").click();
    h.run_steps(12);
    assert_eq!(step_index(&h), Some(1), "selecting Effects finished step 1");
    h.get_by_label("Add Archetype").click();
    h.run_steps(2);
    h.event(egui::Event::Text("Post".into()));
    h.run_steps(2);
    h.get_by_label("Post-process Volume").click();
    h.run_steps(12);
    assert_eq!(step_index(&h), Some(2), "adding the volume finished step 2");
    // Back, then Next on the card.
    h.get_by_label("◀ Back").click();
    h.run_steps(2);
    assert_eq!(step_index(&h), Some(1));
    h.get_by_label("Next ▶").click();
    h.run_steps(2);
    assert_eq!(step_index(&h), Some(2));
    h.get_by_label("Exit").click();
    h.run_steps(2);
    assert!(h.state().s.tutorials.run.is_none());
}

/// Menu targets: open each menu path by clicking it and check every entry is drawn.
#[test]
fn menu_targets_exist() {
    let mut s = state("menus");
    start(&mut s, "terrain");
    let mut h = harness(s);
    h.run_steps(2);
    let mut paths: Vec<&str> = data::all()
        .iter()
        .flat_map(|t| &t.steps)
        .flat_map(|s| &s.targets)
        .map(String::as_str)
        .filter(|t| in_menu(t))
        .collect();
    paths.sort();
    paths.dedup();
    for id in paths {
        let path = id.trim_start_matches("menu.");
        let mut parts = path.split('/');
        let top = parts.next().unwrap();
        h.key_press(egui::Key::Escape);
        h.run_steps(2);
        h.get_by_label(top).click();
        h.run_steps(2);
        for sub in parts
            .clone()
            .take(path.split('/').count().saturating_sub(2))
        {
            h.get_by_label(&format!("{sub} ⏵")).hover();
            h.run_steps(3);
        }
        assert!(
            targets::recorded(&h.ctx).iter().any(|r| r == id),
            "menu target {id} not drawn"
        );
        // Close the menu again.
        h.get_by_label(top).click();
        h.run_steps(2);
    }
}

/// `World Builder` panel targets come from the window's scope.
#[test]
fn world_builder_targets_are_scoped() {
    let mut s = state("wb_scope");
    start(&mut s, "world-builder");
    s.push(Action::FocusTab(Tab::WorldBuilder));
    let mut h = harness(s);
    h.run_steps(3);
    let seen = targets::recorded(&h.ctx);
    assert!(seen.iter().any(|t| t == "World Builder/Create"), "{seen:?}");
    assert!(seen.iter().any(|t| t == "panel.World Builder"));
    assert!(world_gen::builders(&h.state().s.scene).is_empty());
}
