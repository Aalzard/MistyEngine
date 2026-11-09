//! The Script Graph canvas: grid, groups, sticky notes, nodes (header, exec and data pins,
//! inline values of unconnected inputs), wires, pan and zoom, selection (click, Ctrl/Shift+click,
//! rubber band), dragging nodes, dragging pins to wire them (dropped on empty canvas it opens a
//! filtered search), the right-click menus, error badges and breakpoints, and the debugger's
//! glowing exec wires and pin values while playing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::PathBuf;

use egui::{
    Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, Vec2, pos2, vec2,
};
use misty_engine::visual_script::{
    self, FileResolver, Literal, Node, PinType, Ports, Resolver, ScriptGraph, nodes,
};

use super::{ScriptGraphWindow, interacting, search};
use crate::panels::node_canvas::{self, View, draw_grid, error_badge, pan_zoom, wire_width};
use crate::state::EditorState;
use crate::theme;

const MIN_W: f32 = 150.0;
const HEADER_H: f32 = 26.0;
const ROW_H: f32 = 22.0;
const FOOT: f32 = 6.0;
const PIN_R: f32 = 4.5;
const CANVAS_BG: Color32 = Color32::from_rgb(34, 35, 38);
const NODE_BG: Color32 = Color32::from_rgb(52, 53, 57);
const SELECT: Color32 = Color32::from_rgb(88, 156, 236);
const GLOW: Color32 = Color32::from_rgb(255, 214, 110);
const BREAK: Color32 = Color32::from_rgb(220, 60, 60);

/// Subgraph assets read for their pins; cleared when the graph changes.
#[derive(Default)]
pub struct CachedFiles {
    pub root: PathBuf,
    cache: RefCell<HashMap<String, Result<ScriptGraph, String>>>,
}

impl CachedFiles {
    pub fn clear(&self) {
        self.cache.borrow_mut().clear();
    }
}

impl Resolver for CachedFiles {
    fn load(&self, path: &str) -> Result<ScriptGraph, String> {
        if let Some(r) = self.cache.borrow().get(path) {
            return r.clone();
        }
        let r = FileResolver(self.root.clone()).load(path);
        self.cache.borrow_mut().insert(path.to_owned(), r.clone());
        r
    }
}

/// One end of a wire being dragged.
#[derive(Clone, Debug, PartialEq)]
pub struct PinRef {
    pub node: u32,
    pub pin: String,
    pub output: bool,
    pub ty: PinType,
}

#[derive(Default)]
pub(crate) struct CanvasState {
    pub view: Option<View>,
    pub connecting: Option<PinRef>,
    band: Option<Pos2>,
    pub frame_all: bool,
    /// A short message after a refused wire (text, until time; until < 0 = not shown yet).
    pub flash: Option<(String, f64)>,
    menu_at: Option<Pos2>,
    note_drag: Option<usize>,
    group_drag: Option<usize>,
    pub renaming: Option<(usize, String)>,
    pub editing_note: Option<usize>,
    pub files: CachedFiles,
}

/// Pin colour by type.
pub fn type_color(t: PinType) -> Color32 {
    match t {
        PinType::Exec => Color32::from_rgb(235, 235, 235),
        PinType::Bool => Color32::from_rgb(214, 84, 84),
        PinType::Int => Color32::from_rgb(86, 200, 170),
        PinType::Float => Color32::from_rgb(150, 214, 110),
        PinType::String => Color32::from_rgb(228, 120, 210),
        PinType::Vector3 => Color32::from_rgb(240, 200, 90),
        PinType::Quaternion => Color32::from_rgb(140, 170, 250),
        PinType::GameObject => Color32::from_rgb(90, 160, 240),
        PinType::List => Color32::from_rgb(205, 160, 100),
        PinType::Any => Color32::from_rgb(170, 170, 175),
    }
}

/// Header colour by category.
pub fn category_color(category: &str) -> Color32 {
    let top = category.split('/').next().unwrap_or("");
    match top {
        "Events" => Color32::from_rgb(160, 58, 58),
        "Flow" => Color32::from_rgb(92, 94, 104),
        "Variables" => Color32::from_rgb(120, 84, 160),
        "Math" => Color32::from_rgb(70, 128, 84),
        "Logic" | "Comparison" => Color32::from_rgb(60, 120, 130),
        "Constants" => Color32::from_rgb(84, 86, 92),
        "String" => Color32::from_rgb(150, 76, 130),
        "Random" => Color32::from_rgb(110, 120, 70),
        "List" => Color32::from_rgb(140, 104, 62),
        "Engine" => Color32::from_rgb(54, 100, 160),
        "Lua" => Color32::from_rgb(170, 108, 44),
        "Subgraphs" => Color32::from_rgb(54, 140, 150),
        _ => Color32::from_rgb(90, 90, 96),
    }
}

/// Where a node's parts are, in graph units relative to its top-left corner.
pub struct NodeLayout {
    pub size: Vec2,
    pub title: String,
    pub category: String,
    pub ports: Ports,
    pub inputs: Vec<Pos2>,
    pub outputs: Vec<Pos2>,
}

