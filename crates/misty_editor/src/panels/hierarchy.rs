//! Hierarchy panel: scene tree with foldouts, multi-select, drag re-parenting, inline rename.

use egui::{Align2, Color32, FontId, Id, Key, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::hecs::Entity;
use misty_engine::scene::SceneId;

use crate::actions::{Action, SceneOp};
use crate::menu_bar::create_menu_items;
use crate::search::fuzzy::Mode;
use crate::state::{EditorState, object_icon};
use crate::theme;
use crate::theme::{PREFAB, PREFAB_DIM};
use crate::widgets::{panel_toolbar, search_field};

const ROW_H: f32 = 18.0;
const INDENT: f32 = 14.0;

/// Where a dragged row would land.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Drop {
    Before(Entity),
    After(Entity),
    Into(Entity),
    Root,
    /// A scene header: move into that scene.
    Scene(SceneId),
}

/// One Hierarchy line: a scene header or an object with its depth.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Row {
    Scene(SceneId),
    Object(Entity, usize),
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let panel = ui.max_rect();
    update_focus(ui, state, panel);

    panel_toolbar(ui, |ui| {
        ui.menu_button("➕ ⏷", |ui| create_menu_items(ui, state, None));
        search_field(ui, &mut state.hierarchy.search, ui.available_width())
            .on_hover_text("Name words and filters: t:Light t:Camera tag:Player layer:UI");
    });

    let rows = visible_rows(state);
    let drag_id = drag_id();
    let dragging: Option<Vec<Entity>> = ui.data(|d| d.get_temp(drag_id));
    let mut drop: Option<Drop> = None;

    let objects: Vec<(Entity, usize)> = rows
        .iter()
        .filter_map(|r| match r {
            Row::Object(e, d) => Some((*e, *d)),
            Row::Scene(_) => None,
        })
        .collect();

    egui::ScrollArea::vertical()
        .auto_shrink(false)
        .show(ui, |ui| {
            // Only rows inside the visible area are laid out; the rest just take their
            // height (thousands of rows otherwise cost milliseconds every frame).
            let clip = ui.clip_rect();
            let spacing = ui.spacing().item_spacing.y;
            let step = ROW_H + spacing;
            let start = ((clip.top() - ui.cursor().top()) / step).floor().max(0.0) as usize;
            let shown = (clip.height() / step).ceil() as usize + 2;
            let start = start.min(rows.len());
            let end = (start + shown).min(rows.len());
            let width = ui.available_width();
            if start > 0 {
                ui.allocate_space(vec2(width, start as f32 * step - spacing));
            }
            for r in &rows[start..end] {
                let hovered = match *r {
                    Row::Scene(id) => scene_row(ui, state, id, dragging.is_some()),
                    Row::Object(e, depth) => row(ui, state, e, depth, &objects, dragging.is_some()),
                };
                if let Some(d) = hovered {
                    drop = Some(d);
                }
            }
            if end < rows.len() {
                ui.allocate_space(vec2(width, (rows.len() - end) as f32 * step - spacing));
            }

            // Empty space below the rows: deselect, create menu, drop to root.
            let rest =
                ui.allocate_response(ui.available_size().max(vec2(0.0, 40.0)), Sense::click());
            if rest.clicked() {
                state.selection.clear();
            }
            rest.context_menu(|ui| create_menu_items(ui, state, None));
            if let Some(payload) = rest.dnd_release_payload::<super::AssetPayload>() {
                drop_prefab(state, &payload.0, None);
                drop_scene(state, &payload.0);
            }
            if dragging.is_some() && rest.contains_pointer() {
                drop = Some(Drop::Root);
            }
        });

    drag_and_drop(ui, state, drag_id, dragging, drop);
    keyboard(ui, state, &objects);
}

/// Temp-data key of the Hierarchy's own drag (the dragged objects).
pub fn drag_id() -> Id {
    Id::new("hierarchy_drag")
}

/// The Hierarchy owns keyboard navigation after a click inside it.
fn update_focus(ui: &Ui, state: &mut EditorState, panel: Rect) {
    if let Some(pos) = ui.input(|i| i.pointer.press_origin().filter(|_| i.pointer.any_pressed())) {
        state.hierarchy.focused = panel.contains(pos);
    }
}

