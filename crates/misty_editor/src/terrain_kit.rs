//! Building blocks of the Terrain Inspector: the icon tool bar, sections with icons, property
//! rows with aligned labels, units, tooltips and a reset button, a two-handle range slider,
//! gradient sliders that show the colour they pick, the brush shape picker, confirmed actions
//! and the palette of thumbnail cards (texture layers, grass, detail meshes, trees, prefabs).
//!
//! Callers pass text already translated (`tr!` at the call site). Rows follow the Inspector's
//! value column, so they line up with the other components; in a right-to-left UI labels are
//! right-aligned next to their values like `card::prop_label`.

use egui::{Align, Color32, CornerRadius, Layout, Pos2, Rect, Response, Sense, Stroke, Ui, vec2};

use crate::branding::icons::{self, Icon};
use crate::i18n::tr;
use crate::panels::inspector::card::{ROW_HEIGHT, fit_widgets};
use crate::widgets::{LABEL_GAP, label_width_or};

/// Vertical room between sections.
pub const SECTION_GAP: f32 = 10.0;
/// Width of the reset column at the end of each row.
const RESET_W: f32 = 18.0;

/// Accent colour (selected cards, active tools).
pub fn accent(ui: &Ui) -> Color32 {
    icons::accent(ui.visuals())
}

/// Fill of the selected tool / card.
fn selected_fill(ui: &Ui) -> Color32 {
    accent(ui).gamma_multiply(0.28)
}

// ---------------------------------------------------------------------------------------------
// Tool bar

/// One tool bar button.
pub struct Tool<'a> {
    pub icon: Icon,
    /// Tutorial / test id ("tool/<label>") and the tooltip's first line.
    pub label: &'a str,
    pub tip: String,
    pub selected: bool,
}

/// A segmented bar of icon buttons; returns the index clicked. Buttons wrap onto a second row
/// in a narrow Inspector.
pub fn tool_bar(
    ui: &mut Ui,
    id: &str,
    tools: &[Tool],
    mut mark: impl FnMut(&Ui, &str, Rect),
) -> Option<usize> {
    let mut clicked = None;
    let size = vec2(34.0, 30.0);
    let bg = ui.visuals().extreme_bg_color;
    egui::Frame::new()
        .fill(bg)
        .corner_radius(7)
        .inner_margin(3)
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                ui.spacing_mut().item_spacing = vec2(2.0, 2.0);
                for (i, t) in tools.iter().enumerate() {
                    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
                    let resp = resp.on_hover_ui(|ui| {
                        ui.label(crate::theme::bold(ui.ctx(), t.label));
                        ui.label(&t.tip);
                    });
                    let hovered = resp.hovered();
                    let v = ui.visuals();
                    let tint = if t.selected {
                        Color32::WHITE
                    } else if hovered {
                        v.strong_text_color()
                    } else {
                        v.text_color()
                    };
                    if t.selected {
                        ui.painter()
                            .rect_filled(rect, 5.0, accent(ui).gamma_multiply(0.75));
                    } else if hovered {
                        ui.painter()
                            .rect_filled(rect, 5.0, v.widgets.hovered.weak_bg_fill);
                    }
                    icons::paint(ui, rect.shrink(6.0), t.icon, tint);
                    resp.widget_info(|| {
                        egui::WidgetInfo::selected(
                            egui::WidgetType::Button,
                            true,
                            t.selected,
                            t.label,
                        )
                    });
                    mark(ui, t.label, rect);
                    if resp.clicked() {
                        clicked = Some(i);
                    }
                }
            });
        });
    let _ = id;
    clicked
}

// ---------------------------------------------------------------------------------------------
// Sections and rows

/// A collapsible section: arrow, icon, bold title and a rule to the right; the body gets
/// breathing room above and below.
pub fn section<R>(
    ui: &mut Ui,
    id: &str,
    icon: Icon,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    section_open(ui, id, icon, title, true, body)
}

/// [`section`] that starts closed (less used settings).
pub fn section_closed<R>(
    ui: &mut Ui,
    id: &str,
    icon: Icon,
    title: &str,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    section_open(ui, id, icon, title, false, body)
}

fn section_open<R>(
    ui: &mut Ui,
    id: &str,
    icon: Icon,
    title: &str,
    open: bool,
    body: impl FnOnce(&mut Ui) -> R,
) -> Option<R> {
    let id = ui.make_persistent_id(("terrain_section", id));
    let mut state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, open);
    ui.add_space(SECTION_GAP * 0.5);
    let header = ui.horizontal(|ui| {
        ui.set_min_height(22.0);
        state.show_toggle_button(ui, egui::collapsing_header::paint_default_icon);
        let tint = ui.visuals().strong_text_color();
        icons::image(ui, icon, 15.0, tint);
        let r = ui.add(
            egui::Label::new(crate::theme::bold(ui.ctx(), title))
                .sense(Sense::click())
                .selectable(false),
        );
        let y = r.rect.center().y;
        let x0 = r.rect.right() + 8.0;
        let x1 = ui.max_rect().right();
        if x1 > x0 {
            ui.painter().hline(
                x0..=x1,
                y,
                Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
            );
        }
        r
    });
    if header.inner.clicked() {
        state.toggle(ui);
    }
    let r = state
        .show_body_unindented(ui, |ui| {
            ui.add_space(2.0);
            let r = body(ui);
            ui.add_space(SECTION_GAP * 0.5);
            r
        })
        .map(|r| r.inner);
    state.store(ui.ctx());
    r
}

