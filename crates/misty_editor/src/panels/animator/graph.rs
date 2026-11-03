//! The state machine graph: grid, state nodes (default state orange, Any State, Entry, Exit),
//! transition arrows, pan (middle mouse or Alt+drag) and zoom (wheel), dragging nodes, the
//! right-click menus and, in Play mode, the current state's progress bar.

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::AnimatorStatus;
use misty_engine::animation::controller::{BlendTree, Node, Transition};
use misty_engine::scene::clip_name;

use super::{GraphItem, delete, select};
use crate::anim_controllers::{self, interacting};
use crate::panels::AssetPayload;
use crate::state::EditorState;

/// Selection outline, live progress and the transition being played.
pub const LIVE_BLUE: Color32 = Color32::from_rgb(70, 150, 245);
const NODE_SIZE: Vec2 = vec2(170.0, 38.0);
const STATE_FILL: Color32 = Color32::from_rgb(92, 92, 92);
const DEFAULT_FILL: Color32 = Color32::from_rgb(196, 110, 36);
const ENTRY_FILL: Color32 = Color32::from_rgb(54, 128, 60);
const ANY_FILL: Color32 = Color32::from_rgb(52, 130, 140);
const EXIT_FILL: Color32 = Color32::from_rgb(160, 48, 48);
const ARROW: Color32 = Color32::from_gray(205);
const CANVAS_BG: Color32 = Color32::from_rgb(38, 38, 38);

/// Graph ↔ screen mapping: screen = centre + (graph + pan) × zoom.
#[derive(Clone, Copy)]
struct View {
    centre: Pos2,
    pan: Vec2,
    zoom: f32,
}

impl View {
    fn screen(&self, p: [f32; 2]) -> Pos2 {
        self.centre + (vec2(p[0], p[1]) + self.pan) * self.zoom
    }

    fn graph(&self, p: Pos2) -> Pos2 {
        pos2(0.0, 0.0) + ((p - self.centre) / self.zoom - self.pan)
    }

    fn rect(&self, p: [f32; 2]) -> Rect {
        Rect::from_center_size(self.screen(p), NODE_SIZE * self.zoom)
    }
}

/// Every node in drawing order: the special nodes, then states.
fn nodes(c: &AnimatorController) -> Vec<(Node, [f32; 2])> {
    let mut out = vec![
        (Node::Entry, c.nodes.entry),
        (Node::AnyState, c.nodes.any_state),
        (Node::Exit, c.nodes.exit),
    ];
    out.extend(
        c.states
            .iter()
            .map(|s| (Node::State(s.name.clone()), s.position)),
    );
    out
}

fn node_pos(c: &AnimatorController, n: &Node) -> Option<[f32; 2]> {
    match n {
        Node::Entry => Some(c.nodes.entry),
        Node::AnyState => Some(c.nodes.any_state),
        Node::Exit => Some(c.nodes.exit),
        Node::State(name) => c.state(name).map(|s| s.position),
    }
}

fn set_node_pos(c: &mut AnimatorController, n: &Node, p: [f32; 2]) {
    match n {
        Node::Entry => c.nodes.entry = p,
        Node::AnyState => c.nodes.any_state = p,
        Node::Exit => c.nodes.exit = p,
        Node::State(name) => {
            if let Some(s) = c.state_mut(name) {
                s.position = p;
            }
        }
    }
}

pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    canvas: Rect,
    path: &str,
    c: &AnimatorController,
    live: Option<&AnimatorStatus>,
) {
    let canvas_resp = ui.interact(
        canvas,
        ui.id().with("animator_canvas"),
        Sense::click_and_drag(),
    );
    let painter = ui.painter_at(canvas);
    painter.rect_filled(canvas, 0.0, CANVAS_BG);
    navigate(ui, s, canvas, &canvas_resp);
    let view = View {
        centre: canvas.center(),
        pan: s.animator.pan,
        zoom: s.animator.zoom(),
    };
    draw_grid(&painter, canvas, view);

    // Transitions under the nodes.
    let hovered_transition = canvas_resp
        .hover_pos()
        .and_then(|p| transition_at(c, view, p));
    // Entry always leads to the default state.
    if let Some(d) = c.default_state.as_deref().and_then(|d| c.state(d)) {
        let entry = view.screen(c.nodes.entry);
        arrow(
            &painter,
            entry,
            view.screen(d.position),
            view.zoom,
            DEFAULT_FILL,
        );
    }
    let playing = live.and_then(|l| {
        Some((
            l.current.as_ref()?.name.clone(),
            l.next.as_ref()?.0.name.clone(),
        ))
    });
    for (i, t) in c.transitions.iter().enumerate() {
        let (Some(a), Some(b)) = (node_pos(c, &t.from), node_pos(c, &t.to)) else {
            continue;
        };
        let selected = s.animator.selected == Some(GraphItem::Transition(i));
        let active = playing.as_ref().is_some_and(|(from, to)| {
            (t.from == Node::state(from) || t.from == Node::AnyState) && t.to == Node::state(to)
        });
        let color = if selected || active {
            LIVE_BLUE
        } else if hovered_transition == Some(i) {
            Color32::WHITE
        } else {
            ARROW
        };
        arrow(&painter, view.screen(a), view.screen(b), view.zoom, color);
    }

    // Make Transition: an arrow follows the mouse.
    if let Some(from) = s.animator.making.clone() {
        if let (Some(a), Some(p)) = (node_pos(c, &from), ui.input(|i| i.pointer.hover_pos())) {
            arrow(&painter, view.screen(a), p, view.zoom, Color32::WHITE);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.animator.making = None;
        }
    }

    let mut clicked_node = false;
    for (node, pos) in nodes(c) {
        let rect = view.rect(pos);
        if !rect.intersects(canvas) {
            continue;
        }
        let id = ui.id().with(("animator_node", &node));
        let resp = ui.interact(rect.intersect(canvas), id, Sense::click_and_drag());
        let label = node.label().to_owned();
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        draw_node(&painter, s, c, &node, rect, view.zoom, live);
        clicked_node |= node_input(ui, s, path, &node, pos, &resp, view);
    }

    // Empty canvas: click deselects (or picks a transition), right-click opens the Create menu.
    if canvas_resp.clicked() && !clicked_node {
        if s.animator.making.is_some() {
            s.animator.making = None;
        } else {
            select(s, path, hovered_transition.map(GraphItem::Transition));
        }
    }
    if canvas_resp.secondary_clicked() {
        s.animator.making = None;
        let at = canvas_resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |p| view.graph(p));
        s.animator.menu = Some((hovered_transition.map(GraphItem::Transition), at));
    }
    canvas_resp.context_menu(|ui| canvas_menu(ui, s, path));

    // A clip dropped from the Project window becomes a state playing it.
    if let Some(p) = canvas_resp.dnd_release_payload::<AssetPayload>()
        && misty_engine::animation::is_clip(&p.0)
    {
        let at = canvas_resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |q| view.graph(q));
        let clip = p.0.clone();
        let mut name = String::new();
        anim_controllers::edit(s, path, "Create State", false, |c| {
            name = c.add_state(&clip_name(&clip), [at.x, at.y]);
            if let Some(st) = c.state_mut(&name) {
                st.motion = Some(clip.clone());
            }
        });
        select(s, path, Some(GraphItem::Node(Node::State(name))));
    }
}

/// Wheel zoom (around the mouse) and middle-mouse / Alt+left drag panning.
fn navigate(ui: &Ui, s: &mut EditorState, canvas: Rect, resp: &Response) {
    if let Some(p) = resp.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let old = s.animator.zoom();
            let zoom = (old * (scroll * 0.002).exp()).clamp(0.25, 2.5);
            let centre = canvas.center();
            let g = (p - centre) / old - s.animator.pan;
            s.animator.zoom = zoom;
            s.animator.pan = (p - centre) / zoom - g;
        }
    }
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.dragged_by(egui::PointerButton::Middle)
        || (alt && resp.dragged_by(egui::PointerButton::Primary))
    {
        s.animator.pan += resp.drag_delta() / s.animator.zoom();
    }
}

