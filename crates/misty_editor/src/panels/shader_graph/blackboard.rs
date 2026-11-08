//! The Blackboard: the graph's exposed properties and keywords. "+" adds a property (Float,
//! Range, Color, Vector, Texture2D, Boolean) or a keyword (Boolean, Enum); drag a row onto the
//! canvas for a Property or Keyword node; select a row to edit its name, reference, default and
//! options (an Enum keyword's entries). In a Sub Graph the properties are its inputs.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, UiBuilder, vec2};
use misty_engine::shader_graph::{Keyword, KeywordKind, PropertyType, TextureFallback};

use super::canvas::{KeywordPayload, PropertyPayload, type_color};
use super::{card, interacting};
use crate::state::EditorState;
use crate::theme;

#[derive(Default)]
pub(crate) struct BlackboardState {
    /// Property shown in the settings below the list.
    pub selected: Option<String>,
    /// Reference name being typed (property, text).
    reference: Option<(String, String)>,
    /// Why the last reference rename failed.
    error: Option<String>,
}

/// Height the Blackboard card wants: the list, plus the settings of a selected property.
pub fn height(w: &super::ShaderGraphWindow) -> f32 {
    let rows = (w.graph.properties.len() + w.graph.keywords.len()).max(1) as f32;
    let heading = if w.graph.keywords.is_empty() {
        0.0
    } else {
        22.0
    };
    let settings = match w.blackboard.selected.as_deref() {
        Some(n) => match w.graph.keyword(n) {
            Some(k) => 150.0 + k.count() as f32 * 26.0,
            None => 230.0,
        },
        None => 0.0,
    };
    44.0 + rows * 24.0 + heading + settings
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.interact(
        rect,
        ui.id().with("sg_blackboard_bg"),
        Sense::click_and_drag(),
    );
    let inner = card(ui, rect, "Blackboard");
    // "+" menu in the title row.
    let plus = Rect::from_min_size(
        egui::pos2(rect.right() - 30.0, rect.top() + 4.0),
        vec2(22.0, 20.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(plus), |ui| {
        ui.menu_button("+", |ui| {
            for (label, ty) in PropertyType::MENU {
                if ui.button(label).clicked() {
                    let mut name = String::new();
                    s.shader_graph.edit("Add Property", false, |g| {
                        name = g.add_property(label, ty);
                    });
                    s.shader_graph.blackboard.selected = Some(name);
                    ui.close();
                }
            }
            if s.shader_graph.graph.is_sub_graph() {
                return;
            }
            ui.separator();
            for (label, kind) in [
                ("Boolean Keyword", KeywordKind::Boolean),
                (
                    "Enum Keyword",
                    KeywordKind::Enum {
                        entries: vec!["A".into(), "B".into(), "C".into()],
                    },
                ),
            ] {
                if ui.button(label).clicked() {
                    let mut name = String::new();
                    let base = if kind == KeywordKind::Boolean {
                        "Boolean"
                    } else {
                        "Enum"
                    };
                    s.shader_graph.edit("Add Keyword", false, |g| {
                        name = g.add_keyword(base, kind.clone());
                    });
                    s.shader_graph.blackboard.selected = Some(name);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Add a property or keyword");
    });
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        let props = s.shader_graph.graph.properties.clone();
        let keywords = s.shader_graph.graph.keywords.clone();
        let settings_h = match s.shader_graph.blackboard.selected.as_deref() {
            Some(n) => match s.shader_graph.graph.keyword(n) {
                Some(k) => 140.0 + k.count() as f32 * 26.0,
                None => 220.0,
            },
            None => 0.0,
        };
        let heading = if keywords.is_empty() { 0.0 } else { 22.0 };
        let list_h = ((props.len() + keywords.len()) as f32 * 24.0 + 4.0 + heading)
            .min(inner.height() - settings_h)
            .max(28.0);
        egui::ScrollArea::vertical()
            .id_salt("sg_bb_list")
            .max_height(list_h)
            .show(ui, |ui| {
                if props.is_empty() && keywords.is_empty() {
                    ui.label(RichText::new("No properties yet: use +").color(theme::TEXT_DIM));
                }
                for p in &props {
                    row(ui, s, p);
                }
                if !keywords.is_empty() {
                    ui.label(
                        RichText::new("Keywords")
                            .font(FontId::proportional(11.0))
                            .color(theme::TEXT_DIM),
                    );
                }
                for k in &keywords {
                    keyword_row(ui, s, k);
                }
            });
        let Some(name) = s.shader_graph.blackboard.selected.clone() else {
            return;
        };
        let is_keyword = s.shader_graph.graph.keyword(&name).is_some();
        if s.shader_graph.graph.property(&name).is_none() && !is_keyword {
            s.shader_graph.blackboard.selected = None;
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("sg_bb_settings")
            .show(ui, |ui| {
                if is_keyword {
                    keyword_settings(ui, s, &name);
                } else {
                    settings(ui, s, &name);
                }
            });
    });
}

fn keyword_row(ui: &mut Ui, s: &mut EditorState, k: &Keyword) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &k.display));
    let selected = s.shader_graph.blackboard.selected.as_deref() == Some(k.name.as_str());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 5.0, theme::SELECTION);
    } else if resp.hovered() {
        painter.rect_filled(rect, 5.0, Color32::from_white_alpha(10));
    }
    let pill = Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(rect.width() - 8.0, 16.0));
    painter.rect_filled(
        Rect::from_center_size(pill.left_center() + vec2(6.0, 0.0), vec2(8.0, 8.0)),
        1.5,
        Color32::from_rgb(200, 170, 110),
    );
    painter.text(
        pill.left_center() + vec2(16.0, 0.0),
        Align2::LEFT_CENTER,
        &k.display,
        FontId::proportional(12.5),
        theme::TEXT,
    );
    let kind = match k.kind {
        KeywordKind::Boolean => "Boolean",
        KeywordKind::Enum { .. } => "Enum",
    };
    painter.text(
        pill.right_center(),
        Align2::RIGHT_CENTER,
        kind,
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    if resp.clicked() || resp.drag_started() {
        let w = &mut s.shader_graph;
        w.blackboard.selected = Some(k.name.clone());
        w.blackboard.reference = None;
        w.blackboard.error = None;
        w.selected.clear();
    }
    resp.dnd_set_drag_payload(KeywordPayload(k.name.clone()));
    let name = k.name.clone();
    resp.context_menu(|ui| {
        if ui.button("Delete").clicked() {
            s.shader_graph
                .edit("Delete Keyword", false, |g| g.remove_keyword(&name));
            ui.close();
        }
    });
}