/// A row: the label (with `tip` on hover) in the label column, `body` in the value column, and
/// a reset button when `reset` is `Some(false)` (the value differs from its default). Returns
/// the body's value and whether reset was clicked.
pub fn row<R>(
    ui: &mut Ui,
    label: &str,
    tip: &str,
    reset: Option<bool>,
    body: impl FnOnce(&mut Ui) -> R,
) -> (R, bool) {
    let r = ui.horizontal(|ui| {
        ui.set_min_height(ROW_HEIGHT);
        let lw = label_width_or(ui, 110.0);
        let layout = if crate::i18n::is_rtl() {
            Layout::right_to_left(Align::Center)
        } else {
            Layout::left_to_right(Align::Center)
        };
        ui.allocate_ui_with_layout(vec2(lw, ROW_HEIGHT), layout, |ui| {
            ui.set_min_width(lw);
            ui.add(egui::Label::new(label).truncate())
                .on_hover_text(tip);
        });
        ui.add_space((LABEL_GAP - ui.spacing().item_spacing.x).max(0.0));
        let value_w = (ui.available_width() - RESET_W - ui.spacing().item_spacing.x).max(40.0);
        let r = ui
            .allocate_ui_with_layout(
                vec2(value_w, ROW_HEIGHT),
                Layout::left_to_right(Align::Center),
                |ui| {
                    ui.set_min_width(value_w);
                    fit_widgets(ui, value_w);
                    body(ui)
                },
            )
            .inner;
        let clicked = reset_button(ui, reset);
        (r, clicked)
    });
    let rect = r.response.rect;
    ui.data_mut(|d| d.insert_temp(last_row_id(), rect));
    r.inner
}

fn last_row_id() -> egui::Id {
    egui::Id::new("terrain_kit_last_row")
}

/// Mark the row drawn last as tutorial target `<scope>/<id>` (English id).
pub fn mark_last_row(ui: &Ui, id: &str) {
    if let Some(r) = ui.data(|d| d.get_temp::<Rect>(last_row_id())) {
        crate::tutorials::targets::row(ui, id, r);
    }
}

/// The small reset button at the end of a row (an empty slot when nothing to reset).
fn reset_button(ui: &mut Ui, reset: Option<bool>) -> bool {
    let (rect, resp) = ui.allocate_exact_size(vec2(RESET_W, RESET_W), Sense::click());
    if reset != Some(false) {
        return false;
    }
    let hovered = resp.hovered();
    if hovered {
        ui.painter()
            .rect_filled(rect, 3.0, ui.visuals().widgets.hovered.weak_bg_fill);
    }
    let tint = if hovered {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    icons::paint(ui, rect.shrink(3.0), Icon::Reset, tint);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "Reset"));
    resp.on_hover_text(tr!("Reset to default")).clicked()
}

/// Slider row in `range` with a unit; `log` for wide ranges. Resets to `default`.
#[allow(clippy::too_many_arguments)]
pub fn slider(
    ui: &mut Ui,
    label: &str,
    tip: &str,
    v: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    default: f32,
    suffix: &str,
    log: bool,
) -> bool {
    let (ch, reset) = row(ui, label, tip, Some(close(*v, default)), |ui| {
        let mut s = egui::Slider::new(v, range).suffix(suffix);
        if log {
            s = s.logarithmic(true);
        }
        ui.add(s).on_hover_text(tip).changed()
    });
    if reset {
        *v = default;
    }
    ch || reset
}

fn close(a: f32, b: f32) -> bool {
    (a - b).abs() <= 1e-4 * b.abs().max(1.0)
}

/// Checkbox row.
pub fn check(ui: &mut Ui, label: &str, tip: &str, v: &mut bool, default: bool) -> bool {
    let (ch, reset) = row(ui, label, tip, Some(*v == default), |ui| {
        ui.checkbox(v, "").on_hover_text(tip).changed()
    });
    if reset {
        *v = default;
    }
    ch || reset
}

