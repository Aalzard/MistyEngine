//! The Effect Graph canvas: contexts as vertical stacks of blocks joined by flow arrows,
//! operator nodes, links from operator outputs into block or operator inputs, inline values,
//! pan (middle mouse or Alt+drag) and zoom (wheel, both shared with the Shader Graph canvas),
//! selection (click, Ctrl/Shift+click, rubber band), moving contexts and operators, reordering
//! blocks by dragging their title, and the right-click menus.

use egui::{
    Align2, Color32, CornerRadius, FontId, Pos2, Rect, Response, Sense, Stroke, StrokeKind, Ui,
    Vec2, pos2, vec2,
};
use misty_engine::vfx::graph::{
    BlendMode, ContextKind, EventSource, FlipbookMode, Item, OutputKind, SimulationSpace, SortMode,
};
use misty_engine::vfx::types::{Value, ValueType};
use misty_engine::vfx::{
    Attribute, AttributeRead, BlockKind, Composition, ContextType, OperatorKind, OrientMode,
    RandomMode, ShapeMode,
};

use super::fields::{self, small_scope};
use super::layout::{self, ContextLayout, OperatorLayout};
use super::{EffectGraphWindow, interacting, search};
use crate::panels::shader_graph::canvas::{View, draw_grid, wire};
use crate::state::EditorState;
use crate::theme;

const PORT_R: f32 = 4.5;
const CANVAS_BG: Color32 = Color32::from_rgb(34, 35, 38);
const CTX_BG: Color32 = Color32::from_rgb(52, 53, 57);
const BLOCK_BG: Color32 = Color32::from_rgb(66, 67, 72);
const NODE_BG: Color32 = Color32::from_rgb(58, 58, 61);
const HEADER_BG: Color32 = Color32::from_rgb(66, 66, 70);
const SELECT: Color32 = Color32::from_rgb(88, 156, 236);

/// A Blackboard property dragged onto the canvas.
#[derive(Clone, Debug)]
pub struct PropertyPayload(pub String);

/// One end of a link being dragged.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PortEnd {
    pub node: u32,
    pub port: String,
    pub output: bool,
}

#[derive(Default)]
pub(crate) struct CanvasState {
    pub view: Option<View>,
    /// A link being dragged from this port.
    pub connecting: Option<PortEnd>,
    band: Option<Pos2>,
    moving: bool,
    pub frame_all: bool,
    /// A short message after a refused link (text, until time).
    pub flash: Option<(String, f64)>,
    menu_at: Option<Pos2>,
    /// The gradient / curve editor popup: (owner id, port name, screen position).
    pub ramp: Option<(u32, String, Pos2)>,
    /// A block being dragged to a new place.
    block_drag: Option<u32>,
}

/// Port colour by type.
pub fn type_color(t: ValueType) -> Color32 {
    match t {
        ValueType::Float => Color32::from_rgb(132, 196, 236),
        ValueType::Vec3 => Color32::from_rgb(236, 214, 120),
        ValueType::Color => Color32::from_rgb(226, 150, 200),
        ValueType::Gradient => Color32::from_rgb(150, 214, 140),
        ValueType::Curve => Color32::from_rgb(120, 200, 190),
    }
}

/// Accent per context type.
pub fn context_color(t: ContextType) -> Color32 {
    match t {
        ContextType::Spawn => Color32::from_rgb(110, 190, 120),
        ContextType::GpuEvent => Color32::from_rgb(226, 112, 96),
        ContextType::Initialize => Color32::from_rgb(222, 170, 80),
        ContextType::Update => Color32::from_rgb(96, 156, 226),
        ContextType::Output => Color32::from_rgb(186, 126, 222),
    }
}

fn op_color(kind: &OperatorKind) -> Color32 {
    match kind.category() {
        "Properties" => Color32::from_rgb(96, 150, 210),
        "Inputs" | "Constants" => Color32::from_rgb(150, 150, 160),
        "Random / Noise" => Color32::from_rgb(180, 130, 210),
        "Sampling" => Color32::from_rgb(150, 200, 120),
        _ => Color32::from_rgb(120, 170, 120),
    }
}

/// Title of a context (Output contexts name what they draw; GPU Event systems say which
/// system they are).
pub fn context_title(kind: &ContextKind) -> String {
    match kind {
        ContextKind::Output(o) => {
            format!("Output Particle {} ({})", o.kind.label(), o.blend.label())
        }
        ContextKind::GpuEvent(e) => format!("GPU Event ({})", e.source.label()),
        k => k.ty().label().to_owned(),
    }
}

/// [`context_title`] with the system number for GPU Event systems.
pub fn context_title_in(c: &misty_engine::vfx::graph::Context) -> String {
    let t = context_title(&c.kind);
    if c.system == 0 {
        t
    } else {
        format!("{t} · System {}", c.system)
    }
}

fn scr(view: View, r: Rect) -> Rect {
    Rect::from_min_max(view.screen(r.min), view.screen(r.max))
}

/// Open the Create Node search at a screen point.
pub fn open_search_at(s: &mut EditorState, screen: Pos2) {
    let w = &mut s.effect_graph;
    let graph = w.canvas.view.map_or(pos2(0.0, 0.0), |v| v.graph(screen));
    // Over a context: add a block to it instead.
    let over = w
        .graph
        .contexts
        .iter()
        .map(layout::context)
        .find(|c| c.rect.contains(graph))
        .map(|c| c.id);
    w.search = Some(match over {
        Some(ctx) => search::Search::blocks(screen, ctx),
        None => search::Search::nodes(screen, graph),
    });
}

fn frame_all(w: &mut EffectGraphWindow, area: Rect) {
    let bounds = layout::bounds(&w.graph);
    if !bounds.is_positive() {
        return;
    }
    let left = if w.show_blackboard { 260.0 } else { 0.0 };
    let free = Rect::from_min_max(
        pos2(area.left() + left, area.top()),
        pos2(area.right().max(area.left() + left + 100.0), area.bottom()),
    );
    let margin = 30.0;
    let fit = |r: Rect| {
        ((r.width() - 2.0 * margin) / bounds.width())
            .min((r.height() - 2.0 * margin) / bounds.height())
    };
    // Big graphs stay readable: at most zoomed out to 0.55, showing the top (Spawn) first.
    let zoom = fit(free).clamp(0.55, 1.0);
    w.zoom = zoom;
    let target = if fit(free) >= 0.55 {
        bounds.center()
    } else {
        let visible = (free.height() - 2.0 * margin) / zoom;
        pos2(bounds.center().x, bounds.top() + visible * 0.5)
    };
    w.pan = (free.center() - area.center()) / zoom - target.to_vec2();
}