/// Width of a label at zoom 1 (rough, monospace-ish estimate that never clips).
fn text_w(s: &str, size: f32) -> f32 {
    s.chars().count() as f32 * size * 0.56
}

/// Width of the inline field of an unconnected input.
fn field_w(t: PinType) -> f32 {
    match t {
        PinType::Float | PinType::Int => 46.0,
        PinType::Bool => 18.0,
        PinType::String | PinType::Any => 78.0,
        PinType::Vector3 | PinType::Quaternion => 3.0 * 38.0 + 4.0,
        PinType::GameObject => 32.0,
        PinType::Exec | PinType::List => 0.0,
    }
}

fn shows_label(p: &visual_script::Pin) -> bool {
    !(p.ty.is_exec() && (p.name == "In" || p.name == "Out"))
}

pub fn layout(g: &ScriptGraph, node: &Node, files: &dyn Resolver) -> NodeLayout {
    let ports = visual_script::ports(node, g, files);
    let title = nodes::title(node, g);
    let category = nodes::category(node);
    let left = ports
        .inputs
        .iter()
        .map(|p| {
            let label = if shows_label(p) {
                text_w(&p.name, 11.5)
            } else {
                0.0
            };
            label + field_w(p.ty) + 22.0
        })
        .fold(0.0, f32::max);
    let right = ports
        .outputs
        .iter()
        .map(|p| {
            let label = if shows_label(p) {
                text_w(&p.name, 11.5)
            } else {
                0.0
            };
            label + 22.0
        })
        .fold(0.0, f32::max);
    let w = (text_w(&title, 12.5) + 36.0)
        .max(left + right + 12.0)
        .max(MIN_W)
        .ceil();
    let rows = ports.inputs.len().max(ports.outputs.len()).max(1);
    let h = HEADER_H + rows as f32 * ROW_H + FOOT;
    let row_y = |i: usize| HEADER_H + (i as f32 + 0.5) * ROW_H;
    NodeLayout {
        size: vec2(w, h),
        inputs: (0..ports.inputs.len())
            .map(|i| pos2(0.0, row_y(i)))
            .collect(),
        outputs: (0..ports.outputs.len())
            .map(|i| pos2(w, row_y(i)))
            .collect(),
        title,
        category,
        ports,
    }
}

fn node_rect(view: View, node: &Node, l: &NodeLayout) -> Rect {
    let min = view.screen(pos2(node.position[0], node.position[1]));
    Rect::from_min_size(min, l.size * view.zoom)
}

fn pin_screen(view: View, node: &Node, at: Pos2) -> Pos2 {
    view.screen(pos2(node.position[0] + at.x, node.position[1] + at.y))
}

/// Every pin on screen: (node, pin, output, type, position).
type PinSpot = (u32, String, bool, PinType, Pos2);

fn all_pins(w: &ScriptGraphWindow, view: View) -> Vec<PinSpot> {
    let g = w.view();
    let mut out = Vec::new();
    for n in &g.nodes {
        let l = layout(g, n, &w.canvas.files);
        for (p, at) in l.ports.inputs.iter().zip(&l.inputs) {
            out.push((n.id, p.name.clone(), false, p.ty, pin_screen(view, n, *at)));
        }
        for (p, at) in l.ports.outputs.iter().zip(&l.outputs) {
            out.push((n.id, p.name.clone(), true, p.ty, pin_screen(view, n, *at)));
        }
    }
    out
}

/// Open the node search at a screen point (optionally wiring the new node to a dragged pin).
pub fn open_search_at(s: &mut EditorState, screen: Pos2, from: Option<PinRef>) {
    let root = s.project.root.clone();
    let subgraphs = search::project_subgraphs(&root, s.script_graph.path.as_deref());
    let w = &mut s.script_graph;
    let graph = w.canvas.view.map_or(pos2(0.0, 0.0), |v| v.graph(screen));
    w.search = Some(search::Search::new(screen, graph, from, subgraphs));
}

fn frame_all(w: &mut ScriptGraphWindow, area: Rect) {
    let mut bounds = Rect::NOTHING;
    let g = w.view();
    for n in &g.nodes {
        let l = layout(g, n, &w.canvas.files);
        bounds = bounds.union(Rect::from_min_size(
            pos2(n.position[0], n.position[1]),
            l.size,
        ));
    }
    if !bounds.is_positive() {
        w.pan = Vec2::ZERO;
        w.zoom = 1.0;
        return;
    }
    let left = if w.show_blackboard { 260.0 } else { 0.0 };
    let right = if w.show_inspector { 270.0 } else { 0.0 };
    let free = Rect::from_min_max(
        pos2(area.left() + left, area.top()),
        pos2(
            (area.right() - right).max(area.left() + left + 100.0),
            area.bottom(),
        ),
    );
    let margin = 30.0;
    let fit = |r: Rect| {
        ((r.width() - 2.0 * margin) / bounds.width())
            .min((r.height() - 2.0 * margin) / bounds.height())
    };
    let (free, zoom) = if fit(free) >= 0.55 {
        (free, fit(free))
    } else {
        (area, fit(area))
    };
    let zoom = zoom.clamp(0.4, 1.0);
    w.zoom = zoom;
    w.pan = (free.center() - area.center()) / zoom - bounds.center().to_vec2();
}