/// Colour row (sRGB 0..1) with a swatch that opens the picker and the hex value.
pub fn color(ui: &mut Ui, label: &str, tip: &str, c: &mut [f32; 3], default: [f32; 3]) -> bool {
    let same = c.iter().zip(default).all(|(a, b)| close(*a, b));
    let (ch, reset) = row(ui, label, tip, Some(same), |ui| swatch(ui, c, tip));
    if reset {
        *c = default;
    }
    ch || reset
}

/// A colour swatch button (sRGB 0..1); returns true while the colour changes.
pub fn swatch(ui: &mut Ui, c: &mut [f32; 3], tip: &str) -> bool {
    let mut v = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    let ch = ui.scope(|ui| {
        ui.spacing_mut().interact_size = vec2(46.0, 18.0);
        ui.color_edit_button_srgb(&mut v)
            .on_hover_text(tip)
            .changed()
    });
    let ch = ch.inner;
    if ch {
        *c = v.map(|x| x as f32 / 255.0);
    }
    ui.weak(format!("#{:02X}{:02X}{:02X}", v[0], v[1], v[2]));
    ch
}

/// Two-handle range row for `[min, max]` values (heights, scales, slopes): number fields at
/// both ends and a track between them.
#[allow(clippy::too_many_arguments)]
pub fn range(
    ui: &mut Ui,
    label: &str,
    tip: &str,
    v: &mut [f32; 2],
    bounds: std::ops::RangeInclusive<f32>,
    default: [f32; 2],
    suffix: &str,
    speed: f64,
) -> bool {
    let same = close(v[0], default[0]) && close(v[1], default[1]);
    let (ch, reset) = row(ui, label, tip, Some(same), |ui| {
        range_slider(ui, v, bounds, suffix, speed, tip)
    });
    if reset {
        *v = default;
    }
    ch || reset
}

/// The two-handle slider itself (number fields at both ends).
pub fn range_slider(
    ui: &mut Ui,
    v: &mut [f32; 2],
    bounds: std::ops::RangeInclusive<f32>,
    suffix: &str,
    speed: f64,
    tip: &str,
) -> bool {
    let (lo, hi) = (*bounds.start(), *bounds.end());
    let mut ch = false;
    ui.spacing_mut().item_spacing.x = 4.0;
    let number_w = 58.0;
    let (a, b) = (v[0], v[1]);
    ch |= ui
        .add_sized(
            [number_w, 18.0],
            egui::DragValue::new(&mut v[0])
                .speed(speed)
                .range(lo..=b)
                .suffix(suffix),
        )
        .on_hover_text(tip)
        .changed();
    let track_w = (ui.available_width() - number_w - 4.0).max(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(track_w, 18.0), Sense::click_and_drag());
    let to_x = |f: f32| rect.left() + 7.0 + (f - lo) / (hi - lo) * (rect.width() - 14.0);
    let to_v =
        |x: f32| (lo + (x - rect.left() - 7.0) / (rect.width() - 14.0) * (hi - lo)).clamp(lo, hi);
    let handle_id = resp.id.with("handle");
    if let Some(p) = resp.interact_pointer_pos() {
        let h = if resp.drag_started()
            || resp.clicked()
            || resp.is_pointer_button_down_on()
                && ui.data(|d| d.get_temp::<usize>(handle_id)).is_none()
        {
            let h = usize::from((p.x - to_x(v[0])).abs() > (p.x - to_x(v[1])).abs());
            ui.data_mut(|d| d.insert_temp(handle_id, h));
            h
        } else {
            ui.data(|d| d.get_temp::<usize>(handle_id)).unwrap_or(0)
        };
        let new = to_v(p.x);
        let new = if h == 0 { new.min(v[1]) } else { new.max(v[0]) };
        if (v[h] - new).abs() > f32::EPSILON {
            v[h] = new;
            ch = true;
        }
    }
    if resp.drag_stopped() || !resp.is_pointer_button_down_on() && !resp.dragged() {
        ui.data_mut(|d| d.remove::<usize>(handle_id));
    }
    let vis = ui.visuals();
    let y = rect.center().y;
    let track = Rect::from_min_max(
        Pos2::new(rect.left() + 7.0, y - 2.0),
        Pos2::new(rect.right() - 7.0, y + 2.0),
    );
    ui.painter().rect_filled(track, 2.0, vis.extreme_bg_color);
    let (x0, x1) = (to_x(v[0]), to_x(v[1]));
    ui.painter().rect_filled(
        Rect::from_min_max(Pos2::new(x0, y - 2.5), Pos2::new(x1, y + 2.5)),
        2.0,
        accent(ui),
    );
    for x in [x0, x1] {
        let c = Pos2::new(x, y);
        ui.painter().circle(
            c,
            6.0,
            vis.widgets
                .inactive
                .bg_fill
                .lerp_to_gamma(Color32::WHITE, 0.55),
            Stroke::new(1.0, vis.widgets.noninteractive.bg_stroke.color),
        );
    }
    let _ = (a, b);
    let resp = resp.on_hover_text(tip);
    resp.widget_info(|| egui::WidgetInfo::slider(true, v[0] as f64, "range"));
    let min = v[0];
    ch |= ui
        .add_sized(
            [number_w, 18.0],
            egui::DragValue::new(&mut v[1])
                .speed(speed)
                .range(min..=hi)
                .suffix(suffix),
        )
        .on_hover_text(tip)
        .changed();
    ch
}