pub fn show(ui: &mut Ui, s: &mut EditorState, area: Rect) {
    let resp = ui.interact(area, ui.id().with("vfx_canvas"), Sense::click_and_drag());
    let painter = ui.painter_at(area);
    painter.rect_filled(area, 0.0, CANVAS_BG);
    if s.effect_graph.canvas.frame_all {
        s.effect_graph.canvas.frame_all = false;
        frame_all(&mut s.effect_graph, area);
    }
    navigate(ui, &mut s.effect_graph, area, &resp);
    let view = View {
        centre: area.center(),
        pan: s.effect_graph.pan,
        zoom: s.effect_graph.zoom(),
    };
    s.effect_graph.canvas.view = Some(view);
    draw_grid(&painter, area, view);
    flow_arrows(&painter, &s.effect_graph, view);

    let busy = interacting(ui.ctx());
    let mut clicked = false;
    let ctx_ids: Vec<u32> = s.effect_graph.graph.contexts.iter().map(|c| c.id).collect();
    for id in ctx_ids {
        let Some(c) = s.effect_graph.graph.context(id) else {
            continue;
        };
        let l = layout::context(c);
        if !scr(view, l.rect).expand(40.0).intersects(area) {
            continue;
        }
        clicked |= context_ui(ui, s, &painter, view, &l, busy);
    }
    let op_ids: Vec<u32> = s
        .effect_graph
        .graph
        .operators
        .iter()
        .map(|o| o.id)
        .collect();
    for id in op_ids {
        let Some(o) = s.effect_graph.graph.operator(id) else {
            continue;
        };
        let l = layout::operator(&s.effect_graph.graph, o);
        if !scr(view, l.rect).expand(200.0 * view.zoom).intersects(area) {
            continue;
        }
        clicked |= operator_ui(ui, s, &painter, view, &l, busy);
    }
    links(&painter, &s.effect_graph, view);

    // A link being dragged follows the mouse.
    let w = &mut s.effect_graph;
    if let Some(from) = w.canvas.connecting.clone()
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
    {
        let outs = layout::output_ports(&w.graph);
        if let Some((_, _, at, ty)) = outs
            .iter()
            .find(|(n, port, _, _)| *n == from.node && *port == from.port)
        {
            wire(&painter, view.screen(*at), p, type_color(*ty), view.zoom);
        }
        if ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            w.canvas.connecting = None;
        }
    }
    // A block being dragged: a line where it would go.
    if let Some(b) = w.canvas.block_drag
        && let Some(p) = ui.input(|i| i.pointer.hover_pos())
        && let Some((ctx, index)) = block_drop_target(w, view.graph(p), b)
        && let Some(c) = w.graph.context(ctx)
    {
        let l = layout::context(c);
        let y = if index < l.blocks.len() {
            l.blocks[index].rect.top() - layout::GAP * 0.5
        } else {
            l.foot.top() - layout::GAP * 0.5
        };
        let a = view.screen(pos2(l.rect.left() + 6.0, y));
        let bb = view.screen(pos2(l.rect.right() - 6.0, y));
        painter.line_segment([a, bb], Stroke::new(2.5, SELECT));
    }

    // Rubber band on empty canvas; click deselects.
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
            if !ui.input(|i| i.modifiers.shift || i.modifiers.command) {
                w.selected.clear();
            }
            for c in &w.graph.contexts {
                if scr(view, layout::context(c).rect).intersects(band) {
                    w.selected.insert(c.id);
                }
            }
            for o in &w.graph.operators {
                if scr(view, layout::operator(&w.graph, o).rect).intersects(band) {
                    w.selected.insert(o.id);
                }
            }
            w.canvas.band = None;
        }
    }
    if !ui.input(|i| i.pointer.primary_down()) {
        w.canvas.band = None;
    }
    if resp.clicked() && !clicked {
        w.selected.clear();
        w.search = None;
        w.blackboard.selected = None;
    }
    if resp.secondary_clicked() {
        w.canvas.connecting = None;
        w.canvas.menu_at = resp.interact_pointer_pos();
    }
    let menu_at = s.effect_graph.canvas.menu_at;
    resp.context_menu(|ui| canvas_menu(ui, s, menu_at));

    if let Some(p) = resp.dnd_release_payload::<PropertyPayload>() {
        let at = resp
            .interact_pointer_pos()
            .map_or(pos2(0.0, 0.0), |q| view.graph(q));
        add_property_node(&mut s.effect_graph, &p.0, at);
    }
    ramp_popup(ui, s);
    flash(ui, &painter, &mut s.effect_graph, area);
}

