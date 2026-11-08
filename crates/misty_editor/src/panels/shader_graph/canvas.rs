//! The node canvas: grid, groups, nodes (header, ports, settings, preview thumbnail), the master
//! stack, connections, inline values of unconnected inputs, sticky notes, pan (middle mouse or
//! Alt+drag) and zoom (wheel), selection (click, Ctrl/Shift+click, rubber band), dragging nodes
//! and groups, dragging ports to connect, and the right-click menus (Copy, Cut, Paste,
//! Duplicate, Group Selection).

use egui::epaint::CubicBezierShape;
use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    UiBuilder, Vec2, pos2, vec2,
};
use misty_engine::shader_graph::{
    Block, MASTER, Node, NodeKind, Port, PortDefault, ShaderGraph, Space, StickyNote, ValueType,
};

use super::{ShaderGraphWindow, interacting, search};
use crate::state::EditorState;
use crate::theme;

const NODE_W: f32 = 164.0;
const MASTER_W: f32 = 214.0;
const HEADER_H: f32 = 28.0;
const ROW_H: f32 = 22.0;
const PARAM_H: f32 = 26.0;
const PREVIEW: f32 = 124.0;
const FOOT_H: f32 = 12.0;
const PORT_R: f32 = 4.5;
const CANVAS_BG: Color32 = Color32::from_rgb(36, 36, 38);
const NODE_BG: Color32 = Color32::from_rgb(58, 58, 61);
const HEADER_BG: Color32 = Color32::from_rgb(66, 66, 70);
const SELECT: Color32 = Color32::from_rgb(88, 156, 236);

/// A Blackboard property dragged onto the canvas.
#[derive(Clone, Debug)]
pub struct PropertyPayload(pub String);

/// A Blackboard keyword dragged onto the canvas.
#[derive(Clone, Debug)]
pub struct KeywordPayload(pub String);

/// Graph ↔ screen mapping: screen = centre + (graph + pan) × zoom.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct View {
    pub centre: Pos2,
    pub pan: Vec2,
    pub zoom: f32,
}

impl View {
    pub fn screen(&self, p: Pos2) -> Pos2 {
        self.centre + (p.to_vec2() + self.pan) * self.zoom
    }

    pub fn graph(&self, p: Pos2) -> Pos2 {
        pos2(0.0, 0.0) + ((p - self.centre) / self.zoom - self.pan)
    }
}

/// One end of a connection being dragged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortRef {
    pub node: u32,
    pub port: String,
    pub output: bool,
}

#[derive(Default)]
pub(crate) struct CanvasState {
    pub view: Option<View>,
    /// A connection being dragged from this port.
    pub connecting: Option<PortRef>,
    /// Rubber band start (screen).
    band: Option<Pos2>,
    /// Nodes are being dragged.
    moving: bool,
    /// Fit every node into view next frame.
    pub frame_all: bool,
    /// A short message after a refused connection (text, until time).
    pub flash: Option<(String, f64)>,
    /// Sticky note being dragged.
    note_drag: Option<usize>,
    /// Where the canvas menu was opened (screen).
    menu_at: Option<Pos2>,
    /// Group being dragged by its title bar.
    group_drag: Option<usize>,
    /// Group title being renamed: (group, text).
    pub renaming: Option<(usize, String)>,
}

/// Port colour by type (soft, readable on the dark canvas).
pub fn type_color(t: ValueType) -> Color32 {
    match t {
        ValueType::Float => Color32::from_rgb(132, 196, 236),
        ValueType::Vec2 => Color32::from_rgb(150, 214, 140),
        ValueType::Vec3 => Color32::from_rgb(236, 214, 120),
        ValueType::Vec4 => Color32::from_rgb(226, 150, 200),
        ValueType::Texture => Color32::from_rgb(236, 120, 110),
    }
}

/// Header accent per category.
fn category_color(kind: &NodeKind) -> Color32 {
    match kind.category() {
        "Input" => Color32::from_rgb(96, 150, 210),
        "Math" => Color32::from_rgb(120, 170, 120),
        "Texture" => Color32::from_rgb(210, 120, 100),
        "Procedural" => Color32::from_rgb(180, 130, 210),
        "Master" => Color32::from_rgb(200, 160, 80),
        "Sub Graphs" => Color32::from_rgb(90, 180, 180),
        _ => Color32::from_rgb(150, 150, 150),
    }
}

/// Where a node's parts are, in graph units relative to its top-left corner.
pub struct NodeLayout {
    pub size: Vec2,
    /// (port, centre, block heading above it)
    pub inputs: Vec<(Port, Pos2)>,
    pub outputs: Vec<(Port, Pos2)>,
    /// Block headings of the master stack: (text, y).
    pub headings: Vec<(&'static str, f32)>,
    pub params: Option<Rect>,
    pub preview: Option<Rect>,
}

/// Constants and properties show their value inline, so they get no preview square.
pub fn has_preview(kind: &NodeKind) -> bool {
    !matches!(
        kind,
        NodeKind::Property { .. }
            | NodeKind::Float
            | NodeKind::Vector2
            | NodeKind::Vector3
            | NodeKind::Vector4
            | NodeKind::Time
            | NodeKind::VertexColor
            | NodeKind::CustomFunction { .. }
    )
}

fn has_params(kind: &NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Color { .. }
            | NodeKind::Position { .. }
            | NodeKind::Normal { .. }
            | NodeKind::ViewDirection { .. }
            | NodeKind::SampleTexture2D { .. }
            | NodeKind::Triplanar { .. }
    )
}

/// A node's title on the canvas (properties and keywords show their display names; a Sub
/// Graph's master stack is its Output node).
pub fn node_title(g: &ShaderGraph, node: &Node) -> String {
    match &node.kind {
        NodeKind::Property { name } => g
            .property(name)
            .map_or_else(|| name.clone(), |p| p.display.clone()),
        NodeKind::Keyword { name } => g
            .keyword(name)
            .map_or_else(|| name.clone(), |k| k.display.clone()),
        NodeKind::Master if g.is_sub_graph() => "Output".into(),
        k => k.title(),
    }
}