/// Slider row whose track shows `stops` (the colours it picks between), with a number field.
#[allow(clippy::too_many_arguments)]
pub fn gradient(
    ui: &mut Ui,
    label: &str,
    tip: &str,
    v: &mut f32,
    bounds: std::ops::RangeInclusive<f32>,
    default: f32,
    stops: &[Color32],
) -> bool {
    let (ch, reset) = row(ui, label, tip, Some(close(*v, default)), |ui| {
        gradient_slider(ui, v, bounds, stops, tip)
    });
    if reset {
        *v = default;
    }
    ch || reset
}

pub fn gradient_slider(
    ui: &mut Ui,
    v: &mut f32,
    bounds: std::ops::RangeInclusive<f32>,
    stops: &[Color32],
    tip: &str,
) -> bool {
    let (lo, hi) = (*bounds.start(), *bounds.end());
    ui.spacing_mut().item_spacing.x = 6.0;
    let number_w = 52.0;
    let w = (ui.available_width() - number_w - 6.0).max(30.0);
    let (rect, resp) = ui.allocate_exact_size(vec2(w, 18.0), Sense::click_and_drag());
    let inner = rect.shrink2(vec2(6.0, 4.0));
    let mut ch = false;
    if let Some(p) = resp.interact_pointer_pos() {
        let t = ((p.x - inner.left()) / inner.width()).clamp(0.0, 1.0);
        let new = lo + t * (hi - lo);
        if (new - *v).abs() > f32::EPSILON {
            *v = new;
            ch = true;
        }
    }
    // The gradient: one quad per pair of stops.
    let mut mesh = egui::Mesh::default();
    let n = stops.len().max(2);
    for (i, c) in stops.iter().enumerate() {
        let x = inner.left() + inner.width() * i as f32 / (n - 1) as f32;
        mesh.colored_vertex(Pos2::new(x, inner.top()), *c);
        mesh.colored_vertex(Pos2::new(x, inner.bottom()), *c);
        if i > 0 {
            let b = (i as u32 - 1) * 2;
            mesh.add_triangle(b, b + 1, b + 2);
            mesh.add_triangle(b + 1, b + 3, b + 2);
        }
    }
    let p = ui.painter();
    p.add(egui::Shape::mesh(mesh));
    p.rect_stroke(
        inner,
        2.0,
        Stroke::new(1.0, ui.visuals().widgets.noninteractive.bg_stroke.color),
        egui::StrokeKind::Outside,
    );
    let t = ((*v - lo) / (hi - lo)).clamp(0.0, 1.0);
    let x = inner.left() + inner.width() * t;
    let knob = Rect::from_center_size(Pos2::new(x, rect.center().y), vec2(6.0, 16.0));
    p.rect(
        knob,
        2.0,
        Color32::WHITE,
        Stroke::new(1.0, Color32::from_black_alpha(160)),
        egui::StrokeKind::Outside,
    );
    let resp = resp.on_hover_text(tip);
    resp.widget_info(|| egui::WidgetInfo::slider(true, *v as f64, "gradient"));
    ch |= ui
        .add_sized(
            [number_w, 18.0],
            egui::DragValue::new(v)
                .speed(0.01)
                .range(lo..=hi)
                .max_decimals(2),
        )
        .on_hover_text(tip)
        .changed();
    ch
}

/// sRGB 0..1 to a colour.
pub fn color32(c: [f32; 3]) -> Color32 {
    let b = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    Color32::from_rgb(b[0], b[1], b[2])
}

// ---------------------------------------------------------------------------------------------
// Buttons

/// A button with an icon and text.
pub fn icon_button(ui: &mut Ui, icon: Icon, text: &str, tip: &str) -> Response {
    let tex = icons::texture(ui.ctx(), icon, 14.0);
    let tint = ui.visuals().text_color();
    let img = egui::Image::new((tex, vec2(14.0, 14.0))).tint(tint);
    ui.add(egui::Button::image_and_text(img, text).min_size(vec2(0.0, 24.0)))
        .on_hover_text(tip)
}