/// A keyword's name, reference, default and (Enum) entries.
fn keyword_settings(ui: &mut Ui, s: &mut EditorState, name: &str) {
    let busy = interacting(ui.ctx());
    let Some(mut k) = s.shader_graph.graph.keyword(name).cloned() else {
        return;
    };
    let before = k.clone();
    egui::Grid::new("sg_bb_kw_grid")
        .num_columns(2)
        .spacing([6.0, 5.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut k.display).desired_width(120.0));
            ui.end_row();
            ui.label("Reference");
            let bb = &mut s.shader_graph.blackboard;
            let text = match &mut bb.reference {
                Some((n, t)) if n == name => t,
                _ => {
                    bb.reference = Some((name.to_owned(), name.to_owned()));
                    &mut bb.reference.as_mut().expect("set").1
                }
            };
            let r = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(120.0)
                    .font(egui::TextStyle::Monospace),
            );
            let commit = r.lost_focus() && text != name;
            let typed = text.clone();
            ui.end_row();
            if commit {
                let mut result = Ok(());
                let old = name.to_owned();
                s.shader_graph.edit("Rename Keyword", false, |g| {
                    result = g.rename_keyword(&old, &typed);
                });
                let bb = &mut s.shader_graph.blackboard;
                match result {
                    Ok(()) => {
                        bb.selected = Some(typed.clone());
                        bb.reference = Some((typed.clone(), typed));
                        bb.error = None;
                    }
                    Err(e) => {
                        bb.reference = None;
                        bb.error = Some(e);
                    }
                }
                return;
            }
            ui.label("Default");
            let options = k.options();
            egui::ComboBox::from_id_salt("sg_bb_kw_default")
                .selected_text(options.get(k.default as usize).cloned().unwrap_or_default())
                .show_ui(ui, |ui| {
                    for (i, o) in options.iter().enumerate() {
                        ui.selectable_value(&mut k.default, i as u32, o);
                    }
                });
            ui.end_row();
        });
    if let KeywordKind::Enum { entries } = &mut k.kind {
        ui.label(RichText::new("Entries").color(theme::TEXT_DIM));
        let mut remove = None;
        for (i, e) in entries.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.add(egui::TextEdit::singleline(e).desired_width(120.0));
                if ui.small_button("✕").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove
            && entries.len() > 1
        {
            entries.remove(i);
        }
        if ui.button("+ Entry").clicked() {
            let mut n = entries.len() + 1;
            while entries.iter().any(|e| *e == format!("Entry{n}")) {
                n += 1;
            }
            entries.push(format!("Entry{n}"));
        }
        k.default = k.default.min(entries.len().saturating_sub(1) as u32);
    }
    if let Some(e) = &s.shader_graph.blackboard.error {
        ui.label(RichText::new(e).color(super::ERROR).small());
    }
    if k != before && s.shader_graph.graph.keyword(name).is_some() {
        let n = name.to_owned();
        let renamed_ports = before.ports() != k.ports();
        s.shader_graph.edit("Edit Keyword", busy, |g| {
            if let Some(q) = g.keywords.iter_mut().find(|q| q.name == n) {
                *q = k.clone();
            }
            if renamed_ports {
                // Keyword nodes lose connections into entries that went away.
                let ports: Vec<&str> = k.ports().iter().map(|p| p.name).collect();
                let ids: Vec<u32> = g
                    .nodes
                    .iter()
                    .filter(|x| matches!(&x.kind, misty_engine::shader_graph::NodeKind::Keyword { name } if *name == n))
                    .map(|x| x.id)
                    .collect();
                g.connections
                    .retain(|c| !ids.contains(&c.to) || ports.contains(&c.input.as_str()));
            }
        });
    }
    ui.add_space(4.0);
    if ui.button("Delete Keyword").clicked() {
        let n = name.to_owned();
        s.shader_graph
            .edit("Delete Keyword", false, |g| g.remove_keyword(&n));
        s.shader_graph.blackboard.selected = None;
    }
}

