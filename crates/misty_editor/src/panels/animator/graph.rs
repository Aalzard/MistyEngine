//! The state machine graph of one layer, at one sub-state machine: grid, state nodes (default
//! state orange), sub-state machine nodes (stacked), Any State, Entry, Exit and "(Up)", transition
//! arrows (a transition to or from a state elsewhere is drawn from the sub-machine holding it or
//! from "(Up)"), pan (middle mouse or Alt+drag) and zoom (wheel), dragging nodes, the right-click
//! menus, double-click to enter a sub-machine or a blend tree and, in Play mode, the current
//! state's progress bar.

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};
use misty_engine::animation::AnimatorController;
use misty_engine::animation::animator::LayerStatus;
use misty_engine::animation::controller::{
    BlendTree, Node, StateMachine, Transition, machine_name, machine_parent, machine_within,
};
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
const MACHINE_FILL: Color32 = Color32::from_rgb(74, 74, 82);
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

/// Every node of machine `at` in drawing order: the special nodes, sub-machines, then states.
pub(super) fn nodes(m: &StateMachine, at: &str) -> Vec<(Node, [f32; 2])> {
    let np = m.nodes_of(at);
    let mut out = vec![
        (Node::Entry, np.entry),
        (Node::AnyState, np.any_state),
        (Node::Exit, np.exit),
    ];
    if !at.is_empty() {
        out.push((Node::Up, np.up));
    }
    out.extend(
        m.children_of(at)
            .map(|c| (Node::Machine(c.path.clone()), c.position)),
    );
    out.extend(
        m.states
            .iter()
            .filter(|s| s.machine == at)
            .map(|s| (Node::State(s.name.clone()), s.position)),
    );
    out
}

fn node_pos(m: &StateMachine, at: &str, n: &Node) -> Option<[f32; 2]> {
    let np = m.nodes_of(at);
    match n {
        Node::Entry => Some(np.entry),
        Node::AnyState => Some(np.any_state),
        Node::Exit => Some(np.exit),
        Node::Up => (!at.is_empty()).then_some(np.up),
        Node::State(name) => m.state(name).map(|s| s.position),
        Node::Machine(p) => m.sub(p).map(|s| s.position),
    }
}

fn set_node_pos(m: &mut StateMachine, at: &str, n: &Node, p: [f32; 2]) {
    match n {
        Node::Entry => m.nodes_of_mut(at).entry = p,
        Node::AnyState => m.nodes_of_mut(at).any_state = p,
        Node::Exit => m.nodes_of_mut(at).exit = p,
        Node::Up => m.nodes_of_mut(at).up = p,
        Node::State(name) => {
            if let Some(s) = m.state_mut(name) {
                s.position = p;
            }
        }
        Node::Machine(path) => {
            if let Some(s) = m.sub_mut(path) {
                s.position = p;
            }
        }
    }
}

/// The node standing for something in machine `loc` when looking at machine `at`: itself, the
/// child sub-machine holding it, or "(Up)".
fn shown(at: &str, loc: &str, node: Node) -> Option<Node> {
    if loc == at {
        return Some(node);
    }
    if machine_within(loc, at) {
        let rest = if at.is_empty() {
            loc
        } else {
            &loc[at.len() + 1..]
        };
        let first = rest.split('/').next().unwrap_or(rest);
        let child = if at.is_empty() {
            first.to_owned()
        } else {
            format!("{at}/{first}")
        };
        return Some(Node::Machine(child));
    }
    (!at.is_empty()).then_some(Node::Up)
}

/// Where a transition end shows in the graph of `at` (`context` = the transition's machine).
fn visible(m: &StateMachine, at: &str, n: &Node, context: &str) -> Option<Node> {
    match n {
        Node::Entry | Node::Exit | Node::AnyState => (context == at).then(|| n.clone()),
        Node::State(s) => shown(at, &m.state(s)?.machine, n.clone()),
        Node::Machine(p) => shown(at, machine_parent(p), n.clone()),
        Node::Up => None,
    }
}