/// A button that asks first: a click opens a small confirmation with `question`; returns true
/// when it is confirmed.
pub fn confirm_button(
    ui: &mut Ui,
    icon: Icon,
    text: &str,
    tip: &str,
    question: &str,
    confirm: &str,
    enabled: bool,
) -> bool {
    let resp = ui
        .add_enabled_ui(enabled, |ui| icon_button(ui, icon, text, tip))
        .inner;
    let mut yes = false;
    egui::Popup::menu(&resp).show(|ui| {
        ui.set_max_width(240.0);
        ui.label(question);
        ui.weak(tr!("Edit ▸ Undo takes it back."));
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            let danger = Color32::from_rgb(200, 70, 60);
            if ui
                .add(
                    egui::Button::new(egui::RichText::new(confirm).color(Color32::WHITE))
                        .fill(danger),
                )
                .clicked()
            {
                yes = true;
                ui.close();
            }
            if ui.button(tr!("Cancel")).clicked() {
                ui.close();
            }
        });
    });
    yes
}

// ---------------------------------------------------------------------------------------------
// Brush shapes

/// Brush shapes: a name and the falloff it sets (0 = hard edge .. 1 = fades from the centre).
pub const BRUSHES: [(&str, f32); 4] = [
    ("Hard", 0.0),
    ("Firm", 0.35),
    ("Soft", 0.7),
    ("Feather", 1.0),
];

/// Paint the brush's influence (the brush's own falloff curve) as a disc in `rect`.
pub fn paint_brush(
    painter: &egui::Painter,
    rect: Rect,
    falloff: f32,
    strength: f32,
    color: Color32,
) {
    let b = misty_engine::terrain::brush::Brush {
        center: glam::Vec2::ZERO,
        size: 2.0,
        strength: 1.0,
        falloff,
    };
    let c = rect.center();
    let r = rect.width().min(rect.height()) * 0.5;
    const RINGS: usize = 14;
    const SEG: usize = 40;
    let mut mesh = egui::Mesh::default();
    let alpha = |w: f32| color.gamma_multiply((0.25 + 0.75 * strength) * w);
    mesh.colored_vertex(c, alpha(1.0));
    for k in 1..=RINGS {
        let t = k as f32 / RINGS as f32;
        let w = b.weight(glam::Vec2::new(t.min(0.999), 0.0));
        for s in 0..SEG {
            let a = s as f32 / SEG as f32 * std::f32::consts::TAU;
            mesh.colored_vertex(c + vec2(a.cos(), a.sin()) * r * t, alpha(w));
        }
    }
    for s in 0..SEG as u32 {
        let n = (s + 1) % SEG as u32;
        mesh.add_triangle(0, 1 + s, 1 + n);
        for k in 1..RINGS as u32 {
            let a = 1 + (k - 1) * SEG as u32;
            let b2 = 1 + k * SEG as u32;
            mesh.add_triangle(a + s, b2 + s, b2 + n);
            mesh.add_triangle(a + s, b2 + n, a + n);
        }
    }
    painter.add(egui::Shape::mesh(mesh));
    painter.circle_stroke(c, r, Stroke::new(1.0, color.gamma_multiply(0.6)));
}

/// Brush shape picker: one thumbnail per shape; picking one sets the falloff.
pub fn brush_picker(ui: &mut Ui, falloff: &mut f32, strength: f32) -> bool {
    let mut ch = false;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        for (name, f) in BRUSHES {
            let size = vec2(40.0, 40.0);
            let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
            let selected = (*falloff - f).abs() < 0.02;
            let v = ui.visuals();
            let fill = if selected {
                selected_fill(ui)
            } else if resp.hovered() {
                v.widgets.hovered.weak_bg_fill
            } else {
                v.extreme_bg_color
            };
            ui.painter().rect_filled(rect, 5.0, fill);
            if selected {
                ui.painter().rect_stroke(
                    rect,
                    5.0,
                    Stroke::new(1.5, accent(ui)),
                    egui::StrokeKind::Inside,
                );
            }
            paint_brush(
                ui.painter(),
                rect.shrink(6.0),
                f,
                strength.max(0.6),
                v.strong_text_color(),
            );
            let label = tr(name);
            resp.widget_info(|| {
                egui::WidgetInfo::selected(egui::WidgetType::Button, true, selected, name)
            });
            if resp
                .on_hover_text(format!(
                    "{label}\n{}",
                    tr!("Sets the brush falloff to {f}", f = format!("{f:.2}"))
                ))
                .clicked()
            {
                *falloff = f;
                ch = true;
            }
        }
    });
    ch
}

// ---------------------------------------------------------------------------------------------
// Palette of cards

/// What a card's thumbnail shows.
pub enum Thumb {
    /// A grass tuft (painted from its mesh).
    Tuft(std::sync::Arc<crate::terrain_previews::Tuft>),
    /// An image (texture, tree, rendered mesh), with an optional colour swatch in a corner.
    Image(egui::TextureId, Option<Color32>),
    /// An icon while there is nothing better (or the preview is not ready: `pending`).
    Icon { icon: Icon, pending: bool },
}