/// Each open scene's header, then its objects: depth-first respecting foldouts, or a flat
/// list of matches while searching.
fn visible_rows(state: &EditorState) -> Vec<Row> {
    let scene = &state.scene;
    // Name words plus t: / tag: / layer: filters, shared with the search palette; matches are
    // listed flat under their scene's header.
    let matches: Option<Vec<Entity>> = (!state.hierarchy.search.trim().is_empty()).then(|| {
        let q = crate::search::query::Query::parse(&state.hierarchy.search);
        let tl = &state.project_settings.tags_and_layers;
        crate::search::scene_search::search(scene, tl, &q, Mode::Contains, 0)
            .into_iter()
            .map(|(e, _)| e)
            .collect()
    });
    let mut out = Vec::new();
    fn visit(state: &EditorState, e: Entity, depth: usize, out: &mut Vec<Row>) {
        out.push(Row::Object(e, depth));
        if state.hierarchy.expanded.contains(&e) {
            for c in state.scene.children(e) {
                visit(state, c, depth + 1, out);
            }
        }
    }
    for entry in scene.scenes() {
        out.push(Row::Scene(entry.id));
        if !entry.loaded || state.hierarchy.collapsed_scenes.contains(&entry.id) {
            continue;
        }
        match &matches {
            None => {
                for r in scene.roots_in(entry.id) {
                    visit(state, r, 0, &mut out);
                }
            }
            Some(hits) => out.extend(
                hits.iter()
                    .filter(|e| scene.scene_of(**e) == entry.id)
                    .map(|e| Row::Object(*e, 0)),
            ),
        }
    }
    out
}

/// A scene header: foldout, name (bold = active, * = unsaved, grey = not loaded) and its
/// menu (⋮ or right-click). Returns a drop target while objects are dragged over it.
fn scene_row(ui: &mut Ui, state: &mut EditorState, id: SceneId, dragging: bool) -> Option<Drop> {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    let loaded = state.scene.entry(id).is_some_and(|e| e.loaded);
    let active = state.scene.active_scene() == id;
    let dirty = if state.files.dirty.contains(&id) {
        "*"
    } else {
        ""
    };
    let name = state.scene.scene_name(id);
    let label = if loaded {
        format!("{name}{dirty}")
    } else {
        format!("{name} (not loaded)")
    };
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, &label));
    let fill = if resp.hovered() && !dragging {
        Color32::from_rgb(62, 62, 62)
    } else {
        Color32::from_rgb(52, 52, 52)
    };
    ui.painter().rect_filled(rect, 0.0, fill);

    // Foldout (double-clicking the header also folds it).
    let arrow_rect = Rect::from_min_size(pos2(rect.left() + 2.0, rect.top()), vec2(14.0, ROW_H));
    let collapsed = state.hierarchy.collapsed_scenes.contains(&id);
    if loaded {
        let arrow = ui.interact(
            arrow_rect,
            Id::new(("hier_scene_arrow", id)),
            Sense::click(),
        );
        ui.painter().text(
            arrow_rect.center(),
            Align2::CENTER_CENTER,
            if collapsed { "⏵" } else { "⏷" },
            FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
        if arrow.clicked() || resp.double_clicked() {
            if collapsed {
                state.hierarchy.collapsed_scenes.remove(&id);
            } else {
                state.hierarchy.collapsed_scenes.insert(id);
            }
        }
    }
    let color = match (loaded, active) {
        (false, _) => theme::TEXT_DIM,
        (true, true) => Color32::WHITE,
        (true, false) => theme::TEXT,
    };
    let text_pos = pos2(rect.left() + 18.0, rect.center().y);
    let text = format!("🎬 {label}");
    let font = FontId::proportional(13.0);
    ui.painter()
        .text(text_pos, Align2::LEFT_CENTER, &text, font.clone(), color);
    // The active scene is bold (drawn twice, a little apart: the UI font has no bold face).
    if active {
        ui.painter().text(
            text_pos + vec2(0.7, 0.0),
            Align2::LEFT_CENTER,
            &text,
            font,
            color,
        );
    }

    // ⋮ menu at the right edge, same items as the right-click menu.
    let menu_rect = Rect::from_min_max(pos2(rect.right() - 20.0, rect.top()), rect.max);
    let menu = ui.interact(menu_rect, Id::new(("hier_scene_menu", id)), Sense::click());
    ui.painter().text(
        menu_rect.center(),
        Align2::CENTER_CENTER,
        "⋮",
        FontId::proportional(13.0),
        theme::TEXT_DIM,
    );
    egui::Popup::menu(&menu).show(|ui| scene_menu(ui, state, id));
    resp.context_menu(|ui| scene_menu(ui, state, id));

    if let Some(payload) = resp.dnd_release_payload::<super::AssetPayload>() {
        drop_scene(state, &payload.0);
    }
    if dragging
        && loaded
        && ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| rect.contains(p))
    {
        let stroke = Stroke::new(2.0, Color32::from_rgb(90, 150, 230));
        ui.painter()
            .rect_stroke(rect.shrink(1.0), 2.0, stroke, egui::StrokeKind::Inside);
        return Some(Drop::Scene(id));
    }
    None
}

