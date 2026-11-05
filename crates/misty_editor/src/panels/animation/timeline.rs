//! Right side of the Animation window: time ruler, playhead, and either the dopesheet (key
//! diamonds per row) or the curve editor. Keys can be selected, box-selected, dragged, deleted
//! and given tangent modes.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, Ui, pos2, vec2};
use misty_engine::animation::{AnimationClip, TangentMode};

use super::rows::Row;
use super::{RECORD_RED, ROW_H, preview};
use crate::state::EditorState;
use crate::theme;

/// Left margin so time 0 is not on the edge.
const PAD: f32 = 12.0;
/// Click distance to a key, in pixels.
const HIT: f32 = 6.0;

/// A drag that started on a key (moves the selection) or on empty space (box selection).
pub enum KeyDrag {
    Move {
        /// Clip before the drag; every frame re-applies the offset to it.
        base: AnimationClip,
        grab: Pos2,
        keys: Vec<(usize, f32)>,
        /// Curve view: value change per pixel of vertical drag (0 in the dopesheet).
        value_per_px: f32,
    },
    Box {
        start: Pos2,
    },
}

/// Seconds ↔ pixels for the visible part of the timeline.
#[derive(Clone, Copy)]
struct Axis {
    left: f32,
    start: f32,
    pps: f32,
}

impl Axis {
    fn x(&self, t: f32) -> f32 {
        self.left + PAD + (t - self.start) * self.pps
    }
    fn t(&self, x: f32) -> f32 {
        self.start + (x - self.left - PAD) / self.pps
    }
}

/// Vertical value mapping of the curve editor.
#[derive(Clone, Copy)]
struct ValueAxis {
    rect: Rect,
    min: f32,
    max: f32,
}