pub fn layout(g: &ShaderGraph, node: &Node) -> NodeLayout {
    if node.kind == NodeKind::Master {
        let mut inputs = Vec::new();
        let mut headings = Vec::new();
        let mut y = HEADER_H;
        let mut block = None;
        for (b, p) in g.master_ports() {
            if block != Some(b) {
                block = Some(b);
                headings.push((
                    match b {
                        Block::Vertex => "Vertex",
                        Block::Fragment => "Fragment",
                        Block::Output => "Outputs",
                    },
                    y + 10.0,
                ));
                y += 20.0;
            }
            inputs.push((p, pos2(0.0, y + ROW_H * 0.5)));
            y += ROW_H;
        }
        return NodeLayout {
            size: vec2(MASTER_W, y + 8.0),
            inputs,
            outputs: Vec::new(),
            headings,
            params: None,
            preview: None,
        };
    }
    let ins = g.inputs(node);
    let outs = g.outputs(node);
    let rows = ins.len().max(outs.len()) as f32;
    let mut y = HEADER_H + rows * ROW_H + 4.0;
    let inputs = ins
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p, pos2(0.0, HEADER_H + (i as f32 + 0.5) * ROW_H)))
        .collect();
    let has_outputs = !outs.is_empty();
    let outputs = outs
        .into_iter()
        .enumerate()
        .map(|(i, p)| (p, pos2(NODE_W, HEADER_H + (i as f32 + 0.5) * ROW_H)))
        .collect();
    let params = has_params(&node.kind).then(|| {
        let r = Rect::from_min_size(pos2(10.0, y), vec2(NODE_W - 20.0, PARAM_H - 4.0));
        y += PARAM_H;
        r
    });
    let preview = (has_outputs && !node.hide_preview && has_preview(&node.kind)).then(|| {
        let r = Rect::from_min_size(
            pos2((NODE_W - PREVIEW) * 0.5, y + 2.0),
            vec2(PREVIEW, PREVIEW),
        );
        y += PREVIEW + 6.0;
        r
    });
    if has_outputs && has_preview(&node.kind) {
        y += FOOT_H;
    }
    NodeLayout {
        size: vec2(NODE_W, y.max(HEADER_H + 10.0)),
        inputs,
        outputs,
        headings: Vec::new(),
        params,
        preview,
    }
}

/// Screen rect of a node.
fn node_rect(view: View, node: &Node, l: &NodeLayout) -> Rect {
    let min = view.screen(pos2(node.position[0], node.position[1]));
    Rect::from_min_size(min, l.size * view.zoom)
}

fn port_screen(view: View, node: &Node, at: Pos2) -> Pos2 {
    view.screen(pos2(node.position[0] + at.x, node.position[1] + at.y))
}

/// Screen positions of every port: (port ref, position, type).
fn all_ports(g: &ShaderGraph, view: View) -> Vec<(PortRef, Pos2, ValueType)> {
    let types = g.types();
    let mut out = Vec::new();
    for n in &g.nodes {
        let l = layout(g, n);
        let dynamic = types.dynamic(n.id);
        for (p, at) in &l.inputs {
            out.push((
                PortRef {
                    node: n.id,
                    port: p.name.to_owned(),
                    output: false,
                },
                port_screen(view, n, *at),
                p.ty.resolve(dynamic),
            ));
        }
        for (p, at) in &l.outputs {
            out.push((
                PortRef {
                    node: n.id,
                    port: p.name.to_owned(),
                    output: true,
                },
                port_screen(view, n, *at),
                types.output(n.id, p.name).unwrap_or(p.ty.resolve(dynamic)),
            ));
        }
    }
    out
}

/// Connect two port ends (either order); Err holds why not.
pub fn connect_refs(w: &mut ShaderGraphWindow, a: &PortRef, b: &PortRef) -> Result<(), String> {
    let (from, to) = match (a.output, b.output) {
        (true, false) => (a, b),
        (false, true) => (b, a),
        _ => return Err("connect an output to an input".into()),
    };
    if let Some(e) = w
        .graph
        .check_connection(from.node, &from.port, to.node, &to.port)
    {
        return Err(e);
    }
    let (f, t) = (from.clone(), to.clone());
    w.edit("Connect", false, |g| {
        let _ = g.connect(f.node, &f.port, t.node, &t.port);
    });
    Ok(())
}

/// Open the Create Node search at a screen point (optionally connecting to a dragged port).
pub fn open_search_at(s: &mut EditorState, screen: Pos2, from: Option<PortRef>) {
    let sub_graphs = search::project_sub_graphs(&s.project.root, s.shader_graph.path.as_deref());
    let w = &mut s.shader_graph;
    let graph = w.canvas.view.map_or(pos2(0.0, 0.0), |v| v.graph(screen));
    let mut search = search::Search::new(screen, graph, from);
    search.sub_graphs = sub_graphs;
    w.search = Some(search);
}

fn frame_all(w: &mut ShaderGraphWindow, area: Rect) {
    let mut bounds = Rect::NOTHING;
    for n in &w.graph.nodes {
        let l = layout(&w.graph, n);
        bounds = bounds.union(Rect::from_min_size(
            pos2(n.position[0], n.position[1]),
            l.size,
        ));
    }
    if !bounds.is_positive() {
        return;
    }
    // Keep clear of the floating panels.
    let left = if w.show_blackboard { 250.0 } else { 0.0 };
    let right = if w.show_settings || w.show_preview {
        250.0
    } else {
        0.0
    };
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
    // A large graph rather uses the whole canvas (under the panels) than becomes unreadable.
    let (free, zoom) = if fit(free) >= 0.55 {
        (free, fit(free))
    } else {
        (area, fit(area))
    };
    let zoom = zoom.clamp(0.45, 1.0);
    w.zoom = zoom;
    // screen = centre + (graph + pan) * zoom: put the bounds centre at the free area's centre.
    w.pan = (free.center() - area.center()) / zoom - bounds.center().to_vec2();
}

