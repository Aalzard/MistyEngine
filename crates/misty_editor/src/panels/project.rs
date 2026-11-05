//! Project window: folder tree on the left, folder contents on the right.
//! Right-click for Create / Rename / Duplicate / Delete / Show in Explorer; drag assets onto a
//! folder to move them; F2, Ctrl+D and Delete act on the selected asset while the window has focus.

use std::path::PathBuf;

use egui::{Align2, Color32, FontId, Rect, Sense, Ui};

use super::AssetPayload;
use super::project_menus::{self, Requests};
use crate::actions::Action;
use crate::project::{AssetEntry, ProjectBrowser};
use crate::state::EditorState;
use crate::theme;
use crate::widgets::{panel_toolbar, search_field};

use super::hierarchy::row_with_icon;
use crate::branding::icons::{self, Icon};

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    // Keyboard focus follows the last click.
    let panel = ui.max_rect();
    if let Some(pos) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        state.project.focused = panel.contains(pos);
    }
    let selected_before = state.project.selected.clone();
    let mut opened_file: Option<PathBuf> = None;
    let mut req = Requests::default();
    let browser = &mut state.project;
    // One toolbar row: Create on the left, search, type filter and refresh on the right.
    panel_toolbar(ui, |ui| {
        ui.menu_button("➕ ⏷", |ui| {
            project_menus::create_items(ui, &mut req.actions)
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui.button("⟳").on_hover_text("Refresh (Ctrl+R)").clicked() {
                req.actions.push(Action::Refresh);
            }
            type_filter(ui, &mut browser.search);
            search_field(ui, &mut browser.search, 180.0)
                .on_hover_text("Name words and type filters: t:Material t:Script t:Scene t:Prefab t:Texture t:Model t:AudioClip");
        });
    });

    // Bottom bar: selected path and icon-size slider.
    egui::Panel::bottom("project_bottom_bar")
        .exact_size(22.0)
        .show_separator_line(false)
        .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::TOOLBAR_BG))
        .show(ui, |ui| {
            ui.horizontal_centered(|ui| {
                if let Some(sel) = &browser.selected {
                    ui.label(browser.display_path(sel));
                }
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.add(egui::Slider::new(&mut browser.icon_size, 0.0..=1.0).show_value(false));
                });
            });
        });

    egui::Panel::left("project_tree")
        .default_size(180.0)
        .show_separator_line(false)
        .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::SIDEBAR_BG))
        .resizable(true)
        .show(ui, |ui| {
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut open = None;
                    super::project_tree::show(ui, browser, &mut open, &mut req);
                    if let Some(path) = open {
                        browser.open_folder(path);
                    }
                });
        });

    let dragging_objects = ui
        .data(|d| d.get_temp::<Vec<misty_engine::hecs::Entity>>(super::hierarchy::drag_id()))
        .is_some();
    egui::CentralPanel::default().show(ui, |ui| {
        browser.drop_rect = ui.max_rect();
        // Hierarchy rows dragged here become prefabs in this folder.
        if dragging_objects && ui.rect_contains_pointer(ui.max_rect()) {
            ui.painter().rect_stroke(
                ui.max_rect().shrink(1.0),
                2.0,
                egui::Stroke::new(2.0, Color32::from_rgb(90, 150, 230)),
                egui::StrokeKind::Inside,
            );
        }
        // Breadcrumb.
        let mut jump = None;
        ui.horizontal(|ui| {
            if browser.search.is_empty() {
                for (i, (name, path)) in browser.breadcrumb().into_iter().enumerate() {
                    if i > 0 {
                        ui.label("›");
                    }
                    let crumb = ui.add(egui::Button::new(name).frame(false));
                    if crumb.clicked() {
                        jump = Some(path.clone());
                    }
                    drop_target(&crumb, &browser.root, &path, &mut req);
                }
            } else {
                ui.label(format!("Search: '{}'", browser.search));
            }
        });
        ui.separator();
        if let Some(path) = jump {
            browser.open_folder(path);
        }

        let entries = if browser.search.is_empty() {
            browser.entries.clone()
        } else {
            browser.search_results()
        };
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if browser.icon_size < 0.05 {
                    list_view(ui, &entries, browser, &mut open, &mut req);
                } else {
                    grid_view(ui, &entries, browser, &mut open, &mut req);
                }
                // Empty space: deselect, folder menu.
                let rest = ui.allocate_response(
                    ui.available_size().max(egui::vec2(0.0, 40.0)),
                    Sense::click(),
                );
                if rest.clicked() {
                    browser.selected = None;
                }
                let current = browser.current.clone();
                rest.context_menu(|ui| project_menus::folder_menu(ui, &current, &mut req));
            });
        if let Some(path) = open {
            if path.is_dir() {
                browser.search.clear();
                browser.open_folder(path);
            } else {
                opened_file = Some(path);
            }
        }
    });
    open_file(state, opened_file);
    apply_requests(ui, state, req);

    let rel = state
        .project
        .confirm_delete
        .as_ref()
        .map(|p| state.assets.relative(p))
        .unwrap_or_default();
    if let Some(action) = project_menus::delete_dialog(ui.ctx(), &mut state.project, &rel) {
        state.push(action);
    }

    // Selecting an asset shows it in the Inspector (and deselects scene objects).
    if state.project.selected != selected_before
        && let Some(sel) = state.project.selected.clone()
    {
        state.selection.clear();
        state.inspector_asset = Some(state.assets.relative(&sel));
    }
}

