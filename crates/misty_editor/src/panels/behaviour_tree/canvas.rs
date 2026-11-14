//! The Behaviour Tree canvas: grid, groups, comments, nodes (title, a line about their settings,
//! the input on the left and the children output on the right), wires numbered in run order,
//! pan and zoom, selection (click, Ctrl/Shift+click, rubber band), dragging nodes, dragging from
//! an output to a node to link it (dropped on empty canvas it opens the add menu there), nodes
//! dropped from the palette, the right-click menus, error badges, and the live colours while
//! playing.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use misty_engine::behaviour_tree::asset::{Category, NodeKind};
use misty_engine::behaviour_tree::{BtNode, Status};

use super::palette::PaletteItem;
use super::{BehaviourTreeWindow, interacting};
use crate::i18n::tr;
use crate::panels::node_canvas::{View, draw_grid, error_badge, pan_zoom, wire_width};
use crate::state::EditorState;
use crate::theme;

const NODE_W: f32 = 184.0;
const HEADER_H: f32 = 24.0;
const LINE_H: f32 = 18.0;
const PORT_R: f32 = 5.0;
// Colours follow the theme: the graph tokens for the canvas, nodes and selection, the rest
// made for Dark and shaded for other themes.
fn canvas_bg() -> Color32 {
    theme::near(|p| p.graph_canvas, Color32::from_rgb(34, 35, 38))
}
fn node_bg() -> Color32 {
    theme::near(|p| p.graph_node, Color32::from_rgb(52, 53, 57))
}
fn select() -> Color32 {
    theme::near(|p| p.graph_select, Color32::from_rgb(88, 156, 236))
}
fn running() -> Color32 {
    theme::shade(Color32::from_rgb(255, 210, 90))
}
fn success() -> Color32 {
    theme::shade(Color32::from_rgb(110, 210, 120))
}
fn failure() -> Color32 {
    theme::shade(Color32::from_rgb(232, 96, 90))
}
fn wire_color() -> Color32 {
    theme::shade(Color32::from_rgb(170, 172, 180))
}
/// Ticks a finished node keeps its green or red outline.
const FLASH_TICKS: u64 = 40;

#[derive(Default)]
pub(crate) struct CanvasState {
    pub frame_all: bool,
    pub view: Option<View>,
    /// A link being dragged from this parent.
    pub linking: Option<u32>,
    band: Option<Pos2>,
    /// Where the add menu opened (screen) and the parent a new node joins.
    menu_at: Option<Pos2>,
    pub menu_parent: Option<u32>,
    /// The add menu shown after a link was dropped on empty canvas.
    pub add_menu: bool,
    /// Comment / group being dragged.
    comment_drag: Option<usize>,
    group_drag: Option<usize>,
    /// Comment or group shown in the Node panel.
    pub comment: Option<usize>,
    pub group: Option<usize>,
    /// A short message after a refused link (text, until; until < 0 = not shown yet).
    pub flash: Option<(String, f64)>,
}

/// Header colour by family.
pub fn category_color(c: Category) -> Color32 {
    match c {
        Category::Root => theme::shade(Color32::from_rgb(92, 94, 104)),
        Category::Composite => theme::shade(Color32::from_rgb(54, 100, 160)),
        Category::Decorator => theme::shade(Color32::from_rgb(120, 84, 160)),
        Category::Action => theme::shade(Color32::from_rgb(70, 128, 84)),
        Category::Condition => theme::shade(Color32::from_rgb(60, 120, 130)),
        Category::Scripting => theme::shade(Color32::from_rgb(170, 108, 44)),
    }
}