/// Is the exec entry `(node, pin)` reached in the last few frames?
fn active(w: &ScriptGraphWindow, node: u32, pin: &str) -> bool {
    let Some(live) = &w.live else {
        return false;
    };
    if !w.at_root() {
        return false;
    }
    live.hits
        .get(&(node, pin.to_owned()))
        .is_some_and(|f| live.frame.saturating_sub(*f) <= 6)
}

pub fn show(ui: &mut Ui, s: &mut EditorState, area: Rect) {
    s.script_graph.canvas.files.root = s.project.root.clone();
    let resp = ui.interact(area, ui.id().with("vs_canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, CANVAS_BG);
    if s.script_graph.canvas.frame_all {
        s.script_graph.canvas.frame_all = false;
        frame_all(&mut s.script_graph, area);
    }
    {
        let w = &mut s.script_graph;
        let wheel = w.search.is_none();
        if pan_zoom(ui, &resp, area, &mut w.pan, &mut w.zoom, wheel) {
            w.canvas.band = None;
        }
    }
    let view = View {
        centre: area.center(),
        pan: s.script_graph.pan,
        zoom: s.script_graph.zoom(),
    };
    s.script_graph.canvas.view = Some(view);
    draw_grid(&painter, area, view);

    groups(ui, s, &painter, view, area);
    notes(ui, s, &painter, view, area);
    let pins = all_pins(&s.script_graph, view);
    wires(ui, &painter, &s.script_graph, view, &pins);

    let busy = interacting(ui.ctx());
    let ids: Vec<u32> = s.script_graph.view().nodes.iter().map(|n| n.id).collect();
    let mut clicked_node = false;
    for id in ids {
        let Some(node) = s.script_graph.view().node(id).cloned() else {
            continue;
        };
        let l = layout(s.script_graph.view(), &node, &s.script_graph.canvas.files);
        let rect = node_rect(view, &node, &l);
        if !rect.expand(40.0).intersects(area) {
            continue;
        }
        clicked_node |= node_ui(ui, s, &painter, view, &node, &l, rect, busy);
    }

    // A wire being dragged follows the mouse; released, it wires to the pin under it or opens
    // the search filtered to nodes that fit.
    if let Some(from) = s.script_graph.canvas.connecting.clone() {
        let hover = ui.input(|i| i.pointer.hover_pos());
        if let (Some(p), Some(spot)) = (
            hover,
            pins.iter()
                .find(|x| x.0 == from.node && x.1 == from.pin && x.2 == from.output),
        ) {
            let (a, b) = if from.output {
                (spot.4, p)
            } else {
                (p, spot.4)
            };
            wire_width(&painter, a, b, type_color(from.ty), view.zoom, 2.4);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            s.script_graph.canvas.connecting = None;
        } else if ui.input(|i| i.pointer.any_released()) {
            s.script_graph.canvas.connecting = None;
            finish_connection(s, from, hover, &pins, view);
        }
    }

    // Rubber band selection on empty canvas; a click clears the selection.
    let w = &mut s.script_graph;
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt {
        w.canvas.band = resp.interact_pointer_pos();
    }
    if let (Some(a), Some(b)) = (w.canvas.band, ui.input(|i| i.pointer.hover_pos())) {
        let band = Rect::from_two_pos(a, b);
        painter.rect(
            band,
            2.0,
            SELECT.gamma_multiply(0.12),
            Stroke::new(1.0, SELECT.gamma_multiply(0.7)),
            StrokeKind::Inside,
        );
        if resp.drag_stopped() {
            let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
            if !add {
                w.selected.clear();
            }
            let g = w.view();
            let hits: Vec<u32> = g
                .nodes
                .iter()
                .filter(|n| node_rect(view, n, &layout(g, n, &w.canvas.files)).intersects(band))
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
        w.search = None;
        w.canvas.editing_note = None;
    }
    if resp.secondary_clicked() {
        w.canvas.connecting = None;
        w.canvas.menu_at = resp.interact_pointer_pos();
    }
    let menu_at = w.canvas.menu_at;
    resp.context_menu(|ui| canvas_menu(ui, s, menu_at));

    // Refused wire message.
    let w = &mut s.script_graph;
    let now = ui.input(|i| i.time);
    if let Some((_, until)) = &mut w.canvas.flash
        && *until < 0.0
    {
        *until = now + 2.5;
    }
    if let Some((text, until)) = w.canvas.flash.clone() {
        if now < until {
            let at = pos2(area.center().x, area.bottom() - 24.0);
            let galley = painter.layout_no_wrap(text, FontId::proportional(12.5), theme::TEXT);
            let r = Rect::from_center_size(at, galley.size() + vec2(20.0, 10.0));
            painter.rect_filled(r, 6.0, Color32::from_rgb(90, 44, 44));
            painter.galley(r.min + vec2(10.0, 5.0), galley, theme::TEXT);
            ui.ctx()
                .request_repaint_after(std::time::Duration::from_secs_f64(until - now));
        } else {
            w.canvas.flash = None;
        }
    }
    if w.live.is_some() {
        // Glowing wires fade over a few frames.
        ui.ctx().request_repaint();
    }
}

fn wires(ui: &Ui, painter: &egui::Painter, w: &ScriptGraphWindow, view: View, pins: &[PinSpot]) {
    let find = |node: u32, pin: &str, output: bool| {
        pins.iter()
            .find(|x| x.0 == node && x.1 == pin && x.2 == output)
    };
    let t = ui.input(|i| i.time) as f32;
    for c in &w.view().connections {
        let (Some(a), Some(b)) = (find(c.from, &c.output, true), find(c.to, &c.input, false))
        else {
            continue;
        };
        let selected = w.selected.contains(&c.from) || w.selected.contains(&c.to);
        let base = type_color(a.3);
        if a.3.is_exec() && active(w, c.to, &c.input) {
            wire_width(painter, a.4, b.4, GLOW.gamma_multiply(0.25), view.zoom, 9.0);
            wire_width(painter, a.4, b.4, GLOW, view.zoom, 3.2);
            let dot = node_canvas::wire_point(a.4, b.4, view.zoom, (t * 1.6).fract());
            painter.circle_filled(dot, 3.5 * view.zoom.max(0.6), Color32::WHITE);
            continue;
        }
        let color = if selected {
            base
        } else {
            base.gamma_multiply(0.7)
        };
        let width = if a.3.is_exec() { 2.6 } else { 2.0 };
        wire_width(painter, a.4, b.4, color, view.zoom, width);
    }
}

/// A wire dropped: onto a pin it connects, onto empty canvas it opens the filtered search.
fn finish_connection(
    s: &mut EditorState,
    from: PinRef,
    at: Option<Pos2>,
    pins: &[PinSpot],
    view: View,
) {
    let Some(p) = at else {
        return;
    };
    let reach = 12.0 * view.zoom.max(0.6);
    let target = pins
        .iter()
        .filter(|x| x.2 != from.output && x.0 != from.node)
        .min_by(|a, b| a.4.distance(p).total_cmp(&b.4.distance(p)))
        .filter(|x| x.4.distance(p) <= reach)
        .cloned();
    match target {
        Some((node, pin, output, _, _)) => {
            let root = s.project.root.clone();
            let r = s.script_graph.connect(
                &root,
                (from.node, &from.pin, from.output),
                (node, &pin, output),
            );
            if let Err(e) = r {
                s.script_graph.canvas.flash = Some((format!("Can't connect: {e}"), -1.0));
            }
        }
        None => {
            let over_pin_start = pins
                .iter()
                .any(|x| x.0 == from.node && x.1 == from.pin && x.4.distance(p) <= reach);
            if !over_pin_start {
                open_search_at(s, p, Some(from));
            }
        }
    }
}

/// One node: drawing and interaction. Returns true if it was clicked.
#[allow(clippy::too_many_arguments)]
fn node_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    node: &Node,
    l: &NodeLayout,
    rect: Rect,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let id = node.id;
    let round = 6.0 * z;
    let w = &s.script_graph;
    let selected = w.selected.contains(&id);
    let paused = w.at_root() && w.live.as_ref().and_then(|l| l.paused) == Some(id);
    let breakpoint = w.at_root() && w.breakpoints.contains(&id);
    let error = w.error_of(id);
    // Body and header.
    painter.rect_filled(
        rect.translate(vec2(2.0, 3.0) * z),
        round,
        Color32::from_black_alpha(60),
    );
    painter.rect_filled(rect, round, NODE_BG);
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H * z));
    let hc = category_color(&l.category);
    painter.rect_filled(
        header,
        egui::CornerRadius {
            nw: round as u8,
            ne: round as u8,
            sw: 0,
            se: 0,
        },
        hc,
    );
    let pure = !l.ports.has_exec();
    let title_x = header.left() + if breakpoint { 20.0 } else { 9.0 } * z;
    painter.text(
        pos2(title_x, header.center().y),
        Align2::LEFT_CENTER,
        &l.title,
        FontId::proportional(12.5 * z),
        Color32::WHITE,
    );
    if breakpoint {
        painter.circle_filled(
            pos2(header.left() + 10.0 * z, header.center().y),
            5.0 * z,
            BREAK,
        );
    }
    if pure {
        // Pure nodes: a thin top strip instead of a full header colour.
        painter.line_segment(
            [header.left_bottom(), header.right_bottom()],
            Stroke::new(1.0, Color32::from_black_alpha(80)),
        );
    }
    let outline = if paused {
        Some(Stroke::new(3.0, theme::WARN))
    } else if error.is_some() {
        Some(Stroke::new(2.0, node_canvas::ERROR))
    } else if selected {
        Some(Stroke::new(2.0, SELECT))
    } else {
        None
    };
    if let Some(st) = outline {
        painter.rect_stroke(rect, round, st, StrokeKind::Outside);
    }
    if paused {
        painter.text(
            rect.right_top() + vec2(-8.0 * z, -10.0 * z),
            Align2::RIGHT_BOTTOM,
            "⏸ paused",
            FontId::proportional(11.0 * z),
            theme::WARN,
        );
    }

    // The body takes clicks and drags (pins and fields on top win).
    let body = ui.interact(rect, ui.id().with(("vs_node", id)), Sense::click_and_drag());
    let mut clicked = false;
    if body.clicked() || body.drag_started() {
        clicked = true;
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        let w = &mut s.script_graph;
        if add && body.clicked() {
            if !w.selected.remove(&id) {
                w.selected.insert(id);
            }
        } else if !w.selected.contains(&id) {
            w.selected = [id].into();
        }
        w.search = None;
    }
    if body.dragged_by(egui::PointerButton::Primary) && !ui.input(|i| i.modifiers.alt) {
        let d = body.drag_delta() / z;
        if d != Vec2::ZERO {
            let ids = s.script_graph.selected.clone();
            s.script_graph.edit("Move", true, |g| {
                for n in &mut g.nodes {
                    if ids.contains(&n.id) {
                        n.position[0] += d.x;
                        n.position[1] += d.y;
                    }
                }
            });
        }
    }
    if body.double_clicked() && node.kind == "Subgraph" {
        open_subgraph(s, node);
    }
    if body.secondary_clicked() && !s.script_graph.selected.contains(&id) {
        s.script_graph.selected = [id].into();
    }
    body.context_menu(|ui| node_menu(ui, s, node));

    // Pins.
    let connected_in = |pin: &str| {
        s.script_graph
            .view()
            .connections
            .iter()
            .any(|c| c.to == id && c.input == pin)
    };
    let connected_out = |pin: &str| {
        s.script_graph
            .view()
            .connections
            .iter()
            .any(|c| c.from == id && c.output == pin)
    };
    let inputs: Vec<(visual_script::Pin, Pos2, bool)> = l
        .ports
        .inputs
        .iter()
        .zip(&l.inputs)
        .map(|(p, at)| {
            (
                p.clone(),
                pin_screen(view, node, *at),
                connected_in(&p.name),
            )
        })
        .collect();
    let outputs: Vec<(visual_script::Pin, Pos2, bool)> = l
        .ports
        .outputs
        .iter()
        .zip(&l.outputs)
        .map(|(p, at)| {
            (
                p.clone(),
                pin_screen(view, node, *at),
                connected_out(&p.name),
            )
        })
        .collect();
    for (p, pos, connected) in &inputs {
        draw_pin(painter, *pos, p.ty, *connected, z);
        let mut x = pos.x + 10.0 * z;
        if shows_label(p) {
            painter.text(
                pos2(x, pos.y),
                Align2::LEFT_CENTER,
                &p.name,
                FontId::proportional(11.5 * z),
                theme::TEXT,
            );
            x += text_w(&p.name, 11.5) * z + 4.0 * z;
        }
        if !connected && !p.ty.is_exec() && z >= 0.5 {
            inline_value(ui, s, node, p, pos2(x, pos.y), z, busy);
        }
        pin_interact(ui, s, node, p, *pos, false, z);
    }
    for (p, pos, connected) in &outputs {
        draw_pin(painter, *pos, p.ty, *connected, z);
        if shows_label(p) {
            painter.text(
                pos2(pos.x - 10.0 * z, pos.y),
                Align2::RIGHT_CENTER,
                &p.name,
                FontId::proportional(11.5 * z),
                theme::TEXT,
            );
        }
        pin_interact(ui, s, node, p, *pos, true, z);
    }
    if let Some(message) = error {
        error_badge(
            ui,
            painter,
            rect.right_top() + vec2(-2.0, 2.0) * z,
            z,
            ui.id().with(("vs_err", id)),
            &message,
        );
    }
    clicked
}

