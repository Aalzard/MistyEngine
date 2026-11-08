//! Input Actions window (double-click a `.inputactions` asset, or Window ▸ Input ▸ Input
//! Actions): three columns, Action Maps | Actions (with their bindings) | Properties of the
//! selection. A control scheme filter and Save Asset / Auto-Save sit in the toolbar; the
//! binding properties have a path picker and a Listen button that takes the next key or
//! gamepad input.

mod props;

#[cfg(test)]
mod tests;

use egui::{Align, Layout, Rect, RichText, Sense, Ui, UiBuilder, pos2, vec2};
use misty_engine::input::actions::asset::{self, new_id};
use misty_engine::input::actions::path::display_path;
use misty_engine::input::actions::{
    Action, ActionMap, ActionType, Binding, CompositePart, ControlType, InputActionAsset,
};

use crate::actions::Action as EditorAction;
use crate::layout::Tab;
use crate::state::EditorState;
use crate::theme;
use crate::widgets::panel_toolbar;

/// Icon of Input Actions assets and the window.
pub const ICON: &str = "🎮";
const MAPS_W: f32 = 190.0;
const PROPS_W: f32 = 320.0;
const ROW_H: f32 = 22.0;

/// What is selected in the Actions column.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Selection {
    #[default]
    None,
    Action(usize),
    /// Action, binding, composite part.
    Binding(usize, usize, Option<usize>),
}

/// What the rename field edits.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Rename {
    Map(usize),
    Action(usize),
    Composite(usize, usize),
}

/// Input Actions window state (the working copy; Save Asset writes it).
#[derive(Default)]
pub struct InputActionsWindow {
    /// Asset shown (project-relative).
    pub path: Option<String>,
    pub asset: Option<InputActionAsset>,
    /// As last saved (dirty = differs).
    saved: Option<InputActionAsset>,
    pub auto_save: bool,
    pub map: usize,
    pub selection: Selection,
    /// Only show bindings of this control scheme.
    pub scheme_filter: Option<usize>,
    /// Waiting for a key / gamepad input for the selected binding's path.
    pub listening: bool,
    /// Modifier keys held when Listen started (egui reports them as flags, not keys).
    listen_modifiers: egui::Modifiers,
    renaming: Option<(Rename, String)>,
    /// Control scheme being added or edited: (index or None for new, working copy).
    scheme_edit: Option<(Option<usize>, asset::ControlScheme)>,
    pub status: Option<String>,
}

impl InputActionsWindow {
    /// Edits not saved yet (the toolbar compares this frame's working copy itself).
    #[cfg(test)]
    pub fn dirty(&self) -> bool {
        self.asset != self.saved
    }
}

/// Show an asset in the window and bring it to the front.
pub fn open(s: &mut EditorState, rel: &str) {
    let w = &mut s.input_actions;
    if w.path.as_deref() != Some(rel) || w.asset.is_none() {
        let abs = s.assets.absolute(rel);
        match InputActionAsset::load(&abs) {
            Ok(a) => {
                w.saved = Some(a.clone());
                w.asset = Some(a);
                w.status = None;
            }
            Err(e) => {
                w.asset = None;
                w.saved = None;
                w.status = Some(e);
            }
        }
        w.path = Some(rel.to_owned());
        w.map = 0;
        w.selection = Selection::None;
        w.listening = false;
        w.renaming = None;
        w.scheme_filter = None;
    }
    s.push(EditorAction::FocusTab(Tab::InputActions));
}

/// Select `Map/Action`, `Map/Action/binding` or `Map/Action/binding.part` (indices).
pub fn select(s: &mut EditorState, what: &str) {
    let Some(a) = s.input_actions.asset.as_ref() else {
        return;
    };
    let mut it = what.splitn(3, '/');
    let (Some(map), Some(action)) = (it.next(), it.next()) else {
        return;
    };
    let Some((m, ai)) = a.find_action(&format!("{map}/{action}")) else {
        return;
    };
    let w = &mut s.input_actions;
    w.map = m;
    w.selection = match it.next() {
        None => Selection::Action(ai),
        Some(b) => {
            let (b, p) = b.split_once('.').map_or((b, None), |(b, p)| (b, Some(p)));
            let bi = b.parse().unwrap_or(0);
            Selection::Binding(ai, bi, p.and_then(|p| p.parse().ok()))
        }
    };
}

