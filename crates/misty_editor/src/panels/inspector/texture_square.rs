//! The square texture slot of material properties: a small thumbnail at the left of the
//! property's name.
//!
//! - A texture dragged from Asset Files and dropped on the square (or on the name beside it) is
//!   assigned; the square is outlined while a texture is dragged and brighter under the pointer.
//! - A click opens a searchable picker (with "None"); a right-click clears the slot.
//! - Ctrl+click or a double-click shows the texture in Asset Files (the Properties view stays
//!   on the material).
//! - Hovering shows a larger preview with the texture's name and path.
//! - A normal-map slot holding a texture that is not imported as a normal map gets a warning
//!   row with a "Fix Now" button ([`normal_map_fix_row`]).
//!
//! Thumbnails come from the editor's cached thumbnail store ([`crate::thumbnails`]); import
//! types are read once per texture and again after import settings are applied.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::{LazyLock, Mutex};

use egui::{Align2, Color32, Id, Rect, Response, RichText, Sense, Stroke, StrokeKind, Ui, vec2};
use misty_engine::sprites::{TextureImporter, TextureType};

use super::card::ROW_HEIGHT;
use crate::i18n::tr;
use crate::panels::AssetPayload;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::label_width_or;

/// Side of the square.
pub const SLOT_SIZE: f32 = 18.0;
/// Gap between the square and the property name.
pub const SLOT_GAP: f32 = 4.0;
/// Indent of a row nested under a property (its label starts where texture names start).
pub const INDENT: f32 = SLOT_SIZE + SLOT_GAP;
/// Longest side of the tooltip preview.
const PREVIEW: f32 = 96.0;
const PICKER_WIDTH: f32 = 240.0;
const PICKER_ROW: f32 = 22.0;
/// Rows the picker shows before it scrolls.
const PICKER_ROWS: f32 = 10.0;

fn file_name(rel: &str) -> String {
    std::path::Path::new(rel)
        .file_name()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| rel.to_owned())
}

/// The cached thumbnail of a project texture.
fn thumbnail(ui: &Ui, state: &mut EditorState, rel: &str) -> Option<egui::TextureHandle> {
    let abs = state.assets.absolute(rel);
    state.project.thumbnails.get(ui.ctx(), &abs)
}

/// The centre square of an image `size` (so wide images are not squashed).
fn square_uv(size: egui::Vec2) -> Rect {
    let (w, h) = (size.x.max(1.0), size.y.max(1.0));
    let (u, v) = if w > h { (h / w, 1.0) } else { (1.0, w / h) };
    Rect::from_center_size(egui::pos2(0.5, 0.5), vec2(u, v))
}

/// Show `rel` in Asset Files (its folder opened, the file selected) without changing what the
/// Properties view shows.
pub fn show_in_asset_files(state: &mut EditorState, rel: &str) {
    let abs = state.assets.absolute(rel);
    if let Some(dir) = abs.parent() {
        state.project.search.clear();
        state.project.open_folder(dir.to_path_buf());
    }
    state.project.selected = Some(abs);
}

// ---- Import type (normal-map check) ------------------------------------------------------------

/// Import type per texture file, read on first use and forgotten when import settings are
/// applied (so no file is read while drawing after the first frame).
static IMPORT_TYPES: LazyLock<Mutex<HashMap<PathBuf, TextureType>>> =
    LazyLock::new(|| Mutex::new(HashMap::new()));

/// Forget the cached import type of the texture at `abs` (its settings were written).
pub fn forget_import_type(abs: &std::path::Path) {
    IMPORT_TYPES
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .remove(abs);
}

fn import_type(state: &EditorState, rel: &str) -> TextureType {
    let abs = state.assets.absolute(rel);
    let mut cache = IMPORT_TYPES.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(t) = cache.get(&abs) {
        return *t;
    }
    let t = TextureImporter::load(&abs).texture_type;
    cache.insert(abs, t);
    t
}

/// A normal-map slot holds a project texture that is not imported as a normal map.
pub fn needs_normal_fix(state: &EditorState, map: Option<&str>) -> bool {
    map.is_some_and(|rel| {
        state.assets.textures.iter().any(|t| t == rel)
            && import_type(state, rel) != TextureType::NormalMap
    })
}