/// Scene header menu: active scene, save, select, unload / load, remove, close others.
fn scene_menu(ui: &mut Ui, state: &mut EditorState, id: SceneId) {
    let scene = &state.scene;
    let loaded = scene.entry(id).is_some_and(|e| e.loaded);
    let active = scene.active_scene() == id;
    let several_loaded = scene.loaded_scenes().len() > 1;
    let several = scene.scenes().len() > 1;
    let load_item = if loaded {
        ("Unload Scene", SceneOp::Unload, several_loaded)
    } else {
        ("Load Scene", SceneOp::Load, true)
    };
    let groups = [
        vec![("Set Active Scene", SceneOp::SetActive, loaded && !active)],
        vec![
            ("Save Scene", SceneOp::Save, loaded),
            ("Save Scene As", SceneOp::SaveAs, loaded),
            ("Select All", SceneOp::SelectAll, loaded),
        ],
        vec![
            load_item,
            (
                "Remove Scene",
                SceneOp::Remove,
                several && (!loaded || several_loaded),
            ),
            ("Close Others", SceneOp::CloseOthers, several),
        ],
    ];
    for (i, group) in groups.into_iter().enumerate() {
        if i > 0 {
            ui.separator();
        }
        for (label, op, enabled) in group {
            if ui.add_enabled(enabled, egui::Button::new(label)).clicked() {
                state.push(Action::Scene(id, op));
                ui.close();
            }
        }
    }
}

