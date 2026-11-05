//! The Mesh Builder strip: a small floating row under the Scene view's tools row, shown while
//! an object with an Editable Mesh is selected. Object / Vertex / Edge / Face modes, the
//! operations menu and New Shape.

use egui::{Color32, Painter, Rect, Stroke, Ui, pos2, vec2};

use super::{EditMode, Op, run, target};
use crate::state::EditorState;

const BUTTON: egui::Vec2 = egui::vec2(28.0, 26.0);
const PAD: f32 = 3.0;
const GAP: f32 = 2.0;
const DIVIDER: f32 = 9.0;
/// Top of the strip: below the tools row (8 px margin + 32 px row + 6 px gap).
pub const TOP: f32 = 46.0;

/// Draw the strip in the Scene view `rect` (no-op without an Editable Mesh selected).
pub fn show(ui: &mut Ui, rect: Rect, state: &mut EditorState) {
    if target(state).is_none() || state.play.in_play_mode() {
        return;
    }
    let menu_w = 52.0;
    let width = 4.0 * (BUTTON.x + GAP) - GAP + DIVIDER + menu_w + GAP + BUTTON.x + PAD * 2.0;
    let panel = Rect::from_min_size(
        rect.left_top() + vec2(8.0, TOP),
        vec2(width, BUTTON.y + PAD * 2.0),
    );
    let v = ui.visuals();
    let (fill, line) = (v.extreme_bg_color.gamma_multiply(0.92), v.weak_text_color());
    ui.painter().rect_filled(panel, 5.0, fill);
    for (i, mode) in EditMode::ALL.into_iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(
                panel.left() + PAD + i as f32 * (BUTTON.x + GAP),
                panel.top() + PAD,
            ),
            BUTTON,
        );
        let selected = state.mesh_builder.mode == mode;
        let resp = ui
            .allocate_rect(r, egui::Sense::click())
            .on_hover_text(mode.tooltip());
        let v = ui.visuals();
        if selected {
            ui.painter()
                .rect_filled(r, 4.0, v.widgets.hovered.weak_bg_fill);
        } else if resp.hovered() {
            ui.painter()
                .rect_filled(r, 4.0, v.widgets.inactive.weak_bg_fill);
        }
        let tint = if selected {
            Color32::from_rgb(255, 170, 60)
        } else {
            v.text_color()
        };
        paint_mode_icon(ui.painter(), r.shrink2(vec2(7.0, 6.0)), mode, tint);
        if resp.clicked() {
            state.mesh_builder.mode = mode;
        }
    }
    let x = panel.left() + PAD + 4.0 * (BUTTON.x + GAP) - GAP + DIVIDER;
    ui.painter().vline(
        x - DIVIDER * 0.5,
        panel.y_range().shrink(6.0),
        Stroke::new(1.0, line.gamma_multiply(0.6)),
    );
    let menu = Rect::from_min_size(pos2(x, panel.top() + PAD), vec2(menu_w, BUTTON.y));
    ui.scope_builder(egui::UiBuilder::new().max_rect(menu), |ui| {
        ui.spacing_mut().button_padding = vec2(6.0, 4.0);
        let (resp, _) = crate::widgets::dropdown(ui, "Edit ⏷", |ui| ops_menu(ui, state));
        resp.on_hover_text("Mesh Builder operations");
    });
    let plus = Rect::from_min_size(pos2(menu.right() + GAP, panel.top() + PAD), BUTTON);
    let b = egui::Button::new(egui::RichText::new("+").size(16.0)).frame_when_inactive(false);
    if ui.put(plus, b).on_hover_text("New Shape…").clicked() {
        super::open_new_shape(state);
    }
}

/// Operations for the current mode, then the object ones.
fn ops_menu(ui: &mut Ui, state: &mut EditorState) {
    ui.set_min_width(200.0);
    let ctx = ui.ctx().clone();
    let item = |ui: &mut Ui, state: &mut EditorState, op: Op| {
        let enabled = op.enabled(state);
        let button = egui::Button::new(op.label()).shortcut_text(op.shortcut_text(&ctx));
        if ui
            .add_enabled(enabled, button)
            .on_hover_text(op.tooltip())
            .clicked()
        {
            run(state, op);
        }
    };
    let mode = state.mesh_builder.mode;
    if mode == EditMode::Object {
        ui.weak("Pick Vertex, Edge or Face (2–4) to edit elements.");
    } else {
        for op in Op::ALL.into_iter().filter(|o| !o.object_level()) {
            item(ui, state, op);
        }
    }
    ui.separator();
    for op in Op::ALL.into_iter().filter(|o| o.object_level()) {
        item(ui, state, op);
    }
}

/// Mode icons: a cube outline (Object), corner dots (Vertex), a bold side (Edge), a filled
/// square (Face).
pub fn paint_mode_icon(p: &Painter, r: Rect, mode: EditMode, color: Color32) {
    let thin = Stroke::new(1.2, color.gamma_multiply(0.55));
    let corners = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
    ];
    match mode {
        EditMode::Object => {
            // Front square and a shifted back square: a little cube.
            let d = r.width() * 0.28;
            let front =
                Rect::from_min_max(r.left_top() + vec2(0.0, d), r.right_bottom() - vec2(d, 0.0));
            let back = front.translate(vec2(d, -d));
            let s = Stroke::new(1.4, color);
            p.rect_stroke(
                back,
                0.0,
                Stroke::new(1.0, color.gamma_multiply(0.6)),
                egui::StrokeKind::Middle,
            );
            p.rect_filled(front, 0.0, color.gamma_multiply(0.25));
            p.rect_stroke(front, 0.0, s, egui::StrokeKind::Middle);
            for (a, b) in [
                (front.left_top(), back.left_top()),
                (front.right_top(), back.right_top()),
                (front.right_bottom(), back.right_bottom()),
            ] {
                p.line_segment([a, b], Stroke::new(1.0, color.gamma_multiply(0.8)));
            }
        }
        EditMode::Vertex => {
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Middle);
            for c in corners {
                p.rect_filled(Rect::from_center_size(c, vec2(5.0, 5.0)), 1.0, color);
            }
        }
        EditMode::Edge => {
            p.rect_stroke(r, 0.0, thin, egui::StrokeKind::Middle);
            p.line_segment([r.left_top(), r.right_top()], Stroke::new(3.0, color));
        }
        EditMode::Face => {
            p.rect_filled(r, 1.0, color.gamma_multiply(0.75));
            p.rect_stroke(r, 1.0, thin, egui::StrokeKind::Middle);
        }
    }
}