pub fn show(ui: &mut Ui, s: &mut EditorState, area: Rect) {
    let resp = ui.interact(area, ui.id().with("sg_canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, CANVAS_BG);
    if s.shader_graph.canvas.frame_all {
        s.shader_graph.canvas.frame_all = false;
        frame_all(&mut s.shader_graph, area);
    }
    navigate(ui, &mut s.shader_graph, area, &resp);
    let view = View {
        centre: area.center(),
        pan: s.shader_graph.pan,
        zoom: s.shader_graph.zoom(),
    };
    s.shader_graph.canvas.view = Some(view);
    draw_grid(&painter, area, view);

    groups(ui, s, &painter, view, area);
    notes(ui, s, &painter, view, area);
    connections(&painter, &s.shader_graph, view);

    let busy = interacting(ui.ctx());
    let ids: Vec<u32> = s.shader_graph.graph.nodes.iter().map(|n| n.id).collect();
    let mut clicked_node = false;
    for id in ids {
        let Some(node) = s.shader_graph.graph.node(id).cloned() else {
            continue;
        };
        let l = layout(&s.shader_graph.graph, &node);
        let rect = node_rect(view, &node, &l);
        if !rect.expand(200.0 * view.zoom).intersects(area) {
            continue;
        }
        clicked_node |= node_ui(ui, s, &painter, view, area, &node, &l, rect, busy);
    }

    // A connection being dragged follows the mouse.
    let w = &mut s.shader_graph;
    if let Some(from) = w.canvas.connecting.clone()
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        let ports = all_ports(&w.graph, view);
        if let Some((_, a, ty)) = ports.iter().find(|(r, _, _)| *r == from) {
            let (start, end) = if from.output { (*a, p) } else { (p, *a) };
            wire(&painter, start, end, type_color(*ty), view.zoom);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            w.canvas.connecting = None;
        }
    }

    // Rubber band selection on empty canvas; click deselects.
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
            for n in &w.graph.nodes {
                let l = layout(&w.graph, n);
                if node_rect(view, n, &l).intersects(band) {
                    w.selected.insert(n.id);
                }
            }
            w.canvas.band = None;
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        w.canvas.band = None;
    }
    if resp.clicked() && !clicked_node {
        w.selected.clear();
        w.search = None;
        s.shader_graph.blackboard.selected = None;
    }
    let w = &mut s.shader_graph;
    if resp.secondary_clicked() {
        w.canvas.connecting = None;
        w.canvas.menu_at = resp.interact_pointer_pos();
    }
    let menu_at = w.canvas.menu_at;
    resp.context_menu(|ui| canvas_menu(ui, s, menu_at));

    // A Blackboard property dropped on the canvas becomes a Property node. (Taking a payload
    // of the wrong type drops it, so check the keyword type first.)
    let keyword_drag = egui::DragAndDrop::has_payload_of_type::<KeywordPayload>(ui.ctx());
    if !keyword_drag && let Some(p) = resp.dnd_release_payload::<PropertyPayload>() {
        let at = resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |q| view.graph(q));
        add_property_node(&mut s.shader_graph, &p.0, at);
    }
    // A keyword too, as a Keyword node.
    if let Some(k) = resp.dnd_release_payload::<KeywordPayload>() {
        let at = resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |q| view.graph(q));
        add_keyword_node(&mut s.shader_graph, &k.0, at);
    }

    // Refused connection message.
    let w = &mut s.shader_graph;
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
}

/// Add a Keyword node for a Blackboard keyword at a graph point.
pub fn add_keyword_node(w: &mut ShaderGraphWindow, name: &str, at: Pos2) {
    if w.graph.keyword(name).is_none() {
        return;
    }
    let mut id = 0;
    let name = name.to_owned();
    w.edit("Add Keyword Node", false, |g| {
        id = g.add_node(NodeKind::Keyword { name }, [at.x, at.y]);
    });
    w.selected = [id].into();
}

/// Add a Property node for a Blackboard property at a graph point.
pub fn add_property_node(w: &mut ShaderGraphWindow, name: &str, at: Pos2) {
    if w.graph.property(name).is_none() {
        return;
    }
    let mut id = 0;
    let name = name.to_owned();
    w.edit("Add Property Node", false, |g| {
        id = g.add_node(NodeKind::Property { name }, [at.x, at.y]);
    });
    w.selected = [id].into();
}

/// Wheel zoom (around the mouse) and middle-mouse / Alt+left drag panning.
fn navigate(ui: &Ui, w: &mut ShaderGraphWindow, area: Rect, resp: &Response) {
    if let Some(p) = resp.hover_pos()
        && w.search.is_none()
    {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let old = w.zoom();
            let zoom = (old * (scroll * 0.002).exp()).clamp(0.2, 2.0);
            let centre = area.center();
            let g = (p - centre) / old - w.pan;
            w.zoom = zoom;
            w.pan = (p - centre) / zoom - g;
        }
    }
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.dragged_by(egui::PointerButton::Middle)
        || (alt && resp.dragged_by(egui::PointerButton::Primary))
    {
        w.pan += resp.drag_delta() / w.zoom();
        w.canvas.band = None;
    }
}