fn row(ui: &mut Ui, s: &mut EditorState, p: &misty_engine::shader_graph::GraphProperty) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &p.display));
    let selected = s.shader_graph.blackboard.selected.as_deref() == Some(p.name.as_str());
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 5.0, theme::SELECTION);
    } else if resp.hovered() {
        painter.rect_filled(rect, 5.0, Color32::from_white_alpha(10));
    }
    let pill = Rect::from_min_size(rect.min + vec2(4.0, 3.0), vec2(rect.width() - 8.0, 16.0));
    painter.circle_filled(
        pill.left_center() + vec2(6.0, 0.0),
        4.0,
        type_color(p.ty.value_type()),
    );
    painter.text(
        pill.left_center() + vec2(16.0, 0.0),
        Align2::LEFT_CENTER,
        &p.display,
        FontId::proportional(12.5),
        theme::TEXT,
    );
    painter.text(
        pill.right_center(),
        Align2::RIGHT_CENTER,
        p.ty.label(),
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    if resp.clicked() || resp.drag_started() {
        let w = &mut s.shader_graph;
        w.blackboard.selected = Some(p.name.clone());
        w.blackboard.reference = None;
        w.blackboard.error = None;
        w.selected.clear();
    }
    resp.dnd_set_drag_payload(PropertyPayload(p.name.clone()));
    let name = p.name.clone();
    resp.context_menu(|ui| {
        if ui.button("Delete").clicked() {
            s.shader_graph
                .edit("Delete Property", false, |g| g.remove_property(&name));
            ui.close();
        }
    });
}