fn draw_pin(painter: &egui::Painter, pos: Pos2, ty: PinType, connected: bool, z: f32) {
    let color = type_color(ty);
    if ty.is_exec() {
        let r = 5.5 * z;
        let pts = vec![
            pos + vec2(-r * 0.7, -r),
            pos + vec2(r, 0.0),
            pos + vec2(-r * 0.7, r),
        ];
        if connected {
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        } else {
            painter.add(egui::Shape::closed_line(
                pts,
                Stroke::new(1.4 * z.max(0.6), color),
            ));
        }
    } else if connected {
        painter.circle_filled(pos, PIN_R * z, color);
    } else {
        painter.circle_stroke(pos, PIN_R * z, Stroke::new(1.6 * z.max(0.6), color));
    }
}

/// Dragging a pin starts a wire; right-click (or Alt+click) breaks its wires; hovering it
/// while playing shows its value.
fn pin_interact(
    ui: &mut Ui,
    s: &mut EditorState,
    node: &Node,
    p: &visual_script::Pin,
    pos: Pos2,
    output: bool,
    z: f32,
) {
    let r = Rect::from_center_size(pos, vec2(16.0, 16.0) * z.max(0.7));
    let resp = ui.interact(
        r,
        ui.id().with(("vs_pin", node.id, &p.name, output)),
        Sense::click_and_drag(),
    );
    if resp.drag_started_by(egui::PointerButton::Primary) {
        s.script_graph.canvas.connecting = Some(PinRef {
            node: node.id,
            pin: p.name.clone(),
            output,
            ty: p.ty,
        });
    }
    let alt_click = resp.clicked() && ui.input(|i| i.modifiers.alt);
    if resp.secondary_clicked() || alt_click {
        let (id, name) = (node.id, p.name.clone());
        s.script_graph
            .edit("Break Links", false, |g| g.disconnect(id, &name, output));
    }
    if resp.hovered() {
        let mut tip = format!("{} ({})", p.name, p.ty.label());
        if s.script_graph.at_root()
            && s.script_graph.live.is_some()
            && !p.ty.is_exec()
            && let Some(rt) = s.runtime.as_ref()
        {
            match rt.graph_value(node.id, &p.name, output) {
                Some(v) => tip = format!("{} = {v}", p.name),
                None => tip.push_str("\nnot computed yet"),
            }
        }
        resp.on_hover_text(tip);
    }
}