/// Import `rel` as a normal map (what "Fix Now" does).
pub fn fix_normal_map(ctx: &egui::Context, state: &mut EditorState, rel: &str) {
    let mut settings = TextureImporter::load(&state.assets.absolute(rel));
    settings.set_type(TextureType::NormalMap);
    super::texture_import::apply(state, rel, &settings);
    // The texture's own Properties view shows the new type too.
    super::texture_import::set_pending_settings(ctx, state, rel, settings);
}

/// The grid row under a normal-map slot whose texture is not imported as a normal map: a
/// "Fix Now" button and the warning. Draws nothing (and no row) when the slot is fine.
pub fn normal_map_fix_row(ui: &mut Ui, state: &mut EditorState, map: Option<&str>) {
    if !needs_normal_fix(state, map) {
        return;
    }
    let Some(rel) = map else { return };
    ui.label("");
    ui.horizontal(|ui| {
        if ui
            .button(tr!("Fix Now"))
            .on_hover_text(tr!("Set this texture's import type to Normal Map"))
            .clicked()
        {
            fix_normal_map(ui.ctx(), state, rel);
        }
        ui.add(
            egui::Label::new(RichText::new(tr!("⚠ Not a normal map")).color(theme::WARN))
                .truncate(),
        )
        .on_hover_text(tr!(
            "This texture is not imported as a normal map, so it may be read as a colour image"
        ));
    });
    ui.end_row();
}

// ---- The square --------------------------------------------------------------------------------

/// The square texture slot on its own. `name` is the property's name (tooltip and screen
/// readers); `drop_zone` widens the area that takes a dropped texture (the row's label cell).
/// Returns true when the texture changed.
pub fn texture_square(
    ui: &mut Ui,
    state: &mut EditorState,
    id: &str,
    name: &str,
    value: &mut Option<String>,
    drop_zone: Option<Rect>,
) -> bool {
    let mut changed = false;
    let (_, rect) = ui.allocate_space(vec2(SLOT_SIZE, SLOT_SIZE));
    let resp = ui.interact(rect, Id::new(("misty_texture_square", id)), Sense::click());
    let popup_id = egui::Popup::default_response_id(&resp);
    let open = egui::Popup::is_id_open(ui.ctx(), popup_id);
    if !open {
        picker_forget(ui, popup_id);
    }

    // A texture dragged from Asset Files.
    let dragging = egui::DragAndDrop::payload::<AssetPayload>(ui.ctx())
        .is_some_and(|p| misty_engine::assets::is_texture(&p.0));
    let zone = drop_zone.map_or(rect, |z| z.union(rect));
    let over = dragging && (resp.contains_pointer() || ui.rect_contains_pointer(zone));
    if over
        && ui.input(|i| i.pointer.any_released())
        && let Some(p) = egui::DragAndDrop::take_payload::<AssetPayload>(ui.ctx())
        && value.as_deref() != Some(p.0.as_str())
    {
        *value = Some(p.0.clone());
        changed = true;
    }

    // Clicks: Ctrl+click or double-click shows the texture in Asset Files, right-click clears,
    // a plain click opens the picker.
    let ctrl = ui.input(|i| i.modifiers.command);
    let show = value.is_some() && (resp.double_clicked() || (resp.clicked() && ctrl));
    if show && let Some(rel) = value.as_deref() {
        show_in_asset_files(state, rel);
    }
    let clear = resp.secondary_clicked();
    if clear && value.is_some() {
        *value = None;
        changed = true;
    }
    let command = if show || clear {
        Some(egui::SetOpenCommand::Bool(false))
    } else if resp.clicked() {
        Some(egui::SetOpenCommand::Toggle)
    } else if !open && picker_from_env(name) {
        Some(egui::SetOpenCommand::Bool(true))
    } else {
        None
    };

    // Paint.
    let tex = value.as_deref().and_then(|rel| thumbnail(ui, state, rel));
    let painter = ui.painter();
    painter.rect_filled(rect, 2.0, theme::FIELD_BG);
    match (&tex, value.is_some()) {
        (Some(t), _) => {
            let uv = square_uv(t.size_vec2());
            painter.image(t.id(), rect.shrink(1.0), uv, Color32::WHITE);
        }
        // Assigned, but there is no such image (deleted or moved outside the editor).
        (None, true) => {
            painter.text(
                rect.center(),
                Align2::CENTER_CENTER,
                "?",
                egui::FontId::proportional(12.0),
                theme::WARN,
            );
        }
        (None, false) => {}
    }
    let stroke = if over {
        Stroke::new(2.0, theme::OVERRIDE)
    } else if dragging {
        Stroke::new(1.0, theme::PREFAB_DIM)
    } else if resp.hovered() || open {
        Stroke::new(1.0, theme::TEXT_DIM)
    } else {
        Stroke::new(1.0, Color32::from_rgb(30, 30, 30))
    };
    painter.rect_stroke(rect, 2.0, stroke, StrokeKind::Inside);

    let enabled = resp.enabled();
    let label = tr!("{name} texture", name = name);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, &label));

    egui::Popup::from_response(&resp)
        .open_memory(command)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .width(PICKER_WIDTH)
        .show(|ui| changed |= picker(ui, state, popup_id, value));

    if !open && !dragging {
        resp.on_hover_ui(|ui| tooltip(ui, name, value.as_deref(), tex.as_ref()));
    }
    changed
}