/// Write the working copy to its file.
pub fn save(s: &mut EditorState) {
    let (Some(rel), Some(a)) = (s.input_actions.path.clone(), s.input_actions.asset.clone()) else {
        return;
    };
    match a.save(&s.assets.absolute(&rel)) {
        Ok(()) => {
            s.input_actions.saved = Some(a);
            s.input_actions.status = None;
        }
        Err(e) => s.input_actions.status = Some(format!("Could not save {rel}: {e}")),
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    if s.input_actions.path.is_none()
        && let Some(first) = s.assets.input_actions.iter().next().cloned()
    {
        open(s, &first);
    }
    let Some(mut a) = s.input_actions.asset.clone() else {
        empty(ui, s);
        return;
    };
    let before = a.clone();
    toolbar(ui, s, &mut a);
    let full = ui.available_rect_before_wrap();
    ui.allocate_rect(full, Sense::hover());
    let maps_w = MAPS_W.min(full.width() * 0.25);
    let props_w = PROPS_W.min(full.width() * 0.4);
    let maps = Rect::from_min_size(full.min, vec2(maps_w, full.height()));
    let props = Rect::from_min_max(pos2(full.right() - props_w, full.top()), full.max);
    let actions = Rect::from_min_max(
        pos2(maps.right() + 1.0, full.top()),
        pos2(props.left() - 1.0, full.bottom()),
    );
    let p = ui.painter();
    p.rect_filled(maps, 0.0, theme::SIDEBAR_BG);
    p.rect_filled(props, 0.0, theme::SIDEBAR_BG);
    for x in [maps.right(), props.left()] {
        p.vline(x, full.y_range(), egui::Stroke::new(1.0, theme::BORDER));
    }
    if s.input_actions.map >= a.maps.len() {
        s.input_actions.map = a.maps.len().saturating_sub(1);
        s.input_actions.selection = Selection::None;
    }
    ui.scope_builder(UiBuilder::new().max_rect(maps.shrink(4.0)), |ui| {
        maps_column(ui, s, &mut a)
    });
    ui.scope_builder(UiBuilder::new().max_rect(actions.shrink(4.0)), |ui| {
        actions_column(ui, s, &mut a)
    });
    ui.scope_builder(UiBuilder::new().max_rect(props.shrink(6.0)), |ui| {
        props::column(ui, s, &mut a)
    });
    if let Some(edit) = s.input_actions.scheme_edit.take() {
        s.input_actions.scheme_edit = props::scheme_dialog(ui.ctx(), &mut a, edit);
    }
    if a != before {
        s.input_actions.asset = Some(a);
        if s.input_actions.auto_save {
            save(s);
        }
    }
}

fn empty(ui: &mut Ui, s: &mut EditorState) {
    ui.vertical_centered(|ui| {
        ui.add_space(40.0);
        ui.label(RichText::new(ICON).size(32.0));
        match &s.input_actions.status {
            Some(e) => {
                ui.colored_label(egui::Color32::LIGHT_RED, e);
            }
            None => {
                ui.label("No Input Actions asset open.");
                ui.weak("Double-click a .inputactions asset, or make one with Assets ▸ Create ▸ Input Actions.");
            }
        }
        if ui.button("Create Input Actions").clicked() {
            s.push(EditorAction::CreateAsset(crate::actions::AssetKind::InputActions));
        }
    });
}

fn toolbar(ui: &mut Ui, s: &mut EditorState, a: &mut InputActionAsset) {
    panel_toolbar(ui, |ui| {
        let w = &mut s.input_actions;
        let shown = match w.scheme_filter.and_then(|i| a.control_schemes.get(i)) {
            Some(c) => c.name.clone(),
            None => "All Control Schemes".to_owned(),
        };
        egui::ComboBox::from_id_salt("ia_scheme")
            .selected_text(shown)
            .width(170.0)
            .show_ui(ui, |ui| {
                if ui
                    .selectable_label(w.scheme_filter.is_none(), "All Control Schemes")
                    .clicked()
                {
                    w.scheme_filter = None;
                }
                for (i, c) in a.control_schemes.iter().enumerate() {
                    if ui
                        .selectable_label(w.scheme_filter == Some(i), &c.name)
                        .clicked()
                    {
                        w.scheme_filter = Some(i);
                    }
                }
                ui.separator();
                if ui.button("Add Control Scheme…").clicked() {
                    w.scheme_edit = Some((
                        None,
                        asset::ControlScheme {
                            name: "New Control Scheme".into(),
                            ..Default::default()
                        },
                    ));
                }
                if let Some(i) = w.scheme_filter {
                    if ui.button("Edit Control Scheme…").clicked() {
                        w.scheme_edit = Some((Some(i), a.control_schemes[i].clone()));
                    }
                    if ui.button("Delete Control Scheme").clicked() {
                        delete_scheme(a, i);
                        w.scheme_filter = None;
                    }
                }
            });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let name = s
                .input_actions
                .path
                .as_deref()
                .map(misty_engine::scene::clip_name)
                .unwrap_or_default();
            let dirty = s.input_actions.saved.as_ref() != Some(&*a);
            ui.checkbox(&mut s.input_actions.auto_save, "Auto-Save");
            let save_btn = ui.add_enabled(dirty, egui::Button::new("Save Asset"));
            if save_btn.clicked() {
                s.input_actions.asset = Some(a.clone());
                save(s);
            }
            ui.label(theme::bold(
                ui.ctx(),
                format!("{name}{}", if dirty { " *" } else { "" }),
            ));
            if let Some(st) = &s.input_actions.status {
                ui.colored_label(egui::Color32::LIGHT_RED, st);
            }
        });
    });
}