/// One line about a node's settings.
pub fn summary(n: &BtNode) -> String {
    let key = |k: &str| {
        if k.is_empty() {
            tr!("(no key)")
        } else {
            k.to_owned()
        }
    };
    match &n.kind {
        NodeKind::Root => tr!("Start"),
        NodeKind::Sequence => tr!("Until one fails"),
        NodeKind::Selector => tr!("Until one succeeds"),
        NodeKind::Parallel { policy } => tr(policy.label()),
        NodeKind::RandomSelector => tr!("Random order"),
        NodeKind::Inverter => tr!("Swap success and failure"),
        NodeKind::Repeat { count, until_fail } => {
            if *until_fail {
                tr!("Until it fails")
            } else if *count == 0 {
                tr!("Forever")
            } else {
                tr!("{count} times", count = count)
            }
        }
        NodeKind::Cooldown { seconds } | NodeKind::TimeLimit { seconds } => {
            tr!("{seconds} s", seconds = format!("{seconds:.2}"))
        }
        NodeKind::Conditional {
            key: k,
            op,
            value,
            abort,
        } => {
            let cond = if op.needs_value() {
                format!("{} {} {value}", key(k), op.label())
            } else {
                format!("{} {}", key(k), tr(op.label()))
            };
            if abort.aborts_self() || abort.aborts_lower() {
                format!("{cond} · {}", tr(abort.label()))
            } else {
                cond
            }
        }
        NodeKind::ForceSuccess => tr!("Always succeeds"),
        NodeKind::ForceFailure => tr!("Always fails"),
        NodeKind::MoveTo { target, .. } => tr!("To {target}", target = key(target)),
        NodeKind::Wait { seconds, random } => {
            if *random > 0.0 {
                format!("{seconds:.2} ± {random:.2} s")
            } else {
                tr!("{seconds} s", seconds = format!("{seconds:.2}"))
            }
        }
        NodeKind::PlayAnimation { state, .. } => state.clone(),
        NodeKind::SetAnimatorParameter {
            parameter,
            value,
            from_key,
        } => {
            if from_key.is_empty() {
                format!("{parameter} = {value}")
            } else {
                format!("{parameter} = {from_key}")
            }
        }
        NodeKind::LookAt { target, .. } => tr!("At {target}", target = key(target)),
        NodeKind::SetBlackboard {
            key: k,
            value,
            from_key,
        } => {
            if from_key.is_empty() {
                format!("{} = {value}", key(k))
            } else {
                format!("{} = {from_key}", key(k))
            }
        }
        NodeKind::IsInRange { target, range } => {
            tr!(
                "{target} within {range} m",
                target = key(target),
                range = range
            )
        }
        NodeKind::CanSeeTarget {
            target, range, fov, ..
        } => tr!(
            "{target} within {range} m, {fov}°",
            target = key(target),
            range = range,
            fov = fov
        ),
        NodeKind::HasLineOfSight { target, .. } => tr!("To {target}", target = key(target)),
        NodeKind::RunScriptGraph { graph, event, .. } => {
            let g = misty_engine::scene::clip_name(graph);
            if g.is_empty() {
                event.clone()
            } else {
                format!("{g} ▸ {event}")
            }
        }
        NodeKind::RunLua { function } => format!("{function}()"),
        NodeKind::CustomLua { script, .. } => misty_engine::scene::clip_name(script),
        NodeKind::Log { message, .. } => message.clone(),
    }
}

fn node_size(n: &BtNode) -> Vec2 {
    let lines = if n.name.is_empty() { 1.0 } else { 2.0 };
    vec2(NODE_W, HEADER_H + LINE_H * lines + 6.0)
}

pub fn node_rect(view: View, n: &BtNode) -> Rect {
    let min = view.screen(pos2(n.position[0], n.position[1]));
    Rect::from_min_size(min, node_size(n) * view.zoom)
}

fn in_port(view: View, n: &BtNode) -> Pos2 {
    node_rect(view, n).left_center()
}

fn out_port(view: View, n: &BtNode) -> Pos2 {
    node_rect(view, n).right_center()
}

/// Fit every node in `area`.
fn frame_all(w: &mut BehaviourTreeWindow, area: Rect) {
    let mut bounds: Option<Rect> = None;
    for n in &w.tree.nodes {
        let r = Rect::from_min_size(pos2(n.position[0], n.position[1]), node_size(n));
        bounds = Some(bounds.map_or(r, |b| b.union(r)));
    }
    let Some(b) = bounds else {
        w.pan = Vec2::ZERO;
        w.zoom = 1.0;
        return;
    };
    let b = b.expand(40.0);
    let zoom = (area.width() / b.width())
        .min(area.height() / b.height())
        .clamp(0.3, 1.2);
    w.zoom = zoom;
    w.pan = -b.center().to_vec2();
}