fn tooltip(ui: &mut Ui, name: &str, value: Option<&str>, tex: Option<&egui::TextureHandle>) {
    ui.set_max_width(260.0);
    let Some(rel) = value else {
        ui.label(format!("{name}: {}", tr!("None (Texture)")));
        ui.weak(tr!("Click to choose a texture, or drop one here"));
        return;
    };
    if let Some(t) = tex {
        let size = t.size_vec2();
        let scale = PREVIEW / size.x.max(size.y).max(1.0);
        ui.image((t.id(), size * scale));
    }
    ui.label(theme::bold(
        ui.ctx(),
        crate::i18n::display(&file_name(rel)).into_owned(),
    ));
    ui.weak(crate::i18n::display(rel).into_owned());
    if tex.is_none() {
        ui.colored_label(theme::WARN, tr!("Missing texture"));
    }
    ui.weak(tr!(
        "Click: choose · Right-click: clear · Ctrl+click: show in Asset Files"
    ));
}

// ---- Picker ------------------------------------------------------------------------------------

/// `MISTY_TEXTURE_PICKER=<property name>` keeps that slot's picker open (screenshots).
fn picker_from_env(name: &str) -> bool {
    static WANTED: LazyLock<Option<String>> =
        LazyLock::new(|| std::env::var("MISTY_TEXTURE_PICKER").ok());
    WANTED.as_deref() == Some(name)
}

fn search_id(popup: Id) -> Id {
    popup.with("search")
}

fn focused_id(popup: Id) -> Id {
    popup.with("focused")
}

/// Forget a closed picker's search text.
fn picker_forget(ui: &Ui, popup: Id) {
    ui.data_mut(|d| {
        d.remove::<String>(search_id(popup));
        d.remove::<bool>(focused_id(popup));
    });
}

fn picker_row(
    ui: &mut Ui,
    tex: Option<&egui::TextureHandle>,
    text: &str,
    selected: bool,
) -> Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), PICKER_ROW), Sense::click());
    let enabled = resp.enabled();
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::Button, enabled, selected, text)
    });
    let painter = ui.painter().with_clip_rect(rect.intersect(ui.clip_rect()));
    if selected {
        painter.rect_filled(rect, 2.0, theme::SELECTION);
    } else if resp.hovered() {
        painter.rect_filled(rect, 2.0, theme::HOVER_BG);
    }
    let icon = Rect::from_center_size(
        egui::pos2(rect.left() + 4.0 + 8.0, rect.center().y),
        vec2(16.0, 16.0),
    );
    painter.rect_filled(icon, 2.0, theme::FIELD_BG);
    if let Some(t) = tex {
        painter.image(t.id(), icon, square_uv(t.size_vec2()), Color32::WHITE);
    }
    painter.text(
        egui::pos2(icon.right() + 6.0, rect.center().y),
        Align2::LEFT_CENTER,
        crate::i18n::display(text),
        egui::TextStyle::Body.resolve(ui.style()),
        theme::TEXT,
    );
    resp
}