fn flash(ui: &Ui, painter: &egui::Painter, w: &mut EffectGraphWindow, area: Rect) {
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

/// Add a Property operator for a Blackboard property at a graph point.
pub fn add_property_node(w: &mut EffectGraphWindow, name: &str, at: Pos2) {
    let Some(p) = w.graph.property(name) else {
        return;
    };
    if p.value.value_type().is_none() {
        w.canvas.flash = Some((
            "Texture properties are picked in an Output context's Texture".into(),
            -1.0,
        ));
        return;
    }
    let mut id = 0;
    let name = name.to_owned();
    w.edit("Add Property Node", false, |g| {
        id = g.add_operator(OperatorKind::Property { name }, [at.x, at.y]);
    });
    w.selected = [id].into();
}

fn navigate(ui: &Ui, w: &mut EffectGraphWindow, area: Rect, resp: &Response) {
    if let Some(p) = resp.hover_pos()
        && w.search.is_none()
        && w.canvas.ramp.is_none()
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

/// Thick arrows from each context to the next in the particle flow.
fn flow_arrows(painter: &egui::Painter, w: &EffectGraphWindow, view: View) {
    let z = view.zoom;
    for (a, b) in layout::flow(&w.graph) {
        let (Some(ca), Some(cb)) = (w.graph.context(a), w.graph.context(b)) else {
            continue;
        };
        let (la, lb) = (layout::context(ca), layout::context(cb));
        let start = view.screen(pos2(la.rect.center().x, la.rect.bottom()));
        let end = view.screen(pos2(lb.rect.center().x, lb.rect.top()));
        let color = context_color(cb.kind.ty()).gamma_multiply(0.7);
        let dy = ((end.y - start.y).abs() * 0.5).max(30.0 * z);
        let shape = egui::epaint::CubicBezierShape::from_points_stroke(
            [start, start + vec2(0.0, dy), end - vec2(0.0, dy), end],
            false,
            Color32::TRANSPARENT,
            Stroke::new((5.0 * z).max(2.0), color),
        );
        painter.add(shape);
        let s = 7.0 * z;
        painter.add(egui::Shape::convex_polygon(
            vec![end, end + vec2(-s, -s * 1.2), end + vec2(s, -s * 1.2)],
            color,
            Stroke::NONE,
        ));
    }
}

fn links(painter: &egui::Painter, w: &EffectGraphWindow, view: View) {
    let ins = layout::input_ports(&w.graph);
    let outs = layout::output_ports(&w.graph);
    for l in &w.graph.links {
        let a = outs
            .iter()
            .find(|(n, p, _, _)| *n == l.from && *p == l.output);
        let b = ins.iter().find(|(n, p, _, _)| *n == l.to && *p == l.input);
        let (Some((_, _, a, ty)), Some((_, _, b, _))) = (a, b) else {
            continue;
        };
        let selected = w.selected.contains(&l.from) || w.selected.contains(&l.to);
        let c = if selected {
            type_color(*ty)
        } else {
            type_color(*ty).gamma_multiply(0.75)
        };
        wire(painter, view.screen(*a), view.screen(*b), c, view.zoom);
    }
}

fn error_badge(ui: &Ui, painter: &egui::Painter, at: Pos2, z: f32, id: u32, message: &str) {
    painter.circle_filled(at, 8.0 * z, super::ERROR);
    painter.text(
        at,
        Align2::CENTER_CENTER,
        "!",
        FontId::proportional(12.0 * z),
        Color32::WHITE,
    );
    ui.interact(
        Rect::from_center_size(at, vec2(16.0, 16.0) * z),
        ui.id().with(("vfx_err", id)),
        Sense::hover(),
    )
    .on_hover_text(message);
}

/// One context with its settings and blocks. Returns true if something in it was clicked.
fn context_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    l: &ContextLayout,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let id = l.id;
    let Some(ctx) = s.effect_graph.graph.context(id).cloned() else {
        return false;
    };
    let ty = ctx.kind.ty();
    let rect = scr(view, l.rect);
    let round = CornerRadius::same((10.0 * z).round().clamp(2.0, 14.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(0.0, 5.0 * z)),
        round,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(rect, round, CTX_BG);
    let header = scr(view, l.header);
    let accent = context_color(ty);
    painter.rect_filled(
        header,
        CornerRadius {
            nw: round.nw,
            ne: round.ne,
            sw: 0,
            se: 0,
        },
        accent.gamma_multiply(0.35),
    );
    painter.rect_filled(
        Rect::from_min_size(
            header.left_bottom() - vec2(0.0, 2.0 * z),
            vec2(header.width(), 2.0 * z),
        ),
        0.0,
        accent,
    );
    let title = context_title_in(&ctx);
    painter.text(
        header.left_center() + vec2(12.0 * z, 0.0),
        Align2::LEFT_CENTER,
        &title,
        FontId::proportional(14.0 * z),
        theme::TEXT,
    );
    let w = &s.effect_graph;
    let selected = w.selected.contains(&id);
    if let Some(e) = w.error_of(id) {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(2.0, super::ERROR),
            StrokeKind::Outside,
        );
        let msg = e.message.clone();
        error_badge(
            ui,
            painter,
            header.right_center() - vec2(16.0 * z, 0.0),
            z,
            id,
            &msg,
        );
    } else if selected {
        painter.rect_stroke(rect, round, Stroke::new(2.0, SELECT), StrokeKind::Outside);
    }

    // Header: select / move / menu.
    let hresp = ui.interact(
        header,
        ui.id().with(("vfx_ctx", id)),
        Sense::click_and_drag(),
    );
    hresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
    let mut clicked = select_and_move(ui, &mut s.effect_graph, id, &hresp, z, busy);
    hresp.context_menu(|ui| {
        if ui.button("Add Block").clicked() {
            let at = ui
                .ctx()
                .input(|i| i.pointer.hover_pos())
                .unwrap_or(header.center());
            s.effect_graph.search = Some(search::Search::blocks(at, id));
            ui.close();
        }
        ui.separator();
        if ui.button("Delete Context").clicked() {
            s.effect_graph.selected = [id].into();
            s.effect_graph.delete_selected();
            ui.close();
        }
    });

    // Settings rows.
    if z > 0.45 && !l.settings.is_empty() {
        context_settings(ui, s, &ctx.kind, id, &l.settings, view);
    }

    // Blocks.
    for bl in &l.blocks {
        clicked |= block_ui(ui, s, painter, view, ty, bl, busy);
    }

    // Add Block footer (GPU Event contexts take no blocks).
    if ty == ContextType::GpuEvent {
        return clicked;
    }
    let foot = scr(view, l.foot);
    let fresp = ui.interact(foot, ui.id().with(("vfx_foot", id)), Sense::click());
    let label = format!("Add Block to {}", ty.label());
    fresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let hover = fresp.hovered();
    painter.rect_stroke(
        foot,
        6.0 * z,
        Stroke::new(1.0, Color32::from_white_alpha(if hover { 60 } else { 25 })),
        StrokeKind::Inside,
    );
    painter.text(
        foot.center(),
        Align2::CENTER_CENTER,
        "+ Add Block",
        FontId::proportional(12.0 * z),
        if hover { theme::TEXT } else { theme::TEXT_DIM },
    );
    if fresp.clicked() {
        s.effect_graph.search = Some(search::Search::blocks(foot.left_bottom(), id));
        clicked = true;
    }
    clicked
}

/// Click selects (Ctrl/Shift toggles), drag moves the selection.
fn select_and_move(
    ui: &Ui,
    w: &mut EffectGraphWindow,
    id: u32,
    resp: &Response,
    z: f32,
    busy: bool,
) -> bool {
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
            w.edit("Move", busy, |g| {
                for c in &mut g.contexts {
                    if ids.contains(&c.id) {
                        c.position[0] += d.x;
                        c.position[1] += d.y;
                    }
                }
                for o in &mut g.operators {
                    if ids.contains(&o.id) {
                        o.position[0] += d.x;
                        o.position[1] += d.y;
                    }
                }
            });
        }
    }
    if resp.drag_stopped() {
        w.canvas.moving = false;
    }
    if resp.clicked() {
        if add {
            if !w.selected.remove(&id) {
                w.selected.insert(id);
            }
        } else {
            w.selected = [id].into();
        }
        w.blackboard.selected = None;
        return true;
    }
    if resp.secondary_clicked() && !w.selected.contains(&id) {
        w.selected = [id].into();
    }
    false
}

