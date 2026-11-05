//! The Blackboard: exposed properties (Float, Vector3, Color, Texture, Gradient) every Visual
//! Effect using the graph can override. "+" adds one; drag a row onto the canvas for a Property
//! node; select a row to rename it, edit its default and tooltip.

use egui::{Align2, Color32, FontId, Rect, RichText, Sense, Ui, UiBuilder, vec2};
use misty_engine::vfx::{PropertyValue, ValueType};

use super::canvas::{PropertyPayload, type_color};
use super::interacting;
use crate::panels::shader_graph::card;
use crate::state::EditorState;
use crate::theme;

#[derive(Default)]
pub(crate) struct BlackboardState {
    pub selected: Option<String>,
    /// Name being typed (property, text).
    rename: Option<(String, String)>,
    error: Option<String>,
}

pub fn height(w: &super::EffectGraphWindow) -> f32 {
    let rows = w.graph.properties.len().max(1) as f32;
    let settings = match w
        .blackboard
        .selected
        .as_deref()
        .and_then(|n| w.graph.property(n))
    {
        Some(p) if matches!(p.value, PropertyValue::Gradient(_)) => 330.0,
        Some(_) => 170.0,
        None => 0.0,
    };
    44.0 + rows * 24.0 + settings
}

/// Edit a property value (Blackboard defaults and Inspector overrides share it).
pub(crate) fn value_ui(
    ui: &mut Ui,
    salt: &str,
    v: &mut PropertyValue,
    textures: &[String],
) -> bool {
    match v {
        PropertyValue::Float(f) => ui.add(egui::DragValue::new(f).speed(0.01)).changed(),
        PropertyValue::Vector3(a) => {
            let mut ch = false;
            ui.horizontal(|ui| {
                for c in a.iter_mut() {
                    ch |= ui
                        .add(egui::DragValue::new(c).speed(0.01).max_decimals(3))
                        .changed();
                }
            });
            ch
        }
        PropertyValue::Color(c) => {
            let mut ch = false;
            ui.horizontal(|ui| {
                let k = c[..3].iter().cloned().fold(1.0, f32::max);
                let mut bytes = [
                    (c[0] / k * 255.0).round() as u8,
                    (c[1] / k * 255.0).round() as u8,
                    (c[2] / k * 255.0).round() as u8,
                    (c[3].clamp(0.0, 1.0) * 255.0).round() as u8,
                ];
                let mut intensity = k;
                let a = ui
                    .color_edit_button_srgba_unmultiplied(&mut bytes)
                    .changed();
                let b = ui
                    .add(
                        egui::DragValue::new(&mut intensity)
                            .speed(0.02)
                            .range(1.0..=64.0)
                            .prefix("× "),
                    )
                    .on_hover_text("HDR intensity (above 1 glows)")
                    .changed();
                if a || b {
                    for i in 0..3 {
                        c[i] = bytes[i] as f32 / 255.0 * intensity;
                    }
                    c[3] = bytes[3] as f32 / 255.0;
                    ch = true;
                }
            });
            ch
        }
        PropertyValue::Texture(t) => {
            crate::panels::inspector::slots::texture_slot(ui, salt, t, textures)
        }
        PropertyValue::Gradient(g) => crate::panels::inspector::particles::gradient(ui, salt, g),
    }
}