/// The picker's contents: a search field, "None" and the project's textures (only the rows in
/// view ask for a thumbnail). Returns true when another texture was chosen.
fn picker(ui: &mut Ui, state: &mut EditorState, popup: Id, value: &mut Option<String>) -> bool {
    let mut search = ui
        .data(|d| d.get_temp::<String>(search_id(popup)))
        .unwrap_or_default();
    let field = crate::widgets::search_field(ui, &mut search, ui.available_width());
    if !ui
        .data(|d| d.get_temp::<bool>(focused_id(popup)))
        .unwrap_or(false)
    {
        field.request_focus();
        ui.data_mut(|d| d.insert_temp(focused_id(popup), true));
    }
    let needle = search.trim().to_lowercase();
    let matches: Vec<String> = state
        .assets
        .textures
        .iter()
        .filter(|t| needle.is_empty() || t.to_lowercase().contains(&needle))
        .cloned()
        .collect();
    ui.data_mut(|d| d.insert_temp(search_id(popup), search));

    let mut pick: Option<Option<String>> = None;
    // Enter takes the first match.
    if field.lost_focus()
        && ui.input(|i| i.key_pressed(egui::Key::Enter))
        && !needle.is_empty()
        && let Some(first) = matches.first()
    {
        pick = Some(Some(first.clone()));
    }
    ui.add_space(2.0);
    egui::ScrollArea::vertical()
        .max_height(PICKER_ROW * PICKER_ROWS)
        .show_rows(ui, PICKER_ROW, matches.len() + 1, |ui, rows| {
            ui.spacing_mut().item_spacing.y = 0.0;
            for i in rows {
                if i == 0 {
                    if picker_row(ui, None, &tr!("None"), value.is_none()).clicked() {
                        pick = Some(None);
                    }
                    continue;
                }
                let rel = &matches[i - 1];
                let tex = thumbnail(ui, state, rel);
                let selected = value.as_deref() == Some(rel.as_str());
                if picker_row(ui, tex.as_ref(), &file_name(rel), selected)
                    .on_hover_text(crate::i18n::display(rel).into_owned())
                    .clicked()
                {
                    pick = Some(Some(rel.clone()));
                }
            }
        });
    if matches.is_empty() {
        ui.weak(tr!("No textures match"));
    }
    let Some(pick) = pick else { return false };
    egui::Popup::close_id(ui.ctx(), popup);
    if *value == pick {
        return false;
    }
    *value = pick;
    true
}

// ---- Label cells -------------------------------------------------------------------------------

/// The label cell of a property row, `indent` points in from the label column's left edge.
fn label_cell<R>(ui: &mut Ui, indent: f32, add: impl FnOnce(&mut Ui, Rect) -> R) -> R {
    let w = label_width_or(ui, 110.0);
    let cell = Rect::from_min_size(ui.cursor().min, vec2(w, ROW_HEIGHT));
    let layout = if crate::i18n::is_rtl() {
        egui::Layout::right_to_left(egui::Align::Center)
    } else {
        egui::Layout::left_to_right(egui::Align::Center)
    };
    ui.allocate_ui_with_layout(vec2(w, ROW_HEIGHT), layout, |ui| {
        ui.set_min_width(w);
        ui.spacing_mut().item_spacing.x = SLOT_GAP;
        if indent > 0.0 {
            ui.add_space(indent);
        }
        add(ui, cell)
    })
    .inner
}

/// Label cell of a texture property: the square, then the (already translated) name. A texture
/// dropped anywhere on the cell is assigned. Returns true when the texture changed.
pub fn map_label(
    ui: &mut Ui,
    state: &mut EditorState,
    id: &str,
    label: &str,
    tip: &str,
    value: &mut Option<String>,
) -> bool {
    label_cell(ui, 0.0, |ui, cell| {
        let changed = texture_square(ui, state, id, label, value, Some(cell));
        let r = ui.add(egui::Label::new(label).truncate());
        if !tip.is_empty() {
            r.on_hover_text(tip);
        }
        changed
    })
}

/// Label cell without a texture: `level` 0 lines up with the squares, 1 with the texture names
/// (a value of the property above), 2 one step further in.
pub fn row_label(ui: &mut Ui, label: &str, level: u8) -> Response {
    let indent = match level {
        0 => 0.0,
        1 => INDENT,
        _ => INDENT + 14.0,
    };
    label_cell(ui, indent, |ui, _| {
        ui.add(egui::Label::new(label).truncate())
    })
}