impl ValueAxis {
    fn y(&self, v: f32) -> f32 {
        self.rect.bottom() - (v - self.min) / (self.max - self.min) * self.rect.height()
    }
    fn per_px(&self) -> f32 {
        (self.max - self.min) / self.rect.height().max(1.0)
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState, clip: &AnimationClip, rows: &[Row], rect: Rect) {
    let axis = zoom_and_pan(ui, s, clip, rect);
    let painter = ui.painter().clone();
    painter.rect_filled(rect, 0.0, Color32::from_rgb(44, 44, 44));
    // Beyond the clip's end is shaded.
    let end_x = axis.x(clip.length);
    if end_x < rect.right() {
        painter.rect_filled(
            Rect::from_min_max(pos2(end_x.max(rect.left()), rect.top()), rect.max),
            0.0,
            Color32::from_black_alpha(60),
        );
    }
    let ruler = Rect::from_min_size(rect.min, vec2(rect.width(), ROW_H));
    let body = Rect::from_min_max(
        pos2(rect.left(), ruler.bottom()),
        pos2(rect.right(), rect.bottom() - ROW_H - 4.0),
    );
    if s.animation.curves_view {
        curves(ui, s, clip, rows, axis, body);
    } else {
        dopesheet(ui, s, clip, rows, axis, body);
    }
    time_ruler(ui, s, clip, axis, ruler);
    // Playhead over everything (red while recording).
    let x = axis.x(s.animation.time);
    if rect.x_range().contains(x) {
        let color = if s.animation.recording {
            RECORD_RED
        } else {
            Color32::WHITE
        };
        painter.vline(x, rect.y_range(), Stroke::new(1.0, color));
    }
}

/// Mouse wheel zooms around the pointer, Shift+wheel / horizontal wheel scrolls.
fn zoom_and_pan(ui: &mut Ui, s: &mut EditorState, clip: &AnimationClip, rect: Rect) -> Axis {
    let w = &mut s.animation;
    if w.pixels_per_second <= 0.0 {
        w.pixels_per_second = (rect.width() - 2.0 * PAD).max(50.0) / clip.length.max(0.1) * 0.9;
        w.scroll = 0.0;
    }
    let mut axis = Axis {
        left: rect.left(),
        start: w.scroll,
        pps: w.pixels_per_second,
    };
    let hovered = ui.rect_contains_pointer(rect);
    if hovered {
        let (delta, shift, pointer) = ui.input(|i| {
            (
                i.smooth_scroll_delta,
                i.modifiers.shift,
                i.pointer.hover_pos(),
            )
        });
        let (zoom, pan) = if shift {
            (0.0, delta.y)
        } else {
            (delta.y, delta.x)
        };
        if zoom != 0.0
            && let Some(p) = pointer
        {
            let t = axis.t(p.x);
            axis.pps = (axis.pps * (1.0 + zoom * 0.003)).clamp(5.0, 5000.0);
            axis.start = t - (p.x - axis.left - PAD) / axis.pps;
        }
        axis.start -= pan / axis.pps;
        axis.start = axis.start.max(-1.0);
        w.pixels_per_second = axis.pps;
        w.scroll = axis.start;
    }
    axis
}

/// Frame step between labelled ticks so labels are at least ~60 px apart.
pub(crate) fn tick_step(sample_rate: f32, pps: f32) -> f32 {
    let frame_px = pps / sample_rate;
    [
        1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0, 1800.0, 3600.0,
    ]
    .into_iter()
    .find(|f| f * frame_px >= 60.0)
    .unwrap_or(3600.0)
}

/// "seconds:frames", like 1:30 for one and a half seconds at 60 samples.
pub fn time_label(t: f32, sample_rate: f32) -> String {
    let frames = (t * sample_rate).round() as i64;
    let rate = sample_rate.round().max(1.0) as i64;
    format!("{}:{:02}", frames / rate, frames % rate)
}

fn time_ruler(ui: &mut Ui, s: &mut EditorState, clip: &AnimationClip, axis: Axis, rect: Rect) {
    let painter = ui.painter();
    let bg = if s.animation.recording {
        Color32::from_rgb(90, 45, 45)
    } else {
        Color32::from_rgb(58, 58, 58)
    };
    painter.rect_filled(rect, 0.0, bg);
    let step = tick_step(clip.sample_rate, axis.pps) / clip.sample_rate;
    let minor = step / 5.0;
    let first = (axis.t(rect.left()) / minor).floor().max(0.0) as i64;
    let last = (axis.t(rect.right()) / minor).ceil() as i64;
    for i in first..=last.min(first + 2000) {
        let t = i as f32 * minor;
        let x = axis.x(t);
        let major = i % 5 == 0;
        let h = if major { ROW_H * 0.6 } else { ROW_H * 0.25 };
        painter.vline(
            x,
            rect.bottom() - h..=rect.bottom(),
            Stroke::new(1.0, theme::TEXT_DIM),
        );
        if major {
            painter.text(
                pos2(x + 3.0, rect.top() + 1.0),
                Align2::LEFT_TOP,
                time_label(t, clip.sample_rate),
                FontId::proportional(10.0),
                theme::TEXT_DIM,
            );
        }
    }
    // Animation events: small flags at their times.
    for ev in &clip.events {
        let x = axis.x(ev.time);
        let flag = vec![
            pos2(x, rect.top() + 2.0),
            pos2(x + 6.0, rect.top() + 5.0),
            pos2(x, rect.top() + 8.0),
        ];
        painter.add(Shape::convex_polygon(flag, Color32::WHITE, Stroke::NONE));
    }
    // Clicking or dragging in the ruler moves the playhead (and starts previewing).
    let resp = ui.interact(rect, ui.id().with("anim_ruler"), Sense::click_and_drag());
    if (resp.clicked() || resp.dragged())
        && let Some(p) = resp.interact_pointer_pos()
    {
        let w = &mut s.animation;
        w.time = clip.snap(axis.t(p.x)).max(0.0);
        w.preview = true;
        w.playing = false;
    }
}

/// Key positions on a row: (time, x) for every distinct key time of its curves.
fn row_times(clip: &AnimationClip, curves: &[usize]) -> Vec<f32> {
    let mut times: Vec<f32> = curves
        .iter()
        .filter_map(|&c| clip.curves.get(c))
        .flat_map(|c| c.keys.as_slice().iter().map(|k| k.time))
        .collect();
    times.sort_by(f32::total_cmp);
    times.dedup();
    times
}

/// Keys (curve, time) of `curves` at `t`.
fn keys_at(clip: &AnimationClip, curves: &[usize], t: f32) -> Vec<(usize, u32)> {
    curves
        .iter()
        .filter_map(|&c| {
            let k = clip.curves.get(c)?.keys.find(t)?;
            Some((c, clip.curves[c].keys.get(k)?.time.to_bits()))
        })
        .collect()
}

fn diamond(painter: &egui::Painter, center: Pos2, r: f32, fill: Color32) {
    let pts = vec![
        center + vec2(0.0, -r),
        center + vec2(r, 0.0),
        center + vec2(0.0, r),
        center + vec2(-r, 0.0),
    ];
    painter.add(Shape::convex_polygon(
        pts,
        fill,
        Stroke::new(1.0, Color32::BLACK),
    ));
}

/// Summary row (all curves) followed by one line per property row.
fn dopesheet(
    ui: &mut Ui,
    s: &mut EditorState,
    clip: &AnimationClip,
    rows: &[Row],
    axis: Axis,
    body: Rect,
) {
    let all: Vec<usize> = (0..clip.curves.len()).collect();
    let mut lines: Vec<(Rect, &[usize])> = vec![(
        Rect::from_min_size(body.min, vec2(body.width(), ROW_H)),
        &all[..],
    )];
    for (i, row) in rows.iter().enumerate() {
        let r = Rect::from_min_size(
            pos2(body.left(), body.top() + (i + 1) as f32 * ROW_H),
            vec2(body.width(), ROW_H),
        );
        if r.bottom() > body.bottom() {
            break;
        }
        lines.push((r, &row.curves));
    }
    let painter = ui.painter().clone();
    for (i, (r, _)) in lines.iter().enumerate() {
        let fill = if i == 0 {
            Color32::from_rgb(50, 50, 50)
        } else if i % 2 == 1 {
            Color32::from_rgb(48, 48, 48)
        } else {
            Color32::from_rgb(44, 44, 44)
        };
        painter.rect_filled(*r, 0.0, fill);
    }
    // Keys under the pointer: the row's keys at the nearest time.
    let hit = |p: Pos2| -> Option<Vec<(usize, u32)>> {
        let (_, curves) = lines.iter().find(|(r, _)| r.contains(p))?;
        let t = row_times(clip, curves)
            .into_iter()
            .find(|t| (axis.x(*t) - p.x).abs() <= HIT)?;
        Some(keys_at(clip, curves, t))
    };
    let resp = ui.interact(body, ui.id().with("anim_keys"), Sense::click_and_drag());
    interact(ui, s, clip, &resp, &hit);
    // Double-clicking an empty spot of a property row adds keys there (values from the curves).
    if resp.double_clicked()
        && let Some(p) = resp.interact_pointer_pos()
        && hit(p).is_none()
        && let Some((_, curves)) = lines.iter().skip(1).find(|(r, _)| r.contains(p))
    {
        let (curves, t) = (curves.to_vec(), clip.snap(axis.t(p.x)).max(0.0));
        preview::edit_clip(s, "Add Key", |c| {
            for i in curves {
                if let Some(curve) = c.curves.get_mut(i) {
                    let v = curve.evaluate(t);
                    curve.keys.set(t, v);
                }
            }
            c.fit_length();
        });
    }
    box_outline(&painter, s, &resp);
    let box_select = |r: Rect| -> Vec<(usize, u32)> {
        let mut out = Vec::new();
        for (line, curves) in &lines {
            if !r.intersects(*line) {
                continue;
            }
            for t in row_times(clip, curves) {
                if r.x_range().contains(axis.x(t)) {
                    out.extend(keys_at(clip, curves, t));
                }
            }
        }
        out
    };
    finish_box(s, &resp, box_select);
    // Diamonds (a row's key is highlighted when all its keys at that time are selected).
    for (r, curves) in &lines {
        for t in row_times(clip, curves) {
            let x = axis.x(t);
            if !r.x_range().contains(x) {
                continue;
            }
            let keys = keys_at(clip, curves, t);
            let selected =
                !keys.is_empty() && keys.iter().all(|k| s.animation.selected.contains(k));
            let fill = if selected {
                Color32::from_rgb(90, 160, 255)
            } else {
                Color32::from_gray(200)
            };
            diamond(&painter, pos2(x, r.center().y), 5.0, fill);
        }
    }
}

/// Outline of a box selection in progress.
fn box_outline(painter: &egui::Painter, s: &EditorState, resp: &egui::Response) {
    if let Some(KeyDrag::Box { start }) = &s.animation.drag
        && let Some(p) = resp.interact_pointer_pos()
    {
        painter.rect_stroke(
            Rect::from_two_pos(*start, p),
            0.0,
            Stroke::new(1.0, theme::PREFAB),
            egui::StrokeKind::Inside,
        );
    }
}

/// Click / drag / context-menu handling shared by both views; `hit` finds the keys under a point.
fn interact(
    ui: &mut Ui,
    s: &mut EditorState,
    clip: &AnimationClip,
    resp: &egui::Response,
    hit: &dyn Fn(Pos2) -> Option<Vec<(usize, u32)>>,
) {
    let shift = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    let press = resp.interact_pointer_pos();
    if resp.clicked()
        && let Some(p) = press
    {
        select(s, hit(p), shift);
    }
    if resp.secondary_clicked()
        && let Some(p) = press
        && let Some(keys) = hit(p)
        && !keys.iter().all(|k| s.animation.selected.contains(k))
    {
        select(s, Some(keys), false);
    }
    resp.context_menu(|ui| key_menu(ui, s));
    if resp.drag_started()
        && let Some(p) = press
    {
        let start = ui.input(|i| i.pointer.press_origin()).unwrap_or(p);
        match hit(start) {
            Some(keys) => {
                if !keys.iter().all(|k| s.animation.selected.contains(k)) {
                    select(s, Some(keys), shift);
                }
                // One undo step for the whole drag.
                preview::edit_clip(s, "Move Keys", |_| {});
                s.animation.drag = Some(KeyDrag::Move {
                    base: clip.clone(),
                    grab: start,
                    keys: selected_keys(s),
                    value_per_px: 0.0,
                });
            }
            None => s.animation.drag = Some(KeyDrag::Box { start }),
        }
    }
    if resp.dragged()
        && let (Some(p), Some(KeyDrag::Move { .. })) = (press, &s.animation.drag)
    {
        move_keys(s, p);
    }
    if resp.drag_stopped() && matches!(s.animation.drag, Some(KeyDrag::Move { .. })) {
        s.animation.drag = None;
    }
}

/// Apply a finished box selection.
fn finish_box(
    s: &mut EditorState,
    resp: &egui::Response,
    select_in: impl Fn(Rect) -> Vec<(usize, u32)>,
) {
    if !resp.drag_stopped() {
        return;
    }
    if let Some(KeyDrag::Box { start }) = s.animation.drag.take()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let keys = select_in(Rect::from_two_pos(start, p));
        if !resp.ctx.input(|i| i.modifiers.shift) {
            s.animation.selected.clear();
        }
        s.animation.selected.extend(keys);
    }
}