fn delete_scheme(a: &mut InputActionAsset, i: usize) {
    let group = a.control_schemes[i].group().to_owned();
    a.control_schemes.remove(i);
    for m in &mut a.maps {
        for act in &mut m.actions {
            for b in &mut act.bindings {
                asset::set_group(&mut b.groups, &group, false);
                for p in &mut b.parts {
                    asset::set_group(&mut p.groups, &group, false);
                }
            }
        }
    }
}

/// Column title with a "+" button on the right; true when "+" was clicked.
fn header(ui: &mut Ui, title: &str, tip: &str) -> egui::Response {
    ui.horizontal(|ui| {
        ui.label(theme::bold(ui.ctx(), title));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.small_button("+").on_hover_text(tip)
        })
        .inner
    })
    .inner
}

/// How a row's text is drawn.
#[derive(Clone, Copy, PartialEq)]
enum Look {
    Plain,
    Dim,
    Bold,
}

/// A selectable list row; returns its response (click, double-click, context menu).
fn row(ui: &mut Ui, selected: bool, indent: f32, text: &str, look: Look) -> egui::Response {
    let (rect, resp) = ui.allocate_exact_size(vec2(ui.available_width(), ROW_H), Sense::click());
    resp.widget_info(|| {
        egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, selected, text)
    });
    if selected {
        ui.painter().rect_filled(rect, 2.0, theme::SELECTION);
    } else if resp.hovered() {
        ui.painter()
            .rect_filled(rect, 2.0, theme::HOVER_BG.gamma_multiply(0.4));
    }
    let font = match look {
        Look::Bold => egui::FontId::new(13.0, theme::bold_family(ui.ctx())),
        _ => egui::FontId::proportional(13.0),
    };
    let color = if look == Look::Dim {
        theme::TEXT_DIM
    } else {
        theme::TEXT
    };
    let galley = ui.painter().layout_no_wrap(text.to_owned(), font, color);
    ui.painter().galley(
        pos2(
            rect.left() + 6.0 + indent,
            rect.center().y - galley.size().y / 2.0,
        ),
        galley,
        theme::TEXT,
    );
    resp
}

/// Inline rename field; returns the new name when confirmed.
fn rename_field(ui: &mut Ui, text: &mut String) -> Option<Option<String>> {
    let r = ui.add(egui::TextEdit::singleline(text).desired_width(ui.available_width()));
    r.request_focus();
    if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
        return Some(None);
    }
    if r.lost_focus() || ui.input(|i| i.key_pressed(egui::Key::Enter)) {
        let t = text.trim().to_owned();
        return Some((!t.is_empty()).then_some(t));
    }
    None
}