/// Double-clicked file: scenes open in the editor, scripts in the Code Editor, other files in
/// their default app.
pub(crate) fn open_file(state: &mut EditorState, opened: Option<PathBuf>) {
    let Some(path) = opened else { return };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == misty_engine::scene::prefab::EXTENSION {
        state
            .console
            .info("Prefab Mode is not available yet: edit an instance and use Apply All.");
    } else if ext == misty_engine::scene::file::EXTENSION {
        state.push(Action::OpenScenePath(state.assets.relative(&path)));
    } else if misty_engine::animation::is_controller(&path.to_string_lossy()) {
        state.animator.open(Some(state.assets.relative(&path)));
        state.push(Action::FocusTab(crate::layout::Tab::Animator));
    } else if misty_engine::timeline::is_timeline(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::timeline::open_asset(state, &rel);
    } else if misty_engine::audio::is_mixer(&path.to_string_lossy()) {
        state.audio_mixer.open(Some(state.assets.relative(&path)));
        state.push(Action::FocusTab(crate::layout::Tab::AudioMixer));
    } else if misty_engine::vfx::is_effect_path(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::effect_graph::open(state, &rel);
    } else if misty_engine::shader_graph::is_graph_path(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::shader_graph::open(state, &rel);
    } else if ext == "lua" || ext == misty_engine::shader_asset::EXTENSION {
        let rel = state.assets.relative(&path);
        crate::panels::code_editor::open_script(state, &rel, None);
    } else if let Err(err) = crate::assets::open_external(&path) {
        state
            .console
            .warn(format!("Could not open {}: {err}", path.display()));
    }
}

fn apply_requests(ui: &Ui, state: &mut EditorState, req: Requests) {
    for a in req.actions {
        state.push(a);
    }
    if let Some(p) = req.rename {
        state.project.selected = Some(p.clone());
        state.project.start_rename(p);
    }
    if let Some(p) = req.delete {
        state.project.selected = Some(p.clone());
        state.project.confirm_delete = Some(p);
    }
    if let Some(text) = req.copy_path {
        ui.ctx().copy_text(text);
    }
}

/// Project "+" menu and Assets ▸ Create.
pub fn create_menu(ui: &mut Ui, state: &mut EditorState) {
    let mut actions = Vec::new();
    project_menus::create_items(ui, &mut actions);
    for a in actions {
        state.push(a);
    }
}

/// Accept assets dragged onto a folder (row, tile or breadcrumb): move them there.
pub(super) fn drop_target(
    resp: &egui::Response,
    root: &std::path::Path,
    folder: &std::path::Path,
    req: &mut Requests,
) {
    let Some(payload) = resp.dnd_hover_payload::<AssetPayload>() else {
        return;
    };
    let from = root.join(&payload.0);
    // Not onto itself, its own folder or one of its children.
    if folder.starts_with(&from) || from.parent() == Some(folder) {
        return;
    }
    resp.ctx.layer_painter(resp.layer_id).rect_stroke(
        resp.rect,
        2.0,
        egui::Stroke::new(2.0, Color32::from_rgb(90, 150, 230)),
        egui::StrokeKind::Inside,
    );
    if resp.dnd_release_payload::<AssetPayload>().is_some() {
        req.actions.push(Action::MoveAsset {
            path: from,
            folder: folder.to_path_buf(),
        });
    }
}

