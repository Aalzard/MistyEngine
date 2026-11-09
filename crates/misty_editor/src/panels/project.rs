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
    state.project.os_drop_folder = None;
    let mut opened_file: Option<PathBuf> = None;
    let mut req = Requests::default();
    let browser = &mut state.project;
    let vcs = &state.vcs;
    // One toolbar row: Create on the left, search, type filter and refresh on the right.
    panel_toolbar(ui, |ui| {
        let create = ui.menu_button("➕ ⏷", |ui| {
            project_menus::create_items(ui, &mut req.actions)
        });
        crate::a11y::name(create.response, egui::WidgetType::Button, "Create");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let refresh = ui.button("⟳").on_hover_text("Refresh (Ctrl+R)");
            if crate::a11y::name(refresh, egui::WidgetType::Button, "Refresh").clicked() {
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
                    let size = ui.add(
                        egui::Slider::new(&mut browser.icon_size, 0.0..=1.0).show_value(false),
                    );
                    crate::a11y::name(size, egui::WidgetType::Slider, "Icon size");
                });
            });
        });

    egui::Panel::left("project_tree")
        .default_size(180.0)
        .show_separator_line(false)
        .frame(egui::Frame::side_top_panel(ui.style()).fill(theme::SIDEBAR_BG))
        .resizable(true)
        .show(ui, |ui| {
            // Empty space under the folders: the folder menu of the open folder.
            let background = ui.interact(
                ui.max_rect(),
                ui.id().with("project_tree_background"),
                Sense::click(),
            );
            egui::ScrollArea::vertical()
                .auto_shrink(false)
                .show(ui, |ui| {
                    let mut open = None;
                    super::project_tree::show(ui, browser, &mut open, &mut req);
                    if let Some(path) = open {
                        browser.open_folder(path);
                    }
                });
            let current = browser.current.clone();
            if !browser.is_read_only(&current) {
                background.context_menu(|ui| project_menus::folder_menu(ui, &current, &mut req));
            }
        });

    let dragging_objects = ui
        .data(|d| d.get_temp::<Vec<misty_engine::hecs::Entity>>(super::hierarchy::drag_id()))
        .is_some();
    egui::CentralPanel::default().show(ui, |ui| {
        browser.drop_rect = ui.max_rect();
        // The whole content area, under the breadcrumb, tiles and rows (they sit on top of
        // it): a click deselects, a right-click shows the folder menu, also in an empty folder,
        // beside the last row and in the gaps between tiles.
        let background = ui.interact(
            ui.max_rect(),
            ui.id().with("project_content_background"),
            Sense::click(),
        );
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
            browser.cached_search_results()
        };
        let mut open = None;
        egui::ScrollArea::vertical()
            .auto_shrink(false)
            .show(ui, |ui| {
                if browser.icon_size < 0.05 {
                    list_view(ui, &entries, browser, vcs, &mut open, &mut req);
                } else {
                    grid_view(ui, &entries, browser, vcs, &mut open, &mut req);
                }
            });
        if background.clicked() {
            browser.selected = None;
        }
        let current = browser.current.clone();
        background.context_menu(|ui| project_menus::folder_menu(ui, &current, &mut req));
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
    os_files(ui, state, panel);
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

/// Files dragged in from the OS over the window: outline the folder they would land in (the
/// one under the pointer, else the open folder); dropped, copy and import them there.
fn os_files(ui: &Ui, state: &mut EditorState, panel: Rect) {
    if !crate::os_drop::pointer_in(ui, panel) && !crate::os_drop::preview() {
        return;
    }
    let target = state.project.os_drop_folder.clone();
    let folder = target
        .as_ref()
        .map_or_else(|| state.project.current.clone(), |(p, _)| p.clone());
    if crate::os_drop::hovering(ui.ctx()) {
        let caption = format!("Import into {}", state.project.display_path(&folder));
        if let Some((_, rect)) = target {
            crate::os_drop::highlight(ui, rect, "");
        }
        crate::os_drop::highlight(ui, state.project.drop_rect, &caption);
    }
    if crate::os_drop::peek(ui.ctx()).is_empty() {
        return;
    }
    let paths = crate::os_drop::take(ui.ctx());
    state.push(Action::ImportFiles {
        paths,
        folder,
        instantiate: false,
    });
}

/// Double-clicked file: scenes open in the editor, scripts in the Code Editor, other files in
/// their default app.
pub(crate) fn open_file(state: &mut EditorState, opened: Option<PathBuf>) {
    let Some(path) = opened else { return };
    let ext = path.extension().and_then(|e| e.to_str()).unwrap_or("");
    if ext == misty_engine::scene::prefab::EXTENSION {
        state.push(Action::OpenPrefab {
            path: state.assets.relative(&path),
            from: None,
        });
    } else if ext == misty_engine::scene::file::EXTENSION {
        state.push(Action::OpenScenePath(state.assets.relative(&path)));
    } else if ext == misty_engine::build_profile::EXTENSION {
        // A Build Profile asset opens in File ▸ Build Profiles.
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        crate::build_settings::load(state);
        if let Some(i) = state.build_profiles.index_of(&name) {
            state.build_window.page = crate::build_settings::Page::Profile(i);
        }
        state.build_window.open = true;
    } else if misty_engine::animation::is_controller(&path.to_string_lossy()) {
        state.animator.open(Some(state.assets.relative(&path)));
        state.push(Action::FocusTab(crate::layout::Tab::Animator));
    } else if misty_engine::timeline::is_timeline(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::timeline::open_asset(state, &rel);
    } else if misty_engine::input::actions::asset::is_actions_path(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::input_actions::open(state, &rel);
    } else if misty_engine::audio::is_mixer(&path.to_string_lossy()) {
        state.audio_mixer.open(Some(state.assets.relative(&path)));
        state.push(Action::FocusTab(crate::layout::Tab::AudioMixer));
    } else if misty_engine::vfx::is_effect_path(&path.to_string_lossy())
        || misty_engine::vfx::subgraph::is_subgraph_path(&path.to_string_lossy())
    {
        let rel = state.assets.relative(&path);
        crate::panels::effect_graph::open(state, &rel);
    } else if misty_engine::visual_script::is_graph_path(&path.to_string_lossy()) {
        let rel = state.assets.relative(&path);
        crate::panels::script_graph::open(state, &rel);
    } else if misty_engine::shader_graph::is_graph_path(&path.to_string_lossy())
        || misty_engine::shader_graph::is_sub_graph_path(&path.to_string_lossy())
    {
        let rel = state.assets.relative(&path);
        crate::panels::shader_graph::open(state, &rel);
    } else if ext == misty_engine::localization::table::EXTENSION {
        let rel = state.assets.relative(&path);
        crate::saveloc::tables::open(state, &rel);
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
    // Create ▸ in a folder's own menu: open it, so the new asset is made (and renamed) there.
    if let Some(dir) = req.create_in
        && dir != state.project.current
    {
        state.project.search.clear();
        state.project.open_folder(dir);
    }
    for a in req.actions {
        state.push(a);
    }
    if let Some(p) = req.rename {
        state.project.selected = Some(p.clone());
        state.project.start_rename(p);
        state.project.rename_hold_guard = req.rename_by_hold;
        state.project.rename_in_tree = req.rename_in_tree;
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
    vcs: &crate::vcs::Vcs,
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
        // Version control status at the row's right end.
        let resp = match vcs.badge_at(&browser.root, &e.path) {
            Some(b) => {
                let at = egui::pos2(resp.rect.right() - 10.0, resp.rect.center().y);
                crate::vcs::paint_badge(ui, at, b, 12.0);
                resp.on_hover_text(crate::vcs::badge_tip(b))
            }
            None => resp,
        };
        if renaming {
            let r = resp.rect.with_min_x(resp.rect.left() + 26.0);
            project_menus::rename_field(ui, r, browser, req);
        }
        item_interaction(ui, &resp, e, browser, open, req);
    }
}

fn is_renaming(browser: &ProjectBrowser, e: &AssetEntry) -> bool {
    !browser.rename_in_tree && browser.renaming.as_ref().is_some_and(|(p, _)| *p == e.path)
}

fn grid_view(
    ui: &mut Ui,
    entries: &[AssetEntry],
    browser: &mut ProjectBrowser,
    vcs: &crate::vcs::Vcs,
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
            // Tiles scrolled out of view keep their place and interaction but draw nothing
            // (big folders otherwise paint every icon and name each frame).
            let visible = ui.is_rect_visible(rect);
            let is_sel = browser.selected.as_ref() == Some(&e.path);
            if visible && is_sel {
                ui.painter().rect_filled(rect, 3.0, theme::SELECTION);
            } else if visible && resp.hovered() {
                ui.painter()
                    .rect_filled(rect, 3.0, Color32::from_rgb(69, 69, 69));
            }
            if visible {
                tile_icon(ui, e, rect, cell, &mut browser.thumbnails);
            }
            // Version control status on the icon's bottom-left corner.
            let badge = visible
                .then(|| vcs.badge_at(&browser.root, &e.path))
                .flatten();
            let resp = match badge {
                Some(b) => {
                    let size = (cell * 0.22).clamp(11.0, 16.0);
                    let at = rect.left_top() + egui::vec2(size * 0.7, cell - size * 0.5);
                    crate::vcs::paint_badge(ui, at, b, size);
                    resp.on_hover_text(crate::vcs::badge_tip(b))
                }
                None => resp,
            };
            if is_renaming(browser, e) {
                let r = Rect::from_center_size(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    egui::vec2(cell + 6.0, 18.0),
                );
                project_menus::rename_field(ui, r, browser, req);
            } else if visible {
                ui.painter().text(
                    rect.center_bottom() - egui::vec2(0.0, 9.0),
                    Align2::CENTER_CENTER,
                    truncate(&e.name, (cell / 7.0) as usize),
                    FontId::proportional(12.0),
                    theme::TEXT,
                );
            }
            item_interaction(ui, &resp, e, browser, open, req);
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
    ui: &Ui,
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
        crate::hold_rename::drag_payload(ui, resp, AssetPayload(rel));
    }
    if e.is_dir && !read_only {
        drop_target(resp, &browser.root, &e.path, req);
    }
    // Press and hold renames it (a bar along the bottom fills meanwhile).
    if !read_only && !is_renaming(browser, e) {
        let hold = crate::hold_rename::check(ui, resp);
        crate::hold_rename::paint(ui, resp.rect, hold);
        if hold == crate::hold_rename::Hold::Fire {
            req.rename = Some(e.path.clone());
            req.rename_by_hold = true;
        }
    }
    // Files dragged in from the OS land in the folder under the pointer.
    if e.is_dir && !read_only && resp.contains_pointer() {
        browser.os_drop_folder = Some((e.path.clone(), resp.rect));
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

#[cfg(test)]
#[path = "project_tests.rs"]
mod dock_tests;