fn row_label(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).color(theme::TEXT_DIM));
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    salt: impl std::hash::Hash + std::fmt::Debug,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> String,
) {
    egui::ComboBox::from_id_salt(salt)
        .width((ui.available_width() * 0.4).clamp(30.0, 110.0))
        .selected_text(label(*value))
        .show_ui(ui, |ui| {
            for v in all {
                ui.selectable_value(value, *v, label(*v));
            }
        });
}

fn vec3_fields(ui: &mut Ui, v: &mut [f32; 3]) {
    for c in v.iter_mut() {
        ui.add(egui::DragValue::new(c).speed(0.05).max_decimals(2));
    }
}

/// Settings rows under a context header (capacity, bounds, space; update toggles; output).
fn context_settings(
    ui: &mut Ui,
    s: &mut EditorState,
    kind: &ContextKind,
    id: u32,
    rows: &[Rect],
    view: View,
) {
    let z = view.zoom;
    let mut k = kind.clone();
    let textures = s.assets.textures.clone();
    let tex_props: Vec<String> = s
        .effect_graph
        .graph
        .properties
        .iter()
        .filter(|p| matches!(p.value, misty_engine::vfx::PropertyValue::Texture(_)))
        .map(|p| p.name.clone())
        .collect();
    let row = |i: usize| scr(view, rows[i]);
    match &mut k {
        ContextKind::Spawn => {}
        ContextKind::GpuEvent(st) => {
            let systems = s.effect_graph.graph.systems();
            small_scope(ui, row(0), z, |ui| {
                ui.horizontal_centered(|ui| {
                    combo(
                        ui,
                        ("vfx_evsrc", id),
                        &mut st.source,
                        &EventSource::ALL,
                        |v| v.label().to_owned(),
                    );
                    row_label(ui, "Count");
                    ui.add(egui::DragValue::new(&mut st.count).range(1..=1024))
                        .on_hover_text("Particles spawned per event");
                    row_label(ui, "From");
                    let parents: Vec<u32> = systems
                        .iter()
                        .copied()
                        .filter(|p| *p < s.effect_graph.graph.context(id).map_or(0, |c| c.system))
                        .collect();
                    combo(ui, ("vfx_evparent", id), &mut st.parent, &parents, |p| {
                        if p == 0 {
                            "Main System".to_owned()
                        } else {
                            format!("System {p}")
                        }
                    });
                });
            });
        }
        ContextKind::Initialize(st) => {
            small_scope(ui, row(0), z, |ui| {
                ui.horizontal_centered(|ui| {
                    row_label(ui, "Capacity");
                    ui.add(
                        egui::DragValue::new(&mut st.capacity)
                            .speed(64.0)
                            .range(1..=misty_engine::vfx::graph::MAX_CAPACITY),
                    )
                    .on_hover_text("Most particles alive at once (GPU memory is sized for it)");
                });
            });
            small_scope(ui, row(1), z, |ui| {
                ui.horizontal_centered(|ui| {
                    row_label(ui, "Bounds Center");
                    vec3_fields(ui, &mut st.bounds_center);
                });
            });
            small_scope(ui, row(2), z, |ui| {
                ui.horizontal_centered(|ui| {
                    row_label(ui, "Bounds Size");
                    vec3_fields(ui, &mut st.bounds_size);
                });
            });
            small_scope(ui, row(3), z, |ui| {
                ui.horizontal_centered(|ui| {
                    row_label(ui, "Space");
                    combo(
                        ui,
                        ("vfx_space", id),
                        &mut st.space,
                        &[SimulationSpace::Local, SimulationSpace::World],
                        |v| format!("{v:?}"),
                    );
                });
            });
        }
        ContextKind::Update(st) => {
            small_scope(ui, row(0), z, |ui| {
                ui.horizontal_centered(|ui| {
                    ui.checkbox(&mut st.integrate, "Update Position");
                    ui.checkbox(&mut st.age, "Age");
                    ui.checkbox(&mut st.reap, "Reap");
                });
            });
        }
        ContextKind::Output(st) => {
            small_scope(ui, row(0), z, |ui| {
                ui.horizontal_centered(|ui| {
                    combo(ui, ("vfx_okind", id), &mut st.kind, &OutputKind::ALL, |v| {
                        v.label().to_owned()
                    });
                    combo(ui, ("vfx_blend", id), &mut st.blend, &BlendMode::ALL, |v| {
                        v.label().to_owned()
                    });
                    combo(ui, ("vfx_sort", id), &mut st.sort, &SortMode::ALL, |v| {
                        v.label().to_owned()
                    });
                });
            });
            small_scope(ui, row(1), z, |ui| {
                ui.horizontal_centered(|ui| {
                    if st.kind == OutputKind::Mesh {
                        row_label(ui, "Mesh");
                        combo(
                            ui,
                            ("vfx_mesh", id),
                            &mut st.mesh,
                            &misty_engine::scene::PrimitiveMesh::ALL,
                            |m| m.name().to_owned(),
                        );
                        return;
                    }
                    row_label(ui, "Texture");
                    if st.texture_property.is_none() {
                        crate::panels::inspector::slots::texture_slot(
                            ui,
                            &format!("vfx_tex_{id}"),
                            &mut st.texture,
                            &textures,
                        );
                    }
                    if !tex_props.is_empty() {
                        let text = st.texture_property.clone().unwrap_or("(asset)".into());
                        egui::ComboBox::from_id_salt(("vfx_texprop", id))
                            .selected_text(text)
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut st.texture_property, None, "(asset)");
                                for p in &tex_props {
                                    ui.selectable_value(
                                        &mut st.texture_property,
                                        Some(p.clone()),
                                        p,
                                    );
                                }
                            });
                    }
                });
            });
            small_scope(ui, row(2), z, |ui| {
                ui.horizontal_centered(|ui| {
                    if st.kind == OutputKind::Trail {
                        row_label(ui, "Trail Points");
                        ui.add(
                            egui::DragValue::new(&mut st.trail_points)
                                .range(2..=misty_engine::vfx::graph::MAX_TRAIL_POINTS),
                        )
                        .on_hover_text("Positions remembered per particle (one per step)");
                        return;
                    }
                    if st.kind != OutputKind::Quad {
                        return;
                    }
                    row_label(ui, "Flipbook");
                    ui.add(
                        egui::DragValue::new(&mut st.flipbook[0])
                            .range(1..=32)
                            .prefix("× "),
                    );
                    ui.add(
                        egui::DragValue::new(&mut st.flipbook[1])
                            .range(1..=32)
                            .prefix("× "),
                    );
                    combo(
                        ui,
                        ("vfx_flipmode", id),
                        &mut st.flipbook_mode,
                        &[FlipbookMode::OverLife, FlipbookMode::Fps],
                        |m| match m {
                            FlipbookMode::OverLife => "Over Life".into(),
                            FlipbookMode::Fps => "FPS".into(),
                        },
                    );
                    if st.flipbook_mode == FlipbookMode::Fps {
                        ui.add(egui::DragValue::new(&mut st.flipbook_fps).range(0.1..=120.0));
                    }
                });
            });
            small_scope(ui, row(3), z, |ui| {
                ui.horizontal_centered(|ui| {
                    if st.kind == OutputKind::Line {
                        row_label(ui, "Line Length");
                        ui.add(
                            egui::DragValue::new(&mut st.line_length)
                                .speed(0.005)
                                .range(0.0..=10.0),
                        );
                    } else if st.kind == OutputKind::Quad {
                        ui.checkbox(&mut st.soft_particles, "Soft Particles")
                            .on_hover_text("Fade where particles meet opaque geometry");
                        if st.soft_particles {
                            ui.add(
                                egui::DragValue::new(&mut st.soft_distance)
                                    .speed(0.01)
                                    .range(0.01..=10.0),
                            );
                        }
                    }
                });
            });
        }
    }
    if &k != kind {
        let busy = interacting(ui.ctx());
        s.effect_graph.edit("Context Settings", busy, |g| {
            if let Some(c) = g.context_mut(id) {
                c.kind = k;
            }
        });
    }
}