/// Transitions drawn in the graph of `at`: (index, from node, to node).
pub(super) fn edges(m: &StateMachine, at: &str) -> Vec<(usize, Node, Node)> {
    m.transitions
        .iter()
        .enumerate()
        .filter_map(|(i, t)| {
            let ctx = m.context(t);
            let a = visible(m, at, &t.from, &ctx)?;
            let b = visible(m, at, &t.to, &ctx)?;
            (a != b).then_some((i, a, b))
        })
        .collect()
}

/// Edges with their screen end points.
fn edge_lines(m: &StateMachine, at: &str, view: View) -> Vec<(usize, Pos2, Pos2)> {
    edges(m, at)
        .into_iter()
        .filter_map(|(i, a, b)| {
            Some((
                i,
                view.screen(node_pos(m, at, &a)?),
                view.screen(node_pos(m, at, &b)?),
            ))
        })
        .collect()
}

/// Change the shown layer's machine with undo.
pub(super) fn edit_machine(
    s: &mut EditorState,
    path: &str,
    label: &str,
    busy: bool,
    f: impl FnOnce(&mut StateMachine),
) {
    let layer = s.animator.layer;
    anim_controllers::edit(s, path, label, busy, |c| {
        if let Some(m) = c.machine_mut(layer) {
            f(m);
        }
    });
}

#[allow(clippy::too_many_arguments)]
pub fn show(
    ui: &mut Ui,
    s: &mut EditorState,
    canvas: Rect,
    path: &str,
    c: &AnimatorController,
    live: Option<&LayerStatus>,
) {
    let layer = s.animator.layer;
    let ml = c.machine_layer(layer);
    let synced = ml != layer;
    let Some(m) = c.machine(ml) else {
        return;
    };
    if !m.machine_exists(&s.animator.machine) {
        s.animator.machine.clear();
    }
    let at = s.animator.machine.clone();
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
    let lines = edge_lines(m, &at, view);
    let hovered_transition = canvas_resp
        .hover_pos()
        .and_then(|p| transition_at(&lines, view, p));
    // Entry always leads to the default state.
    if let Some(d) = m
        .default_of(&at)
        .and_then(|d| m.state(d))
        .and_then(|d| shown(&at, &d.machine, Node::State(d.name.clone())))
        .and_then(|n| node_pos(m, &at, &n))
    {
        let entry = view.screen(m.nodes_of(&at).entry);
        arrow(&painter, entry, view.screen(d), view.zoom, DEFAULT_FILL);
    }
    let playing = live.and_then(|l| {
        Some((
            l.current.as_ref()?.name.clone(),
            l.next.as_ref()?.0.name.clone(),
        ))
    });
    for &(i, a, b) in &lines {
        let t = &m.transitions[i];
        let selected = s.animator.selected == Some(GraphItem::Transition(i));
        let active = playing.as_ref().is_some_and(|(from, to)| {
            (t.from == Node::state(from) || t.from == Node::AnyState) && t.to == Node::state(to)
        });
        let color = if selected || active {
            LIVE_BLUE
        } else if hovered_transition == Some(i) {
            Color32::WHITE
        } else if t.mute {
            Color32::from_rgb(200, 70, 70)
        } else {
            ARROW
        };
        arrow(&painter, a, b, view.zoom, color);
    }

    // Make Transition: an arrow follows the mouse.
    if let Some(from) = s.animator.making.clone() {
        if let (Some(a), Some(p)) = (node_pos(m, &at, &from), ui.input(|i| i.pointer.hover_pos())) {
            arrow(&painter, view.screen(a), p, view.zoom, Color32::WHITE);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.animator.making = None;
        }
    }

    let mut clicked_node = false;
    for (node, pos) in nodes(m, &at) {
        let rect = view.rect(pos);
        if !rect.intersects(canvas) {
            continue;
        }
        let id = ui.id().with(("animator_node", &node));
        let resp = ui.interact(rect.intersect(canvas), id, Sense::click_and_drag());
        let label = label_of(c, layer, &at, &node);
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        draw_node(&painter, s, m, &at, &node, &label, rect, view.zoom, live);
        clicked_node |= node_input(ui, s, path, c, &node, pos, &resp, view, synced);
    }
    up_menu(ui, s, path, m, &at);

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
    canvas_resp.context_menu(|ui| canvas_menu(ui, s, path, synced));

    // A clip dropped from the Project window becomes a state playing it.
    if let Some(p) = canvas_resp.dnd_release_payload::<AssetPayload>()
        && misty_engine::animation::is_clip(&p.0)
        && !synced
    {
        let pos = canvas_resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |q| view.graph(q));
        let clip = p.0.clone();
        let mut name = String::new();
        edit_machine(s, path, "Create State", false, |m| {
            name = m.add_state_in(&clip_name(&clip), &at, [pos.x, pos.y]);
            if let Some(st) = m.state_mut(&name) {
                st.motion = Some(clip.clone());
            }
        });
        select(s, path, Some(GraphItem::Node(Node::State(name))));
    }
}