/// A name not used by `taken` ("New action", "New action 1", …).
fn unique(base: &str, taken: &[&str]) -> String {
    (0..)
        .map(|n| {
            if n == 0 {
                base.to_owned()
            } else {
                format!("{base} {n}")
            }
        })
        .find(|n| !taken.iter().any(|t| t.eq_ignore_ascii_case(n)))
        .expect("a free name")
}

fn maps_column(ui: &mut Ui, s: &mut EditorState, a: &mut InputActionAsset) {
    if header(ui, "Action Maps", "Add Action Map").clicked() {
        let taken: Vec<&str> = a.maps.iter().map(|m| m.name.as_str()).collect();
        let name = unique("New action map", &taken);
        a.maps.push(ActionMap {
            name: name.clone(),
            id: new_id(),
            actions: vec![new_action(&[])],
        });
        let w = &mut s.input_actions;
        w.map = a.maps.len() - 1;
        w.selection = Selection::Action(0);
        w.renaming = Some((Rename::Map(w.map), name));
    }
    ui.separator();
    let mut delete = None;
    let mut duplicate = None;
    egui::ScrollArea::vertical()
        .id_salt("ia_maps")
        .auto_shrink(false)
        .show(ui, |ui| {
            for m in 0..a.maps.len() {
                let w = &mut s.input_actions;
                if let Some((Rename::Map(i), text)) = &mut w.renaming
                    && *i == m
                {
                    if let Some(done) = rename_field(ui, text) {
                        if let Some(name) = done {
                            a.maps[m].name = name;
                        }
                        w.renaming = None;
                    }
                    continue;
                }
                let r = row(ui, w.map == m, 0.0, &a.maps[m].name, Look::Plain);
                if r.clicked() && w.map != m {
                    w.map = m;
                    w.selection = Selection::None;
                    w.listening = false;
                }
                if r.double_clicked() {
                    w.renaming = Some((Rename::Map(m), a.maps[m].name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        w.renaming = Some((Rename::Map(m), a.maps[m].name.clone()));
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        duplicate = Some(m);
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        delete = Some(m);
                        ui.close();
                    }
                });
            }
        });
    if let Some(m) = duplicate {
        let mut copy = a.maps[m].clone();
        let taken: Vec<&str> = a.maps.iter().map(|m| m.name.as_str()).collect();
        copy.name = unique(&format!("{} Copy", copy.name), &taken);
        fresh_ids_map(&mut copy);
        a.maps.insert(m + 1, copy);
        s.input_actions.map = m + 1;
        s.input_actions.selection = Selection::None;
    }
    if let Some(m) = delete {
        a.maps.remove(m);
        s.input_actions.map = s.input_actions.map.min(a.maps.len().saturating_sub(1));
        s.input_actions.selection = Selection::None;
    }
}

fn new_action(existing: &[Action]) -> Action {
    let taken: Vec<&str> = existing.iter().map(|x| x.name.as_str()).collect();
    let mut act = Action::new(
        &unique("New action", &taken),
        ActionType::Button,
        ControlType::Button,
    );
    act.id = new_id();
    act.bindings.push(Binding {
        id: new_id(),
        ..Default::default()
    });
    act
}

fn fresh_ids_map(m: &mut ActionMap) {
    m.id = new_id();
    for a in &mut m.actions {
        fresh_ids_action(a);
    }
}

fn fresh_ids_action(a: &mut Action) {
    a.id = new_id();
    for b in &mut a.bindings {
        b.id = new_id();
        for p in &mut b.parts {
            p.id = new_id();
        }
    }
}

/// A new composite binding of `kind` with empty parts (in the filtered scheme, if any).
pub fn new_composite(kind: &str, groups: &str) -> Binding {
    use misty_engine::input::actions::composites::KINDS;
    let (parts, params, label) = KINDS
        .iter()
        .find(|k| k.0 == kind)
        .map_or((&[][..], "", kind), |k| (k.2, k.3, k.1));
    let name = match kind {
        "2DVector" => "2D Vector",
        "1DAxis" => "1D Axis",
        _ => label,
    };
    Binding {
        name: name.into(),
        composite: kind.into(),
        parameters: params.into(),
        id: new_id(),
        parts: parts
            .iter()
            .map(|p| CompositePart {
                name: (*p).into(),
                id: new_id(),
                groups: groups.into(),
                ..Default::default()
            })
            .collect(),
        ..Default::default()
    }
}