fn list_view(
    ui: &mut Ui,
    entries: &[AssetEntry],
    browser: &mut ProjectBrowser,
    open: &mut Option<PathBuf>,
    req: &mut Requests,
) {
    for e in entries {
        let is_sel = browser.selected.as_ref() == Some(&e.path);
        let renaming = is_renaming(browser, e);
        let folder = e.is_dir.then_some(Icon::Folder);
        let label = match (renaming, folder) {
            (true, Some(_)) => String::new(),
            (true, None) => e.icon().to_owned(),
            (false, Some(_)) => e.name.clone(),
            (false, None) => format!("{} {}", e.icon(), e.name),
        };
        let resp = row_with_icon(ui, is_sel, folder, &label, theme::TEXT, 0);
        if renaming {
            let r = resp.rect.with_min_x(resp.rect.left() + 26.0);
            project_menus::rename_field(ui, r, browser, req);
        }
        item_interaction(&resp, e, browser, open, req);
    }
}

fn is_renaming(browser: &ProjectBrowser, e: &AssetEntry) -> bool {
    browser.renaming.as_ref().is_some_and(|(p, _)| *p == e.path)
}

fn grid_view(
    ui: &mut Ui,
    entries: &[AssetEntry],
    browser: &mut ProjectBrowser,
    open: &mut Option<PathBuf>,
    req: &mut Requests,
) {
    let cell = 48.0 + browser.icon_size * 80.0;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = egui::vec2(8.0, 8.0);
        for e in entries {
            let (rect, resp) =
                ui.allocate_exact_size(egui::vec2(cell, cell + 18.0), Sense::click_and_drag());
            let label = if is_renaming(browser, e) { "" } else { &e.name };
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, false, label)
            });
            let is_sel = browser.selected.as_ref() == Some(&e.path);
            if is_sel {
                ui.painter().rect_filled(rect, 3.0, theme::SELECTION);
            } else if resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 3.0, Color32::from_rgb(69, 69, 69));
            }
            tile_icon(ui, e, rect, cell, &mut browser.thumbnails);
            if is_renaming(browser, e) {
                let r = Rect::from_center_size(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    egui::vec2(cell + 6.0, 18.0),
                );
                project_menus::rename_field(ui, r, browser, req);
            } else {
                ui.painter().text(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    Align2::CENTER_CENTER,
                    truncate(&e.name, (cell / 7.0) as usize),
                    FontId::proportional(12.0),
                    theme::TEXT,
                );
            }
            item_interaction(&resp, e, browser, open, req);
        }
    });
}