/// One palette card.
pub struct Card {
    pub name: String,
    pub thumb: Thumb,
    /// Small badge over the thumbnail's corner (count, density, coverage).
    pub badge: Option<String>,
    pub tip: String,
    /// The asset the card stands for (Select in Project / Open).
    pub asset: Option<String>,
    pub can_duplicate: bool,
    pub can_remove: bool,
    /// Drawn dimmed (switched off).
    pub dimmed: bool,
}

/// What happened on the palette this frame.
#[derive(Clone, Debug, PartialEq)]
pub enum CardEvent {
    Select(usize),
    Duplicate(usize),
    Remove(usize),
    /// Show the card's asset in the Project window.
    Locate(usize),
    /// Open the card's asset (double click).
    Open(usize),
    /// Drag a card onto another: move `from` to `to`.
    Move {
        from: usize,
        to: usize,
    },
    /// An asset dropped from the Project window that `accepts` took.
    Drop(String),
}

/// Payload of a card being dragged (palette id hash, index).
#[derive(Clone, Copy)]
struct CardDrag(egui::Id, usize);

/// The "+" card: label and tooltip.
pub struct AddCard<'a> {
    pub label: &'a str,
    pub tip: &'a str,
    pub enabled: bool,
    /// Tutorial target id of the card (English).
    pub target: &'a str,
}

/// Palette output: events, and the "+" card's response (to open an Add menu on).
pub struct Palette {
    pub events: Vec<CardEvent>,
    pub add: Option<Response>,
}

/// Card width at least this (points).
const CARD_MIN: f32 = 76.0;
const CARD_GAP: f32 = 6.0;
const NAME_H: f32 = 18.0;