/// One object row. Returns a drop target if a drag is hovering it.
fn row(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    depth: usize,
    rows: &[(Entity, usize)],
    dragging: bool,
) -> Option<Drop> {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    let selected = state.selection.contains(e);
    let name = state.scene.name(e);
    if crate::tutorials::targets::enabled() {
        crate::tutorials::targets::mark(ui, &format!("hierarchy.{name}"), rect);
    }
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, &name)
    });
    let painter = ui.painter();
    if selected {
        let fill = if state.hierarchy.focused {
            theme::SELECTION
        } else {
            Color32::from_gray(77)
        };
        painter.rect_filled(rect, 0.0, fill);
    } else if resp.hovered() && !dragging {
        painter.rect_filled(rect, 0.0, Color32::from_rgb(69, 69, 69));
    }

    let x = rect.left() + 4.0 + depth as f32 * INDENT;
    let has_children = !state.scene.children(e).is_empty();
    let searching = !state.hierarchy.search.is_empty();

    // Foldout arrow (Alt+click opens/closes the whole subtree).
    if has_children && !searching {
        let arrow_rect = Rect::from_min_size(pos2(x, rect.top()), vec2(14.0, ROW_H));
        let expanded = state.hierarchy.expanded.contains(&e);
        let arrow = ui.interact(arrow_rect, Id::new(("hier_arrow", e)), Sense::click());
        ui.painter().text(
            arrow_rect.center(),
            Align2::CENTER_CENTER,
            if expanded { "⏷" } else { "⏵" },
            FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
        if arrow.clicked() {
            let recursive = ui.input(|i| i.modifiers.alt);
            set_expanded(state, e, !expanded, recursive);
        }
    }

    // Icon + name, or the inline rename field.
    let text_x = x + 16.0;
    let active = state.scene.active_in_hierarchy(e);
    // Prefab instances (root and children) have blue names.
    let prefab = state.scene.prefab_root(e).is_some();
    let color = match (selected && state.hierarchy.focused, active, prefab) {
        (true, _, _) => Color32::WHITE,
        (false, true, true) => PREFAB,
        (false, false, true) => PREFAB_DIM,
        (false, true, false) => theme::TEXT,
        (false, false, false) => theme::TEXT_DIM,
    };
    let renaming = state
        .hierarchy
        .renaming
        .as_ref()
        .is_some_and(|(r, _)| *r == e);
    if renaming {
        rename_field(
            ui,
            state,
            e,
            Rect::from_min_max(pos2(text_x, rect.top()), rect.max),
        );
    } else {
        // The designer's icon where there is one, else the glyph.
        let (x, label) = match crate::state::object_brand_icon(&state.scene, e) {
            Some(icon) => {
                let slot = Rect::from_min_size(pos2(text_x, rect.top()), vec2(15.0, ROW_H));
                crate::branding::icons::paint(ui, slot.shrink2(vec2(0.0, 2.0)), icon, color);
                (text_x + ICON_ADVANCE, name.to_string())
            }
            None => (text_x, format!("{} {name}", object_icon(&state.scene, e))),
        };
        ui.painter().text(
            pos2(x, rect.center().y),
            Align2::LEFT_CENTER,
            label,
            FontId::proportional(13.0),
            color,
        );
    }

    // Clicks: plain / Ctrl toggle / Shift range; double-click frames.
    if resp.clicked() && !renaming {
        let mods = ui.input(|i| i.modifiers);
        let order: Vec<Entity> = rows.iter().map(|(e, _)| *e).collect();
        if mods.command {
            state.selection.toggle(e);
        } else if mods.shift {
            state.selection.select_range(e, &order);
        } else {
            state.selection.set(e);
        }
    }
    if resp.double_clicked() {
        state.selection.set(e);
        state.push(Action::FrameSelected);
    }
    if resp.secondary_clicked() && !selected {
        state.selection.set(e);
    }
    resp.context_menu(|ui| row_menu(ui, state, e));
    if let Some(payload) = resp.dnd_release_payload::<super::AssetPayload>() {
        drop_asset(state, e, &payload.0);
        drop_prefab(state, &payload.0, Some(e));
        drop_scene(state, &payload.0);
    }

    // Start dragging: carry the selection (or just this row).
    if resp.drag_started() {
        if !state.selection.contains(e) {
            state.selection.set(e);
        }
        let list = state.selection.top_level(&state.scene);
        ui.data_mut(|d| d.insert_temp(drag_id(), list));
    }

    // Drop zone under the pointer: top/bottom quarter = between rows, middle = make child.
    if dragging
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
        && rect.contains(p)
    {
        let t = (p.y - rect.top()) / rect.height();
        let target = if t < 0.25 {
            Drop::Before(e)
        } else if t > 0.75 {
            Drop::After(e)
        } else {
            Drop::Into(e)
        };
        let stroke = Stroke::new(2.0, Color32::from_rgb(90, 150, 230));
        let painter = ui.painter();
        match target {
            Drop::Before(_) => painter.hline(text_x..=rect.right(), rect.top(), stroke),
            Drop::After(_) => painter.hline(text_x..=rect.right(), rect.bottom(), stroke),
            _ => painter.rect_stroke(rect.shrink(1.0), 2.0, stroke, egui::StrokeKind::Inside),
        };
        return Some(target);
    }
    None
}

/// A prefab dropped in the Hierarchy: instantiate under `parent` (None = scene root).
/// Prefabs and model files dropped on a row (or the empty area) are instantiated there.
fn drop_prefab(state: &mut EditorState, path: &str, parent: Option<Entity>) {
    if path.ends_with(".prefab") {
        state.push(Action::InstantiatePrefab {
            path: path.to_owned(),
            parent,
        });
    } else if misty_engine::models::is_model(path) {
        state.push(Action::InstantiateModel {
            path: path.to_owned(),
            parent,
        });
    }
}

/// A scene file dropped in the Hierarchy opens additively.
fn drop_scene(state: &mut EditorState, path: &str) {
    if path.ends_with(&format!(".{}", misty_engine::scene::file::EXTENSION)) {
        state.push(Action::OpenSceneAdditive(path.to_owned()));
    }
}

