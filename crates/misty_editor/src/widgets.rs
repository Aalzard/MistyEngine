//! Small reusable editor widgets.

use egui::{Color32, Response, RichText, Sense, Ui};

use crate::theme;

/// Toolbar toggle button with a pressed look when `selected`.
pub fn toggle_button(
    ui: &mut Ui,
    selected: bool,
    text: impl Into<RichText>,
    tip: &str,
) -> Response {
    ui.add(
        egui::Button::new(text.into())
            .selected(selected)
            .min_size(egui::vec2(24.0, 20.0)),
    )
    .on_hover_text(tip)
}

/// One `X [..] Y [..] Z [..]` row. Dragging an axis letter scrubs its value.
/// Returns true when any value changed.
pub fn vec3_row(ui: &mut Ui, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        ui.add_sized([72.0, 18.0], egui::Label::new(label).truncate());
        let avail = ui.available_width();
        let field_w = ((avail - 3.0 * 14.0 - 12.0) / 3.0).max(30.0);
        for (i, axis) in ["X", "Y", "Z"].into_iter().enumerate() {
            let letter = ui.add(
                egui::Label::new(RichText::new(axis).color(axis_color(i))).sense(Sense::drag()),
            );
            if letter.hovered() || letter.dragged() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::ResizeHorizontal);
            }
            if letter.dragged() {
                value[i] += letter.drag_delta().x * speed;
                changed = true;
            }
            changed |= ui
                .add_sized(
                    [field_w, 18.0],
                    egui::DragValue::new(&mut value[i])
                        .speed(speed)
                        .max_decimals(3),
                )
                .changed();
        }
    });
    changed
}

fn axis_color(i: usize) -> Color32 {
    match i {
        0 => theme::AXIS_X,
        1 => theme::AXIS_Y,
        _ => theme::AXIS_Z,
    }
}

/// Thin horizontal strip used as a panel toolbar.
pub fn panel_toolbar<R>(ui: &mut Ui, add: impl FnOnce(&mut Ui) -> R) -> R {
    let frame = egui::Frame::new()
        .fill(Color32::from_rgb(60, 60, 60))
        .inner_margin(egui::Margin::symmetric(4, 2));
    let r = frame
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(add).inner
        })
        .inner;
    let rect = ui.min_rect();
    ui.painter().hline(
        rect.x_range(),
        ui.cursor().top(),
        egui::Stroke::new(1.0, theme::BORDER),
    );
    r
}

/// Search field with a magnifier hint.
pub fn search_field(ui: &mut Ui, text: &mut String, width: f32) -> Response {
    ui.add(
        egui::TextEdit::singleline(text)
            .hint_text("🔍 Search")
            .desired_width(width),
    )
}