pub(crate) fn draw_grid(painter: &egui::Painter, area: Rect, view: View) {
    for (step, alpha) in [(20.0, 10u8), (100.0, 18u8)] {
        let px = step * view.zoom;
        if px < 6.0 {
            continue;
        }
        let stroke = Stroke::new(1.0, Color32::from_white_alpha(alpha));
        let origin = view.screen(pos2(0.0, 0.0));
        let mut x = origin.x - ((origin.x - area.left()) / px).floor() * px;
        while x < area.right() {
            painter.vline(x, area.y_range(), stroke);
            x += px;
        }
        let mut y = origin.y - ((origin.y - area.top()) / px).floor() * px;
        while y < area.bottom() {
            painter.hline(area.x_range(), y, stroke);
            y += px;
        }
    }
}

/// A connection curve.
pub(crate) fn wire(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, zoom: f32) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    let shape = CubicBezierShape::from_points_stroke(
        [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new((2.4 * zoom).max(1.2), color),
    );
    painter.add(shape);
}

fn connections(painter: &egui::Painter, w: &ShaderGraphWindow, view: View) {
    let ports = all_ports(&w.graph, view);
    let find = |node: u32, port: &str, output: bool| {
        ports
            .iter()
            .find(|(r, _, _)| r.node == node && r.port == port && r.output == output)
    };
    for c in &w.graph.connections {
        let (Some((_, a, ty)), Some((_, b, _))) =
            (find(c.from, &c.output, true), find(c.to, &c.input, false))
        else {
            continue;
        };
        let selected = w.selected.contains(&c.from) || w.selected.contains(&c.to);
        let color = if selected {
            type_color(*ty)
        } else {
            type_color(*ty).gamma_multiply(0.75)
        };
        wire(painter, *a, *b, color, view.zoom);
    }
}