/// Node caption: "(Up) Base Layer" / "(Up) Combat" for the up node, else the node's label.
fn label_of(c: &AnimatorController, layer: usize, at: &str, n: &Node) -> String {
    match n {
        Node::Up => {
            let parent = machine_parent(at);
            if parent.is_empty() {
                format!("(Up) {}", c.layer_name(layer))
            } else {
                format!("(Up) {}", machine_name(parent))
            }
        }
        _ => n.label().to_owned(),
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
fn transition_at(lines: &[(usize, Pos2, Pos2)], view: View, p: Pos2) -> Option<usize> {
    lines.iter().find_map(|&(i, a, b)| {
        let d = b - a;
        if d.length_sq() < 1.0 {
            return None;
        }
        let side = vec2(-d.y, d.x).normalized() * 5.0 * view.zoom;
        let (a, b) = (a + side, b + side);
        let t = ((p - a).dot(b - a) / (b - a).length_sq()).clamp(0.0, 1.0);
        ((a + (b - a) * t).distance(p) < 6.0).then_some(i)
    })
}

#[allow(clippy::too_many_arguments)]
fn draw_node(
    painter: &egui::Painter,
    s: &EditorState,
    m: &StateMachine,
    at: &str,
    node: &Node,
    label: &str,
    rect: Rect,
    zoom: f32,
    live: Option<&LayerStatus>,
) {
    let is_default = matches!(node, Node::State(n) if m.default_of(at) == Some(n.as_str()));
    let fill = match node {
        Node::Entry => ENTRY_FILL,
        Node::AnyState => ANY_FILL,
        Node::Exit => EXIT_FILL,
        Node::Up | Node::Machine(_) => MACHINE_FILL,
        Node::State(_) if is_default => DEFAULT_FILL,
        Node::State(_) => STATE_FILL,
    };
    let round = CornerRadius::same((5.0 * zoom).round().clamp(1.0, 12.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(2.0, 3.0) * zoom),
        round,
        Color32::from_black_alpha(90),
    );
    // Sub-state machines look like a stack of states.
    if matches!(node, Node::Machine(_) | Node::Up) {
        let back = rect.translate(vec2(5.0, -5.0) * zoom);
        painter.rect_filled(back, round, fill.gamma_multiply(0.7));
        painter.rect_stroke(
            back,
            round,
            Stroke::new(1.0, Color32::from_black_alpha(160)),
            StrokeKind::Outside,
        );
    }
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
        label,
        FontId::proportional(13.0 * zoom),
        Color32::WHITE,
    );
    let Some(l) = live else { return };
    // Live: a sub-machine holding the playing state is outlined.
    if let Node::Machine(p) = node {
        let inside = l.current.iter().any(|i| {
            m.state(&i.name)
                .is_some_and(|st| machine_within(&st.machine, p))
        });
        if inside {
            painter.rect_stroke(
                rect,
                round,
                Stroke::new(2.0, LIVE_BLUE),
                StrokeKind::Outside,
            );
        }
        return;
    }
    // Live: progress bar of the current state (and of the cross-fade destination).
    let Node::State(name) = node else { return };
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

/// Click / drag / double-click / right-click on one node. Returns true if it was clicked.
#[allow(clippy::too_many_arguments)]
fn node_input(
    ui: &Ui,
    s: &mut EditorState,
    path: &str,
    c: &AnimatorController,
    node: &Node,
    pos: [f32; 2],
    resp: &Response,
    view: View,
    synced: bool,
) -> bool {
    let alt = ui.input(|i| i.modifiers.alt);
    let at = s.animator.machine.clone();
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt && !synced {
        s.animator.drag = Some(node.clone());
        select(s, path, Some(GraphItem::Node(node.clone())));
    }
    if s.animator.drag.as_ref() == Some(node) && resp.dragged_by(egui::PointerButton::Primary) {
        let d = resp.drag_delta() / view.zoom;
        if d != Vec2::ZERO {
            let n = node.clone();
            let to = [pos[0] + d.x, pos[1] + d.y];
            let at = at.clone();
            edit_machine(s, path, "Move State", interacting(ui.ctx()), |m| {
                set_node_pos(m, &at, &n, to)
            });
        }
    }
    if resp.drag_stopped() {
        s.animator.drag = None;
    }
    if resp.double_clicked() {
        match node {
            Node::Machine(p) => {
                s.animator.enter_machine(p.clone());
                return true;
            }
            Node::Up => {
                s.animator.enter_machine(machine_parent(&at).to_owned());
                return true;
            }
            Node::State(name) => {
                let tree = c
                    .machine(c.machine_layer(s.animator.layer))
                    .and_then(|m| m.state(name))
                    .is_some_and(|st| st.blend_tree.is_some());
                if tree && !synced {
                    s.animator.blend_view = Some(name.clone());
                    return true;
                }
            }
            _ => {}
        }
    }
    let mut clicked = false;
    if resp.clicked() {
        clicked = true;
        match s.animator.making.take() {
            Some(from) if *node == Node::Up => {
                s.animator.up_menu = Some((from, resp.rect.left_bottom()));
            }
            Some(from) => make_transition(s, path, from, node.clone()),
            None => select(s, path, Some(GraphItem::Node(node.clone()))),
        }
    }
    if resp.secondary_clicked() {
        s.animator.making = None;
        select(s, path, Some(GraphItem::Node(node.clone())));
        s.animator.menu = Some((Some(GraphItem::Node(node.clone())), pos2(pos[0], pos[1])));
    }
    resp.context_menu(|ui| node_menu(ui, s, path, c, node, synced));
    clicked
}

/// Finish Make Transition on `to`.
fn make_transition(s: &mut EditorState, path: &str, from: Node, to: Node) {
    let valid_to = matches!(to, Node::State(_) | Node::Machine(_) | Node::Exit);
    let valid_from = !matches!(from, Node::Exit | Node::Up);
    if !valid_to || !valid_from || (from == Node::Entry && to == Node::Exit) {
        return;
    }
    let at = s.animator.machine.clone();
    let mut index = 0;
    edit_machine(s, path, "Make Transition", false, |m| {
        let mut t = Transition::new(from.clone(), to);
        if matches!(from, Node::Entry | Node::AnyState) {
            t.machine = at;
        }
        m.transitions.push(t);
        index = m.transitions.len() - 1;
    });
    select(s, path, Some(GraphItem::Transition(index)));
}

/// After Make Transition ends on "(Up)": pick a state outside this sub-machine.
fn up_menu(ui: &Ui, s: &mut EditorState, path: &str, m: &StateMachine, at: &str) {
    let Some((from, pos)) = s.animator.up_menu.clone() else {
        return;
    };
    let targets: Vec<String> = m
        .states
        .iter()
        .filter(|st| !machine_within(&st.machine, at))
        .map(|st| st.name.clone())
        .collect();
    let mut close = ui.input(|i| i.key_pressed(egui::Key::Escape));
    egui::Area::new(ui.id().with("animator_up_menu"))
        .order(egui::Order::Foreground)
        .fixed_pos(pos)
        .show(ui.ctx(), |ui| {
            egui::Frame::menu(ui.style()).show(ui, |ui| {
                ui.label(egui::RichText::new("Transition to").weak());
                if targets.is_empty() {
                    ui.weak("No states outside this machine");
                }
                for t in &targets {
                    if ui.button(t).clicked() {
                        make_transition(s, path, from.clone(), Node::State(t.clone()));
                        close = true;
                    }
                }
                if ui.button("Cancel").clicked() {
                    close = true;
                }
            });
        });
    if close {
        s.animator.up_menu = None;
    }
}

fn node_menu(
    ui: &mut Ui,
    s: &mut EditorState,
    path: &str,
    c: &AnimatorController,
    node: &Node,
    synced: bool,
) {
    let at = s.animator.machine.clone();
    let can_start = matches!(
        node,
        Node::State(_) | Node::AnyState | Node::Entry | Node::Machine(_)
    );
    if ui
        .add_enabled(can_start && !synced, egui::Button::new("Make Transition"))
        .clicked()
    {
        s.animator.making = Some(node.clone());
        ui.close();
    }
    if let Node::Machine(p) = node {
        if ui.button("Open").clicked() {
            s.animator.enter_machine(p.clone());
            ui.close();
        }
        ui.separator();
        if ui
            .add_enabled(!synced, egui::Button::new("Delete"))
            .clicked()
        {
            delete(s, path, &GraphItem::Node(node.clone()));
            ui.close();
        }
        return;
    }
    let Node::State(name) = node else {
        return;
    };
    if ui
        .add_enabled(!synced, egui::Button::new("Set as Layer Default State"))
        .clicked()
    {
        let name = name.clone();
        edit_machine(s, path, "Set Default State", false, |m| {
            m.set_default(&at, Some(name))
        });
        ui.close();
    }
    let machines: Vec<String> = c
        .machine(s.animator.layer)
        .map(|m| m.sub_machines.iter().map(|x| x.path.clone()).collect())
        .unwrap_or_default();
    if !machines.is_empty() || !at.is_empty() {
        ui.add_enabled_ui(!synced, |ui| {
            ui.menu_button("Move to", |ui| {
                for target in std::iter::once(String::new()).chain(machines) {
                    if target == at {
                        continue;
                    }
                    let label = if target.is_empty() {
                        c.layer_name(s.animator.layer).to_owned()
                    } else {
                        target.clone()
                    };
                    if ui.button(label).clicked() {
                        let n = name.clone();
                        edit_machine(s, path, "Move State", false, |m| m.move_state(&n, &target));
                        ui.close();
                    }
                }
            });
        });
    }
    ui.separator();
    if ui
        .add_enabled(!synced, egui::Button::new("Delete"))
        .clicked()
    {
        delete(s, path, &GraphItem::Node(node.clone()));
        ui.close();
    }
}

/// Right-click on empty canvas (Create State / Sub-State Machine) or on a transition (Delete).
fn canvas_menu(ui: &mut Ui, s: &mut EditorState, path: &str, synced: bool) {
    let Some((target, at)) = s.animator.menu.clone() else {
        ui.close();
        return;
    };
    if let Some(item @ GraphItem::Transition(_)) = target {
        if ui
            .add_enabled(!synced, egui::Button::new("Delete"))
            .clicked()
        {
            delete(s, path, &item);
            ui.close();
        }
        return;
    }
    if synced {
        ui.weak("A synced layer uses its source layer's states.");
        return;
    }
    let machine = s.animator.machine.clone();
    ui.menu_button("Create State", |ui| {
        for (label, tree) in [("Empty", false), ("From New Blend Tree", true)] {
            if ui.button(label).clicked() {
                let mut name = String::new();
                edit_machine(s, path, "Create State", false, |m| {
                    let base = if tree { "Blend Tree" } else { "New State" };
                    name = m.add_state_in(base, &machine, [at.x, at.y]);
                    if tree && let Some(st) = m.state_mut(&name) {
                        st.blend_tree = Some(BlendTree::default());
                    }
                });
                select(s, path, Some(GraphItem::Node(Node::State(name))));
                ui.close();
            }
        }
    });
    if ui.button("Create Sub-State Machine").clicked() {
        let mut sub = String::new();
        edit_machine(s, path, "Create Sub-State Machine", false, |m| {
            sub = m.add_sub_machine("New StateMachine", &machine, [at.x, at.y]);
        });
        select(s, path, Some(GraphItem::Node(Node::Machine(sub))));
        ui.close();
    }
}

/// Breadcrumb segments of the shown machine: (label, machine path).
pub(super) fn breadcrumb(c: &AnimatorController, layer: usize, at: &str) -> Vec<(String, String)> {
    let mut out = vec![(c.layer_name(layer).to_owned(), String::new())];
    let mut acc = String::new();
    for part in at.split('/').filter(|p| !p.is_empty()) {
        if !acc.is_empty() {
            acc.push('/');
        }
        acc.push_str(part);
        out.push((machine_name(&acc).to_owned(), acc.clone()));
    }
    out
}
