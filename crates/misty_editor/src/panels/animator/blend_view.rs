//! Blend tree diagram and the Animator window's blend tree view.
//!
//! 1D: each motion's influence as a triangle along the parameter line. 2D: the motions as
//! points on the parameter plane, the selected motion's influence as a heat map, each motion's
//! current weight as a circle. Direct: one bar per motion. A red marker shows the parameter
//! values; dragging it changes them (the defaults in Edit mode, the live values in Play mode).

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, Vec2, pos2, vec2};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::{BlendTree, BlendType, ParamKind};
use misty_engine::scene::clip_name;

use crate::anim_controllers::{self, interacting};
use crate::state::EditorState;

const MOTION_BLUE: Color32 = Color32::from_rgb(70, 150, 245);
const PREVIEW_RED: Color32 = Color32::from_rgb(235, 70, 60);
const PLOT_BG: Color32 = Color32::from_rgb(30, 30, 30);
/// Cells per side of the 2D influence heat map.
const HEAT_CELLS: usize = 28;

/// Accessible name of the diagram (tests drag it).
pub const DIAGRAM_LABEL: &str = "Blend Tree Preview";

/// Value of a parameter shown by the editor: live in Play mode, the default otherwise.
pub fn param_value(c: &AnimatorController, live: Option<&AnimatorStatus>, name: &str) -> f32 {
    live.and_then(|l| l.params.iter().find(|(n, _, _)| n == name).map(|p| p.2))
        .or_else(|| c.parameter(name).map(|p| p.default))
        .unwrap_or(0.0)
}

/// Set a float parameter from the diagram: live while playing, else its saved default.
pub fn set_param(s: &mut EditorState, path: &str, name: &str, v: f32, live: bool, busy: bool) {
    if name.is_empty() {
        return;
    }
    if live {
        if let (Some(rt), Some(e)) = (s.runtime.as_mut(), s.animator.target) {
            rt.set_animator_param(&s.scene, e, name, v);
        }
        return;
    }
    let name = name.to_owned();
    anim_controllers::edit(s, path, "Parameter Default", busy, |c| {
        if let Some(p) = c.parameters.iter_mut().find(|p| p.name == name) {
            p.default = v;
        }
    });
}

/// Range shown on each axis: the motions' extent with a margin, at least -1..1 in 2D.
fn bounds(tree: &BlendTree) -> (Pos2, Pos2) {
    if tree.blend_type == BlendType::Simple1D {
        let (lo, hi) = tree
            .children
            .iter()
            .map(|c| c.threshold)
            .fold((f32::MAX, f32::MIN), |(a, b), t| (a.min(t), b.max(t)));
        let (lo, hi) = if lo > hi { (0.0, 1.0) } else { (lo, hi) };
        let pad = ((hi - lo) * 0.08).max(0.1);
        return (pos2(lo - pad, 0.0), pos2(hi + pad, 1.0));
    }
    let mut lo = pos2(-1.0, -1.0);
    let mut hi = pos2(1.0, 1.0);
    for c in &tree.children {
        lo = lo.min(pos2(c.position[0], c.position[1]));
        hi = hi.max(pos2(c.position[0], c.position[1]));
    }
    let pad = (hi - lo) * 0.1;
    (lo - pad, hi + pad)
}