/// One node: drawing and interaction. Returns true if it was clicked.
#[allow(clippy::too_many_arguments)]
fn node_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    area: Rect,
    node: &Node,
    l: &NodeLayout,
    rect: Rect,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let id = node.id;
    let title = node_title(&s.shader_graph.graph, node);
    let resp = ui.interact(
        rect.intersect(area),
        ui.id().with(("sg_node", id)),
        Sense::click_and_drag(),
    );
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));

    // Body.
    let w = &s.shader_graph;
    let round = CornerRadius::same((8.0 * z).round().clamp(2.0, 14.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(0.0, 4.0 * z)),
        round,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(rect, round, NODE_BG);
    let header = Rect::from_min_size(rect.min, vec2(rect.width(), HEADER_H * z));
    painter.rect_filled(
        header,
        CornerRadius {
            nw: round.nw,
            ne: round.ne,
            sw: 0,
            se: 0,
        },
        HEADER_BG,
    );
    let accent = category_color(&node.kind);
    painter.rect_filled(
        Rect::from_min_size(
            header.min + vec2(0.0, header.height() - 2.0 * z),
            vec2(header.width(), 2.0 * z),
        ),
        0.0,
        accent.gamma_multiply(0.8),
    );
    painter.text(
        header.left_center() + vec2(10.0 * z, 0.0),
        Align2::LEFT_CENTER,
        &title,
        FontId::proportional(13.0 * z),
        theme::TEXT,
    );
    let error = w.error.as_ref().filter(|e| e.node == Some(id));
    let selected = w.selected.contains(&id);
    if let Some(e) = error {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(2.0, super::ERROR),
            StrokeKind::Outside,
        );
        let badge = Rect::from_center_size(
            header.right_center() - vec2(14.0 * z, 0.0),
            vec2(16.0, 16.0) * z,
        );
        painter.circle_filled(badge.center(), 8.0 * z, super::ERROR);
        painter.text(
            badge.center(),
            Align2::CENTER_CENTER,
            "!",
            FontId::proportional(12.0 * z),
            Color32::WHITE,
        );
        ui.interact(badge, ui.id().with(("sg_err", id)), Sense::hover())
            .on_hover_text(&e.message);
    } else if selected {
        painter.rect_stroke(rect, round, Stroke::new(2.0, SELECT), StrokeKind::Outside);
    }

    // Master stack block headings.
    for (text, y) in &l.headings {
        let at = view.screen(pos2(node.position[0] + 10.0, node.position[1] + y));
        painter.text(
            at,
            Align2::LEFT_CENTER,
            *text,
            FontId::proportional(11.0 * z),
            theme::TEXT_DIM,
        );
    }

    // Ports.
    let types = w.graph.types();
    let dynamic = types.dynamic(id);
    let connecting = w.canvas.connecting.clone();
    let mut port_events = Vec::new();
    for (output, list) in [(false, &l.inputs), (true, &l.outputs)] {
        for (p, at) in list.iter() {
            let pos = port_screen(view, node, *at);
            let ty = if output {
                types.output(id, p.name).unwrap_or(p.ty.resolve(dynamic))
            } else {
                p.ty.resolve(dynamic)
            };
            let connected = if output {
                w.graph
                    .connections
                    .iter()
                    .any(|c| c.from == id && c.output == p.name)
            } else {
                w.graph.input_connection(id, p.name).is_some()
            };
            let color = type_color(ty);
            let hit = Rect::from_center_size(pos, vec2(16.0, 16.0) * z.max(0.7));
            let label = format!("{title} {} {}", if output { "out" } else { "in" }, p.name);
            let presp = ui.interact(
                hit,
                ui.id().with(("sg_port", id, p.name, output)),
                Sense::click_and_drag(),
            );
            presp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
            let compatible = connecting
                .as_ref()
                .is_some_and(|c| c.output != output && c.node != id);
            let r = PORT_R
                * z
                * if presp.hovered() || compatible {
                    1.3
                } else {
                    1.0
                };
            if connected {
                painter.circle_filled(pos, r, color);
            } else {
                painter.circle_filled(pos, r, NODE_BG);
                painter.circle_stroke(pos, r, Stroke::new(1.6 * z.max(0.6), color));
            }
            let (text_pos, align) = if output {
                (pos - vec2(10.0 * z, 0.0), Align2::RIGHT_CENTER)
            } else {
                (pos + vec2(10.0 * z, 0.0), Align2::LEFT_CENTER)
            };
            painter.text(
                text_pos,
                align,
                p.name,
                FontId::proportional(11.5 * z),
                theme::TEXT.gamma_multiply(0.85),
            );
            port_events.push((*p, output, presp, pos, connected, ty));
        }
    }

    // Inline values of unconnected inputs, outside the node on the left.
    if z > 0.55 {
        for (p, output, _, pos, connected, ty) in &port_events {
            if *output || *connected {
                continue;
            }
            inline_value(ui, s, node, p, *ty, *pos, z, busy);
        }
    }

    // Port interaction.
    for (p, output, presp, _, connected, _) in port_events {
        let w = &mut s.shader_graph;
        let me = PortRef {
            node: id,
            port: p.name.to_owned(),
            output,
        };
        if presp.drag_started_by(egui::PointerButton::Primary) {
            if !output && connected {
                // Pick up the existing connection from its source.
                let c = w.graph.input_connection(id, p.name).cloned();
                if let Some(c) = c {
                    let name = p.name;
                    w.edit("Disconnect", false, |g| {
                        g.disconnect(id, name);
                    });
                    w.canvas.connecting = Some(PortRef {
                        node: c.from,
                        port: c.output,
                        output: true,
                    });
                }
            } else {
                w.canvas.connecting = Some(me.clone());
            }
        }
        if presp.drag_stopped()
            && let Some(from) = w.canvas.connecting.take()
        {
            let at = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
            finish_connection(s, from, at, view);
        }
        let w = &mut s.shader_graph;
        if presp.clicked() && ui.input(|i| i.modifiers.alt) {
            let name = p.name;
            if output {
                w.edit("Disconnect", false, |g| {
                    g.connections
                        .retain(|c| !(c.from == id && c.output == name))
                });
            } else {
                w.edit("Disconnect", false, |g| {
                    g.disconnect(id, name);
                });
            }
        }
    }

    // Node settings.
    if let Some(pr) = l.params
        && z > 0.45
    {
        let r = Rect::from_min_size(
            view.screen(pos2(node.position[0], node.position[1]) + pr.min.to_vec2()),
            pr.size() * z,
        );
        params_ui(ui, s, node, r, z);
    }

    // Preview thumbnail and its fold toggle.
    if let Some(pv) = l.preview {
        let r = Rect::from_min_size(
            view.screen(pos2(node.position[0], node.position[1]) + pv.min.to_vec2()),
            pv.size() * z,
        );
        checker(painter, r, z);
        match s.shader_graph.previews.texture(id) {
            Some(tex) => {
                painter.image(
                    tex,
                    r,
                    Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                    Color32::WHITE,
                );
            }
            None => {
                painter.text(
                    r.center(),
                    Align2::CENTER_CENTER,
                    "—",
                    FontId::proportional(14.0 * z),
                    theme::TEXT_DIM,
                );
            }
        }
    }
    if !l.outputs.is_empty() && has_preview(&node.kind) {
        let foot = Rect::from_min_max(pos2(rect.left(), rect.bottom() - FOOT_H * z), rect.max);
        let fresp = ui.interact(foot, ui.id().with(("sg_fold", id)), Sense::click());
        let arrow = if node.hide_preview { "⏷" } else { "⏶" };
        painter.text(
            foot.center(),
            Align2::CENTER_CENTER,
            arrow,
            FontId::proportional(9.0 * z),
            theme::TEXT_DIM.gamma_multiply(if fresp.hovered() { 1.6 } else { 1.0 }),
        );
        if fresp.clicked() {
            s.shader_graph.edit("Toggle Preview", false, |g| {
                if let Some(n) = g.node_mut(id) {
                    n.hide_preview = !n.hide_preview;
                }
            });
        }
    }

    // Select / drag / menu.
    let w = &mut s.shader_graph;
    let alt = ui.input(|i| i.modifiers.alt);
    let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
    if resp.drag_started_by(egui::PointerButton::Primary) && !alt {
        if !w.selected.contains(&id) {
            if !add {
                w.selected.clear();
            }
            w.selected.insert(id);
        }
        w.canvas.moving = true;
    }
    if w.canvas.moving && resp.dragged_by(egui::PointerButton::Primary) && !alt {
        let d = resp.drag_delta() / z;
        if d != Vec2::ZERO {
            let ids: Vec<u32> = w.selected.iter().copied().collect();
            w.edit("Move Nodes", busy, |g| {
                for n in &mut g.nodes {
                    if ids.contains(&n.id) {
                        n.position[0] += d.x;
                        n.position[1] += d.y;
                    }
                }
            });
        }
    }
    if resp.drag_stopped() {
        w.canvas.moving = false;
    }
    let mut clicked = false;
    if resp.clicked() {
        clicked = true;
        if add {
            if !w.selected.remove(&id) {
                w.selected.insert(id);
            }
        } else {
            w.selected = [id].into();
        }
        w.blackboard.selected = None;
    }
    if resp.secondary_clicked() && !w.selected.contains(&id) {
        w.selected = [id].into();
    }
    resp.context_menu(|ui| node_menu(ui, &mut s.shader_graph, id));
    clicked
}

/// Drop a dragged connection: onto a port connects, onto empty canvas opens Create Node.
fn finish_connection(s: &mut EditorState, from: PortRef, at: Option<Pos2>, view: View) {
    let Some(at) = at else { return };
    let w = &mut s.shader_graph;
    let ports = all_ports(&w.graph, view);
    let target = ports
        .iter()
        .filter(|(r, _, _)| r.output != from.output && r.node != from.node)
        .map(|(r, p, _)| (r, p.distance(at)))
        .filter(|(_, d)| *d < 14.0 * view.zoom.max(0.7))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(r, _)| r.clone());
    match target {
        Some(t) => {
            if let Err(e) = connect_refs(w, &from, &t) {
                // The display time is set on the next frame (see `show`).
                w.canvas.flash = Some((e, -1.0));
            }
        }
        None => {
            // Released on empty canvas: Create Node, connected to the dragged port.
            let on_port = ports.iter().any(|(_, p, _)| p.distance(at) < 14.0);
            if !on_port {
                open_search_at(s, at, Some(from));
            }
        }
    }
}

