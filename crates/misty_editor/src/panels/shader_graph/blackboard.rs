//! The Blackboard: the graph's exposed properties. "+" adds one (Float, Range, Color, Vector,
//! Texture2D, Boolean); drag a row onto the canvas for a Property node; select a row to edit its
//! name, reference, default and options.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, UiBuilder, vec2};
use misty_engine::shader_graph::{PropertyType, TextureFallback};

use super::canvas::{PropertyPayload, type_color};
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
    let rows = w.graph.properties.len().max(1) as f32;
    let settings = if w.blackboard.selected.is_some() {
        230.0
    } else {
        0.0
    };
    44.0 + rows * 24.0 + settings
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
        })
        .response
        .on_hover_text("Add a property");
    });
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        let props = s.shader_graph.graph.properties.clone();
        let settings_h = if s.shader_graph.blackboard.selected.is_some() {
            220.0
        } else {
            0.0
        };
        let list_h = (props.len() as f32 * 24.0 + 4.0)
            .min(inner.height() - settings_h)
            .max(28.0);
        egui::ScrollArea::vertical()
            .id_salt("sg_bb_list")
            .max_height(list_h)
            .show(ui, |ui| {
                if props.is_empty() {
                    ui.label(RichText::new("No properties yet: use +").color(theme::TEXT_DIM));
                }
                for p in &props {
                    row(ui, s, p);
                }
            });
        let Some(name) = s.shader_graph.blackboard.selected.clone() else {
            return;
        };
        if s.shader_graph.graph.property(&name).is_none() {
            s.shader_graph.blackboard.selected = None;
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("sg_bb_settings")
            .show(ui, |ui| settings(ui, s, &name));
    });
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
