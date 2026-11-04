//! Light ▸ Shadows rows under Shadow Type: the source size that soft shadows widen with
//! (sun angle or source radius) and the depth / normal biases.

use egui::{DragValue, Ui};
use misty_engine::scene::{Light, LightType, Shadows};

use super::mixed;

/// Rows inside the Light grid; nothing when the light casts no shadows.
pub fn rows(ui: &mut Ui, l: &mut Light) -> bool {
    if l.shadows == Shadows::None {
        return false;
    }
    let mut ch = false;
    if l.shadows == Shadows::Soft {
        if l.kind == LightType::Directional {
            ui.label("Shadow Angle")
                .on_hover_text("Angular diameter of the sun (the real sun is about 0.53°).");
            let field = DragValue::new(&mut l.shadow_angle)
                .speed(0.01)
                .range(0.0..=20.0)
                .suffix("°");
            ch |= ui.add(mixed::drag("shadow_angle", field)).changed();
            ui.end_row();
        } else if l.kind != LightType::Rectangle {
            // Rectangle lights soften with their size instead.
            ui.label("Shadow Radius")
                .on_hover_text("Radius of the light source in metres: larger = softer shadows.");
            let field = DragValue::new(&mut l.shadow_radius)
                .speed(0.005)
                .range(0.0..=10.0)
                .suffix(" m");
            ch |= ui.add(mixed::drag("shadow_radius", field)).changed();
            ui.end_row();
        }
    }
    ui.label("Bias")
        .on_hover_text("Pushes the shadow away from the surface (shadow-map texels).");
    let bias = DragValue::new(&mut l.shadow_bias)
        .speed(0.01)
        .range(0.0..=10.0);
    ch |= ui.add(mixed::drag("shadow_bias", bias)).changed();
    ui.end_row();
    ui.label("Normal Bias")
        .on_hover_text("Offsets the lookup along the surface normal (shadow-map texels).");
    let normal = DragValue::new(&mut l.shadow_normal_bias)
        .speed(0.01)
        .range(0.0..=10.0);
    ch |= ui.add(mixed::drag("shadow_normal_bias", normal)).changed();
    ui.end_row();
    ch
}