/// Inline value fields for an unconnected input.
#[allow(clippy::too_many_arguments)]
fn inline_value(
    ui: &mut Ui,
    s: &mut EditorState,
    node: &Node,
    port: &Port,
    ty: ValueType,
    pos: Pos2,
    z: f32,
    busy: bool,
) {
    let painter = ui.painter().clone();
    let h = 16.0 * z;
    if let Some(label) = port.default.label() {
        let galley = painter.layout_no_wrap(
            label.to_owned(),
            FontId::proportional(10.5 * z),
            theme::TEXT_DIM,
        );
        let r = Rect::from_min_size(
            pos2(
                pos.x - 14.0 * z - galley.size().x - 12.0 * z,
                pos.y - h * 0.5,
            ),
            vec2(galley.size().x + 12.0 * z, h),
        );
        painter.rect_filled(r, 4.0 * z, Color32::from_rgb(46, 46, 49));
        painter.galley(
            r.min + vec2(6.0 * z, (h - galley.size().y) * 0.5),
            galley,
            theme::TEXT_DIM,
        );
        painter.line_segment(
            [pos2(r.right(), pos.y), pos - vec2(PORT_R * z, 0.0)],
            Stroke::new(1.0, Color32::from_white_alpha(30)),
        );
        return;
    }
    let PortDefault::Value(_) = port.default else {
        return;
    };
    let n = ty.dims().max(1);
    let field_w = 38.0 * z;
    let total = n as f32 * (field_w + 2.0 * z) + 6.0 * z;
    let r = Rect::from_min_size(
        pos2(pos.x - 14.0 * z - total, pos.y - h * 0.5 - 1.0),
        vec2(total, h + 2.0),
    );
    painter.rect_filled(r, 4.0 * z, Color32::from_rgb(46, 46, 49));
    painter.line_segment(
        [pos2(r.right(), pos.y), pos - vec2(PORT_R * z, 0.0)],
        Stroke::new(1.0, Color32::from_white_alpha(30)),
    );
    let mut v = node.value(port);
    let mut changed = false;
    ui.scope_builder(
        UiBuilder::new().max_rect(r.shrink2(vec2(3.0 * z, 1.0))),
        |ui| {
            let st = ui.style_mut();
            st.override_font_id = Some(FontId::proportional(10.5 * z));
            st.spacing.interact_size.y = h;
            st.spacing.item_spacing = vec2(2.0 * z, 0.0);
            st.spacing.button_padding = vec2(2.0 * z, 0.0);
            st.visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(40, 40, 43);
            // Fixed slots so the fields never spill onto the node.
            for (k, c) in v.iter_mut().take(n).enumerate() {
                let slot = Rect::from_min_size(
                    pos2(
                        r.left() + 3.0 * z + k as f32 * (field_w + 2.0 * z),
                        pos.y - h * 0.5,
                    ),
                    vec2(field_w, h),
                );
                changed |= ui
                    .put(slot, egui::DragValue::new(c).speed(0.01).max_decimals(2))
                    .changed();
            }
        },
    );
    if changed {
        let (id, name) = (node.id, port.name);
        if n == 1 {
            v = [v[0]; 4];
        }
        s.shader_graph.edit("Change Value", busy, |g| {
            if let Some(n) = g.node_mut(id) {
                n.values.insert(name.to_owned(), v);
            }
        });
    }
}

/// Settings inside a node (colour, space, texture type).
fn params_ui(ui: &mut Ui, s: &mut EditorState, node: &Node, r: Rect, z: f32) {
    let mut kind = node.kind.clone();
    ui.scope_builder(UiBuilder::new().max_rect(r), |ui| {
        ui.style_mut().override_font_id = Some(FontId::proportional(11.0 * z));
        ui.horizontal_centered(|ui| match &mut kind {
            NodeKind::Color { value } => {
                let mut c = value.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
                if ui
                    .add_sized([r.width(), r.height()], |ui: &mut Ui| {
                        ui.color_edit_button_srgba_unmultiplied(&mut c)
                    })
                    .changed()
                {
                    *value = c.map(|v| v as f32 / 255.0);
                }
            }
            NodeKind::Position { space }
            | NodeKind::Normal { space }
            | NodeKind::ViewDirection { space }
            | NodeKind::Triplanar { space } => {
                ui.label("Space");
                egui::ComboBox::from_id_salt(("sg_space", node.id))
                    .selected_text(match space {
                        Space::Object => "Object",
                        Space::World => "World",
                    })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(space, Space::World, "World");
                        ui.selectable_value(space, Space::Object, "Object");
                    });
            }
            NodeKind::SampleTexture2D { normal_map } => {
                ui.label("Type");
                egui::ComboBox::from_id_salt(("sg_textype", node.id))
                    .selected_text(if *normal_map { "Normal" } else { "Default" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(normal_map, false, "Default");
                        ui.selectable_value(normal_map, true, "Normal");
                    });
            }
            _ => {}
        });
    });
    if kind != node.kind {
        let id = node.id;
        let busy = interacting(ui.ctx());
        s.shader_graph.edit("Change Node Setting", busy, |g| {
            if let Some(n) = g.node_mut(id) {
                n.kind = kind;
            }
        });
    }
}