fn settings(ui: &mut Ui, s: &mut EditorState, name: &str) {
    let busy = interacting(ui.ctx());
    let Some(mut p) = s.shader_graph.graph.property(name).cloned() else {
        return;
    };
    let before = p.clone();
    let textures = s.assets.textures.clone();
    egui::Grid::new("sg_bb_grid")
        .num_columns(2)
        .spacing([6.0, 5.0])
        .show(ui, |ui| {
            ui.label("Name");
            ui.add(egui::TextEdit::singleline(&mut p.display).desired_width(120.0));
            ui.end_row();
            ui.label("Reference");
            let bb = &mut s.shader_graph.blackboard;
            let text = match &mut bb.reference {
                Some((n, t)) if n == name => t,
                _ => {
                    bb.reference = Some((name.to_owned(), name.to_owned()));
                    &mut bb.reference.as_mut().expect("set").1
                }
            };
            let r = ui.add(
                egui::TextEdit::singleline(text)
                    .desired_width(120.0)
                    .font(egui::TextStyle::Monospace),
            );
            let commit = r.lost_focus() && text != name;
            let typed = text.clone();
            ui.end_row();
            if commit {
                let mut result = Ok(());
                let old = name.to_owned();
                s.shader_graph.edit("Rename Property", false, |g| {
                    result = g.rename_property(&old, &typed);
                });
                let bb = &mut s.shader_graph.blackboard;
                match result {
                    Ok(()) => {
                        bb.selected = Some(typed.clone());
                        bb.reference = Some((typed.clone(), typed));
                        bb.error = None;
                    }
                    Err(e) => {
                        bb.reference = None;
                        bb.error = Some(e);
                    }
                }
                return;
            }
            ui.label("Default");
            match p.ty {
                PropertyType::Float => {
                    ui.add(egui::DragValue::new(&mut p.value[0]).speed(0.01));
                }
                PropertyType::Range { min, max } => {
                    ui.add(egui::Slider::new(&mut p.value[0], min..=max));
                }
                PropertyType::Boolean => {
                    let mut on = p.value[0] > 0.5;
                    if ui.checkbox(&mut on, "").changed() {
                        p.value[0] = if on { 1.0 } else { 0.0 };
                    }
                }
                PropertyType::Color => {
                    let mut c = p.value.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                    if ui.color_edit_button_srgba_unmultiplied(&mut c).changed() {
                        p.value = c.map(|v| v as f32 / 255.0);
                    }
                }
                PropertyType::Vector => {
                    ui.horizontal(|ui| {
                        for c in p.value.iter_mut() {
                            ui.add(egui::DragValue::new(c).speed(0.01).max_decimals(3));
                        }
                    });
                }
                PropertyType::Texture2D { .. } => {
                    let current = p
                        .texture
                        .clone()
                        .map_or("None".into(), |t| misty_engine::scene::clip_name(&t));
                    egui::ComboBox::from_id_salt("sg_bb_tex")
                        .selected_text(current)
                        .show_ui(ui, |ui| {
                            ui.selectable_value(&mut p.texture, None, "None");
                            for t in &textures {
                                ui.selectable_value(
                                    &mut p.texture,
                                    Some(t.clone()),
                                    misty_engine::scene::clip_name(t),
                                );
                            }
                        });
                }
            }
            ui.end_row();
            if let PropertyType::Range { min, max } = &mut p.ty {
                ui.label("Min");
                ui.add(egui::DragValue::new(min).speed(0.01));
                ui.end_row();
                ui.label("Max");
                ui.add(egui::DragValue::new(max).speed(0.01));
                ui.end_row();
            }
            if let PropertyType::Texture2D { fallback } = &mut p.ty {
                ui.label("Fallback");
                egui::ComboBox::from_id_salt("sg_bb_fallback")
                    .selected_text(format!("{fallback:?}"))
                    .show_ui(ui, |ui| {
                        for f in TextureFallback::ALL {
                            ui.selectable_value(fallback, f, format!("{f:?}"));
                        }
                    });
                ui.end_row();
            }
        });
    if let Some(e) = &s.shader_graph.blackboard.error {
        ui.label(RichText::new(e).color(super::ERROR).small());
    }
    if p != before && s.shader_graph.graph.property(name).is_some() {
        let n = name.to_owned();
        s.shader_graph.edit("Edit Property", busy, |g| {
            if let Some(q) = g.properties.iter_mut().find(|q| q.name == n) {
                *q = p;
            }
        });
    }
    ui.add_space(4.0);
    if ui.button("Delete Property").clicked() {
        let n = name.to_owned();
        s.shader_graph
            .edit("Delete Property", false, |g| g.remove_property(&n));
        s.shader_graph.blackboard.selected = None;
    }
}