/// The node under a screen point (topmost).
fn node_at(w: &BehaviourTreeWindow, view: View, p: Pos2) -> Option<u32> {
    w.tree
        .nodes
        .iter()
        .rev()
        .find(|n| node_rect(view, n).contains(p))
        .map(|n| n.id)
}

/// The outline a node gets from the live debugger.
fn live_stroke(w: &BehaviourTreeWindow, id: u32) -> Option<Color32> {
    let (now, last, age) = w.live_of(id)?;
    if now == Status::Running {
        return Some(running());
    }
    if age > FLASH_TICKS {
        return None;
    }
    let fade = 1.0 - age as f32 / FLASH_TICKS as f32;
    match last {
        Status::Success => Some(success().gamma_multiply(0.35 + 0.65 * fade)),
        Status::Failure => Some(failure().gamma_multiply(0.35 + 0.65 * fade)),
        _ => None,
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState, area: Rect) {
    let resp = ui.interact(area, ui.id().with("bt_canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, canvas_bg());
    if s.behaviour_tree.canvas.frame_all {
        s.behaviour_tree.canvas.frame_all = false;
        frame_all(&mut s.behaviour_tree, area);
    }
    {
        let w = &mut s.behaviour_tree;
        if pan_zoom(ui, &resp, area, &mut w.pan, &mut w.zoom, true) {
            w.canvas.band = None;
        }
    }
    let view = View {
        centre: area.center(),
        pan: s.behaviour_tree.pan,
        zoom: s.behaviour_tree.zoom(),
    };
    s.behaviour_tree.canvas.view = Some(view);
    draw_grid(&painter, area, view);
    let busy = interacting(ui.ctx());

    groups(ui, s, &painter, view, busy);
    comments(ui, s, &painter, view, busy);
    wires(&painter, &s.behaviour_tree, view);

    let ids: Vec<u32> = s.behaviour_tree.tree.nodes.iter().map(|n| n.id).collect();
    let mut clicked_node = false;
    for id in ids {
        let Some(node) = s.behaviour_tree.tree.node(id).cloned() else {
            continue;
        };
        let rect = node_rect(view, &node);
        if !rect.expand(40.0).intersects(area) {
            continue;
        }
        clicked_node |= node_ui(ui, s, &painter, view, &node, rect, busy);
    }

    // A link being dragged follows the mouse; released on a node it links, on empty canvas it
    // opens the add menu (the new node becomes the child).
    if let Some(from) = s.behaviour_tree.canvas.linking {
        let hover = ui.input(|i| i.pointer.hover_pos());
        if let (Some(p), Some(n)) = (hover, s.behaviour_tree.tree.node(from)) {
            wire_width(&painter, out_port(view, n), p, select(), view.zoom, 2.4);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.behaviour_tree.canvas.linking = None;
        } else if ui.input(|i| i.pointer.any_released()) {
            s.behaviour_tree.canvas.linking = None;
            match hover.and_then(|p| node_at(&s.behaviour_tree, view, p)) {
                Some(child) if child != from => {
                    if let Err(e) = s.behaviour_tree.connect(from, child) {
                        s.behaviour_tree.canvas.flash = Some((e, -1.0));
                    }
                }
                Some(_) => {}
                None => {
                    s.behaviour_tree.canvas.menu_at = hover;
                    s.behaviour_tree.canvas.menu_parent = Some(from);
                    s.behaviour_tree.canvas.add_menu = true;
                }
            }
        }
    }

    // Nodes dropped from the palette.
    if let Some(item) = resp.dnd_release_payload::<PaletteItem>()
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        let g = view.graph(p);
        s.behaviour_tree
            .add_node(item.kind.clone(), [g.x, g.y], None);
    }

    // Rubber band selection on empty canvas; a click clears the selection.
    let w = &mut s.behaviour_tree;
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt {
        w.canvas.band = resp.interact_pointer_pos();
    }
    if let (Some(a), Some(b)) = (w.canvas.band, ui.input(|i| i.pointer.hover_pos())) {
        let band = Rect::from_two_pos(a, b);
        painter.rect(
            band,
            2.0,
            select().gamma_multiply(0.12),
            Stroke::new(1.0, select().gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
        if resp.drag_stopped() {
            if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                w.selected.clear();
            }
            let hits: Vec<u32> = w
                .tree
                .nodes
                .iter()
                .filter(|n| node_rect(view, n).intersects(band))
                .map(|n| n.id)
                .collect();
            w.selected.extend(hits);
            w.canvas.band = None;
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        w.canvas.band = None;
    }
    if resp.clicked() && !clicked_node {
        w.selected.clear();
        w.canvas.comment = None;
        w.canvas.group = None;
    }
    if resp.secondary_clicked() {
        w.canvas.linking = None;
        w.canvas.menu_at = resp.interact_pointer_pos();
        w.canvas.menu_parent = None;
    }
    resp.context_menu(|ui| canvas_menu(ui, s));
    // The add menu after a link dropped on empty canvas.
    let popup = egui::Id::new("bt_add_menu");
    if s.behaviour_tree.canvas.add_menu {
        let at = s.behaviour_tree.canvas.menu_at.unwrap_or(area.center());
        let mut close = false;
        egui::Area::new(popup)
            .fixed_pos(at)
            .order(egui::Order::Foreground)
            .show(ui.ctx(), |ui| {
                egui::Frame::popup(ui.style()).show(ui, |ui| {
                    ui.set_min_width(170.0);
                    close = add_menu(ui, s);
                    if ui.button(tr!("Cancel")).clicked() {
                        close = true;
                    }
                });
            });
        if close || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.behaviour_tree.canvas.add_menu = false;
            s.behaviour_tree.canvas.menu_parent = None;
        }
    }

    flash(ui, &painter, &mut s.behaviour_tree, area);
    if s.behaviour_tree.live.is_some() {
        ui.ctx().request_repaint();
    }
}

fn flash(ui: &Ui, painter: &egui::Painter, w: &mut BehaviourTreeWindow, area: Rect) {
    let now = ui.input(|i| i.time);
    if let Some((_, until)) = &mut w.canvas.flash
        && *until < 0.0
    {
        *until = now + 2.5;
    }
    if let Some((text, until)) = w.canvas.flash.clone() {
        if now < until {
            let at = pos2(area.center().x, area.bottom() - 24.0);
            let galley = painter.layout_no_wrap(text, FontId::proportional(12.5), theme::text());
            let r = Rect::from_center_size(at, galley.size() + vec2(20.0, 10.0));
            painter.rect_filled(r, 6.0, theme::shade(Color32::from_rgb(90, 44, 44)));
            painter.galley(r.min + vec2(10.0, 5.0), galley, theme::text());
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        } else {
            w.canvas.flash = None;
        }
    }
}

fn wires(painter: &egui::Painter, w: &BehaviourTreeWindow, view: View) {
    for n in &w.tree.nodes {
        let many = n.children.len() > 1;
        for (k, c) in n.children.iter().enumerate() {
            let Some(child) = w.tree.node(*c) else {
                continue;
            };
            let live = w
                .live_of(*c)
                .is_some_and(|(now, _, _)| now == Status::Running);
            let (color, width) = if live {
                (running(), 3.2)
            } else {
                (wire_color(), 2.2)
            };
            let (a, b) = (out_port(view, n), in_port(view, child));
            wire_width(painter, a, b, color, view.zoom, width);
            if many {
                let at = b - vec2(14.0 * view.zoom, 0.0);
                painter.circle_filled(
                    at,
                    7.0 * view.zoom,
                    theme::shade(Color32::from_rgb(64, 66, 72)),
                );
                painter.text(
                    at,
                    Align2::CENTER_CENTER,
                    format!("{}", k + 1),
                    FontId::proportional(10.0 * view.zoom),
                    theme::text(),
                );
            }
        }
    }
}

/// One node; true when it was clicked.
fn node_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    n: &BtNode,
    rect: Rect,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let w = &mut s.behaviour_tree;
    let selected = w.selected.contains(&n.id);
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H * z));
    painter.rect_filled(rect, 6.0 * z, node_bg());
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: (6.0 * z) as u8,
            ne: (6.0 * z) as u8,
            sw: 0,
            se: 0,
        },
        category_color(n.kind.category()),
    );
    painter.text(
        header.left_center() + vec2(8.0 * z, 0.0),
        Align2::LEFT_CENTER,
        tr(n.kind.label()),
        FontId::proportional(12.5 * z),
        Color32::WHITE,
    );
    let mut y = header.bottom() + 3.0 * z;
    if !n.name.is_empty() {
        painter.text(
            pos2(rect.left() + 8.0 * z, y + LINE_H * z * 0.5),
            Align2::LEFT_CENTER,
            crate::i18n::display(&n.name),
            FontId::proportional(12.0 * z),
            theme::text(),
        );
        y += LINE_H * z;
    }
    let mut line = summary(n);
    if line.chars().count() > 30 {
        line = format!("{}…", line.chars().take(29).collect::<String>());
    }
    painter.text(
        pos2(rect.left() + 8.0 * z, y + LINE_H * z * 0.5),
        Align2::LEFT_CENTER,
        crate::i18n::display(&line),
        FontId::proportional(11.0 * z),
        theme::text_dim(),
    );
    let outline = live_stroke(w, n.id);
    if let Some(c) = outline {
        painter.rect_stroke(
            rect.expand(2.0),
            7.0 * z,
            Stroke::new(2.5, c),
            StrokeKind::Outside,
        );
    }
    if selected {
        painter.rect_stroke(
            rect,
            6.0 * z,
            Stroke::new(1.8, select()),
            StrokeKind::Outside,
        );
    }
    // Ports.
    if n.kind != NodeKind::Root {
        painter.circle_filled(rect.left_center(), PORT_R * z, wire_color());
    }
    let takes_children = !n.kind.is_leaf();
    let out = rect.right_center();
    if takes_children {
        painter.circle_filled(out, PORT_R * z, wire_color());
    }
    if let Some(e) = w.error_of(n.id) {
        let e = e.to_owned();
        error_badge(
            ui,
            painter,
            rect.right_top() + vec2(-2.0, 2.0),
            z,
            ui.id().with(("bt_err", n.id)),
            &e,
        );
    }

    let resp = ui
        .interact(
            rect,
            ui.id().with(("bt_node", n.id)),
            Sense::click_and_drag(),
        )
        .on_hover_text(tr(n.kind.help()));
    let mut clicked = false;
    if takes_children {
        let port = ui.interact(
            Rect::from_center_size(out, Vec2::splat(16.0 * z)),
            ui.id().with(("bt_out", n.id)),
            Sense::drag(),
        );
        if port.drag_started() {
            w.canvas.linking = Some(n.id);
        }
    }
    if resp.clicked() || resp.drag_started() {
        clicked = true;
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if add && resp.clicked() {
            if !w.selected.remove(&n.id) {
                w.selected.insert(n.id);
            }
        } else if !selected {
            w.selected = [n.id].into();
        }
        w.canvas.comment = None;
        w.canvas.group = None;
    }
    if resp.dragged() && w.canvas.linking.is_none() {
        let d = resp.drag_delta() / z;
        if d != Vec2::ZERO {
            let ids: Vec<u32> = w.selected.iter().copied().collect();
            w.edit("Move", busy, |t| {
                for id in &ids {
                    if let Some(m) = t.node_mut(*id) {
                        m.position[0] += d.x;
                        m.position[1] += d.y;
                    }
                }
                t.sort_all_children();
            });
        }
    }
    if resp.secondary_clicked() && !selected {
        w.selected = [n.id].into();
    }
    let node = n.clone();
    resp.context_menu(|ui| node_menu(ui, s, &node));
    clicked
}