/// A grid of thumbnail cards with a selected outline, hover actions (Duplicate, Remove, Select
/// in Project), badges, drag to reorder (`reorder`), Project assets dropped onto it (`accepts`)
/// and an "+" card. `empty` is shown above the "+" card when there are no cards.
#[allow(clippy::too_many_arguments)]
pub fn palette(
    ui: &mut Ui,
    id: &str,
    cards: &[Card],
    selected: Option<usize>,
    add: Option<AddCard>,
    reorder: bool,
    accepts: &dyn Fn(&str) -> bool,
    empty: &str,
    mut mark: impl FnMut(&Ui, &str, Rect),
) -> Palette {
    let pid = ui.make_persistent_id(("terrain_palette", id));
    let mut events = Vec::new();
    let mut add_resp = None;
    if cards.is_empty() {
        egui::Frame::new()
            .fill(ui.visuals().extreme_bg_color)
            .corner_radius(6)
            .inner_margin(10)
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                ui.vertical_centered(|ui| ui.weak(empty));
            });
        ui.add_space(4.0);
    }
    let width = ui.available_width();
    let cols = (((width + CARD_GAP) / (CARD_MIN + CARD_GAP)).floor() as usize).max(1);
    let card_w = ((width - CARD_GAP * (cols - 1) as f32) / cols as f32).floor();
    let card_h = card_w + NAME_H;
    let total = cards.len() + usize::from(add.is_some());
    let rows = total.div_ceil(cols);
    let (area, area_resp) = ui.allocate_exact_size(
        vec2(
            width,
            rows as f32 * (card_h + CARD_GAP) - CARD_GAP.min(rows as f32 * CARD_GAP),
        ),
        Sense::hover(),
    );
    // Assets dropped anywhere on the palette.
    if let Some(p) = area_resp.dnd_release_payload::<crate::panels::AssetPayload>()
        && accepts(&p.0)
    {
        events.push(CardEvent::Drop(p.0.clone()));
    }
    let dropping_asset = egui::DragAndDrop::payload::<crate::panels::AssetPayload>(ui.ctx())
        .is_some_and(|p| accepts(&p.0))
        && area_resp.contains_pointer();
    if dropping_asset {
        ui.painter().rect_stroke(
            area.expand(3.0),
            6.0,
            Stroke::new(2.0, accent(ui)),
            egui::StrokeKind::Outside,
        );
    }
    let cell = |i: usize| {
        let (r, c) = (i / cols, i % cols);
        let c = if crate::i18n::is_rtl() {
            cols - 1 - c
        } else {
            c
        };
        Rect::from_min_size(
            area.min
                + vec2(
                    c as f32 * (card_w + CARD_GAP),
                    r as f32 * (card_h + CARD_GAP),
                ),
            vec2(card_w, card_h),
        )
    };
    for (i, card) in cards.iter().enumerate() {
        let rect = cell(i);
        let sense = if reorder {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let resp = ui.interact(rect, pid.with(i), sense);
        let is_sel = selected == Some(i);
        paint_card(ui, rect, card, is_sel, resp.hovered());
        resp.widget_info(|| {
            egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, true, is_sel, &card.name)
        });
        // Hover actions.
        let mut action_hit = false;
        if resp.hovered() || resp.contains_pointer() {
            let mut x = rect.right() - 3.0;
            let mut action = |ui: &mut Ui, icon: Icon, tip: &str, ev: CardEvent| {
                let r =
                    Rect::from_min_size(Pos2::new(x - 20.0, rect.top() + 3.0), vec2(20.0, 20.0));
                x -= 22.0;
                let b = ui.interact(r, pid.with((i, tip)), Sense::click());
                let fill = if b.hovered() {
                    Color32::from_black_alpha(220)
                } else {
                    Color32::from_black_alpha(150)
                };
                ui.painter().rect_filled(r, 4.0, fill);
                icons::paint(ui, r.shrink(4.0), icon, Color32::WHITE);
                b.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
                if b.on_hover_text(tip).clicked() {
                    events.push(ev);
                    action_hit = true;
                }
            };
            if card.can_remove {
                action(ui, Icon::Remove, &tr!("Remove"), CardEvent::Remove(i));
            }
            if card.can_duplicate {
                action(
                    ui,
                    Icon::Duplicate,
                    &tr!("Duplicate"),
                    CardEvent::Duplicate(i),
                );
            }
            if card.asset.is_some() {
                action(
                    ui,
                    Icon::Locate,
                    &tr!("Select in Project"),
                    CardEvent::Locate(i),
                );
            }
        }
        let resp = resp.on_hover_text(&card.tip);
        if resp.clicked() && !action_hit {
            events.push(CardEvent::Select(i));
        }
        if resp.double_clicked() && card.asset.is_some() {
            events.push(CardEvent::Open(i));
        }
        if reorder {
            if resp.dragged() && resp.drag_delta().length() > 0.0 {
                resp.dnd_set_drag_payload(CardDrag(pid, i));
            }
            if let Some(p) = resp.dnd_hover_payload::<CardDrag>()
                && p.0 == pid
                && p.1 != i
            {
                // Insertion mark on the side it goes to.
                let x = if p.1 < i {
                    rect.right() + CARD_GAP * 0.5
                } else {
                    rect.left() - CARD_GAP * 0.5
                };
                ui.painter()
                    .vline(x, rect.y_range(), Stroke::new(2.5, accent(ui)));
            }
            if let Some(p) = resp.dnd_release_payload::<CardDrag>()
                && p.0 == pid
                && p.1 != i
            {
                events.push(CardEvent::Move { from: p.1, to: i });
            }
        }
        mark(ui, &format!("card/{}", card.name), rect);
    }
    if let Some(a) = add {
        let rect = cell(cards.len());
        let resp = ui.interact(
            rect,
            pid.with("add"),
            if a.enabled {
                Sense::click()
            } else {
                Sense::hover()
            },
        );
        let v = ui.visuals();
        let hovered = resp.hovered() && a.enabled;
        let color = if !a.enabled {
            v.weak_text_color().gamma_multiply(0.5)
        } else if hovered {
            accent(ui)
        } else {
            v.weak_text_color()
        };
        let thumb = Rect::from_min_size(rect.min, vec2(card_w, card_w));
        if hovered {
            ui.painter()
                .rect_filled(thumb, 6.0, selected_fill(ui).gamma_multiply(0.5));
        }
        dashed_rect(ui.painter(), thumb.shrink(1.0), Stroke::new(1.2, color));
        icons::paint(
            ui,
            Rect::from_center_size(thumb.center(), vec2(26.0, 26.0)),
            Icon::Add,
            color,
        );
        ui.painter().text(
            Pos2::new(rect.center().x, thumb.bottom() + NAME_H * 0.5 + 1.0),
            egui::Align2::CENTER_CENTER,
            truncate(a.label, card_w),
            egui::FontId::proportional(11.5),
            color,
        );
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, a.enabled, a.label)
        });
        let resp = resp.on_hover_text(a.tip);
        mark(ui, a.target, rect);
        add_resp = Some(resp);
    }
    Palette {
        events,
        add: add_resp,
    }
}

fn truncate(s: &str, width: f32) -> String {
    let max = ((width / 6.2) as usize).max(4);
    if s.chars().count() <= max {
        s.to_owned()
    } else {
        let t: String = s.chars().take(max - 1).collect();
        format!("{t}…")
    }
}

fn dashed_rect(p: &egui::Painter, r: Rect, stroke: Stroke) {
    let pts = [
        r.left_top(),
        r.right_top(),
        r.right_bottom(),
        r.left_bottom(),
        r.left_top(),
    ];
    p.extend(egui::Shape::dashed_line(&pts, stroke, 5.0, 3.0));
}