fn draw_grid(painter: &egui::Painter, canvas: Rect, view: View) {
    for (step, gray) in [(20.0, 44u8), (100.0, 52u8)] {
        let px = step * view.zoom;
        if px < 6.0 {
            continue;
        }
        let stroke = Stroke::new(1.0, Color32::from_gray(gray));
        let origin = view.screen([0.0, 0.0]);
        let mut x = origin.x - ((origin.x - canvas.left()) / px).floor() * px;
        while x < canvas.right() {
            painter.vline(x, canvas.y_range(), stroke);
            x += px;
        }
        let mut y = origin.y - ((origin.y - canvas.top()) / px).floor() * px;
        while y < canvas.bottom() {
            painter.hline(canvas.x_range(), y, stroke);
            y += px;
        }
    }
}

/// A transition line with an arrow head halfway. The line is shifted to its right so A→B and
/// B→A stay apart.
fn arrow(painter: &egui::Painter, a: Pos2, b: Pos2, zoom: f32, color: Color32) {
    let d = b - a;
    if d.length() < 1.0 {
        return;
    }
    let dir = d.normalized();
    let side = vec2(-dir.y, dir.x) * 5.0 * zoom;
    let (a, b) = (a + side, b + side);
    painter.line_segment([a, b], Stroke::new(2.0 * zoom.max(0.5), color));
    let mid = a + d * 0.5;
    let size = 7.0 * zoom.max(0.5);
    let tip = mid + dir * size;
    let back = mid - dir * size;
    let n = vec2(-dir.y, dir.x) * size;
    painter.add(egui::Shape::convex_polygon(
        vec![tip, back + n, back - n],
        color,
        Stroke::NONE,
    ));
}

/// The transition whose line passes near `p` (screen).
fn transition_at(c: &AnimatorController, view: View, p: Pos2) -> Option<usize> {
    c.transitions.iter().enumerate().find_map(|(i, t)| {
        let a = view.screen(node_pos(c, &t.from)?);
        let b = view.screen(node_pos(c, &t.to)?);
        let d = b - a;
        let side = vec2(-d.y, d.x).normalized() * 5.0 * view.zoom;
        let (a, b) = (a + side, b + side);
        let t = ((p - a).dot(b - a) / (b - a).length_sq()).clamp(0.0, 1.0);
        ((a + (b - a) * t).distance(p) < 6.0).then_some(i)
    })
}

fn draw_node(
    painter: &egui::Painter,
    s: &EditorState,
    c: &AnimatorController,
    node: &Node,
    rect: Rect,
    zoom: f32,
    live: Option<&AnimatorStatus>,
) {
    let is_default =
        matches!(node, Node::State(n) if c.default_state.as_deref() == Some(n.as_str()));
    let fill = match node {
        Node::Entry => ENTRY_FILL,
        Node::AnyState => ANY_FILL,
        Node::Exit => EXIT_FILL,
        Node::State(_) if is_default => DEFAULT_FILL,
        Node::State(_) => STATE_FILL,
    };
    let round = CornerRadius::same((5.0 * zoom).round().clamp(1.0, 12.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(2.0, 3.0) * zoom),
        round,
        Color32::from_black_alpha(90),
    );
    painter.rect_filled(rect, round, fill);
    let selected = s.animator.selected.as_ref() == Some(&GraphItem::Node(node.clone()));
    let outline = if selected {
        Stroke::new(2.5, LIVE_BLUE)
    } else {
        Stroke::new(1.0, Color32::from_black_alpha(160))
    };
    painter.rect_stroke(rect, round, outline, StrokeKind::Outside);
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        node.label(),
        FontId::proportional(13.0 * zoom),
        Color32::WHITE,
    );
    // Live: progress bar of the current state (and of the cross-fade destination).
    let Node::State(name) = node else { return };
    let Some(l) = live else { return };
    let runs = l
        .current
        .iter()
        .map(|i| (i, 1.0))
        .chain(l.next.iter().map(|(i, _)| (i, 0.6)));
    for (info, alpha) in runs {
        if info.name != *name {
            continue;
        }
        let t = if info.looping {
            info.normalized_time.rem_euclid(1.0)
        } else {
            info.normalized_time.clamp(0.0, 1.0)
        };
        let bar = Rect::from_min_size(
            pos2(rect.left() + 4.0 * zoom, rect.bottom() - 7.0 * zoom),
            vec2((rect.width() - 8.0 * zoom) * t, 4.0 * zoom),
        );
        let track = Rect::from_min_size(bar.min, vec2(rect.width() - 8.0 * zoom, 4.0 * zoom));
        painter.rect_filled(track, 1.0, Color32::from_black_alpha(120));
        painter.rect_filled(bar, 1.0, LIVE_BLUE.gamma_multiply(alpha));
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(2.0, LIVE_BLUE.gamma_multiply(alpha)),
            StrokeKind::Outside,
        );
    }
}