fn node_menu(ui: &mut Ui, s: &mut EditorState, n: &BtNode) {
    let root = n.kind == NodeKind::Root;
    if ui
        .add_enabled(!root, egui::Button::new(tr!("Duplicate")))
        .clicked()
    {
        s.behaviour_tree.duplicate_selected();
        ui.close();
    }
    if ui
        .add_enabled(!root, egui::Button::new(tr!("Copy")))
        .clicked()
    {
        if let Some(t) = s.behaviour_tree.copy_selected() {
            ui.ctx().copy_text(t);
        }
        ui.close();
    }
    let has_parent = s.behaviour_tree.tree.parent_of(n.id).is_some();
    if ui
        .add_enabled(has_parent, egui::Button::new(tr!("Disconnect from Parent")))
        .clicked()
    {
        s.behaviour_tree.disconnect(n.id);
        ui.close();
    }
    if ui.button(tr!("Group Selection")).clicked() {
        s.behaviour_tree.group_selected();
        ui.close();
    }
    if !n.kind.is_leaf() {
        ui.menu_button(tr!("Add Child"), |ui| {
            let w = &mut s.behaviour_tree;
            w.canvas.menu_parent = Some(n.id);
            let at = n.position;
            let below = 60.0 * n.children.len() as f32;
            w.canvas.menu_at = w
                .canvas
                .view
                .map(|v| v.screen(pos2(at[0] + 240.0, at[1] + below)));
            if add_menu(ui, s) {
                ui.close();
            }
        });
    }
    ui.separator();
    if ui
        .add_enabled(!root, egui::Button::new(tr!("Delete")))
        .clicked()
    {
        s.behaviour_tree.delete_selected();
        ui.close();
    }
}