/// A soft checkerboard behind previews (shows transparency).
fn checker(painter: &egui::Painter, r: Rect, z: f32) {
    painter.rect_filled(r, 4.0 * z, Color32::from_rgb(44, 44, 46));
    let cell = 10.0 * z;
    if cell < 3.0 {
        return;
    }
    let (nx, ny) = (
        (r.width() / cell).ceil() as i32,
        (r.height() / cell).ceil() as i32,
    );
    for y in 0..ny {
        for x in 0..nx {
            if (x + y) % 2 == 0 {
                continue;
            }
            let c = Rect::from_min_size(
                r.min + vec2(x as f32 * cell, y as f32 * cell),
                vec2(cell, cell),
            )
            .intersect(r);
            painter.rect_filled(c, 0.0, Color32::from_rgb(52, 52, 55));
        }
    }
}

fn node_menu(ui: &mut Ui, w: &mut ShaderGraphWindow, id: u32) {
    let is_master = id == MASTER;
    if ui
        .add_enabled(!is_master, egui::Button::new("Copy"))
        .clicked()
    {
        if let Some(text) = w.copy_selected() {
            ui.ctx().copy_text(text);
        }
        ui.close();
    }
    if ui
        .add_enabled(!is_master, egui::Button::new("Cut"))
        .clicked()
    {
        if let Some(text) = w.cut_selected() {
            ui.ctx().copy_text(text);
        }
        ui.close();
    }
    if ui
        .add_enabled(!is_master, egui::Button::new("Duplicate"))
        .clicked()
    {
        w.duplicate_selected();
        ui.close();
    }
    if ui
        .add_enabled(!is_master, egui::Button::new("Group Selection"))
        .on_hover_text("Ctrl+G")
        .clicked()
    {
        w.group_selected();
        ui.close();
    }
    let grouped = w.graph.groups.iter().any(|g| g.nodes.contains(&id));
    if grouped && ui.button("Remove From Group").clicked() {
        let ids: Vec<u32> = w.selected.iter().copied().collect();
        w.edit("Remove From Group", false, |g| g.ungroup(&ids));
        ui.close();
    }
    if ui.button("Disconnect All").clicked() {
        let ids: Vec<u32> = w.selected.iter().copied().collect();
        w.edit("Disconnect All", false, |g| {
            g.connections
                .retain(|c| !ids.contains(&c.from) && !ids.contains(&c.to))
        });
        ui.close();
    }
    if !is_master && ui.button("Toggle Preview").clicked() {
        w.edit("Toggle Preview", false, |g| {
            if let Some(n) = g.node_mut(id) {
                n.hide_preview = !n.hide_preview;
            }
        });
        ui.close();
    }
    ui.separator();
    if ui
        .add_enabled(!is_master, egui::Button::new("Delete"))
        .clicked()
    {
        w.delete_selected();
        ui.close();
    }
}

fn canvas_menu(ui: &mut Ui, s: &mut EditorState, at: Option<Pos2>) {
    if ui.button("Create Node").clicked() {
        if let Some(p) = at {
            open_search_at(s, p, None);
        }
        ui.close();
    }
    let can_paste = s
        .shader_graph
        .clipboard
        .as_ref()
        .is_some_and(|c| !c.is_empty());
    if ui
        .add_enabled(can_paste, egui::Button::new("Paste"))
        .clicked()
    {
        let w = &mut s.shader_graph;
        let g = at.zip(w.canvas.view).map(|(p, v)| v.graph(p));
        w.paste(None, g.map(|g| [g.x, g.y]));
        ui.close();
    }
    let any = !s.shader_graph.selected.is_empty();
    if ui
        .add_enabled(any, egui::Button::new("Group Selection"))
        .clicked()
    {
        s.shader_graph.group_selected();
        ui.close();
    }
    if ui.button("Create Sticky Note").clicked() {
        let w = &mut s.shader_graph;
        let g = at
            .zip(w.canvas.view)
            .map_or(pos2(0.0, 0.0), |(p, v)| v.graph(p));
        w.edit("Create Sticky Note", false, |gr| {
            gr.notes.push(StickyNote {
                position: [g.x, g.y],
                size: [200.0, 110.0],
                text: "Note".into(),
            })
        });
        ui.close();
    }
    ui.separator();
    if ui.button("Frame All").clicked() {
        s.shader_graph.canvas.frame_all = true;
        ui.close();
    }
    let any = !s.shader_graph.selected.is_empty();
    if ui
        .add_enabled(any, egui::Button::new("Delete Selection"))
        .clicked()
    {
        s.shader_graph.delete_selected();
        ui.close();
    }
}

/// Screen rect of a group: around its nodes, with a title bar on top.
fn group_rect(
    g: &ShaderGraph,
    group: &misty_engine::shader_graph::Group,
    view: View,
) -> Option<Rect> {
    let mut bounds = Rect::NOTHING;
    for id in &group.nodes {
        if let Some(n) = g.node(*id) {
            let l = layout(g, n);
            bounds = bounds.union(node_rect(view, n, &l));
        }
    }
    bounds
        .is_positive()
        .then(|| bounds.expand2(vec2(14.0, 14.0) * view.zoom))
        .map(|r| Rect::from_min_max(r.min - vec2(0.0, 26.0 * view.zoom), r.max))
}