/// Is this binding shown under the scheme filter?
fn visible(b: &Binding, group: Option<&str>) -> bool {
    match group {
        None => true,
        Some(g) => b.in_group(g) || (!b.is_composite() && b.groups.trim().is_empty()),
    }
}

fn actions_column(ui: &mut Ui, s: &mut EditorState, a: &mut InputActionAsset) {
    let m = s.input_actions.map;
    let group = s
        .input_actions
        .scheme_filter
        .and_then(|i| a.control_schemes.get(i))
        .map(|c| c.group().to_owned());
    let add = header(ui, "Actions", "Add Action");
    let Some(map) = a.maps.get_mut(m) else {
        ui.separator();
        ui.weak("Add an action map first.");
        return;
    };
    if add.clicked() {
        let act = new_action(&map.actions);
        let name = act.name.clone();
        map.actions.push(act);
        let i = map.actions.len() - 1;
        s.input_actions.selection = Selection::Action(i);
        s.input_actions.renaming = Some((Rename::Action(i), name));
    }
    ui.separator();
    let groups_for_new = group.clone().unwrap_or_default();
    let mut op: Option<ActionOp> = None;
    egui::ScrollArea::vertical().id_salt("ia_actions").auto_shrink(false).show(ui, |ui| {
        for (ai, act) in map.actions.iter_mut().enumerate() {
            let w = &mut s.input_actions;
            if let Some((Rename::Action(i), text)) = &mut w.renaming
                && *i == ai
            {
                if let Some(done) = rename_field(ui, text) {
                    if let Some(name) = done {
                        act.name = name;
                    }
                    w.renaming = None;
                }
            } else {
                let label = format!("▾ {}", act.name);
                let r = ui
                    .horizontal(|ui| {
                        let full = ui.available_width();
                        let r = ui
                            .allocate_ui(vec2(full - 24.0, ROW_H), |ui| {
                                row(ui, w.selection == Selection::Action(ai), 0.0, &label, Look::Bold)
                            })
                            .inner;
                        ui.menu_button("+", |ui| {
                            if ui.button("Add Binding").clicked() {
                                op = Some(ActionOp::AddBinding(ai, None));
                                ui.close();
                            }
                            for (kind, label, _, _) in misty_engine::input::actions::composites::KINDS {
                                if ui.button(format!("Add {label}")).clicked() {
                                    op = Some(ActionOp::AddBinding(ai, Some(kind)));
                                    ui.close();
                                }
                            }
                        });
                        r
                    })
                    .inner;
                if r.clicked() {
                    w.selection = Selection::Action(ai);
                    w.listening = false;
                }
                if r.double_clicked() {
                    w.renaming = Some((Rename::Action(ai), act.name.clone()));
                }
                r.context_menu(|ui| {
                    if ui.button("Rename").clicked() {
                        w.renaming = Some((Rename::Action(ai), act.name.clone()));
                        ui.close();
                    }
                    if ui.button("Duplicate").clicked() {
                        op = Some(ActionOp::Duplicate(ai));
                        ui.close();
                    }
                    if ui.button("Delete").clicked() {
                        op = Some(ActionOp::Delete(ai));
                        ui.close();
                    }
                });
            }
            for (bi, b) in act.bindings.iter_mut().enumerate() {
                if !visible(b, group.as_deref()) {
                    continue;
                }
                let w = &mut s.input_actions;
                if b.is_composite() {
                    let renaming = matches!(&w.renaming, Some((Rename::Composite(x, y), _)) if (*x, *y) == (ai, bi));
                    if renaming {
                        if let Some((_, text)) = &mut w.renaming
                            && let Some(done) = rename_field(ui, text)
                        {
                            if let Some(name) = done {
                                b.name = name;
                            }
                            w.renaming = None;
                        }
                    } else {
                        let sel = w.selection == Selection::Binding(ai, bi, None);
                        let r = row(ui, sel, 16.0, &composite_label(b), Look::Plain);
                        binding_row_input(w, &r, ai, bi, None, &mut op, Some(b.name.clone()));
                    }
                    for (pi, part) in b.parts.iter().enumerate() {
                        let text = format!("{}: {}", title_case(&part.name), display_path(&part.path));
                        let look = if part.path.is_empty() { Look::Dim } else { Look::Plain };
                        let sel = w.selection == Selection::Binding(ai, bi, Some(pi));
                        let r = row(ui, sel, 32.0, &text, look);
                        binding_row_input(w, &r, ai, bi, Some(pi), &mut op, None);
                    }
                } else {
                    let text = display_path(&b.path);
                    let look = if b.path.is_empty() { Look::Dim } else { Look::Plain };
                    let sel = w.selection == Selection::Binding(ai, bi, None);
                    let r = row(ui, sel, 16.0, &text, look);
                    binding_row_input(w, &r, ai, bi, None, &mut op, None);
                }
            }
        }
    });
    let Some(op) = op else { return };
    let w = &mut s.input_actions;
    match op {
        ActionOp::AddBinding(ai, kind) => {
            let b = match kind {
                None => Binding {
                    id: new_id(),
                    groups: groups_for_new.clone(),
                    ..Default::default()
                },
                Some(k) => new_composite(k, &groups_for_new),
            };
            map.actions[ai].bindings.push(b);
            let bi = map.actions[ai].bindings.len() - 1;
            w.selection = Selection::Binding(ai, bi, kind.map(|_| 0));
        }
        ActionOp::Duplicate(ai) => {
            let mut copy = map.actions[ai].clone();
            let taken: Vec<&str> = map.actions.iter().map(|x| x.name.as_str()).collect();
            copy.name = unique(&format!("{} Copy", copy.name), &taken);
            fresh_ids_action(&mut copy);
            map.actions.insert(ai + 1, copy);
            w.selection = Selection::Action(ai + 1);
        }
        ActionOp::Delete(ai) => {
            map.actions.remove(ai);
            w.selection = Selection::None;
        }
        ActionOp::DeleteBinding(ai, bi) => {
            map.actions[ai].bindings.remove(bi);
            w.selection = Selection::Action(ai);
        }
        ActionOp::DuplicateBinding(ai, bi) => {
            let mut copy = map.actions[ai].bindings[bi].clone();
            copy.id = new_id();
            for p in &mut copy.parts {
                p.id = new_id();
            }
            map.actions[ai].bindings.insert(bi + 1, copy);
            w.selection = Selection::Binding(ai, bi + 1, None);
        }
    }
}