/// Where a dragged block would land: (context, index), for a graph point.
fn block_drop_target(w: &EffectGraphWindow, p: Pos2, block: u32) -> Option<(u32, usize)> {
    let kind = w.graph.block(block)?.kind.clone();
    for c in &w.graph.contexts {
        let l = layout::context(c);
        if !l.rect.expand(10.0).contains(p) || !kind.allowed_in(c.kind.ty()) {
            continue;
        }
        let index = l
            .blocks
            .iter()
            .position(|b| p.y < b.rect.center().y)
            .unwrap_or(l.blocks.len());
        return Some((c.id, index));
    }
    None
}

fn block_settings(ui: &mut Ui, s: &mut EditorState, id: u32, kind: &BlockKind, r: Rect, z: f32) {
    let mut k = kind.clone();
    small_scope(ui, r, z, |ui| {
        ui.horizontal_centered(|ui| match &mut k {
            BlockKind::SetAttribute {
                attribute,
                random,
                composition,
            } => {
                combo(ui, ("vfx_attr", id), attribute, &Attribute::ALL, |a| {
                    a.label().to_owned()
                });
                combo(ui, ("vfx_comp", id), composition, &Composition::ALL, |c| {
                    c.label().to_owned()
                });
                ui.checkbox(random, "Random");
            }
            BlockKind::PositionSphere { mode }
            | BlockKind::PositionBox { mode }
            | BlockKind::PositionCone { mode }
            | BlockKind::PositionCircle { mode } => {
                row_label(ui, "Mode");
                combo(
                    ui,
                    ("vfx_shape", id),
                    mode,
                    &[ShapeMode::Volume, ShapeMode::Surface],
                    |m| format!("{m:?}"),
                );
            }
            BlockKind::CollideSphere { inverted } => {
                ui.checkbox(inverted, "Keep Inside");
            }
            BlockKind::Orient { mode } => {
                row_label(ui, "Mode");
                combo(ui, ("vfx_orient", id), mode, &OrientMode::ALL, |m| {
                    m.label().to_owned()
                });
            }
            _ => {}
        });
    });
    if &k != kind {
        // Inputs that no longer exist lose their links (e.g. Random off drops A / B).
        let names: Vec<&str> = k.inputs().iter().map(|p| p.name).collect();
        let names: Vec<String> = names.into_iter().map(str::to_owned).collect();
        s.effect_graph.edit("Block Settings", false, |g| {
            if let Some(b) = g.block_mut(id) {
                b.kind = k;
                b.values.retain(|n, _| names.contains(n));
            }
            g.links.retain(|l| l.to != id || names.contains(&l.input));
        });
    }
}