/// An asset dropped on an object: materials are assigned, scripts attached.
pub fn drop_asset(state: &mut EditorState, e: Entity, path: &str) {
    if path.ends_with(".mat") {
        state.assign_material(e, path);
    } else if path.ends_with(".lua") {
        state.push(Action::AddScript {
            entity: e,
            path: path.to_owned(),
        });
    }
}

fn rename_field(ui: &mut Ui, state: &mut EditorState, e: Entity, rect: Rect) {
    let Some((_, text)) = state.hierarchy.renaming.as_mut() else {
        return;
    };
    let edit = ui.put(
        rect,
        egui::TextEdit::singleline(text).margin(vec2(2.0, 0.0)),
    );
    if !edit.has_focus() && !edit.lost_focus() {
        edit.request_focus();
    }
    let (enter, escape) = ui.input(|i| (i.key_pressed(Key::Enter), i.key_pressed(Key::Escape)));
    if escape {
        state.hierarchy.renaming = None;
    } else if edit.lost_focus() || enter {
        let new_name = text.trim().to_owned();
        state.hierarchy.renaming = None;
        if !new_name.is_empty() && new_name != state.scene.name(e) {
            state.record(&format!("Rename {}", state.scene.name(e)));
            state.scene.game_object_mut(e).name = new_name;
        }
    }
}

fn row_menu(ui: &mut Ui, state: &mut EditorState, e: Entity) {
    let paste = |as_child| Action::PasteObjects {
        text: None,
        as_child,
    };
    for (label, action) in [
        ("Cut", Action::CutObjects),
        ("Copy", Action::CopyObjects),
        ("Paste", paste(false)),
        ("Paste As Child", paste(true)),
        ("Rename", Action::RenameSelected),
        ("Duplicate", Action::DuplicateSelected),
        ("Delete", Action::DeleteSelected),
    ] {
        if ui.button(label).clicked() {
            state.push(action);
            ui.close();
        }
    }
    if let Some(root) = state.scene.prefab_root(e) {
        ui.separator();
        ui.menu_button("Prefab", |ui| {
            if ui.button("Select Asset").clicked() {
                state.push(Action::SelectPrefabAsset(root));
                ui.close();
            }
            if ui.button("Unpack Prefab").clicked() {
                state.push(Action::UnpackPrefab(root));
                ui.close();
            }
        });
    }
    ui.separator();
    if ui.button("Select Children").clicked() {
        let mut all = vec![e];
        all.extend(
            state
                .scene
                .walk()
                .into_iter()
                .map(|(x, _)| x)
                .filter(|x| *x != e && state.scene.is_ancestor(e, *x)),
        );
        state.selection.set_many(all);
        ui.close();
    }
    ui.separator();
    create_menu_items(ui, state, Some(e));
}

fn set_expanded(state: &mut EditorState, e: Entity, open: bool, recursive: bool) {
    if open {
        state.hierarchy.expanded.insert(e);
    } else {
        state.hierarchy.expanded.remove(&e);
    }
    if recursive {
        for c in state.scene.children(e) {
            set_expanded(state, c, open, true);
        }
    }
}

fn drag_and_drop(
    ui: &mut Ui,
    state: &mut EditorState,
    drag_id: Id,
    dragging: Option<Vec<Entity>>,
    drop: Option<Drop>,
) {
    let Some(dragged) = dragging else { return };
    // Label following the pointer.
    if let Some(p) = ui.input(|i| i.pointer.hover_pos()) {
        let label = match dragged.as_slice() {
            [one] => state.scene.name(*one),
            many => format!("{} objects", many.len()),
        };
        let painter = ui
            .ctx()
            .layer_painter(egui::LayerId::new(egui::Order::Tooltip, drag_id));
        painter.text(
            p + vec2(14.0, 4.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(13.0),
            Color32::WHITE,
        );
    }
    if !ui.input(|i| i.pointer.any_released()) {
        return;
    }
    ui.data_mut(|d| d.remove::<Vec<Entity>>(drag_id));
    // Released over the Project window: save each dragged object as a prefab there.
    if drop.is_none()
        && ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| state.project.drop_rect.contains(p))
    {
        let folder = state.project.current.clone();
        for entity in dragged {
            state.push(Action::CreatePrefab {
                entity,
                folder: folder.clone(),
            });
        }
        return;
    }
    let Some(drop) = drop else { return };
    apply_drop(state, &dragged, drop);
}