/// Draw the diagram in `rect`. Returns new parameter values when the marker was dragged.
pub fn diagram(
    ui: &mut Ui,
    rect: Rect,
    tree: &BlendTree,
    value: [f32; 2],
    param: &dyn Fn(&str) -> f32,
    selected: Option<usize>,
) -> Option<[f32; 2]> {
    let resp = ui.interact(rect, ui.id().with("blend_diagram"), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Other, true, DIAGRAM_LABEL));
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 3.0, PLOT_BG);
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, Color32::from_gray(70)),
        egui::StrokeKind::Inside,
    );
    let weights = tree.weights_at(value[0], value[1], param);
    let font = FontId::proportional(11.0);
    let name = |i: usize| {
        tree.children[i]
            .motion
            .as_deref()
            .map(clip_name)
            .unwrap_or_else(|| "None".into())
    };
    let mut moved = None;
    match tree.blend_type {
        BlendType::Direct => {
            let n = tree.children.len().max(1);
            let row = (rect.height() - 8.0) / n as f32;
            for (i, w) in weights.iter().enumerate() {
                let top = rect.top() + 4.0 + row * i as f32;
                let track = Rect::from_min_size(
                    pos2(rect.left() + 90.0, top + row * 0.25),
                    vec2(rect.width() - 100.0, row * 0.5),
                );
                painter.rect_filled(track, 2.0, Color32::from_gray(50));
                let mut bar = track;
                bar.set_width(track.width() * w.clamp(0.0, 1.0));
                painter.rect_filled(bar, 2.0, MOTION_BLUE);
                painter.text(
                    pos2(rect.left() + 6.0, top + row * 0.5),
                    Align2::LEFT_CENTER,
                    name(i),
                    font.clone(),
                    Color32::from_gray(210),
                );
            }
        }
        BlendType::Simple1D => {
            let (lo, hi) = bounds(tree);
            let to_x = |v: f32| rect.left() + (v - lo.x) / (hi.x - lo.x) * rect.width();
            let base = rect.bottom() - 18.0;
            let mut order: Vec<usize> = (0..tree.children.len()).collect();
            order.sort_by(|a, b| {
                tree.children[*a]
                    .threshold
                    .total_cmp(&tree.children[*b].threshold)
            });
            for (k, &i) in order.iter().enumerate() {
                let t = tree.children[i].threshold;
                let left = k
                    .checked_sub(1)
                    .map_or(t, |p| tree.children[order[p]].threshold);
                let right = order.get(k + 1).map_or(t, |&n| tree.children[n].threshold);
                let peak = pos2(to_x(t), rect.top() + 14.0);
                let fill = if Some(i) == selected {
                    MOTION_BLUE.gamma_multiply(0.55)
                } else {
                    MOTION_BLUE.gamma_multiply(0.25)
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![pos2(to_x(left), base), peak, pos2(to_x(right), base)],
                    fill,
                    Stroke::new(1.0, MOTION_BLUE),
                ));
                painter.text(
                    pos2(peak.x, base + 2.0),
                    Align2::CENTER_TOP,
                    format!("{} ({:.0}%)", name(i), weights[i] * 100.0),
                    font.clone(),
                    Color32::from_gray(210),
                );
            }
            let x = to_x(value[0]);
            painter.vline(x, rect.top()..=base, Stroke::new(2.0, PREVIEW_RED));
            if let Some(p) = resp.interact_pointer_pos()
                && (resp.dragged() || resp.clicked())
            {
                let v = lo.x + (p.x - rect.left()) / rect.width() * (hi.x - lo.x);
                moved = Some([v, value[1]]);
            }
        }
        _ => {
            let (lo, hi) = bounds(tree);
            let to_screen = |v: [f32; 2]| {
                pos2(
                    rect.left() + (v[0] - lo.x) / (hi.x - lo.x) * rect.width(),
                    rect.bottom() - (v[1] - lo.y) / (hi.y - lo.y) * rect.height(),
                )
            };
            let to_value = |p: Pos2| {
                [
                    lo.x + (p.x - rect.left()) / rect.width() * (hi.x - lo.x),
                    lo.y + (rect.bottom() - p.y) / rect.height() * (hi.y - lo.y),
                ]
            };
            // The selected motion's influence over the plane.
            if let Some(sel) = selected.filter(|i| *i < tree.children.len()) {
                let cell = vec2(rect.width(), rect.height()) / HEAT_CELLS as f32;
                for iy in 0..HEAT_CELLS {
                    for ix in 0..HEAT_CELLS {
                        let min = rect.min + vec2(ix as f32 * cell.x, iy as f32 * cell.y);
                        let v = to_value(min + cell * 0.5);
                        let w = tree.weights_at(v[0], v[1], param)[sel];
                        if w > 0.01 {
                            painter.rect_filled(
                                Rect::from_min_size(min, cell + Vec2::splat(0.5)),
                                0.0,
                                MOTION_BLUE.gamma_multiply(w * 0.6),
                            );
                        }
                    }
                }
            }
            let origin = to_screen([0.0, 0.0]);
            let axis = Stroke::new(1.0, Color32::from_gray(64));
            painter.hline(rect.x_range(), origin.y, axis);
            painter.vline(origin.x, rect.y_range(), axis);
            for (i, c) in tree.children.iter().enumerate() {
                let p = to_screen(c.position);
                let w = weights[i];
                if w > 0.001 {
                    painter.circle_stroke(
                        p,
                        4.0 + 18.0 * w.sqrt(),
                        Stroke::new(1.5, MOTION_BLUE.gamma_multiply(0.8)),
                    );
                }
                let dot = if Some(i) == selected {
                    Color32::WHITE
                } else {
                    MOTION_BLUE
                };
                painter.add(egui::Shape::convex_polygon(
                    vec![
                        p + vec2(0.0, -5.0),
                        p + vec2(5.0, 0.0),
                        p + vec2(0.0, 5.0),
                        p + vec2(-5.0, 0.0),
                    ],
                    dot,
                    Stroke::NONE,
                ));
                painter.text(
                    p + vec2(7.0, -7.0),
                    Align2::LEFT_BOTTOM,
                    name(i),
                    font.clone(),
                    Color32::from_gray(200),
                );
            }
            let marker = to_screen(value);
            painter.circle_filled(marker, 5.0, PREVIEW_RED);
            painter.circle_stroke(marker, 5.0, Stroke::new(1.0, Color32::WHITE));
            if let Some(p) = resp.interact_pointer_pos()
                && (resp.dragged() || resp.clicked())
            {
                moved = Some(to_value(rect.clamp(p)));
            }
        }
    }
    moved
}