/// The field of an unconnected input.
fn inline_value(
    ui: &mut Ui,
    s: &mut EditorState,
    node: &Node,
    p: &visual_script::Pin,
    at: Pos2,
    z: f32,
    busy: bool,
) {
    let h = 16.0 * z;
    let width = field_w(p.ty) * z;
    if width <= 0.0 {
        return;
    }
    let r = Rect::from_min_size(pos2(at.x, at.y - h * 0.5), vec2(width, h));
    if p.ty == PinType::GameObject {
        ui.painter().text(
            r.left_center(),
            Align2::LEFT_CENTER,
            "Self",
            FontId::proportional(10.5 * z),
            theme::TEXT_DIM,
        );
        return;
    }
    let mut lit = node
        .values
        .get(&p.name)
        .map_or_else(|| p.default.clone(), |l| l.coerce(p.ty));
    let before = lit.clone();
    ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
        let st = ui.style_mut();
        st.override_font_id = Some(FontId::proportional(10.5 * z));
        st.spacing.interact_size.y = h;
        st.spacing.item_spacing = vec2(2.0 * z, 0.0);
        st.spacing.button_padding = vec2(2.0 * z, 0.0);
        st.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 40, 43);
        st.visuals.extreme_bg_color = Color32::from_rgb(40, 40, 43);
        let slot = |k: usize, n: usize| {
            let fw = (width - 2.0 * z * (n as f32 - 1.0)) / n as f32;
            Rect::from_min_size(
                pos2(r.left() + k as f32 * (fw + 2.0 * z), r.top()),
                vec2(fw, h),
            )
        };
        match &mut lit {
            Literal::Float(f) => {
                ui.put(
                    slot(0, 1),
                    egui::DragValue::new(f).speed(0.05).max_decimals(3),
                );
            }
            Literal::Int(i) => {
                ui.put(slot(0, 1), egui::DragValue::new(i).speed(0.2));
            }
            Literal::Bool(b) => {
                ui.put(slot(0, 1), egui::Checkbox::without_text(b));
            }
            Literal::String(t) => {
                ui.put(
                    slot(0, 1),
                    egui::TextEdit::singleline(t).margin(vec2(2.0, 0.0)),
                );
            }
            Literal::Vector3(v) | Literal::Quaternion(v) => {
                for (k, c) in v.iter_mut().enumerate() {
                    ui.put(
                        slot(k, 3),
                        egui::DragValue::new(c).speed(0.05).max_decimals(2),
                    );
                }
            }
            Literal::None => {
                // An Any input: typed text (a number, true / false, or a string).
                let mut t = String::new();
                if ui
                    .put(
                        slot(0, 1),
                        egui::TextEdit::singleline(&mut t).hint_text("None"),
                    )
                    .changed()
                {
                    lit = parse_any(&t);
                }
            }
        }
    });
    if p.ty == PinType::Any
        && let Literal::String(t) = &lit
    {
        // Keep Any values typed as what they read as.
        let parsed = parse_any(t);
        if !matches!(parsed, Literal::String(_)) && before != parsed {
            lit = parsed;
        }
    }
    if lit != before {
        let (id, name) = (node.id, p.name.clone());
        s.script_graph.edit("Change Value", busy, |g| {
            g.set_value(id, &name, lit);
        });
    }
}