/// One block: title, settings row and input rows. Returns true when clicked.
fn block_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    ctx_ty: ContextType,
    bl: &layout::BlockLayout,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let id = bl.id;
    let Some(block) = s.effect_graph.graph.block(id).cloned() else {
        return false;
    };
    let rect = scr(view, bl.rect);
    let round = CornerRadius::same((6.0 * z).round().clamp(1.0, 10.0) as u8);
    let dim = if block.enabled { 1.0 } else { 0.45 };
    painter.rect_filled(
        rect,
        round,
        BLOCK_BG.gamma_multiply(if block.enabled { 1.0 } else { 0.8 }),
    );
    let stripe = Rect::from_min_size(rect.min, vec2(3.0 * z, rect.height()));
    painter.rect_filled(
        stripe,
        round,
        context_color(ctx_ty).gamma_multiply(0.8 * dim),
    );
    let title_rect = scr(view, bl.title);
    let title = block.kind.title();
    painter.text(
        title_rect.left_center() + vec2(10.0 * z, 0.0),
        Align2::LEFT_CENTER,
        &title,
        FontId::proportional(12.5 * z),
        theme::TEXT.gamma_multiply(dim),
    );
    let w = &s.effect_graph;
    if let Some(e) = w.error_of(id) {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(2.0, super::ERROR),
            StrokeKind::Outside,
        );
        let msg = e.message.clone();
        error_badge(
            ui,
            painter,
            title_rect.right_center() - vec2(12.0 * z, 0.0),
            z,
            id,
            &msg,
        );
    } else if w.selected.contains(&id) {
        painter.rect_stroke(rect, round, Stroke::new(2.0, SELECT), StrokeKind::Outside);
    }

    // Title: select, drag to reorder, menu.
    let tresp = ui.interact(
        title_rect,
        ui.id().with(("vfx_block", id)),
        Sense::click_and_drag(),
    );
    tresp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
    let mut clicked = false;
    let w = &mut s.effect_graph;
    if tresp.clicked() {
        let add = ui.input(|i| i.modifiers.shift || i.modifiers.command);
        if add {
            if !w.selected.remove(&id) {
                w.selected.insert(id);
            }
        } else {
            w.selected = [id].into();
        }
        clicked = true;
    }
    if tresp.drag_started_by(egui::PointerButton::Primary) {
        w.canvas.block_drag = Some(id);
    }
    if tresp.drag_stopped() && w.canvas.block_drag == Some(id) {
        w.canvas.block_drag = None;
        if let Some(p) = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()))
            && let Some((ctx, index)) = block_drop_target(w, view.graph(p), id)
        {
            w.edit("Move Block", false, |g| {
                g.move_block(id, ctx, index);
            });
        }
    }
    tresp.context_menu(|ui| {
        let w = &mut s.effect_graph;
        let label = if block.enabled { "Disable" } else { "Enable" };
        if ui.button(label).clicked() {
            w.edit("Toggle Block", false, |g| {
                if let Some(b) = g.block_mut(id) {
                    b.enabled = !b.enabled;
                }
            });
            ui.close();
        }
        let place = w.graph.block_context(id).map(|c| {
            (
                c.id,
                c.blocks.iter().position(|b| b.id == id).unwrap_or(0),
                c.blocks.len(),
            )
        });
        if let Some((ctx, i, n)) = place {
            if ui
                .add_enabled(i > 0, egui::Button::new("Move Up"))
                .clicked()
            {
                w.edit("Move Block", false, |g| {
                    g.move_block(id, ctx, i - 1);
                });
                ui.close();
            }
            if ui
                .add_enabled(i + 1 < n, egui::Button::new("Move Down"))
                .clicked()
            {
                w.edit("Move Block", false, |g| {
                    g.move_block(id, ctx, i + 2);
                });
                ui.close();
            }
        }
        ui.separator();
        if ui.button("Delete Block").clicked() {
            w.selected = [id].into();
            w.delete_selected();
            ui.close();
        }
    });

    if let Some(r) = bl.settings
        && z > 0.45
    {
        block_settings(ui, s, id, &block.kind, scr(view, r), z);
    }
    for row in &bl.inputs {
        input_row(ui, s, painter, view, id, &title, row, busy);
    }
    clicked
}

/// An input row: port on the left edge (of the context or operator), label, inline value.
#[allow(clippy::too_many_arguments)]
fn input_row(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    owner: u32,
    owner_title: &str,
    row: &layout::InputRow,
    busy: bool,
) {
    let z = view.zoom;
    let w = &s.effect_graph;
    let name = row.port.name;
    let link = w.graph.input_link(owner, name).cloned();
    let types = w.graph.types();
    let ty = match row.port.ty {
        misty_engine::vfx::types::PortType::Fixed(t) => t,
        misty_engine::vfx::types::PortType::Dynamic => types.dynamic(owner),
    };
    let r = scr(view, row.rect);
    let at = view.screen(row.at);
    let color = type_color(ty);
    let hit = Rect::from_center_size(at, vec2(16.0, 16.0) * z.max(0.7));
    let presp = ui.interact(
        hit,
        ui.id().with(("vfx_in", owner, name)),
        Sense::click_and_drag(),
    );
    let label = format!("{owner_title} in {name}");
    presp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
    let compatible = w.canvas.connecting.as_ref().is_some_and(|c| c.output);
    let rad = PORT_R
        * z
        * if presp.hovered() || compatible {
            1.3
        } else {
            1.0
        };
    if link.is_some() {
        painter.circle_filled(at, rad, color);
    } else {
        painter.circle_filled(at, rad, CTX_BG);
        painter.circle_stroke(at, rad, Stroke::new(1.6 * z.max(0.6), color));
    }
    painter.text(
        r.left_center() + vec2(4.0 * z, 0.0),
        Align2::LEFT_CENTER,
        name,
        FontId::proportional(11.5 * z),
        theme::TEXT.gamma_multiply(0.85),
    );
    // Picking up a linked input detaches it and drags from its source.
    let w = &mut s.effect_graph;
    if presp.drag_started_by(egui::PointerButton::Primary)
        && let Some(l) = &link
    {
        w.edit("Disconnect", false, |g| g.disconnect(owner, name));
        w.canvas.connecting = Some(PortEnd {
            node: l.from,
            port: l.output.clone(),
            output: true,
        });
    }
    if presp.drag_stopped()
        && let Some(from) = w.canvas.connecting.take()
    {
        let p = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
        finish_link(s, from, p, view);
    }
    let w = &mut s.effect_graph;
    if presp.clicked() && ui.input(|i| i.modifiers.alt) {
        w.edit("Disconnect", false, |g| g.disconnect(owner, name));
    }
    if link.is_some() || z < 0.5 {
        return;
    }
    // Inline value on the right part of the row.
    let field = Rect::from_min_max(
        pos2(r.left() + r.width() * 0.42, r.top() + 2.0 * z),
        pos2(r.right() - 2.0 * z, r.bottom() - 2.0 * z),
    );
    let value = match w.graph.find(owner) {
        Some(Item::Block(ci, bi)) => w.graph.contexts[ci].blocks[bi].value(&row.port),
        Some(Item::Operator(oi)) => {
            let v = w.graph.operators[oi].value(&row.port);
            // Dynamic inputs edit as the resolved type.
            if row.port.ty == misty_engine::vfx::types::PortType::Dynamic {
                v.converted(ty)
            } else {
                v
            }
        }
        _ => return,
    };
    let mut v = value.clone();
    let res = fields::value_field(ui, field, z, &mut v, (owner, name));
    if res.open_ramp {
        w.canvas.ramp = Some((owner, name.to_owned(), field.left_bottom()));
    }
    if res.changed && v != value {
        set_value(w, owner, name, v, busy);
    }
}