/// Background of a preview (a soft sky-to-ground gradient that reads in dark and light themes).
pub fn preview_backdrop(ui: &Ui, rect: Rect, radius: f32) {
    let dark = ui.visuals().dark_mode;
    let (top, bottom) = if dark {
        (Color32::from_rgb(64, 72, 82), Color32::from_rgb(38, 40, 44))
    } else {
        (
            Color32::from_rgb(214, 226, 238),
            Color32::from_rgb(178, 184, 190),
        )
    };
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(rect.left_top(), top);
    mesh.colored_vertex(rect.right_top(), top);
    mesh.colored_vertex(rect.left_bottom(), bottom);
    mesh.colored_vertex(rect.right_bottom(), bottom);
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(1, 3, 2);
    if radius > 0.0 {
        // Rounded: clip the gradient with a frame drawn over its corners.
        ui.painter()
            .with_clip_rect(rect)
            .add(egui::Shape::mesh(mesh));
    } else {
        ui.painter().add(egui::Shape::mesh(mesh));
    }
}

/// Paint a thumbnail into `rect`.
pub fn paint_thumb(ui: &Ui, rect: Rect, thumb: &Thumb) {
    match thumb {
        Thumb::Tuft(t) => {
            preview_backdrop(ui, rect, 0.0);
            crate::terrain_previews::paint_tuft(ui.painter(), rect, t);
        }
        Thumb::Image(id, swatch) => {
            preview_backdrop(ui, rect, 0.0);
            let uv = Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0));
            ui.painter().image(*id, rect, uv, Color32::WHITE);
            if let Some(c) = swatch {
                let s = (rect.width() * 0.22).clamp(10.0, 18.0);
                let r = Rect::from_min_size(rect.left_bottom() + vec2(4.0, -s - 4.0), vec2(s, s));
                ui.painter().rect(
                    r,
                    3.0,
                    *c,
                    Stroke::new(1.0, Color32::from_white_alpha(200)),
                    egui::StrokeKind::Outside,
                );
            }
        }
        Thumb::Icon { icon, pending } => {
            preview_backdrop(ui, rect, 0.0);
            let tint = if *pending {
                // A gentle pulse while the preview is being made.
                let t = ui.input(|i| i.time) as f32;
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(60));
                Color32::from_white_alpha((90.0 + 60.0 * (t * 3.0).sin()) as u8)
            } else {
                Color32::from_white_alpha(170)
            };
            icons::paint(
                ui,
                Rect::from_center_size(rect.center(), rect.size() * 0.42),
                *icon,
                tint,
            );
        }
    }
}

fn paint_card(ui: &mut Ui, rect: Rect, card: &Card, selected: bool, hovered: bool) {
    let v = ui.visuals().clone();
    let thumb = Rect::from_min_size(rect.min, vec2(rect.width(), rect.width()));
    let p = ui.painter().clone();
    let bg = if selected {
        selected_fill(ui)
    } else if hovered {
        v.widgets.hovered.weak_bg_fill
    } else {
        v.extreme_bg_color
    };
    p.rect_filled(rect, 7.0, bg);
    // The thumbnail, clipped to rounded-ish corners by drawing inside a 2 px inset.
    let inner = thumb.shrink(3.0);
    let clip = ui.clip_rect().intersect(inner);
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(inner));
    child.set_clip_rect(clip);
    paint_thumb(&child, inner, &card.thumb);
    if card.dimmed {
        p.rect_filled(inner, 4.0, Color32::from_black_alpha(130));
    }
    if let Some(b) = &card.badge {
        let font = egui::FontId::proportional(10.5);
        let galley = p.layout_no_wrap(b.clone(), font, Color32::WHITE);
        let size = galley.size() + vec2(8.0, 2.0);
        let r = Rect::from_min_size(inner.right_bottom() - size - vec2(3.0, 3.0), size);
        p.rect_filled(r, CornerRadius::same(7), Color32::from_black_alpha(170));
        p.galley(r.min + vec2(4.0, 1.0), galley, Color32::WHITE);
    }
    let text_color = if selected {
        v.strong_text_color()
    } else {
        v.text_color()
    };
    p.text(
        Pos2::new(rect.center().x, thumb.bottom() + NAME_H * 0.5 - 1.0),
        egui::Align2::CENTER_CENTER,
        truncate(&crate::i18n::display(&card.name), rect.width()),
        egui::FontId::proportional(11.5),
        text_color,
    );
    if selected {
        p.rect_stroke(
            rect,
            7.0,
            Stroke::new(2.0, accent(ui)),
            egui::StrokeKind::Inside,
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn brush_shapes_cover_hard_to_feather() {
        assert_eq!(BRUSHES[0].1, 0.0);
        assert_eq!(BRUSHES[BRUSHES.len() - 1].1, 1.0);
        assert!(BRUSHES.windows(2).all(|w| w[0].1 < w[1].1));
    }

    #[test]
    fn long_names_are_shortened_to_the_card() {
        assert_eq!(truncate("Grass", 80.0), "Grass");
        let t = truncate("A very long grass species name", 60.0);
        assert!(t.ends_with('…') && t.chars().count() < 12, "{t}");
    }
}