/// Groups: tinted frames behind their nodes. Drag the title bar to move the group,
/// double-click it to rename; right-click for Rename and Ungroup.
fn groups(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, area: Rect) {
    let z = view.zoom;
    let busy = interacting(ui.ctx());
    for i in 0..s.shader_graph.graph.groups.len() {
        let group = s.shader_graph.graph.groups[i].clone();
        let Some(r) = group_rect(&s.shader_graph.graph, &group, view) else {
            continue;
        };
        if !r.intersects(area) {
            continue;
        }
        painter.rect_filled(
            r,
            8.0 * z,
            Color32::from_rgba_unmultiplied(120, 140, 170, 28),
        );
        painter.rect_stroke(
            r,
            8.0 * z,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(150, 170, 200, 70)),
            StrokeKind::Inside,
        );
        let bar = Rect::from_min_size(r.min, vec2(r.width(), 26.0 * z));
        let resp = ui.interact(bar, ui.id().with(("sg_group", i)), Sense::click_and_drag());
        resp.widget_info(|| {
            egui::WidgetInfo::labeled(
                egui::WidgetType::Button,
                true,
                format!("Group {}", group.title),
            )
        });
        let w = &mut s.shader_graph;
        let renaming = w.canvas.renaming.as_ref().is_some_and(|(g, _)| *g == i);
        if renaming {
            let mut commit = false;
            ui.scope_builder(
                UiBuilder::new().max_rect(bar.shrink2(vec2(8.0 * z, 3.0 * z))),
                |ui| {
                    if let Some((_, text)) = &mut w.canvas.renaming {
                        let edit =
                            ui.add(egui::TextEdit::singleline(text).id_salt(("sg_group_name", i)));
                        edit.request_focus();
                        commit = edit.lost_focus();
                    }
                },
            );
            if commit && let Some((_, text)) = w.canvas.renaming.take() {
                w.edit("Rename Group", false, |g| {
                    if let Some(gr) = g.groups.get_mut(i) {
                        gr.title = text;
                    }
                });
            }
        } else {
            painter.text(
                bar.left_center() + vec2(10.0 * z, 0.0),
                Align2::LEFT_CENTER,
                &group.title,
                FontId::proportional(14.0 * z),
                theme::TEXT.gamma_multiply(0.9),
            );
        }
        if resp.double_clicked() {
            w.canvas.renaming = Some((i, group.title.clone()));
        }
        if resp.clicked() {
            w.selected = group.nodes.iter().copied().collect();
        }
        if resp.drag_started() {
            w.canvas.group_drag = Some(i);
        }
        if w.canvas.group_drag == Some(i) && resp.dragged() {
            let d = resp.drag_delta() / z;
            let ids = group.nodes.clone();
            w.edit("Move Group", busy, |g| {
                for n in &mut g.nodes {
                    if ids.contains(&n.id) {
                        n.position[0] += d.x;
                        n.position[1] += d.y;
                    }
                }
            });
        }
        if resp.drag_stopped() {
            w.canvas.group_drag = None;
        }
        resp.context_menu(|ui| {
            if ui.button("Rename").clicked() {
                s.shader_graph.canvas.renaming = Some((i, group.title.clone()));
                ui.close();
            }
            if ui.button("Ungroup").clicked() {
                s.shader_graph.edit("Ungroup", false, |g| {
                    if i < g.groups.len() {
                        g.groups.remove(i);
                    }
                });
                ui.close();
            }
        });
    }
}

/// Sticky notes: soft yellow cards with editable text, dragged by their top strip.

fn notes(ui: &mut Ui, s: &mut EditorState, painter: &egui::Painter, view: View, area: Rect) {
    let z = view.zoom;
    let count = s.shader_graph.graph.notes.len();
    let busy = interacting(ui.ctx());
    for i in 0..count {
        let note = s.shader_graph.graph.notes[i].clone();
        let r = Rect::from_min_size(
            view.screen(pos2(note.position[0], note.position[1])),
            vec2(note.size[0], note.size[1]) * z,
        );
        if !r.intersects(area) {
            continue;
        }
        painter.rect_filled(
            r.translate(vec2(0.0, 3.0 * z)),
            6.0 * z,
            Color32::from_black_alpha(60),
        );
        painter.rect_filled(r, 6.0 * z, Color32::from_rgb(228, 208, 120));
        let strip = Rect::from_min_size(r.min, vec2(r.width(), 14.0 * z));
        let resp = ui.interact(strip, ui.id().with(("sg_note", i)), Sense::click_and_drag());
        let w = &mut s.shader_graph;
        if resp.drag_started() {
            w.canvas.note_drag = Some(i);
        }
        if w.canvas.note_drag == Some(i) && resp.dragged() {
            let d = resp.drag_delta() / z;
            w.edit("Move Sticky Note", busy, |g| {
                if let Some(n) = g.notes.get_mut(i) {
                    n.position[0] += d.x;
                    n.position[1] += d.y;
                }
            });
        }
        if resp.drag_stopped() {
            w.canvas.note_drag = None;
        }
        resp.context_menu(|ui| {
            if ui.button("Delete Sticky Note").clicked() {
                s.shader_graph.edit("Delete Sticky Note", false, |g| {
                    if i < g.notes.len() {
                        g.notes.remove(i);
                    }
                });
                ui.close();
            }
        });
        if z < 0.5 {
            continue;
        }
        let mut text = note.text.clone();
        let body = Rect::from_min_max(
            strip.left_bottom() + vec2(6.0 * z, 0.0),
            r.max - vec2(6.0 * z, 6.0 * z),
        );
        ui.scope_builder(UiBuilder::new().max_rect(body), |ui| {
            let st = ui.style_mut();
            st.override_font_id = Some(FontId::proportional(12.0 * z));
            st.visuals.override_text_color = Some(Color32::from_rgb(50, 44, 20));
            st.visuals.extreme_bg_color = Color32::TRANSPARENT;
            st.visuals.widgets.inactive.bg_stroke = Stroke::NONE;
            st.visuals.widgets.hovered.bg_stroke = Stroke::NONE;
            st.visuals.selection.stroke = Stroke::NONE;
            ui.add_sized(
                body.size(),
                egui::TextEdit::multiline(&mut text)
                    .frame(egui::Frame::NONE)
                    .id_salt(("sg_note_text", i)),
            );
        });
        if text != note.text {
            s.shader_graph.edit("Edit Sticky Note", busy, |g| {
                if let Some(n) = g.notes.get_mut(i) {
                    n.text = text;
                }
            });
        }
    }
}