/// Re-parent/reorder `dragged` at `drop`, keeping world positions.
fn apply_drop(state: &mut EditorState, dragged: &[Entity], drop: Drop) {
    let scene = &state.scene;
    let target = match drop {
        Drop::Before(t) | Drop::After(t) | Drop::Into(t) => Some(t),
        Drop::Root => None,
        Drop::Scene(id) => {
            state.push(Action::MoveToScene {
                objects: dragged.to_vec(),
                scene: id,
            });
            return;
        }
    };
    // Can't drop onto itself or into its own subtree.
    if let Some(t) = target
        && dragged.iter().any(|d| scene.is_ancestor(*d, t))
    {
        return;
    }
    let who = match dragged {
        [one] => scene.name(*one),
        many => format!("{} Objects", many.len()),
    };
    state.record(&format!("Reparent {who}"));
    let mut prev: Option<Entity> = None;
    for &e in dragged {
        let scene = &mut state.scene;
        match drop {
            Drop::Into(t) => {
                scene.set_parent(e, Some(t), None, true);
                state.hierarchy.expanded.insert(t);
            }
            Drop::Root | Drop::Scene(_) => {
                scene.set_parent(e, None, None, true);
            }
            Drop::Before(t) | Drop::After(t) => {
                let parent = scene.parent(t);
                // Between another scene's top-level objects: move into that scene first.
                if parent.is_none() {
                    scene.move_to_scene(e, scene.scene_of(t));
                }
                let index = match (prev, drop) {
                    (Some(p), _) => scene.sibling_index(p) + 1,
                    (None, Drop::Before(_)) => scene.sibling_index(t),
                    _ => scene.sibling_index(t) + 1,
                };
                scene.set_parent(e, parent, Some(index), true);
            }
        }
        prev = Some(e);
    }
}

/// Arrow keys move the selection; Left/Right fold and unfold.
fn keyboard(ui: &Ui, state: &mut EditorState, rows: &[(Entity, usize)]) {
    if !state.hierarchy.focused
        || state.hierarchy.renaming.is_some()
        || ui.ctx().egui_wants_keyboard_input()
    {
        return;
    }
    let Some(active) = state.selection.active() else {
        return;
    };
    let Some(i) = rows.iter().position(|(e, _)| *e == active) else {
        return;
    };
    let (up, down, left, right) = ui.input(|inp| {
        (
            inp.key_pressed(Key::ArrowUp),
            inp.key_pressed(Key::ArrowDown),
            inp.key_pressed(Key::ArrowLeft),
            inp.key_pressed(Key::ArrowRight),
        )
    });
    if up && i > 0 {
        state.selection.set(rows[i - 1].0);
    }
    if down && i + 1 < rows.len() {
        state.selection.set(rows[i + 1].0);
    }
    let children = state.scene.children(active);
    if right && !children.is_empty() {
        if state.hierarchy.expanded.contains(&active) {
            state.selection.set(children[0]);
        } else {
            state.hierarchy.expanded.insert(active);
        }
    }
    if left {
        if state.hierarchy.expanded.remove(&active) && !children.is_empty() {
            // Collapsed.
        } else if let Some(p) = state.scene.parent(active) {
            state.selection.set(p);
        }
    }
}

/// Width of a painted 15 px icon and the space after it, before a row's name.
const ICON_ADVANCE: f32 = 19.0;

