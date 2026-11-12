//! Project window (and Hierarchy) tests in the whole editor layout: menu bar, toolbar and every
//! docked panel, so clicks, focus and drops meet the other panels as in the app.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{Event, PointerButton, Pos2, pos2};
use egui_dock::{DockArea, DockState};
use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use crate::actions::Action;
use crate::layout::{self, Tab};
use crate::panels::PanelViewer;
use crate::project::ProjectBrowser;
use crate::state::EditorState;

/// A small project in the temp folder: a material, a text file and an empty folder.
fn temp_project(tag: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty project ui {tag} {}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    let assets = root.join("Assets");
    std::fs::create_dir_all(assets.join("Prefabs")).unwrap();
    std::fs::create_dir_all(assets.join("Empty")).unwrap();
    std::fs::create_dir_all(assets.join("Materials")).unwrap();
    std::fs::write(assets.join("Materials/Red.mat"), "{}").unwrap();
    std::fs::write(assets.join("Notes.txt"), "notes").unwrap();
    root
}

struct App {
    s: EditorState,
    dock: DockState<Tab>,
}

/// The editor's frame without the GPU: menu bar, toolbar, the dock, the OS-drop fallback, the
/// disk poll, then queued actions.
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
    crate::os_drop::fallback(&ctx, &mut app.s);
    if app.s.project.poll_disk() {
        app.s.refresh_assets();
    }
    for a in std::mem::take(&mut app.s.actions) {
        match a {
            Action::FocusTab(tab) => layout::focus_or_open(&mut app.dock, tab),
            Action::ShowInExplorer(_) => {}
            a => {
                crate::commands::apply(&mut app.s, a);
            }
        }
    }
}

fn harness_with(s: EditorState) -> Harness<'static, App> {
    let app = App {
        s,
        dock: layout::default_layout(),
    };
    let mut h = Harness::builder()
        .with_size([1600.0, 1000.0])
        .with_step_dt(0.1)
        .build_ui_state(frame, app);
    h.run_steps(2);
    h
}

fn harness(root: &Path) -> Harness<'static, App> {
    harness_with(EditorState::new(ProjectBrowser::open(root.to_path_buf())))
}

/// The id of the widget with keyboard focus.
fn focused(h: &Harness<App>) -> Option<egui::Id> {
    h.ctx.memory(|m| m.focused())
}

fn rename_id(path: &Path) -> egui::Id {
    egui::Id::new(("project_rename", path))
}

fn click_at(h: &mut Harness<App>, pos: Pos2, button: PointerButton) {
    h.hover_at(pos);
    for pressed in [true, false] {
        h.event(Event::PointerButton {
            pos,
            button,
            pressed,
            modifiers: Default::default(),
        });
    }
    h.run();
}

/// Right-click at `pos`, then Create ▸ Folder.
fn create_folder_from_menu_at(h: &mut Harness<App>, pos: Pos2) {
    click_at(h, pos, PointerButton::Secondary);
    h.get_by_label("Create ⏵").hover();
    h.run();
    h.get_by_label("Folder").click();
    h.run_steps(2);
}

/// The centre of the widget labelled `name` furthest left (`left`: the folder tree's row) or
/// right (the folder's tile).
fn labelled_at(h: &Harness<App>, name: &str, left: bool) -> Pos2 {
    let rects: Vec<egui::Rect> = h.get_all_by_label(name).map(|n| n.rect()).collect();
    let pick = if left {
        rects.iter().min_by(|a, b| a.left().total_cmp(&b.left()))
    } else {
        rects.iter().max_by(|a, b| a.left().total_cmp(&b.left()))
    };
    pick.unwrap_or_else(|| panic!("no {name}")).center()
}

/// A point of empty space at the bottom right of the Project window's content area.
fn empty_content_point(h: &Harness<App>) -> Pos2 {
    let r = h.state().s.project.drop_rect;
    r.right_bottom() - egui::vec2(30.0, 20.0)
}