fn select(s: &mut EditorState, keys: Option<Vec<(usize, u32)>>, add: bool) {
    let sel = &mut s.animation.selected;
    match keys {
        Some(keys) if add => {
            if keys.iter().all(|k| sel.contains(k)) {
                keys.iter().for_each(|k| {
                    sel.remove(k);
                });
            } else {
                sel.extend(keys);
            }
        }
        Some(keys) => {
            sel.clear();
            sel.extend(keys);
        }
        None if !add => sel.clear(),
        None => {}
    }
}

fn selected_keys(s: &EditorState) -> Vec<(usize, f32)> {
    s.animation
        .selected
        .iter()
        .map(|&(c, t)| (c, f32::from_bits(t)))
        .collect()
}

/// Move the dragged keys by the pointer offset (time snapped to frames).
fn move_keys(s: &mut EditorState, p: Pos2) {
    let Some(KeyDrag::Move {
        base,
        grab,
        keys,
        value_per_px,
    }) = &s.animation.drag
    else {
        return;
    };
    let pps = s.animation.pixels_per_second.max(1e-3);
    let mut clip = base.clone();
    let dt = clip.snap((p.x - grab.x) / pps);
    let dv = (grab.y - p.y) * value_per_px;
    let mut moved = Vec::new();
    for &(c, t) in keys {
        let Some(curve) = clip.curves.get_mut(c) else {
            continue;
        };
        if let Some(i) = curve.keys.find(t) {
            let k = *curve.keys.get(i).expect("found key");
            curve.keys.remove(i);
            moved.push((c, k, (t + dt).max(0.0)));
        }
    }
    let mut selected = std::collections::BTreeSet::new();
    for (c, k, t) in moved {
        let curve = &mut clip.curves[c];
        let i = curve.keys.set(t, k.value + dv);
        curve.keys.set_tangent(i, k.tangent);
        selected.insert((c, curve.keys.get(i).expect("set key").time.to_bits()));
    }
    clip.fit_length();
    if let Some(path) = s.animation.clip.clone()
        && let Some(target) = s.anim_clips.get_mut(&path)
    {
        *target = clip;
    }
    s.animation.selected = selected;
}