/// Click / drag / right-click on one node. Returns true if the node was clicked.
fn node_input(
    ui: &Ui,
    s: &mut EditorState,
    path: &str,
    node: &Node,
    pos: [f32; 2],
    resp: &Response,
    view: View,
) -> bool {
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt {
        s.animator.drag = Some(node.clone());
        select(s, path, Some(GraphItem::Node(node.clone())));
    }
    if s.animator.drag.as_ref() == Some(node) && resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta() / view.zoom;
        if d != Vec2::ZERO {
            let n = node.clone();
            let to = [pos[0] + d.x, pos[1] + d.y];
            anim_controllers::edit(s, path, "Move State", interacting(ui.ctx()), |c| {
                set_node_pos(c, &n, to)
            });
        }
    }
    if resp.drag_stopped() {
        s.animator.drag = None;
    }
    let mut clicked = false;
    if resp.clicked() {
        clicked = true;
        match s.animator.making.take() {
            Some(from) => make_transition(s, path, from, node.clone()),
            None => select(s, path, Some(GraphItem::Node(node.clone()))),
        }
    }
    if resp.secondary_clicked() {
        s.animator.making = None;
        select(s, path, Some(GraphItem::Node(node.clone())));
        s.animator.menu = Some((Some(GraphItem::Node(node.clone())), pos2(pos[0], pos[1])));
    }
    resp.context_menu(|ui| node_menu(ui, s, path, node));
    clicked
}

/// Finish Make Transition on `to` (states and Exit only).
fn make_transition(s: &mut EditorState, path: &str, from: Node, to: Node) {
    if matches!(to, Node::Entry | Node::AnyState) || matches!(from, Node::Exit | Node::Entry) {
        return;
    }
    let mut index = 0;
    anim_controllers::edit(s, path, "Make Transition", false, |c| {
        c.transitions.push(Transition::new(from, to));
        index = c.transitions.len() - 1;
    });
    select(s, path, Some(GraphItem::Transition(index)));
}

fn node_menu(ui: &mut Ui, s: &mut EditorState, path: &str, node: &Node) {
    let can_start = matches!(node, Node::State(_) | Node::AnyState);
    if ui
        .add_enabled(can_start, egui::Button::new("Make Transition"))
        .clicked()
    {
        s.animator.making = Some(node.clone());
        ui.close();
    }
    if let Node::State(name) = node {
        if ui.button("Set as Layer Default State").clicked() {
            let name = name.clone();
            anim_controllers::edit(s, path, "Set Default State", false, |c| {
                c.default_state = Some(name)
            });
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            delete(s, path, &GraphItem::Node(node.clone()));
            ui.close();
        }
    }
}

/// Right-click on empty canvas (Create State) or on a transition (Delete).
fn canvas_menu(ui: &mut Ui, s: &mut EditorState, path: &str) {
    let Some((target, at)) = s.animator.menu.clone() else {
        ui.close();
        return;
    };
    if let Some(item @ GraphItem::Transition(_)) = target {
        if ui.button("Delete").clicked() {
            delete(s, path, &item);
            ui.close();
        }
        return;
    }
    ui.menu_button("Create State", |ui| {
        for (label, tree) in [("Empty", false), ("From New Blend Tree", true)] {
            if ui.button(label).clicked() {
                let mut name = String::new();
                anim_controllers::edit(s, path, "Create State", false, |c| {
                    let base = if tree { "Blend Tree" } else { "New State" };
                    name = c.add_state(base, [at.x, at.y]);
                    if tree && let Some(st) = c.state_mut(&name) {
                        st.blend_tree = Some(BlendTree::default());
                    }
                });
                select(s, path, Some(GraphItem::Node(Node::State(name))));
                ui.close();
            }
        }
    });
}