#[test]
fn right_click_on_empty_space_opens_the_folder_menu_everywhere() {
    let root = temp_project("empty_menu");
    let assets = root.join("Assets");
    let mut h = harness(&root);
    // Grid view, beside and below the tiles of Assets.
    let at = empty_content_point(&h);
    create_folder_from_menu_at(&mut h, at);
    assert!(assets.join("New Folder").is_dir(), "grid: empty space menu");

    // An empty folder, grid view: right in the middle of it.
    h.state_mut().s.project.open_folder(assets.join("Empty"));
    h.run_steps(2);
    let mid = h.state().s.project.drop_rect.center();
    create_folder_from_menu_at(&mut h, mid);
    assert!(
        assets.join("Empty/New Folder").is_dir(),
        "empty folder menu"
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // List view, next to the rows.
    h.state_mut().s.project.icon_size = 0.0;
    h.state_mut().s.project.open_folder(assets.clone());
    h.run_steps(2);
    let at = empty_content_point(&h);
    create_folder_from_menu_at(&mut h, at);
    assert!(
        assets.join("New Folder 1").is_dir(),
        "list: empty space menu"
    );
    h.key_press(egui::Key::Escape);
    h.run();

    // The folder tree's empty space, under the folders: the open folder's menu.
    let tree_point = {
        let r = h.state().s.project.drop_rect;
        pos2(r.left() - 60.0, r.bottom() - 20.0)
    };
    create_folder_from_menu_at(&mut h, tree_point);
    assert!(
        assets.join("New Folder 2").is_dir(),
        "tree: empty space menu"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_new_folder_starts_renaming_with_its_name_selected_and_keeps_the_field() {
    let root = temp_project("create_rename");
    let assets = root.join("Assets");
    let mut h = harness(&root);
    let at = empty_content_point(&h);
    create_folder_from_menu_at(&mut h, at);
    let new = assets.join("New Folder");
    assert!(new.is_dir());
    // Several frames later (2 s): still renaming, selected, focused, whole name selected.
    for _ in 0..20 {
        h.run_steps(1);
    }
    let p = &h.state().s.project;
    assert_eq!(
        p.renaming.as_ref().map(|(p, _)| p.clone()),
        Some(new.clone())
    );
    assert_eq!(p.selected.as_deref(), Some(new.as_path()));
    assert_eq!(focused(&h), Some(rename_id(&new)));
    let state = egui::TextEdit::load_state(&h.ctx, rename_id(&new)).unwrap();
    let range = state.cursor.char_range().unwrap();
    let (a, b) = (range.primary.index.0, range.secondary.index.0);
    assert_eq!(
        (a.min(b), a.max(b)),
        (0, "New Folder".len()),
        "whole name selected"
    );
    // Typing replaces it.
    h.event(Event::Text("Props".into()));
    h.run_steps(2);
    assert_eq!(
        h.state()
            .s
            .project
            .renaming
            .as_ref()
            .map(|(_, t)| t.as_str()),
        Some("Props")
    );
    h.key_press(egui::Key::Enter);
    h.run_steps(3);
    let log: Vec<String> = h
        .state()
        .s
        .console
        .entries()
        .iter()
        .map(|e| e.message.clone())
        .collect();
    assert!(assets.join("Props").is_dir() && !new.exists(), "{log:?}");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn create_in_a_tree_folder_makes_the_asset_there() {
    let root = temp_project("tree_create");
    let assets = root.join("Assets");
    let mut h = harness(&root);
    // Right-click the Prefabs folder in the tree (not the open folder) ▸ Create ▸ Folder.
    let at = labelled_at(&h, "Prefabs", true);
    create_folder_from_menu_at(&mut h, at);
    assert!(assets.join("Prefabs/New Folder").is_dir());
    assert_eq!(h.state().s.project.current, assets.join("Prefabs"));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn context_menu_rename_keeps_its_field_and_selection() {
    let root = temp_project("rename_keep");
    let notes = root.join("Assets/Notes.txt");
    let mut h = harness(&root);
    h.get_by_label("Notes.txt").click_secondary();
    h.run();
    h.get_by_label("Rename").click();
    h.run_steps(2);
    // Two seconds of frames with the pointer resting where the menu was: the field stays,
    // focused, and the asset stays selected.
    for _ in 0..20 {
        h.run_steps(1);
        let p = &h.state().s.project;
        assert!(p.renaming.is_some(), "rename field closed");
        assert_eq!(p.selected.as_deref(), Some(notes.as_path()));
        assert_eq!(focused(&h), Some(rename_id(&notes)), "lost the keyboard");
    }
    // A refresh (files appearing) and the focus dropped for a frame keep the rename going.
    h.state_mut().s.refresh_assets();
    h.ctx.memory_mut(|m| m.surrender_focus(rename_id(&notes)));
    h.run_steps(3);
    assert!(h.state().s.project.renaming.is_some());
    assert_eq!(focused(&h), Some(rename_id(&notes)));
    // Clicking elsewhere still ends it (no change: nothing renamed).
    let at = empty_content_point(&h);
    click_at(&mut h, at, PointerButton::Primary);
    assert!(h.state().s.project.renaming.is_none());
    assert!(notes.is_file());
    let _ = std::fs::remove_dir_all(&root);
}

/// A file dropped from the OS (egui's `DroppedFile` for a path).
#[derive(Debug)]
struct OsFile(PathBuf);

impl egui::DroppedFile for OsFile {
    fn path(&self) -> &Path {
        &self.0
    }

    fn bytes(&self) -> Result<Vec<u8>, String> {
        std::fs::read(&self.0).map_err(|e| e.to_string())
    }
}

fn drop_files(h: &mut Harness<App>, at: Pos2, paths: &[PathBuf]) {
    h.hover_at(at);
    h.run();
    h.input_mut().hovered_files = paths
        .iter()
        .map(|p| egui::HoveredFile {
            path: Some(p.clone()),
            ..Default::default()
        })
        .collect();
    h.run_steps(2);
    h.input_mut().hovered_files.clear();
    for p in paths {
        h.input_mut()
            .dropped_files
            .push(Arc::new(OsFile(p.clone())));
    }
    h.run_steps(3);
}

#[test]
fn files_dropped_from_the_os_are_copied_imported_and_shown() {
    let root = temp_project("os_drop");
    let assets = root.join("Assets");
    // A folder on the "desktop" with a material in a sub-folder, and a loose file.
    let desktop = root.join("Desktop");
    std::fs::create_dir_all(desktop.join("Kit/Mats")).unwrap();
    std::fs::write(desktop.join("Kit/Mats/Blue.mat"), "{}").unwrap();
    std::fs::write(desktop.join("Kit/info.txt"), "x").unwrap();
    std::fs::write(desktop.join("Green.mat"), "{}").unwrap();
    let mut h = harness(&root);

    // On empty space: into the open folder (Assets), recursively, imported and listed.
    let at = empty_content_point(&h);
    drop_files(&mut h, at, &[desktop.join("Kit")]);
    assert!(assets.join("Kit/Mats/Blue.mat").is_file());
    assert!(assets.join("Kit/info.txt").is_file());
    let s = &h.state().s;
    assert!(
        s.assets.materials.contains_key("Assets/Kit/Mats/Blue.mat"),
        "imported"
    );
    assert!(s.project.entries.iter().any(|e| e.name == "Kit"), "listed");
    assert_eq!(
        s.project.selected.as_deref(),
        Some(assets.join("Kit").as_path())
    );
    labelled_at(&h, "Kit", false);

    // On a folder tile: into that folder.
    let materials = labelled_at(&h, "Materials", false);
    drop_files(&mut h, materials, &[desktop.join("Green.mat")]);
    assert!(assets.join("Materials/Green.mat").is_file());
    assert!(
        h.state()
            .s
            .assets
            .materials
            .contains_key("Assets/Materials/Green.mat")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn files_added_on_disk_show_up_without_leaving_the_folder() {
    let root = temp_project("external_add");
    let materials = root.join("Assets/Materials");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.project.open_folder(materials.clone());
    let mut h = harness_with(s);
    h.get_by_label("Red.mat");
    // Another program adds a material and a folder.
    std::fs::write(materials.join("Blue.mat"), "{}").unwrap();
    std::fs::create_dir(materials.join("Metals")).unwrap();
    std::thread::sleep(crate::project::DISK_POLL + std::time::Duration::from_millis(50));
    h.run_steps(2);
    h.get_by_label("Blue.mat");
    labelled_at(&h, "Metals", false);
    let s = &h.state().s;
    assert!(
        s.assets.materials.contains_key("Assets/Materials/Blue.mat"),
        "imported"
    );
    assert!(
        s.project
            .tree
            .children
            .iter()
            .any(|c| c.name == "Materials" && c.children.iter().any(|g| g.name == "Metals")),
        "the folder tree has the new folder"
    );
    // Removed outside the editor: gone too.
    std::fs::remove_file(materials.join("Blue.mat")).unwrap();
    std::thread::sleep(crate::project::DISK_POLL + std::time::Duration::from_millis(50));
    h.run_steps(2);
    assert!(h.query_by_label("Blue.mat").is_none());
    let _ = std::fs::remove_dir_all(&root);
}

fn press_at(h: &mut Harness<App>, pos: Pos2, pressed: bool) {
    h.event(Event::PointerButton {
        pos,
        button: PointerButton::Primary,
        pressed,
        modifiers: Default::default(),
    });
}

#[test]
fn press_and_hold_renames_a_project_item_after_the_hold_time() {
    let root = temp_project("hold");
    let notes = root.join("Assets/Notes.txt");
    let mut h = harness(&root);
    let at = h.get_by_label("Notes.txt").rect().center();
    h.hover_at(at);
    press_at(&mut h, at, true);
    h.run_steps(1);
    // 1.2 s held (steps of 0.1 s): not yet.
    h.run_steps(12);
    assert!(h.state().s.project.renaming.is_none(), "too early");
    h.run_steps(5);
    assert_eq!(
        h.state()
            .s
            .project
            .renaming
            .as_ref()
            .map(|(p, _)| p.clone()),
        Some(notes.clone())
    );
    // Releasing the button (on the icon, beside the name field) keeps the rename.
    press_at(&mut h, at, false);
    h.run_steps(4);
    assert!(
        h.state().s.project.renaming.is_some(),
        "the release ended it"
    );
    assert_eq!(focused(&h), Some(rename_id(&notes)));
    h.key_press(egui::Key::Escape);
    h.run();

    // Moving while holding (a drag) cancels it.
    let red_dir = labelled_at(&h, "Materials", false);
    h.hover_at(red_dir);
    press_at(&mut h, red_dir, true);
    h.run_steps(2);
    h.hover_at(red_dir + egui::vec2(40.0, 0.0));
    h.run_steps(20);
    press_at(&mut h, red_dir + egui::vec2(40.0, 0.0), false);
    h.run_steps(2);
    assert!(h.state().s.project.renaming.is_none(), "a drag renamed");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn press_and_hold_renames_a_hierarchy_object() {
    let root = temp_project("hold_hier");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    let cube = s.scene.spawn("Crate", None);
    let mut h = harness_with(s);
    let at = h.get_by_label("Crate").rect().center();
    h.hover_at(at);
    press_at(&mut h, at, true);
    h.run_steps(17);
    assert_eq!(
        h.state().s.hierarchy.renaming.as_ref().map(|(e, _)| *e),
        Some(cube)
    );
    press_at(&mut h, at, false);
    h.run_steps(3);
    assert!(
        h.state().s.hierarchy.renaming.is_some(),
        "the release ended it"
    );
    // Typing replaces the selected name.
    h.event(Event::Text("Barrel".into()));
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert_eq!(h.state().s.scene.name(cube), "Barrel");
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn right_click_on_empty_hierarchy_space_opens_the_create_menu() {
    let root = temp_project("hier_menu");
    let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
    s.scene.spawn("Crate", None);
    let mut h = harness_with(s);
    let row = h.get_by_label("Crate").rect();
    // Far right of the row's line is the row; well below it is empty space.
    let at = pos2(row.center().x, row.bottom() + 200.0);
    click_at(&mut h, at, PointerButton::Secondary);
    assert!(
        h.query_by_label("Create Folder").is_some(),
        "no Create menu on empty Scene Tree space"
    );
    let _ = std::fs::remove_dir_all(&root);
}