fn value_type_color(v: &PropertyValue) -> Color32 {
    match v.value_type() {
        Some(t) => type_color(t),
        None => type_color(ValueType::Color).gamma_multiply(0.5),
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.interact(
        rect,
        ui.id().with("vfx_blackboard_bg"),
        Sense::click_and_drag(),
    );
    let inner = card(ui, rect, "Blackboard");
    let plus = Rect::from_min_size(
        egui::pos2(rect.right() - 30.0, rect.top() + 4.0),
        vec2(22.0, 20.0),
    );
    ui.scope_builder(UiBuilder::new().max_rect(plus), |ui| {
        ui.menu_button("+", |ui| {
            for (label, make) in PropertyValue::MENU {
                if ui.button(label).clicked() {
                    let mut name = String::new();
                    s.effect_graph.edit("Add Property", false, |g| {
                        name = g.add_property(label, make());
                    });
                    s.effect_graph.blackboard.selected = Some(name);
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text("Add an exposed property");
    });
    ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
        let props = s.effect_graph.graph.properties.clone();
        let selected = s.effect_graph.blackboard.selected.clone();
        let settings_h = if selected.is_some() { 160.0 } else { 0.0 };
        let list_h = (props.len() as f32 * 24.0 + 4.0)
            .min(inner.height() - settings_h)
            .max(28.0);
        egui::ScrollArea::vertical()
            .id_salt("vfx_bb_list")
            .max_height(list_h)
            .show(ui, |ui| {
                if props.is_empty() {
                    ui.label(RichText::new("No properties yet: use +").color(theme::TEXT_DIM));
                }
                for p in &props {
                    row(ui, s, p);
                }
            });
        let Some(name) = selected else {
            return;
        };
        if s.effect_graph.graph.property(&name).is_none() {
            s.effect_graph.blackboard.selected = None;
            return;
        }
        ui.separator();
        egui::ScrollArea::vertical()
            .id_salt("vfx_bb_settings")
            .show(ui, |ui| settings(ui, s, &name));
    });
}

fn row(ui: &mut Ui, s: &mut EditorState, p: &misty_engine::vfx::EffectProperty) {
    let (rect, resp) =
        ui.allocate_exact_size(vec2(ui.available_width(), 22.0), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &p.name));
    let selected = s.effect_graph.blackboard.selected.as_deref() == Some(p.name.as_str());
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
        value_type_color(&p.value),
    );
    painter.text(
        pill.left_center() + vec2(16.0, 0.0),
        Align2::LEFT_CENTER,
        &p.name,
        FontId::proportional(12.5),
        theme::TEXT,
    );
    painter.text(
        pill.right_center(),
        Align2::RIGHT_CENTER,
        p.value.type_label(),
        FontId::proportional(11.0),
        theme::TEXT_DIM,
    );
    if resp.clicked() || resp.drag_started() {
        let w = &mut s.effect_graph;
        w.blackboard.selected = Some(p.name.clone());
        w.blackboard.rename = None;
        w.blackboard.error = None;
        w.selected.clear();
    }
    resp.dnd_set_drag_payload(PropertyPayload(p.name.clone()));
    let name = p.name.clone();
    resp.context_menu(|ui| {
        if ui.button("Delete").clicked() {
            s.effect_graph
                .edit("Delete Property", false, |g| g.remove_property(&name));
            ui.close();
        }
    });
}

fn settings(ui: &mut Ui, s: &mut EditorState, name: &str) {
    let busy = interacting(ui.ctx());
    let Some(mut p) = s.effect_graph.graph.property(name).cloned() else {
        return;
    };
    let before = p.clone();
    let textures = s.assets.textures.clone();
    egui::Grid::new("vfx_bb_grid")
        .num_columns(2)
        .spacing([6.0, 5.0])
        .show(ui, |ui| {
            ui.label("Name");
            let bb = &mut s.effect_graph.blackboard;
            let text = match &mut bb.rename {
                Some((n, t)) if n == name => t,
                _ => {
                    bb.rename = Some((name.to_owned(), name.to_owned()));
                    &mut bb.rename.as_mut().expect("set").1
                }
            };
            let r = ui.add(egui::TextEdit::singleline(text).desired_width(130.0));
            let commit = r.lost_focus() && text != name;
            let typed = text.clone();
            ui.end_row();
            if commit {
                let mut ok = false;
                let old = name.to_owned();
                s.effect_graph.edit("Rename Property", false, |g| {
                    ok = g.rename_property(&old, &typed);
                });
                let bb = &mut s.effect_graph.blackboard;
                if ok {
                    bb.selected = Some(typed.trim().to_owned());
                    bb.rename = None;
                    bb.error = None;
                } else {
                    bb.rename = None;
                    bb.error = Some(format!("\"{typed}\" is empty or already used"));
                }
                return;
            }
            ui.label("Tooltip");
            ui.add(egui::TextEdit::singleline(&mut p.tooltip).desired_width(130.0));
            ui.end_row();
            ui.label("Default");
            ui.end_row();
        });
    value_ui(ui, "vfx_bb_value", &mut p.value, &textures);
    if let Some(e) = &s.effect_graph.blackboard.error {
        ui.label(RichText::new(e).color(super::ERROR).small());
    }
    if p != before && s.effect_graph.graph.property(name).is_some() {
        let n = name.to_owned();
        s.effect_graph.edit("Edit Property", busy, |g| {
            if let Some(q) = g.properties.iter_mut().find(|q| q.name == n) {
                *q = p;
            }
        });
    }
    ui.add_space(4.0);
    if ui.button("Delete Property").clicked() {
        let n = name.to_owned();
        s.effect_graph
            .edit("Delete Property", false, |g| g.remove_property(&n));
        s.effect_graph.blackboard.selected = None;
    }
}