/// Text typed into an Any field as a literal.
pub fn parse_any(t: &str) -> Literal {
    let trimmed = t.trim();
    if trimmed.is_empty() {
        return Literal::None;
    }
    if let Ok(i) = trimmed.parse::<i64>() {
        return Literal::Int(i);
    }
    if let Ok(f) = trimmed.parse::<f32>() {
        return Literal::Float(f);
    }
    match trimmed {
        "true" => Literal::Bool(true),
        "false" => Literal::Bool(false),
        _ => Literal::String(t.to_owned()),
    }
}

/// Double-click on a Subgraph node: open its macro here, or its asset.
fn open_subgraph(s: &mut EditorState, node: &Node) {
    let m = node.param("macro");
    if !m.is_empty() {
        s.script_graph.enter_macro(m);
        return;
    }
    let path = node.param("path").to_owned();
    if !path.is_empty() {
        super::open(s, &path);
    }
}

fn node_menu(ui: &mut Ui, s: &mut EditorState, node: &Node) {
    let root = s.project.root.clone();
    let w = &mut s.script_graph;
    if node.kind == "Subgraph" && ui.button("Open Subgraph").clicked() {
        open_subgraph(s, node);
        ui.close();
        return;
    }
    if w.at_root() {
        let on = w.breakpoints.contains(&node.id);
        let label = if on {
            "Remove Breakpoint (F9)"
        } else {
            "Add Breakpoint (F9)"
        };
        if ui.button(label).clicked() {
            w.toggle_breakpoint(node.id);
            ui.close();
        }
        ui.separator();
    }
    if ui.button("Copy").clicked() {
        if let Some(t) = w.copy_selected() {
            ui.ctx().copy_text(t);
        }
        ui.close();
    }
    if ui.button("Cut").clicked() {
        if let Some(t) = w.cut_selected() {
            ui.ctx().copy_text(t);
        }
        ui.close();
    }
    if ui.button("Duplicate").clicked() {
        w.duplicate_selected();
        ui.close();
    }
    if ui.button("Delete").clicked() {
        w.delete_selected();
        ui.close();
    }
    ui.separator();
    if ui.button("Group Selection").clicked() {
        w.group_selected();
        ui.close();
    }
    if ui
        .button("Collapse To Macro")
        .on_hover_text("Move the selected nodes into a macro used by one Subgraph node")
        .clicked()
    {
        if w.collapse_to_macro(&root).is_none() {
            w.canvas.flash = Some(("Events can't go into a macro".into(), -1.0));
        }
        ui.close();
    }
}