/// Node families and their nodes; a click adds one at the menu's spot. True when one was added.
pub(crate) fn add_menu(ui: &mut Ui, s: &mut EditorState) -> bool {
    let mut added = false;
    let items = super::palette::items(s);
    let mut cats: Vec<Category> = Vec::new();
    for i in &items {
        if !cats.contains(&i.kind.category()) {
            cats.push(i.kind.category());
        }
    }
    for c in cats {
        ui.menu_button(tr(c.label()), |ui| {
            for item in items.iter().filter(|i| i.kind.category() == c) {
                if ui
                    .button(item.title())
                    .on_hover_text(tr(item.kind.help()))
                    .clicked()
                {
                    let w = &mut s.behaviour_tree;
                    let at = match (w.canvas.view, w.canvas.menu_at) {
                        (Some(v), Some(p)) => v.graph(p),
                        _ => pos2(0.0, 0.0),
                    };
                    let parent = w.canvas.menu_parent.take();
                    w.add_node(item.kind.clone(), [at.x, at.y], parent);
                    added = true;
                    ui.close();
                }
            }
        });
    }
    added
}

fn canvas_menu(ui: &mut Ui, s: &mut EditorState) {
    ui.menu_button(tr!("Add Node"), |ui| {
        if add_menu(ui, s) {
            ui.close();
        }
    });
    let w = &mut s.behaviour_tree;
    if ui.button(tr!("Add Comment")).clicked() {
        let at = match (w.canvas.view, w.canvas.menu_at) {
            (Some(v), Some(p)) => v.graph(p),
            _ => pos2(0.0, 0.0),
        };
        w.add_comment([at.x, at.y]);
        w.canvas.comment = Some(w.tree.comments.len() - 1);
        ui.close();
    }
    if ui
        .add_enabled(w.clipboard.is_some(), egui::Button::new(tr!("Paste")))
        .clicked()
    {
        w.paste(None);
        ui.close();
    }
    if ui.button(tr!("Frame All")).clicked() {
        w.canvas.frame_all = true;
        ui.close();
    }
}

