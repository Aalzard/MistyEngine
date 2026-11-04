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

/// Gap between the label column and the value column of property rows.
pub const LABEL_GAP: f32 = 8.0;

fn value_x_id() -> egui::Id {
    egui::Id::new("misty_property_value_x")
}

/// Where values of property rows start (screen x), so every row in a panel lines up.
pub fn set_value_column(ui: &Ui, x: f32) {
    ui.data_mut(|d| d.insert_temp(value_x_id(), x));
}

/// Forget the value column (after the panel that set it is drawn).
pub fn clear_value_column(ui: &Ui) {
    ui.data_mut(|d| d.remove::<f32>(value_x_id()));
}

/// Width for a row label starting at the cursor so its value lands on the value column, or
/// `fallback` outside the panels that set one.
pub fn label_width_or(ui: &Ui, fallback: f32) -> f32 {
    match ui.data(|d| d.get_temp::<f32>(value_x_id())) {
        Some(x) => (x - LABEL_GAP - ui.cursor().left()).max(40.0),
        None => fallback,
    }
}

/// One `X [..] Y [..] Z [..]` row. Dragging an axis letter scrubs its value.
/// Returns true when any value changed.
pub fn vec3_row(ui: &mut Ui, label: &str, value: &mut [f32; 3], speed: f32) -> bool {
    vec3_row_mixed(ui, label, value, speed, [false; 3])
}

/// [`vec3_row`] where axes flagged in `mixed` show "—" (values differ between edited objects).
pub fn vec3_row_mixed(
    ui: &mut Ui,
    label: &str,
    value: &mut [f32; 3],
    speed: f32,
    mixed: [bool; 3],
) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        let h = ui.spacing().interact_size.y;
        let label_w = label_width_or(ui, 72.0);
        ui.allocate_ui_with_layout(
            egui::vec2(label_w, h),
            egui::Layout::left_to_right(egui::Align::Center),
            |ui| {
                ui.set_min_width(label_w);
                ui.add(egui::Label::new(label).truncate());
            },
        );
        ui.add_space(LABEL_GAP - ui.spacing().item_spacing.x);
        let avail = ui.available_width();
        // Per axis: letter (~10 px), field and two item gaps, so the row never overflows.
        let gap = ui.spacing().item_spacing.x;
        let field_w = ((avail - 3.0 * (10.0 + 2.0 * gap)) / 3.0).max(30.0);
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
            let mut field = egui::DragValue::new(&mut value[i])
                .speed(speed)
                .max_decimals(3);
            if mixed[i] {
                field = field.custom_formatter(|_, _| "—".to_owned());
            }
            changed |= ui.add_sized([field_w, h], field).changed();
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