/// Parameter sliders for a tree (x, and y for 2D trees). Returns new values when changed.
fn parameter_sliders(ui: &mut Ui, tree: &BlendTree, value: [f32; 2]) -> Option<[f32; 2]> {
    let mut v = value;
    let mut ch = false;
    ui.horizontal(|ui| {
        if tree.blend_type == BlendType::Direct {
            ui.weak("Each motion's weight is its own parameter.");
            return;
        }
        let names = if tree.blend_type.is_2d() {
            vec![(&tree.parameter, 0), (&tree.parameter_y, 1)]
        } else {
            vec![(&tree.parameter, 0)]
        };
        for (name, i) in names {
            ui.label(if name.is_empty() {
                "None"
            } else {
                name.as_str()
            });
            ch |= ui
                .add(egui::DragValue::new(&mut v[i]).speed(0.01).max_decimals(3))
                .changed();
        }
    });
    ch.then_some(v)
}

/// The Animator window's blend tree view of state `state` on the shown layer.
pub fn show_tree_view(
    ui: &mut Ui,
    s: &mut EditorState,
    canvas: Rect,
    path: &str,
    c: &AnimatorController,
    state: &str,
    live: Option<&AnimatorStatus>,
) {
    let layer = s.animator.layer;
    let Some(tree) = c
        .machine(layer)
        .and_then(|m| m.state(state))
        .and_then(|st| st.blend_tree.clone())
    else {
        s.animator.blend_view = None;
        return;
    };
    ui.painter()
        .rect_filled(canvas, 0.0, Color32::from_rgb(38, 38, 38));
    let busy = interacting(ui.ctx());
    let playing = live.is_some();
    let param = |n: &str| param_value(c, live, n);
    let value = [param(&tree.parameter), param(&tree.parameter_y)];
    let inner = canvas.shrink(12.0);
    ui.scope_builder(egui::UiBuilder::new().max_rect(inner), |ui| {
        ui.horizontal(|ui| {
            ui.label(egui::RichText::new(state).strong());
            ui.weak(format!("{} Blend Tree", tree.blend_type.label()));
        });
        let changed = parameter_sliders(ui, &tree, value);
        ui.add_space(4.0);
        let side = (inner.width() - 260.0)
            .min(inner.height() - 70.0)
            .max(120.0);
        let plot = Rect::from_min_size(ui.cursor().min, vec2(side, side));
        let height = if tree.blend_type == BlendType::Simple1D {
            (side * 0.4).max(90.0)
        } else {
            side
        };
        let plot = Rect::from_min_size(plot.min, vec2(side, height));
        let selected = s.animator.blend_selected;
        let moved = diagram(ui, plot, &tree, value, &param, selected);
        // Motions and their weights on the right.
        let list = Rect::from_min_max(
            pos2(plot.right() + 16.0, plot.top()),
            pos2(inner.right(), inner.bottom()),
        );
        let weights = tree.weights_at(value[0], value[1], &param);
        ui.scope_builder(egui::UiBuilder::new().max_rect(list), |ui| {
            ui.label(egui::RichText::new("Motions").strong());
            for (i, ch) in tree.children.iter().enumerate() {
                let label = format!(
                    "{}  {:.0}%",
                    ch.motion
                        .as_deref()
                        .map(clip_name)
                        .unwrap_or_else(|| "None".into()),
                    weights[i] * 100.0
                );
                if ui.selectable_label(selected == Some(i), label).clicked() {
                    s.animator.blend_selected = Some(i);
                }
            }
        });
        let new = moved.or(changed);
        if let Some(v) = new {
            let names = [tree.parameter.clone(), tree.parameter_y.clone()];
            for (i, name) in names.iter().enumerate() {
                let float = c
                    .parameter(name)
                    .is_some_and(|p| p.kind == ParamKind::Float);
                if float && (v[i] - value[i]).abs() > 1e-6 {
                    set_param(s, path, name, v[i], playing, busy || moved.is_some());
                }
            }
        }
    });
}