/// Right-click menu on keys: delete or change tangents.
fn key_menu(ui: &mut Ui, s: &mut EditorState) {
    let has = !s.animation.selected.is_empty();
    if ui
        .add_enabled(has, egui::Button::new("Delete Key"))
        .clicked()
    {
        preview::delete_selected_keys(s);
        ui.close();
    }
    ui.separator();
    for mode in TangentMode::ALL {
        if ui
            .add_enabled(has, egui::Button::new(mode.label()))
            .clicked()
        {
            let keys = selected_keys(s);
            preview::edit_clip(s, "Change Tangents", |clip| {
                for (c, t) in keys {
                    if let Some(curve) = clip.curves.get_mut(c)
                        && let Some(i) = curve.keys.find(t)
                    {
                        curve.keys.set_tangent(i, mode);
                    }
                }
            });
            ui.close();
        }
    }
}

/// Colour of a curve in the curve editor (x/r red, y/g green, z/b blue, others yellow).
fn curve_color(p: misty_engine::animation::Property) -> Color32 {
    use misty_engine::animation::Property::*;
    match p {
        Position(0) | Rotation(0) | Scale(0) | LightColor(0) => theme::AXIS_X,
        Position(1) | Rotation(1) | Scale(1) | LightColor(1) => theme::AXIS_Y,
        Position(_) | Rotation(_) | Scale(_) | LightColor(_) => theme::AXIS_Z,
        _ => Color32::from_rgb(230, 200, 80),
    }
}