fn canvas_menu(ui: &mut Ui, s: &mut EditorState, at: Option<Pos2>) {
    if ui.button("Create Node…").clicked() {
        if let Some(p) = at {
            open_search_at(s, p, None);
        }
        ui.close();
    }
    let w = &mut s.script_graph;
    if ui
        .add_enabled(w.clipboard.is_some(), egui::Button::new("Paste"))
        .clicked()
    {
        w.paste(None);
        ui.close();
    }
    if ui.button("Add Sticky Note").clicked() {
        let g = at
            .and_then(|p| w.canvas.view.map(|v| v.graph(p)))
            .unwrap_or(pos2(0.0, 0.0));
        w.edit("Add Sticky Note", false, |gr| {
            gr.notes.push(visual_script::StickyNote {
                position: [g.x, g.y],
                size: [200.0, 110.0],
                text: "Note".into(),
            })
        });
        ui.close();
    }
    if !w.selected.is_empty() {
        if ui.button("Group Selection").clicked() {
            w.group_selected();
            ui.close();
        }
        if ui.button("Collapse Selection To Macro").clicked() {
            let root = s.project.root.clone();
            if s.script_graph.collapse_to_macro(&root).is_none() {
                s.script_graph.canvas.flash = Some(("Events can't go into a macro".into(), -1.0));
            }
            ui.close();
            return;
        }
    }
    if ui.button("Frame All (F)").clicked() {
        s.script_graph.canvas.frame_all = true;
        ui.close();
    }
}

fn group_rect(w: &ScriptGraphWindow, view: View, nodes: &[u32]) -> Option<Rect> {
    let g = w.view();
    let mut r = Rect::NOTHING;
    for id in nodes {
        if let Some(n) = g.node(*id) {
            r = r.union(node_rect(view, n, &layout(g, n, &w.canvas.files)));
        }
    }
    r.is_positive().then(|| {
        Rect::from_min_max(
            r.min - vec2(16.0, 40.0) * view.zoom,
            r.max + vec2(16.0, 16.0) * view.zoom,
        )
    })
}