fn group_rect(w: &BehaviourTreeWindow, view: View, nodes: &[u32]) -> Option<Rect> {
    let mut r: Option<Rect> = None;
    for id in nodes {
        if let Some(n) = w.tree.node(*id) {
            let nr = node_rect(view, n);
            r = Some(r.map_or(nr, |x| x.union(nr)));
        }
    }
    r.map(|r| {
        Rect::from_min_max(
            r.min - vec2(14.0, 34.0) * view.zoom,
            r.max + vec2(14.0, 14.0) * view.zoom,
        )
    })
}

fn groups(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, busy: bool) {
    let z = view.zoom;
    let count = s.behaviour_tree.tree.groups.len();
    for gi in 0..count {
        let w = &mut s.behaviour_tree;
        let g = w.tree.groups[gi].clone();
        let Some(r) = group_rect(w, view, &g.nodes) else {
            continue;
        };
        let chosen = w.canvas.group == Some(gi);
        painter.rect(
            r,
            8.0 * z,
            theme::shade(Color32::from_rgba_unmultiplied(120, 130, 160, 22)),
            Stroke::new(
                if chosen { 1.8 } else { 1.0 },
                if chosen {
                    select()
                } else {
                    theme::shade(Color32::from_rgba_unmultiplied(150, 160, 190, 90))
                },
            ),
            StrokeKind::Inside,
        );
        let bar = Rect::from_min_size(r.min, vec2(r.width(), 24.0 * z));
        painter.text(
            bar.left_center() + vec2(10.0 * z, 0.0),
            Align2::LEFT_CENTER,
            crate::i18n::display(&g.title),
            FontId::proportional(13.0 * z),
            theme::text(),
        );
        let resp = ui.interact(bar, ui.id().with(("bt_group", gi)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            w.canvas.group = Some(gi);
            w.canvas.comment = None;
            w.selected = g.nodes.iter().copied().collect();
            if resp.drag_started() {
                w.canvas.group_drag = Some(gi);
            }
        }
        if resp.dragged() && w.canvas.group_drag == Some(gi) {
            let d = resp.drag_delta() / z;
            w.edit("Move Group", busy, |t| {
                for id in &g.nodes {
                    if let Some(n) = t.node_mut(*id) {
                        n.position[0] += d.x;
                        n.position[1] += d.y;
                    }
                }
                t.sort_all_children();
            });
        }
        if resp.drag_stopped() {
            w.canvas.group_drag = None;
        }
    }
}

fn comments(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, busy: bool) {
    let z = view.zoom;
    let count = s.behaviour_tree.tree.comments.len();
    for ci in 0..count {
        let w = &mut s.behaviour_tree;
        let c = w.tree.comments[ci].clone();
        let min = view.screen(pos2(c.position[0], c.position[1]));
        let r = Rect::from_min_size(min, vec2(c.size[0], c.size[1]) * z);
        let chosen = w.canvas.comment == Some(ci);
        painter.rect(
            r,
            4.0 * z,
            theme::shade(Color32::from_rgb(92, 86, 52)),
            Stroke::new(
                if chosen { 1.8 } else { 0.0 },
                if chosen {
                    select()
                } else {
                    Color32::TRANSPARENT
                },
            ),
            StrokeKind::Outside,
        );
        let galley = painter.layout(
            crate::i18n::display(&c.text).into_owned(),
            FontId::proportional(12.0 * z),
            theme::text(),
            (r.width() - 12.0 * z).max(10.0),
        );
        painter.galley(r.min + vec2(6.0, 6.0) * z, galley, theme::text());
        let resp = ui.interact(r, ui.id().with(("bt_comment", ci)), Sense::click_and_drag());
        if resp.clicked() || resp.drag_started() {
            w.canvas.comment = Some(ci);
            w.canvas.group = None;
            w.selected.clear();
            if resp.drag_started() {
                w.canvas.comment_drag = Some(ci);
            }
        }
        if resp.dragged() && w.canvas.comment_drag == Some(ci) {
            let d = resp.drag_delta() / z;
            w.edit("Move Comment", busy, |t| {
                if let Some(c) = t.comments.get_mut(ci) {
                    c.position[0] += d.x;
                    c.position[1] += d.y;
                }
            });
        }
        if resp.drag_stopped() {
            w.canvas.comment_drag = None;
        }
    }
}
