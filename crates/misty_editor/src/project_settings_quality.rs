//! Project Settings ▸ Quality: soft-shadow sample budget and the sun's shadow cascades.

use egui::Ui;
use misty_engine::quality::{QualitySettings, ShadowQuality};

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
    })
    .inner
}

pub fn page(ui: &mut Ui, q: &mut QualitySettings) -> bool {
    let s = &mut q.shadows;
    ui.strong("Shadows");
    let mut edited = row(
        ui,
        "Soft Shadow Quality",
        "Samples per soft-shadowed pixel: more = smoother penumbrae, slower.",
        |ui| {
            let mut changed = false;
            egui::ComboBox::from_id_salt("shadow_quality")
                .selected_text(s.quality.label())
                .show_ui(ui, |ui| {
                    for v in ShadowQuality::ALL {
                        changed |= ui.selectable_value(&mut s.quality, v, v.label()).changed();
                    }
                });
            changed
        },
    );
    edited |= row(
        ui,
        "Shadow Distance",
        "How far from the camera the sun casts shadows (metres).",
        |ui| {
            let field = egui::DragValue::new(&mut s.distance)
                .speed(1.0)
                .range(1.0..=10_000.0)
                .suffix(" m");
            ui.add(field).changed()
        },
    );
    edited |= row(
        ui,
        "Shadow Cascades",
        "Shadow maps the view is split into: more = sharper near shadows.",
        |ui| {
            let mut changed = false;
            for n in 1..=4u32 {
                let label = if n == 1 {
                    "No Cascades".to_owned()
                } else {
                    format!("{n}")
                };
                changed |= ui.selectable_value(&mut s.cascades, n, label).changed();
            }
            changed
        },
    );
    edited |= row(
        ui,
        "Cascade Split",
        "0 = evenly spaced cascades, 1 = logarithmic (more detail near the camera).",
        |ui| {
            let slider = egui::Slider::new(&mut s.split_lambda, 0.0..=1.0);
            ui.add(slider).changed()
        },
    );
    edited |= row(
        ui,
        "Cascade Blend",
        "Fraction of each cascade faded into the next, hiding the seams.",
        |ui| {
            let slider = egui::Slider::new(&mut s.cascade_blend, 0.0..=0.5);
            ui.add(slider).changed()
        },
    );
    ui.add_space(6.0);
    ui.weak("Each light's Shadow Angle / Radius and biases are set in its Inspector.");
    ui.weak("Scene view ▸ Shadow Cascades shows where each cascade starts.");
    edited
}