/// Texture or material preview, or the asset's icon.
fn tile_icon(
    ui: &Ui,
    e: &AssetEntry,
    rect: Rect,
    cell: f32,
    thumbnails: &mut crate::thumbnails::Thumbnails,
) {
    let thumb = if e.path.extension().is_some_and(|x| x == "mat") {
        thumbnails
            .materials
            .thumbnail(&e.path)
            .map(|id| (id, egui::vec2(1.0, 1.0)))
    } else {
        e.is_texture()
            .then(|| thumbnails.get(ui.ctx(), &e.path))
            .flatten()
            .map(|t| (t.id(), t.size_vec2()))
    };
    if let Some((id, size)) = thumb {
        let box_rect = Rect::from_center_size(
            rect.center_top() + egui::vec2(0.0, cell * 0.5),
            egui::vec2(cell * 0.8, cell * 0.8),
        );
        let fit = box_rect.size() / size.max_elem();
        let img_rect = Rect::from_center_size(box_rect.center(), size * fit.x);
        ui.painter().image(
            id,
            img_rect,
            Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    } else if e.is_dir {
        let center = rect.center_top() + egui::vec2(0.0, cell * 0.5);
        let r = Rect::from_center_size(center, egui::vec2(cell * 0.7, cell * 0.7));
        icons::paint(ui, r, Icon::Folder, ui.visuals().text_color());
    } else {
        ui.painter().text(
            rect.center_top() + egui::vec2(0.0, cell * 0.5),
            Align2::CENTER_CENTER,
            e.icon(),
            FontId::proportional(cell * 0.6),
            e.icon_color(),
        );
    }
}

/// Asset types offered by the filter button (the `t:` search filters).
const TYPE_FILTERS: [&str; 7] = [
    "Material",
    "Script",
    "Scene",
    "Prefab",
    "Texture",
    "Model",
    "AudioClip",
];

/// The filter button: a "Search by Type" menu that toggles `t:` filters in the search text.
fn type_filter(ui: &mut Ui, search: &mut String) {
    let active = search.split_whitespace().any(|w| w.starts_with("t:"));
    let button = icons::button(
        ui,
        Icon::Filter,
        active,
        egui::vec2(22.0, 18.0),
        "Search by Type",
    );
    egui::Popup::menu(&button).show(|ui| {
        for kind in TYPE_FILTERS {
            let token = format!("t:{kind}");
            let mut on = search
                .split_whitespace()
                .any(|w| w.eq_ignore_ascii_case(&token));
            if ui.checkbox(&mut on, kind).changed() {
                *search = toggle_word(search, &token, on);
            }
        }
    });
}

/// `text` with `word` added (at the end) or removed.
fn toggle_word(text: &str, word: &str, on: bool) -> String {
    let mut words: Vec<&str> = text
        .split_whitespace()
        .filter(|w| !w.eq_ignore_ascii_case(word))
        .collect();
    if on {
        words.push(word);
    }
    words.join(" ")
}

/// Click selects, double-click opens, right-click shows the menu; drag carries the asset,
/// and folders accept dropped assets.
fn item_interaction(
    resp: &egui::Response,
    e: &AssetEntry,
    browser: &mut ProjectBrowser,
    open: &mut Option<PathBuf>,
    req: &mut Requests,
) {
    if resp.clicked() || resp.secondary_clicked() {
        browser.selected = Some(e.path.clone());
    }
    if resp.double_clicked() {
        *open = Some(e.path.clone());
    }
    let rel = asset_relative(&e.path);
    let read_only = browser.is_read_only(&e.path);
    if !read_only {
        resp.context_menu(|ui| project_menus::item_menu(ui, &e.path, rel.clone(), req));
    }
    // Assets can be dragged onto objects (Scene view, Hierarchy, Inspector slots) or folders.
    if let Some(rel) = rel {
        resp.dnd_set_drag_payload(AssetPayload(rel));
    }
    if e.is_dir && !read_only {
        drop_target(resp, &browser.root, &e.path, req);
    }
}

/// "Assets/…" for a path inside the project's Assets folder.
pub(super) fn asset_relative(path: &std::path::Path) -> Option<String> {
    let s = path.to_string_lossy().replace('\\', "/");
    let assets = s.find("Assets/");
    let packages = s.find("/Packages/").map(|i| i + 1);
    match (assets, packages) {
        (Some(a), Some(p)) => Some(s[a.min(p)..].to_owned()),
        (Some(i), None) | (None, Some(i)) => Some(s[i..].to_owned()),
        (None, None) => None,
    }
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let t: String = s.chars().take(max.saturating_sub(1)).collect();
        format!("{t}…")
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::MeshRenderer;

    use super::*;

    /// A temp copy of the sample project (Assets + ProjectSettings), opened in the editor.
    fn sample_copy(tag: &str) -> (EditorState, PathBuf) {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let root =
            std::env::temp_dir().join(format!("misty_project_ops_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        for dir in ["Assets", "ProjectSettings"] {
            crate::asset_ops::copy_all(&src.join(dir), &root.join(dir)).unwrap();
        }
        let s = EditorState::new(ProjectBrowser::open(root.clone()));
        (s, root)
    }

    fn harness(s: EditorState) -> Harness<'static, EditorState> {
        let mut h = Harness::builder()
            // Tall enough to show every sample material without scrolling.
            .with_size([800.0, 1200.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run_steps(2);
        h
    }

    fn apply_actions(s: &mut EditorState) {
        for a in std::mem::take(&mut s.actions) {
            crate::commands::apply(s, a);
        }
    }

    fn read(p: &Path) -> String {
        std::fs::read_to_string(p).unwrap()
    }

    #[test]
    fn type_filter_words_toggle() {
        assert_eq!(toggle_word("rock", "t:Material", true), "rock t:Material");
        assert_eq!(
            toggle_word("rock t:material x", "t:Material", false),
            "rock x"
        );
        assert_eq!(toggle_word("", "t:Scene", true), "t:Scene");
    }

    #[test]
    fn renaming_a_material_updates_scenes_and_the_open_scene() {
        let (mut s, root) = sample_copy("rename");
        let materials = root.join("Assets/Materials");
        s.project.open_folder(materials.clone());
        let cube = s.scene.spawn("Cube", None);
        s.scene.add(
            cube,
            MeshRenderer {
                material: Some("Assets/Materials/Red.mat".into()),
                ..Default::default()
            },
        );
        let mut h = harness(s);
        h.get_by_label("Red.mat").click_secondary();
        h.run();
        h.get_by_label("Rename").click();
        h.run_steps(2);
        // The field starts with the name (no extension) and has focus: replace it.
        for _ in 0..3 {
            h.key_press(egui::Key::Backspace);
        }
        h.run();
        h.event(egui::Event::Text("Crimson".into()));
        h.run();
        h.key_press(egui::Key::Enter);
        h.run();
        apply_actions(h.state_mut());

        assert!(materials.join("Crimson.mat").exists() && !materials.join("Red.mat").exists());
        // Generated demo scenes, not SampleScene: the user edits that one freely.
        let scene = read(&root.join("Assets/Scenes/PhysicsDemo.scene"));
        assert!(scene.contains("\"Assets/Materials/Crimson.mat\""));
        assert!(!scene.contains("Red.mat"));
        assert!(read(&root.join("Assets/Scenes/AnimationDemo.scene")).contains("Crimson.mat"));
        let s = h.state();
        let mat = s
            .scene
            .world
            .get::<&MeshRenderer>(cube)
            .unwrap()
            .material
            .clone();
        assert_eq!(mat.as_deref(), Some("Assets/Materials/Crimson.mat"));
        assert!(
            s.assets
                .materials
                .contains_key("Assets/Materials/Crimson.mat")
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn moving_and_renaming_scenes_updates_build_settings() {
        let (mut s, root) = sample_copy("move");
        let scenes = root.join("Assets/Scenes");
        let rename = Action::RenameAsset {
            path: scenes.join("SampleScene.scene"),
            name: "Main".into(),
        };
        crate::commands::apply(&mut s, rename);
        let build = read(&root.join("ProjectSettings/BuildSettings.json"));
        assert!(build.contains("\"Assets/Scenes/Main.scene\""), "{build}");
        assert_eq!(s.build_settings.scenes[0].path, "Assets/Scenes/Main.scene");

        // Move the whole Scenes folder into another folder: paths under it follow.
        let levels = root.join("Assets/Levels");
        std::fs::create_dir(&levels).unwrap();
        let move_folder = Action::MoveAsset {
            path: scenes,
            folder: levels,
        };
        crate::commands::apply(&mut s, move_folder);
        let build = read(&root.join("ProjectSettings/BuildSettings.json"));
        assert!(
            build.contains("\"Assets/Levels/Scenes/Main.scene\""),
            "{build}"
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn create_duplicate_and_delete_from_the_context_menu() {
        let (mut s, root) = sample_copy("create");
        let scripts = root.join("Assets/Scripts");
        s.project.open_folder(scripts.clone());
        let mut h = harness(s);
        // Right-click an item ▸ Create ▸ Lua Script; the new asset starts renaming.
        h.get_by_label("Rotator.lua").click_secondary();
        h.run();
        h.get_by_label("Create ⏵").hover();
        h.run();
        h.get_by_label("Lua Script").click();
        h.run();
        apply_actions(h.state_mut());
        h.run_steps(2);
        assert!(scripts.join("NewBehaviourScript.lua").exists());
        assert!(h.state().project.renaming.is_some());
        h.key_press(egui::Key::Escape);
        h.run();

        crate::commands::apply(
            h.state_mut(),
            Action::DuplicateAsset(scripts.join("Rotator.lua")),
        );
        assert!(scripts.join("Rotator 1.lua").exists());
        h.run_steps(2);

        h.get_by_label("Rotator 1.lua").click_secondary();
        h.run();
        h.get_by_label("Delete").click();
        h.run_steps(2);
        h.get_by_label("Delete selected asset?");
        h.get_all_by_label("Delete").last().unwrap().click();
        h.run();
        apply_actions(h.state_mut());
        assert!(
            !scripts.join("Rotator 1.lua").exists(),
            "moved to the trash"
        );
        std::fs::remove_dir_all(root).ok();
    }
}
