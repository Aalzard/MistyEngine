//! Post-process Volume ▸ Volumetric Clouds body: a preset menu, then four small groups
//! (Shape, Layer, Wind, Lighting) of plain sliders.

use egui::{DragValue, Slider, Ui};
use misty_engine::scene::{CloudPreset, CloudsOverride};

fn row(ui: &mut Ui, label: &str, tip: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::tutorials::targets::label(ui, label).on_hover_text(tip);
    let ch = widget(ui);
    ui.end_row();
    ch
}

fn group(ui: &mut Ui, title: &str) {
    super::card::grid_sub_header(ui, title);
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(Slider::new(v, range)).changed()
}

fn metres(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(DragValue::new(v).speed(10.0).range(range).suffix(" m"))
        .changed()
}

/// The Presets dropdown: shows the matching preset or "Custom" and applies a pick.
fn presets(ui: &mut Ui, c: &mut CloudsOverride) -> bool {
    let current = CloudPreset::matching(c).map_or("Custom", CloudPreset::label);
    let mut picked = None;
    egui::ComboBox::from_id_salt("cloud_preset")
        .selected_text(current)
        .show_ui(ui, |ui| {
            for p in CloudPreset::ALL {
                if ui
                    .selectable_label(current == p.label(), p.label())
                    .clicked()
                {
                    picked = Some(p);
                }
            }
        });
    if let Some(p) = picked {
        p.apply(c);
    }
    picked.is_some()
}

/// Body of the Volumetric Clouds section (inside its grid).
pub fn clouds_override(ui: &mut Ui, c: &mut CloudsOverride) -> bool {
    let mut ch = row(ui, "Enable", "Draw the cloud layer.", |ui| {
        ui.checkbox(&mut c.enabled, "").changed()
    });
    ch |= row(
        ui,
        "Preset",
        "Sets coverage, density, shape and layer.",
        |ui| presets(ui, c),
    );

    group(ui, "Shape");
    ch |= row(ui, "Coverage", "0 = clear sky, 1 = overcast.", |ui| {
        slider(ui, &mut c.coverage, 0.0..=1.0)
    });
    ch |= row(ui, "Density", "Thin and bright ↔ thick and dark.", |ui| {
        slider(ui, &mut c.density, 0.0..=2.0)
    });
    ch |= row(
        ui,
        "Fluffy ↔ Sharp",
        "Soft billowy edges ↔ crisp, hard-edged shapes.",
        |ui| slider(ui, &mut c.shape, 0.0..=1.0),
    );

    group(ui, "Layer");
    ch |= row(ui, "Altitude", "Height of the cloud base.", |ui| {
        metres(ui, &mut c.altitude, 0.0..=20_000.0)
    });
    ch |= row(
        ui,
        "Thickness",
        "From the base to the tallest tops.",
        |ui| metres(ui, &mut c.thickness, 50.0..=10_000.0),
    );

    group(ui, "Wind");
    ch |= row(
        ui,
        "Direction",
        "Where the wind blows to (clockwise from +Z).",
        |ui| {
            ui.add(Slider::new(&mut c.wind_direction, 0.0..=360.0).suffix("°"))
                .changed()
        },
    );
    ch |= row(ui, "Speed", "How fast the clouds drift.", |ui| {
        ui.add(
            DragValue::new(&mut c.wind_speed)
                .speed(0.1)
                .range(0.0..=100.0)
                .suffix(" m/s"),
        )
        .changed()
    });

    group(ui, "Lighting");
    ch |= row(
        ui,
        "Silver Lining",
        "Bright rims where the sun shines through thin edges.",
        |ui| slider(ui, &mut c.silver_lining, 0.0..=1.0),
    );
    ch |= row(ui, "Ambient", "Sky light the clouds pick up.", |ui| {
        slider(ui, &mut c.ambient, 0.0..=2.0)
    });
    ch |= row(
        ui,
        "Cast Shadows",
        "Clouds shade the ground, the fog and its light shafts.",
        |ui| ui.checkbox(&mut c.shadows, "").changed(),
    );
    let on = c.shadows;
    ui.add_enabled_ui(on, |ui| ui.label("Shadow Opacity"));
    ch |= ui
        .add_enabled_ui(on, |ui| slider(ui, &mut c.shadow_opacity, 0.0..=1.0))
        .inner;
    ui.end_row();
    ch
}
