//! Mesh Renderer ▸ Element ▸ "Overrides (this object only)": per-object changes to how one
//! material looks on this object (UV tiling/offset/rotation, tint, multipliers), without a
//! new material. A checkbox turns each override on; overridden rows are bold with a marker.

use egui::{RichText, Ui};
use misty_engine::render::MaterialParams;
use misty_engine::render::overrides::{MaterialOverrides, OverrideValue};
use misty_engine::scene::MeshRenderer;

use super::mixed;
use crate::theme;

const LABELS: [(&str, &str); 7] = [
    ("tiling", "Tiling"),
    ("offset", "Offset"),
    ("rotation", "UV Rotation"),
    ("color", "Color Tint"),
    ("smoothness", "Smoothness ×"),
    ("metallic", "Metallic ×"),
    ("emission", "Emission ×"),
];

/// The collapsible section under Materials element `i`. `material` = the element's own values
/// (shown greyed while a property isn't overridden). Returns true when something changed.
pub fn section(ui: &mut Ui, r: &mut MeshRenderer, i: usize, material: &MaterialParams) -> bool {
    let overrides = r.overrides_at(i).cloned().unwrap_or_default();
    let count = MaterialOverrides::NAMES
        .iter()
        .filter(|n| overrides.get(n).is_some())
        .count();
    let any_mixed = mixed::is(&format!("overrides/{i}"));
    let title = match (count, any_mixed) {
        (_, true) => format!("Overrides (this object only)  {}", mixed::DASH),
        (0, _) => "Overrides (this object only)".to_owned(),
        (n, _) => format!("Overrides (this object only)  ● {n}"),
    };
    let mut changed = false;
    let mut next = overrides.clone();
    egui::CollapsingHeader::new(RichText::new(title).small())
        .id_salt(("mr_overrides", i))
        .default_open(count > 0)
        .show(ui, |ui| {
            crate::panels::inspector::card::PropertyGrid::new(("mr_overrides_grid", i)).show(
                ui,
                |ui| {
                    for (name, label) in LABELS {
                        changed |= row(ui, i, name, label, &mut next, material);
                        ui.end_row();
                    }
                },
            );
            ui.horizontal(|ui| {
                let reset = ui
                    .add_enabled(
                        count > 0 || any_mixed,
                        egui::Button::new("Reset All").small(),
                    )
                    .on_hover_text("Use the material's values again on this object");
                if reset.clicked() {
                    next = MaterialOverrides::default();
                    changed = true;
                }
            });
        });
    if changed {
        r.edit_overrides(i, |o| *o = next);
    }
    changed
}

/// One property: on/off checkbox and label, then its value (editable only while on).
fn row(
    ui: &mut Ui,
    i: usize,
    name: &str,
    label: &str,
    o: &mut MaterialOverrides,
    material: &MaterialParams,
) -> bool {
    let key = format!("overrides/{i}/{name}");
    let current = o.get(name);
    let mut on = current.is_some();
    let text = match on {
        true => RichText::new(format!("● {label}"))
            .strong()
            .color(theme::PREFAB),
        false => RichText::new(label),
    };
    let mut changed = false;
    if mixed::checkbox(ui, &key, &mut on, text).changed() {
        match on {
            true => {
                if let Some(v) = MaterialOverrides::neutral(name, material) {
                    o.set(name, v);
                }
            }
            false => {
                o.clear(name);
            }
        }
        changed = true;
    }
    let shown = o
        .get(name)
        .or_else(|| MaterialOverrides::neutral(name, material));
    let Some(mut value) = shown else {
        return changed;
    };
    let edited = ui
        .add_enabled_ui(on, |ui| value_widget(ui, &key, &mut value))
        .inner;
    if edited && on {
        o.set(name, value);
        changed = true;
    }
    changed
}

fn value_widget(ui: &mut Ui, key: &str, v: &mut OverrideValue) -> bool {
    match v {
        OverrideValue::Vec2(xy) => {
            ui.horizontal(|ui| {
                let mut ch = false;
                for (axis, value) in ["X", "Y"].iter().zip(xy.iter_mut()) {
                    ui.label(*axis);
                    let drag = egui::DragValue::new(value).speed(0.01).max_decimals(3);
                    ch |= ui.add(mixed::drag(key, drag)).changed();
                }
                ch
            })
            .inner
        }
        OverrideValue::Color(c) => {
            crate::panels::inspector::card::wide(ui, |ui| ui.color_edit_button_rgba_unmultiplied(c))
                .changed()
        }
        OverrideValue::Float(f) => {
            let drag = if key.ends_with("rotation") {
                egui::DragValue::new(f).speed(0.5).suffix("°")
            } else {
                egui::DragValue::new(f).speed(0.01).range(0.0..=10.0)
            };
            ui.add(mixed::drag(key, drag)).changed()
        }
    }
}
