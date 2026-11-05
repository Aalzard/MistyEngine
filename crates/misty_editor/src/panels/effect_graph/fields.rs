//! Inline value fields inside blocks and operators (floats, vectors, colours) and the swatches
//! of gradients and curves, which open an editor popup when clicked.

use egui::{Color32, DragValue, FontId, Pos2, Rect, Sense, Stroke, Ui, UiBuilder, pos2, vec2};
use misty_engine::vfx::types::{Curve, Gradient, Value};

use crate::theme;

/// Styled scope for small fields at canvas zoom `z`.
pub fn small_scope<R>(ui: &mut Ui, rect: Rect, z: f32, f: impl FnOnce(&mut Ui) -> R) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        let st = ui.style_mut();
        st.override_font_id = Some(FontId::proportional(11.0 * z));
        st.spacing.interact_size.y = (16.0 * z).max(8.0);
        st.spacing.item_spacing = vec2(3.0 * z, 0.0);
        st.spacing.button_padding = vec2(3.0 * z, 0.0);
        st.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 40, 43);
        st.visuals.widgets.inactive.bg_fill = Color32::from_rgb(40, 40, 43);
        f(ui)
    })
    .inner
}

/// What happened in a field.
#[derive(Default)]
pub struct FieldResponse {
    pub changed: bool,
    /// A gradient / curve swatch was clicked (open its editor).
    pub open_ramp: bool,
}

/// Edit `v` inside `rect` (right part of an input row).
pub fn value_field(
    ui: &mut Ui,
    rect: Rect,
    z: f32,
    v: &mut Value,
    salt: impl std::hash::Hash + std::fmt::Debug,
) -> FieldResponse {
    let mut out = FieldResponse::default();
    let h = rect.height();
    match v {
        Value::Float(f) => {
            small_scope(ui, rect, z, |ui| {
                out.changed = ui
                    .put(rect, DragValue::new(f).speed(0.01).max_decimals(3))
                    .changed();
            });
        }
        Value::Vec3(a) => {
            let w = (rect.width() - 4.0 * z) / 3.0;
            small_scope(ui, rect, z, |ui| {
                for (k, c) in a.iter_mut().enumerate() {
                    let slot = Rect::from_min_size(
                        pos2(rect.left() + k as f32 * (w + 2.0 * z), rect.top()),
                        vec2(w, h),
                    );
                    out.changed |= ui
                        .put(slot, DragValue::new(c).speed(0.01).max_decimals(2))
                        .changed();
                }
            });
        }
        Value::Color(c) => {
            let mut bytes = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
            let hdr = c[..3].iter().any(|x| *x > 1.0);
            small_scope(ui, rect, z, |ui| {
                ui.push_id(&salt, |ui| {
                    let r = ui.put(rect, |ui: &mut Ui| {
                        ui.color_edit_button_srgba_unmultiplied(&mut bytes)
                    });
                    if r.changed() {
                        // Keep an HDR colour's intensity when only the hue is edited.
                        let k = if hdr {
                            c[..3].iter().cloned().fold(1.0, f32::max)
                        } else {
                            1.0
                        };
                        for i in 0..3 {
                            c[i] = bytes[i] as f32 / 255.0 * k;
                        }
                        c[3] = bytes[3] as f32 / 255.0;
                        out.changed = true;
                    }
                    if hdr {
                        r.on_hover_text("HDR colour (values above 1 glow)");
                    }
                });
            });
        }
        Value::Gradient(g) => {
            let resp = ui.interact(rect, ui.id().with(("vfx_ramp", &salt)), Sense::click());
            paint_gradient(ui.painter(), rect, g);
            out.open_ramp = resp.clicked();
            resp.on_hover_text("Click to edit the gradient");
        }
        Value::Curve(c) => {
            let resp = ui.interact(rect, ui.id().with(("vfx_ramp", &salt)), Sense::click());
            paint_curve(ui.painter(), rect, c);
            out.open_ramp = resp.clicked();
            resp.on_hover_text("Click to edit the curve");
        }
    }
    out
}

/// A gradient strip over a checkerboard.
pub fn paint_gradient(p: &egui::Painter, rect: Rect, g: &Gradient) {
    let steps = 32;
    let w = rect.width() / steps as f32;
    for i in 0..steps {
        let t = (i as f32 + 0.5) / steps as f32;
        let c = g.evaluate(t);
        for half in 0..2 {
            let r = Rect::from_min_size(
                pos2(
                    rect.left() + i as f32 * w,
                    rect.top() + half as f32 * rect.height() * 0.5,
                ),
                vec2(w + 0.5, rect.height() * 0.5),
            );
            let checker = if (i / 2 + half) % 2 == 0 {
                190.0
            } else {
                130.0
            };
            let mix = |v: f32| (v.clamp(0.0, 1.0) * c[3] * 255.0 + checker * (1.0 - c[3])) as u8;
            p.rect_filled(r, 0.0, Color32::from_rgb(mix(c[0]), mix(c[1]), mix(c[2])));
        }
    }
    p.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_gray(25)),
        egui::StrokeKind::Inside,
    );
}

/// A curve's line in a dark box (0..1 time across, the value range up the box).
pub fn paint_curve(p: &egui::Painter, rect: Rect, c: &Curve) {
    p.rect_filled(rect, 2.0, Color32::from_rgb(36, 36, 39));
    let (lo, hi) = c
        .keys
        .iter()
        .fold((0.0f32, 1.0f32), |(a, b), k| (a.min(k[1]), b.max(k[1])));
    let span = (hi - lo).max(1e-3);
    let pts: Vec<Pos2> = (0..=24)
        .map(|i| {
            let t = i as f32 / 24.0;
            let v = (c.evaluate(t) - lo) / span;
            pos2(
                rect.left() + t * rect.width(),
                rect.bottom() - 2.0 - v * (rect.height() - 4.0),
            )
        })
        .collect();
    p.add(egui::Shape::line(
        pts,
        Stroke::new(1.5, Color32::from_rgb(132, 196, 236)),
    ));
    p.rect_stroke(
        rect,
        2.0,
        Stroke::new(1.0, Color32::from_gray(25)),
        egui::StrokeKind::Inside,
    );
}

/// Curve key editor: (time, value) rows, add and remove keys.
pub fn curve_editor(ui: &mut Ui, c: &mut Curve) -> bool {
    let mut ch = false;
    let (rect, _) = ui.allocate_exact_size(vec2(220.0, 60.0), Sense::hover());
    paint_curve(ui.painter(), rect, c);
    let mut remove = None;
    for (i, k) in c.keys.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ch |= ui
                .add(
                    DragValue::new(&mut k[0])
                        .speed(0.005)
                        .range(0.0..=1.0)
                        .prefix("t "),
                )
                .changed();
            ch |= ui
                .add(DragValue::new(&mut k[1]).speed(0.01).prefix("v "))
                .changed();
            if ui.small_button("−").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove.filter(|_| c.keys.len() > 1) {
        c.keys.remove(i);
        ch = true;
    }
    if c.keys.len() < 8 && ui.small_button("+ Key").clicked() {
        let last = c.keys.last().copied().unwrap_or([1.0, 1.0]);
        let t = match c.keys.len() {
            0 | 1 => 1.0,
            n => (c.keys[n - 2][0] + c.keys[n - 1][0]) * 0.5,
        };
        c.keys.push([t, last[1]]);
        ch = true;
    }
    if ch {
        c.sort();
    }
    ui.label(
        egui::RichText::new("Time 0..1 over the particle's life")
            .small()
            .color(theme::TEXT_DIM),
    );
    ch
}