enum ActionOp {
    AddBinding(usize, Option<&'static str>),
    Duplicate(usize),
    Delete(usize),
    DuplicateBinding(usize, usize),
    DeleteBinding(usize, usize),
}

fn binding_row_input(
    w: &mut InputActionsWindow,
    r: &egui::Response,
    ai: usize,
    bi: usize,
    part: Option<usize>,
    op: &mut Option<ActionOp>,
    composite_name: Option<String>,
) {
    if r.clicked() {
        w.selection = Selection::Binding(ai, bi, part);
        w.listening = false;
    }
    if let Some(name) = &composite_name
        && r.double_clicked()
    {
        w.renaming = Some((Rename::Composite(ai, bi), name.clone()));
    }
    if part.is_some() {
        return;
    }
    r.context_menu(|ui| {
        if let Some(name) = &composite_name
            && ui.button("Rename").clicked()
        {
            w.renaming = Some((Rename::Composite(ai, bi), name.clone()));
            ui.close();
        }
        if ui.button("Duplicate").clicked() {
            *op = Some(ActionOp::DuplicateBinding(ai, bi));
            ui.close();
        }
        if ui.button("Delete").clicked() {
            *op = Some(ActionOp::DeleteBinding(ai, bi));
            ui.close();
        }
    });
}

fn composite_label(b: &Binding) -> String {
    if b.name.is_empty() {
        b.composite.clone()
    } else {
        b.name.clone()
    }
}

/// "up" → "Up", "modifier1" → "Modifier1".
pub fn title_case(s: &str) -> String {
    let mut c = s.chars();
    match c.next() {
        Some(f) => f.to_uppercase().chain(c).collect(),
        None => String::new(),
    }
}