fn groups(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, area: Rect) {
    let z = view.zoom;
    let count = s.script_graph.view().groups.len();
    for gi in 0..count {
        let Some(group) = s.script_graph.view().groups.get(gi).cloned() else {
            continue;
        };
        let Some(r) = group_rect(&s.script_graph, view, &group.nodes) else {
            continue;
        };
        if !r.intersects(area) {
            continue;
        }
        painter.rect(
            r,
            8.0 * z,
            Color32::from_rgba_unmultiplied(120, 140, 170, 22),
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 170, 200, 70)),
            StrokeKind::Inside,
        );
        let bar = Rect::from_min_size(r.min, vec2(r.width(), 26.0 * z));
        let renaming = s
            .script_graph
            .canvas
            .renaming
            .as_ref()
            .filter(|(i, _)| *i == gi)
            .map(|(_, t)| t.clone());
        if let Some(mut text) = renaming {
            let edit = ui.put(
                bar.shrink(3.0),
                egui::TextEdit::singleline(&mut text).font(FontId::proportional(13.0 * z)),
            );
            edit.request_focus();
            if edit.lost_focus() {
                let title = text.clone();
                s.script_graph.edit("Rename Group", false, |g| {
                    if let Some(gr) = g.groups.get_mut(gi) {
                        gr.title = title;
                    }
                });
                s.script_graph.canvas.renaming = None;
            } else {
                s.script_graph.canvas.renaming = Some((gi, text));
            }
            continue;
        }
        painter.text(
            bar.left_center() + vec2(10.0 * z, 0.0),
            Align2::LEFT_CENTER,
            &group.title,
            FontId::proportional(13.0 * z),
            theme::TEXT,
        );
        let resp = ui.interact(bar, ui.id().with(("vs_group", gi)), Sense::click_and_drag());
        if resp.double_clicked() {
            s.script_graph.canvas.renaming = Some((gi, group.title.clone()));
        }
        if resp.drag_started() {
            s.script_graph.canvas.group_drag = Some(gi);
        }
        if s.script_graph.canvas.group_drag == Some(gi) && resp.dragged() {
            let d = resp.drag_delta() / z;
            let ids = group.nodes.clone();
            s.script_graph.edit("Move Group", true, |g| {
                for n in &mut g.nodes {
                    if ids.contains(&n.id) {
                        n.position[0] += d.x;
                        n.position[1] += d.y;
                    }
                }
            });
        }
        if resp.drag_stopped() {
            s.script_graph.canvas.group_drag = None;
        }
        resp.context_menu(|ui| {
            if ui.button("Select Nodes").clicked() {
                s.script_graph.selected = group.nodes.iter().copied().collect();
                ui.close();
            }
            if ui.button("Rename").clicked() {
                s.script_graph.canvas.renaming = Some((gi, group.title.clone()));
                ui.close();
            }
            if ui.button("Ungroup").clicked() {
                s.script_graph.edit("Ungroup", false, |g| {
                    if gi < g.groups.len() {
                        g.groups.remove(gi);
                    }
                });
                ui.close();
            }
        });
    }
}

fn notes(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, area: Rect) {
    let z = view.zoom;
    let count = s.script_graph.view().notes.len();
    for ni in 0..count {
        let Some(note) = s.script_graph.view().notes.get(ni).cloned() else {
            continue;
        };
        let r = Rect::from_min_size(
            view.screen(pos2(note.position[0], note.position[1])),
            vec2(note.size[0], note.size[1]) * z,
        );
        if !r.intersects(area) {
            continue;
        }
        painter.rect_filled(r, 4.0 * z, Color32::from_rgb(222, 205, 120));
        if s.script_graph.canvas.editing_note == Some(ni) {
            let mut text = note.text.clone();
            let edit = ui.put(
                r.shrink(6.0 * z),
                egui::TextEdit::multiline(&mut text)
                    .font(FontId::proportional(12.0 * z))
                    .text_color(Color32::from_rgb(40, 36, 20))
                    .frame(egui::Frame::NONE),
            );
            if edit.changed() {
                s.script_graph.edit("Edit Note", true, |g| {
                    if let Some(n) = g.notes.get_mut(ni) {
                        n.text = text.clone();
                    }
                });
            }
            if edit.lost_focus() {
                s.script_graph.canvas.editing_note = None;
            }
            continue;
        }
        painter.text(
            r.min + vec2(8.0, 6.0) * z,
            Align2::LEFT_TOP,
            &note.text,
            FontId::proportional(12.0 * z),
            Color32::from_rgb(40, 36, 20),
        );
        let resp = ui.interact(r, ui.id().with(("vs_note", ni)), Sense::click_and_drag());
        if resp.double_clicked() {
            s.script_graph.canvas.editing_note = Some(ni);
        }
        if resp.drag_started() {
            s.script_graph.canvas.note_drag = Some(ni);
        }
        if s.script_graph.canvas.note_drag == Some(ni) && resp.dragged() {
            let d = resp.drag_delta() / z;
            s.script_graph.edit("Move Note", true, |g| {
                if let Some(n) = g.notes.get_mut(ni) {
                    n.position[0] += d.x;
                    n.position[1] += d.y;
                }
            });
        }
        if resp.drag_stopped() {
            s.script_graph.canvas.note_drag = None;
        }
        resp.context_menu(|ui| {
            if ui.button("Edit").clicked() {
                s.script_graph.canvas.editing_note = Some(ni);
                ui.close();
            }
            if ui.button("Delete Note").clicked() {
                s.script_graph.edit("Delete Note", false, |g| {
                    if ni < g.notes.len() {
                        g.notes.remove(ni);
                    }
                });
                ui.close();
            }
        });
    }
}