/// Curve editor: the shown curves in one auto-fitted value range, keys as dots.
fn curves(
    ui: &mut Ui,
    s: &mut EditorState,
    clip: &AnimationClip,
    _rows: &[Row],
    axis: Axis,
    body: Rect,
) {
    let focused = &s.animation.focused_curves;
    let shown: Vec<usize> = (0..clip.curves.len())
        .filter(|c| focused.is_empty() || focused.contains(c))
        .collect();
    let (mut min, mut max) = (f32::INFINITY, f32::NEG_INFINITY);
    let (t0, t1) = (axis.t(body.left()), axis.t(body.right()));
    for &c in &shown {
        let curve = &clip.curves[c];
        for k in curve.keys.as_slice() {
            min = min.min(k.value);
            max = max.max(k.value);
        }
        for i in 0..=32 {
            let v = curve.evaluate(t0 + (t1 - t0) * i as f32 / 32.0);
            min = min.min(v);
            max = max.max(v);
        }
    }
    if !min.is_finite() {
        (min, max) = (-1.0, 1.0);
    }
    if max - min < 1e-3 {
        (min, max) = (min - 1.0, max + 1.0);
    }
    let margin = (max - min) * 0.1;
    let va = ValueAxis {
        rect: body.shrink2(vec2(0.0, 8.0)),
        min: min - margin,
        max: max + margin,
    };
    let painter = ui.painter().clone();
    // Value grid with labels.
    for i in 0..=4 {
        let v = va.min + (va.max - va.min) * i as f32 / 4.0;
        let y = va.y(v);
        painter.hline(body.x_range(), y, Stroke::new(1.0, Color32::from_gray(56)));
        painter.text(
            pos2(body.left() + 2.0, y - 1.0),
            Align2::LEFT_BOTTOM,
            format!("{v:.2}"),
            FontId::proportional(10.0),
            theme::TEXT_DIM,
        );
    }
    for &c in &shown {
        let curve = &clip.curves[c];
        let color = curve_color(curve.property);
        let steps = (body.width() / 2.0).max(2.0) as usize;
        let pts: Vec<Pos2> = (0..=steps)
            .map(|i| {
                let x = body.left() + body.width() * i as f32 / steps as f32;
                pos2(x, va.y(curve.evaluate(axis.t(x))))
            })
            .collect();
        painter.add(Shape::line(pts, Stroke::new(1.5, color)));
    }
    let point = |c: usize, t: f32| -> Option<Pos2> {
        let k = clip.curves[c].keys.find(t)?;
        Some(pos2(axis.x(t), va.y(clip.curves[c].keys.get(k)?.value)))
    };
    let hit = |p: Pos2| -> Option<Vec<(usize, u32)>> {
        shown.iter().rev().find_map(|&c| {
            clip.curves[c].keys.as_slice().iter().find_map(|k| {
                let q = point(c, k.time)?;
                (q.distance(p) <= HIT).then(|| vec![(c, k.time.to_bits())])
            })
        })
    };
    let resp = ui.interact(body, ui.id().with("anim_curves"), Sense::click_and_drag());
    interact(ui, s, clip, &resp, &hit);
    if let Some(KeyDrag::Move { value_per_px, .. }) = &mut s.animation.drag {
        *value_per_px = va.per_px();
    }
    box_outline(&painter, s, &resp);
    finish_box(s, &resp, |r| {
        let mut out = Vec::new();
        for &c in &shown {
            for k in clip.curves[c].keys.as_slice() {
                if point(c, k.time).is_some_and(|q| r.contains(q)) {
                    out.push((c, k.time.to_bits()));
                }
            }
        }
        out
    });
    for &c in &shown {
        for k in clip.curves[c].keys.as_slice() {
            let Some(q) = point(c, k.time) else { continue };
            let selected = s.animation.selected.contains(&(c, k.time.to_bits()));
            let fill = if selected {
                Color32::WHITE
            } else {
                curve_color(clip.curves[c].property)
            };
            painter.circle(q, 3.5, fill, Stroke::new(1.0, Color32::BLACK));
        }
    }
}