fn set_value(w: &mut EffectGraphWindow, owner: u32, name: &str, v: Value, busy: bool) {
    let name = name.to_owned();
    w.edit("Change Value", busy, |g| match g.find(owner) {
        Some(Item::Block(ci, bi)) => {
            g.contexts[ci].blocks[bi].values.insert(name, v);
        }
        Some(Item::Operator(oi)) => {
            g.operators[oi].values.insert(name, v);
        }
        _ => {}
    });
}

/// Drop a dragged link: onto an input links it.
fn finish_link(s: &mut EditorState, from: PortEnd, at: Option<Pos2>, view: View) {
    let Some(at) = at else { return };
    let w = &mut s.effect_graph;
    let target = layout::input_ports(&w.graph)
        .into_iter()
        .filter(|(n, _, _, _)| *n != from.node)
        .map(|(n, p, pos, _)| (n, p, view.screen(pos).distance(at)))
        .filter(|(_, _, d)| *d < 14.0 * view.zoom.max(0.7))
        .min_by(|a, b| a.2.total_cmp(&b.2));
    if let Some((to, input, _)) = target {
        match w.graph.check_link(from.node, &from.port, to, input) {
            Some(e) => w.canvas.flash = Some((e, -1.0)),
            None => {
                let port = from.port.clone();
                w.edit("Connect", false, |g| {
                    let _ = g.connect(from.node, &port, to, input);
                });
            }
        }
    }
}

/// One operator node. Returns true when clicked.
fn operator_ui(
    ui: &mut Ui,
    s: &mut EditorState,
    painter: &egui::Painter,
    view: View,
    l: &OperatorLayout,
    busy: bool,
) -> bool {
    let z = view.zoom;
    let id = l.id;
    let Some(op) = s.effect_graph.graph.operator(id).cloned() else {
        return false;
    };
    let rect = scr(view, l.rect);
    let round = CornerRadius::same((8.0 * z).round().clamp(2.0, 14.0) as u8);
    painter.rect_filled(
        rect.translate(vec2(0.0, 4.0 * z)),
        round,
        Color32::from_black_alpha(70),
    );
    painter.rect_filled(rect, round, NODE_BG);
    let header = scr(view, l.header);
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
    painter.rect_filled(
        Rect::from_min_size(
            header.left_bottom() - vec2(0.0, 2.0 * z),
            vec2(header.width(), 2.0 * z),
        ),
        0.0,
        op_color(&op.kind).gamma_multiply(0.8),
    );
    let title = op.kind.title();
    painter.text(
        header.left_center() + vec2(10.0 * z, 0.0),
        Align2::LEFT_CENTER,
        &title,
        FontId::proportional(13.0 * z),
        theme::TEXT,
    );
    let w = &s.effect_graph;
    if let Some(e) = w.error_of(id) {
        painter.rect_stroke(
            rect,
            round,
            Stroke::new(2.0, super::ERROR),
            StrokeKind::Outside,
        );
        let msg = e.message.clone();
        error_badge(
            ui,
            painter,
            header.right_center() - vec2(14.0 * z, 0.0),
            z,
            id,
            &msg,
        );
    } else if w.selected.contains(&id) {
        painter.rect_stroke(rect, round, Stroke::new(2.0, SELECT), StrokeKind::Outside);
    }
    let resp = ui.interact(rect, ui.id().with(("vfx_op", id)), Sense::click_and_drag());
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
    let clicked = select_and_move(ui, &mut s.effect_graph, id, &resp, z, busy);
    resp.context_menu(|ui| {
        let w = &mut s.effect_graph;
        if ui.button("Duplicate").clicked() {
            w.duplicate_selected();
            ui.close();
        }
        if ui.button("Disconnect All").clicked() {
            w.edit("Disconnect All", false, |g| {
                g.links.retain(|l| l.from != id && l.to != id)
            });
            ui.close();
        }
        ui.separator();
        if ui.button("Delete").clicked() {
            w.delete_selected();
            ui.close();
        }
    });

    if let Some(r) = l.settings
        && z > 0.45
    {
        let mut k = op.kind.clone();
        small_scope(ui, scr(view, r), z, |ui| {
            ui.horizontal_centered(|ui| match &mut k {
                OperatorKind::GetAttribute { attribute } => {
                    combo(ui, ("vfx_get", id), attribute, &AttributeRead::ALL, |a| {
                        a.label().to_owned()
                    });
                }
                OperatorKind::Random { mode } => {
                    combo(
                        ui,
                        ("vfx_rand", id),
                        mode,
                        &[RandomMode::PerEvaluation, RandomMode::PerParticle],
                        |m| match m {
                            RandomMode::PerEvaluation => "Per Evaluation".into(),
                            RandomMode::PerParticle => "Per Particle".into(),
                        },
                    );
                }
                OperatorKind::SubgraphOutput { name } => {
                    row_label(ui, "Name");
                    ui.add(egui::TextEdit::singleline(name).desired_width(f32::INFINITY));
                }
                _ => {}
            });
        });
        if k != op.kind {
            let busy = interacting(ui.ctx());
            s.effect_graph.edit("Operator Settings", busy, |g| {
                if let Some(o) = g.operator_mut(id) {
                    o.kind = k;
                }
                // Outputs that changed type keep their links only when they still fit.
                let bad: Vec<(u32, String)> = g
                    .links
                    .iter()
                    .filter(|l| l.from == id)
                    .filter(|l| {
                        let mut probe = g.clone();
                        probe.disconnect(l.to, &l.input);
                        probe
                            .check_link(l.from, &l.output, l.to, &l.input)
                            .is_some()
                    })
                    .map(|l| (l.to, l.input.clone()))
                    .collect();
                for (to, input) in bad {
                    g.disconnect(to, &input);
                }
            });
        }
    }

    for row in &l.inputs {
        input_row(ui, s, painter, view, id, &title, row, busy);
    }
    // Outputs.
    let types = s.effect_graph.graph.types();
    for (p, at) in &l.outputs {
        let pos = view.screen(*at);
        let ty = types.output(id, p.name).unwrap_or(ValueType::Float);
        let color = type_color(ty);
        let connected = s
            .effect_graph
            .graph
            .links
            .iter()
            .any(|x| x.from == id && x.output == p.name);
        let hit = Rect::from_center_size(pos, vec2(16.0, 16.0) * z.max(0.7));
        let presp = ui.interact(
            hit,
            ui.id().with(("vfx_out", id, p.name)),
            Sense::click_and_drag(),
        );
        let label = format!("{title} out {}", p.name);
        presp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &label));
        let rad = PORT_R * z * if presp.hovered() { 1.3 } else { 1.0 };
        if connected {
            painter.circle_filled(pos, rad, color);
        } else {
            painter.circle_filled(pos, rad, NODE_BG);
            painter.circle_stroke(pos, rad, Stroke::new(1.6 * z.max(0.6), color));
        }
        painter.text(
            pos - vec2(10.0 * z, 0.0),
            Align2::RIGHT_CENTER,
            format!("{} ({})", p.name, ty.label()),
            FontId::proportional(11.0 * z),
            theme::TEXT_DIM,
        );
        let w = &mut s.effect_graph;
        if presp.drag_started_by(egui::PointerButton::Primary) {
            w.canvas.connecting = Some(PortEnd {
                node: id,
                port: p.name.to_owned(),
                output: true,
            });
        }
        if presp.drag_stopped()
            && let Some(from) = w.canvas.connecting.take()
        {
            let at = ui.input(|i| i.pointer.interact_pos().or(i.pointer.hover_pos()));
            finish_link(s, from, at, view);
        }
        let w = &mut s.effect_graph;
        if presp.clicked() && ui.input(|i| i.modifiers.alt) {
            let name = p.name;
            w.edit("Disconnect", false, |g| {
                g.links.retain(|l| !(l.from == id && l.output == name))
            });
        }
    }
    clicked
}

