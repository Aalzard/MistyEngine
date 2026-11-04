//! Light rows of Rectangle lights: size, two-sided emission and the visible emitter.

use egui::{DragValue, Ui};
use misty_engine::scene::Light;

use super::mixed;

/// Rows inside the Light grid (Rectangle lights only).
pub fn rows(ui: &mut Ui, l: &mut Light) -> bool {
    let mut ch = false;
    for (label, key, value) in [
        ("Width", "width", &mut l.width),
        ("Height", "height", &mut l.height),
    ] {
        ui.label(label);
        let field = DragValue::new(value)
            .speed(0.01)
            .range(0.01..=100.0)
            .suffix(" m");
        ch |= ui.add(mixed::drag(key, field)).changed();
        ui.end_row();
    }
    ui.label("Two Sided")
        .on_hover_text("Emit from both faces (off: only towards the light's forward axis).");
    ch |= mixed::checkbox(ui, "two_sided", &mut l.two_sided, "").changed();
    ui.end_row();
    ui.label("Show Emitter")
        .on_hover_text("Draw the rectangle as a glowing surface in the Scene and Game views.");
    ch |= mixed::checkbox(ui, "show_emitter", &mut l.show_emitter, "").changed();
    ui.end_row();
    ch
}