/// Full-width selectable row with indentation and an optional icon (the Project window).
pub fn row_with_icon(
    ui: &mut Ui,
    selected: bool,
    icon: Option<crate::branding::icons::Icon>,
    text: &str,
    color: Color32,
    depth: usize,
) -> egui::Response {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click_and_drag());
    if selected {
        ui.painter().rect_filled(rect, 0.0, theme::SELECTION);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 0.0, Color32::from_rgb(69, 69, 69));
    }
    let mut pos = rect.left_center() + vec2(6.0 + depth as f32 * INDENT, 0.0);
    let color = if selected { Color32::WHITE } else { color };
    if let Some(icon) = icon {
        let slot = Rect::from_min_size(pos2(pos.x, rect.top()), vec2(15.0, ROW_H));
        crate::branding::icons::paint(ui, slot.shrink2(vec2(0.0, 2.0)), icon, color);
        pos.x += ICON_ADVANCE;
    }
    ui.painter().text(
        pos,
        Align2::LEFT_CENTER,
        text,
        FontId::proportional(13.0),
        color,
    );
    resp
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_hier_test"),
        ))
    }

    #[test]
    fn drop_into_makes_child_and_undo_restores() {
        let mut s = state();
        let roots = s.scene.roots().to_vec();
        let (cam, light) = (roots[0], roots[1]);
        apply_drop(&mut s, &[light], Drop::Into(cam));
        assert_eq!(s.scene.parent(light), Some(cam));
        assert!(s.hierarchy.expanded.contains(&cam));
        s.undo_step();
        assert_eq!(s.scene.parent(light), None);
    }

    #[test]
    fn drop_before_reorders_and_into_own_child_is_rejected() {
        let mut s = state();
        let roots = s.scene.roots().to_vec();
        let (cam, light) = (roots[0], roots[1]);
        apply_drop(&mut s, &[light], Drop::Before(cam));
        assert_eq!(s.scene.roots(), &[light, cam]);
        apply_drop(&mut s, &[cam], Drop::Into(light));
        apply_drop(&mut s, &[light], Drop::Into(cam));
        assert_eq!(s.scene.parent(cam), Some(light), "cycle drop ignored");
        assert_eq!(s.scene.parent(light), None);
    }

    /// Hierarchy on the left, Project window on the right, like the default layout.
    fn hierarchy_and_project(s: EditorState) -> egui_kittest::Harness<'static, EditorState> {
        let mut h = egui_kittest::Harness::builder()
            .with_size([900.0, 500.0])
            .build_ui_state(
                |ui, s: &mut EditorState| {
                    let full = ui.max_rect();
                    let (left, right) = full.split_left_right_at_x(300.0);
                    ui.scope_builder(egui::UiBuilder::new().max_rect(left), |ui| show(ui, s));
                    ui.scope_builder(egui::UiBuilder::new().max_rect(right), |ui| {
                        super::super::project::show(ui, s)
                    });
                },
                s,
            );
        h.run_steps(2);
        h
    }

    #[test]
    fn dragging_a_row_onto_the_project_window_creates_a_prefab() {
        use egui::{Event, Modifiers, PointerButton};
        use egui_kittest::kittest::Queryable;

        let root = std::env::temp_dir().join(format!("misty_hier_prefab_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        let mut s = EditorState::new(ProjectBrowser::open(root.clone()));
        let crate_e = s.scene.spawn("Crate", None);
        s.scene.spawn("Lid", Some(crate_e));
        let mut h = hierarchy_and_project(s);

        let from = h.get_by_label("Crate").rect().center();
        let to = egui::pos2(600.0, 250.0);
        let press = |h: &mut egui_kittest::Harness<'_, EditorState>, pos, pressed| {
            h.event(Event::PointerMoved(pos));
            h.event(Event::PointerButton {
                pos,
                button: PointerButton::Primary,
                pressed,
                modifiers: Modifiers::NONE,
            });
            h.step();
        };
        press(&mut h, from, true);
        for i in 1..=6 {
            h.event(Event::PointerMoved(from + (to - from) * (i as f32 / 6.0)));
            h.step();
        }
        press(&mut h, to, false);
        h.run_steps(2);

        let s = h.state_mut();
        for a in std::mem::take(&mut s.actions) {
            crate::commands::apply(s, a);
        }
        assert!(root.join("Assets/Crate.prefab").exists());
        assert_eq!(
            s.scene.prefab_link(crate_e).as_deref(),
            Some("Assets/Crate.prefab")
        );
        assert_eq!(s.scene.parent(crate_e), None, "not re-parented by the drop");
        assert_eq!(
            crate::state::object_icon(&s.scene, crate_e),
            crate::state::PREFAB_ICON
        );
        std::fs::remove_dir_all(root).ok();
    }

    #[test]
    fn copy_and_paste_from_the_row_menu_makes_a_deep_sibling_copy() {
        use egui_kittest::kittest::Queryable;
        let mut s = state();
        let rig = s.scene.spawn("Rig", None);
        s.scene.spawn("Lens", Some(rig));
        let mut h = egui_kittest::Harness::builder()
            .with_size([300.0, 400.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        let apply = |h: &mut egui_kittest::Harness<'_, EditorState>| {
            let s = h.state_mut();
            for a in std::mem::take(&mut s.actions) {
                crate::commands::apply(s, a);
            }
            h.run();
        };
        for item in ["Copy", "Paste"] {
            h.get_by_label("Rig").click_secondary();
            h.run();
            h.get_by_label(item).click();
            h.run();
            apply(&mut h);
        }
        let s = h.state();
        let copy = s.selection.active().unwrap();
        assert_eq!(s.scene.name(copy), "Rig (1)");
        assert_eq!(s.scene.sibling_index(copy), s.scene.sibling_index(rig) + 1);
        assert_eq!(s.scene.name(s.scene.children(copy)[0]), "Lens");
    }

    #[test]
    fn shift_click_selects_a_range_and_alt_click_expands_all() {
        use egui_kittest::kittest::Queryable;
        let mut s = state();
        let a = s.scene.spawn("A", None);
        let b = s.scene.spawn("B", Some(a));
        s.scene.spawn("C", Some(b));
        let mut h = egui_kittest::Harness::builder()
            .with_size([300.0, 400.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        // Alt+click A's foldout arrow opens the whole subtree.
        let row = h.get_by_label("A").rect();
        let arrow = egui::pos2(row.left() + 11.0, row.center().y);
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::ALT));
        for pressed in [true, false] {
            h.event(egui::Event::PointerMoved(arrow));
            h.event(egui::Event::PointerButton {
                pos: arrow,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::ALT,
            });
            h.step();
        }
        h.event(egui::Event::ModifiersChanged(egui::Modifiers::NONE));
        h.run();
        h.get_by_label("C");
        // Click the camera, Shift+click C: everything between is selected.
        h.get_by_label("Main Camera").click();
        h.run();
        h.get_by_label("C").click_modifiers(egui::Modifiers::SHIFT);
        h.run();
        assert_eq!(
            h.state().selection.list().len(),
            5,
            "camera, light, A, B, C"
        );
    }

    /// Two scenes: both headers are listed with their objects, the header menu makes the
    /// second one active, and dragging the camera onto its header moves it there.
    #[test]
    fn scene_headers_list_scenes_and_set_the_active_one() {
        use egui_kittest::kittest::Queryable;
        let mut s = state();
        let mut level = misty_engine::scene::Scene::new("Level");
        level.spawn("Ground", None);
        let (b, _, _) = s
            .scene
            .load_additive("Assets/Level.scene", &level.to_json())
            .unwrap();
        let rows = visible_rows(&s);
        assert_eq!(rows.len(), 5, "two headers, camera, light, ground");
        assert_eq!(rows[3], Row::Scene(b));
        let mut h = egui_kittest::Harness::builder()
            .with_size([300.0, 400.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h.get_by_label("Ground");
        h.get_by_label("Level").click_secondary();
        h.run();
        h.get_by_label("Set Active Scene").click();
        h.run();
        let s = h.state_mut();
        for a in std::mem::take(&mut s.actions) {
            crate::commands::apply(s, a);
        }
        assert_eq!(s.scene.active_scene(), b);
        let cam = s.scene.find_by_name("Main Camera").unwrap();
        apply_drop(s, &[cam], Drop::Scene(b));
        for a in std::mem::take(&mut s.actions) {
            crate::commands::apply(s, a);
        }
        assert_eq!(s.scene.scene_of(cam), b);
        // Dropping between the first scene's rows brings it back.
        let light = s.scene.roots_in(0)[0];
        apply_drop(s, &[cam], Drop::Before(light));
        assert_eq!(s.scene.roots_in(0), [cam, light]);
    }

    #[test]
    fn search_lists_matches_flat() {
        let mut s = state();
        let cam = s.scene.roots()[0];
        let child = s.scene.spawn("Camera Rig", Some(cam));
        s.hierarchy.search = "camera".into();
        let rows = visible_rows(&s);
        assert_eq!(
            rows,
            vec![Row::Scene(0), Row::Object(cam, 0), Row::Object(child, 0)]
        );
        s.hierarchy.search.clear();
        assert_eq!(
            visible_rows(&s).len(),
            3,
            "scene header, camera, light: collapsed parent hides child"
        );
    }
}