/// The gradient / curve editor popup.
fn ramp_popup(ui: &mut Ui, s: &mut EditorState) {
    let Some((owner, name, at)) = s.effect_graph.canvas.ramp.clone() else {
        return;
    };
    let w = &s.effect_graph;
    let port = w.graph.inputs(owner).into_iter().find(|p| p.name == name);
    let value = match (w.graph.find(owner), &port) {
        (Some(Item::Block(ci, bi)), Some(p)) => w.graph.contexts[ci].blocks[bi].value(p),
        (Some(Item::Operator(oi)), Some(p)) => w.graph.operators[oi].value(p),
        _ => {
            s.effect_graph.canvas.ramp = None;
            return;
        }
    };
    let mut v = value.clone();
    let mut close = false;
    let area = egui::Area::new(ui.id().with("vfx_ramp_popup"))
        .fixed_pos(at)
        .order(egui::Order::Foreground)
        .constrain(true)
        .show(ui.ctx(), |ui| {
            egui::Frame::popup(ui.style())
                .fill(theme::POPUP_BG)
                .corner_radius(8.0)
                .show(ui, |ui| {
                    ui.set_width(260.0);
                    ui.horizontal(|ui| {
                        ui.label(egui::RichText::new(&name).strong());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                close = true;
                            }
                        });
                    });
                    match &mut v {
                        Value::Gradient(g) => {
                            crate::panels::inspector::particles::gradient(ui, "vfx_gradient", g);
                        }
                        Value::Curve(c) => {
                            fields::curve_editor(ui, c);
                        }
                        _ => {
                            close = true;
                        }
                    }
                });
        });
    let outside = ui.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.expand(4.0).contains(p))
    });
    if v != value {
        let busy = interacting(ui.ctx());
        set_value(&mut s.effect_graph, owner, &name, v, busy);
    }
    if close || outside {
        s.effect_graph.canvas.ramp = None;
    }
}

fn canvas_menu(ui: &mut Ui, s: &mut EditorState, at: Option<Pos2>) {
    if ui.button("Create Node").clicked() {
        if let Some(p) = at {
            let w = &mut s.effect_graph;
            let graph = w.canvas.view.map_or(pos2(0.0, 0.0), |v| v.graph(p));
            w.search = Some(search::Search::nodes(p, graph));
        }
        ui.close();
    }
    if !s.effect_graph.is_subgraph() {
        ui.menu_button("GPU Event System", |ui| {
            for source in EventSource::ALL {
                if ui.button(source.label()).clicked() {
                    let w = &mut s.effect_graph;
                    let graph = match (at, w.canvas.view) {
                        (Some(p), Some(v)) => v.graph(p),
                        _ => pos2(layout::bounds(&w.graph).right() + 60.0, 0.0),
                    };
                    let mut first = None;
                    w.edit("GPU Event System", false, |g| {
                        let sys = g.add_event_system(0, source, [graph.x, graph.y]);
                        first = g.contexts.iter().find(|c| c.system == sys).map(|c| c.id);
                    });
                    if let Some(id) = first {
                        w.selected = [id].into();
                    }
                    ui.close();
                }
            }
        });
    }
    ui.separator();
    if ui.button("Frame All").clicked() {
        s.effect_graph.canvas.frame_all = true;
        ui.close();
    }
    if ui.button("Tidy Layout").clicked() {
        s.effect_graph.edit("Tidy Layout", false, layout::arrange);
        s.effect_graph.canvas.frame_all = true;
        ui.close();
    }
    let any = !s.effect_graph.selected.is_empty();
    if ui
        .add_enabled(any, egui::Button::new("Delete Selection"))
        .clicked()
    {
        s.effect_graph.delete_selected();
        ui.close();
    }
}
