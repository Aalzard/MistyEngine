//! Pieces shared by the node-graph editors (Shader Graph, Effect Graph, Script Graph): the
//! graph ↔ screen mapping, the background grid, wire curves, wheel zoom and middle-mouse /
//! Alt+drag panning, and the red error badge on a node.

use egui::epaint::CubicBezierShape;
use egui::{Align2, Color32, FontId, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2, pos2, vec2};

/// Soft red of error badges and messages.
pub const ERROR: Color32 = Color32::from_rgb(232, 96, 90);

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

/// The background grid (fine and coarse lines).
pub fn draw_grid(painter: &egui::Painter, area: Rect, view: View) {
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
pub fn wire(painter: &egui::Painter, a: Pos2, b: Pos2, color: Color32, zoom: f32) {
    wire_width(painter, a, b, color, zoom, 2.4);
}

/// A connection curve of a given width (at zoom 1).
pub fn wire_width(
    painter: &egui::Painter,
    a: Pos2,
    b: Pos2,
    color: Color32,
    zoom: f32,
    width: f32,
) {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    let shape = CubicBezierShape::from_points_stroke(
        [a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b],
        false,
        Color32::TRANSPARENT,
        Stroke::new((width * zoom).max(1.2), color),
    );
    painter.add(shape);
}

/// Points along a wire curve (hit tests, flow dots).
pub fn wire_point(a: Pos2, b: Pos2, zoom: f32, t: f32) -> Pos2 {
    let dx = ((b.x - a.x).abs() * 0.5).max(40.0 * zoom);
    let (p0, p1, p2, p3) = (a, a + vec2(dx, 0.0), b - vec2(dx, 0.0), b);
    let u = 1.0 - t;
    let k = |x0: f32, x1: f32, x2: f32, x3: f32| {
        u * u * u * x0 + 3.0 * u * u * t * x1 + 3.0 * u * t * t * x2 + t * t * t * x3
    };
    pos2(k(p0.x, p1.x, p2.x, p3.x), k(p0.y, p1.y, p2.y, p3.y))
}

/// Wheel zoom around the mouse (when `wheel`) and middle-mouse / Alt+left drag panning.
/// Returns true while panning.
pub fn pan_zoom(
    ui: &Ui,
    resp: &Response,
    area: Rect,
    pan: &mut Vec2,
    zoom: &mut f32,
    wheel: bool,
) -> bool {
    let z = if *zoom > 0.0 { *zoom } else { 1.0 };
    if wheel && let Some(p) = resp.hover_pos() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll != 0.0 {
            let new = (z * (scroll * 0.002).exp()).clamp(0.2, 2.0);
            let centre = area.center();
            let g = (p - centre) / z - *pan;
            *zoom = new;
            *pan = (p - centre) / new - g;
        }
    }
    let alt = ui.input(|i| i.modifiers.alt);
    if resp.dragged_by(egui::PointerButton::Middle)
        || (alt && resp.dragged_by(egui::PointerButton::Primary))
    {
        *pan += resp.drag_delta() / (if *zoom > 0.0 { *zoom } else { 1.0 });
        return true;
    }
    false
}

/// A red "!" badge with the message on hover.
pub fn error_badge(
    ui: &Ui,
    painter: &egui::Painter,
    at: Pos2,
    z: f32,
    id: egui::Id,
    message: &str,
) {
    painter.circle_filled(at, 8.0 * z, ERROR);
    painter.text(
        at,
        Align2::CENTER_CENTER,
        "!",
        FontId::proportional(12.0 * z),
        Color32::WHITE,
    );
    ui.interact(
        Rect::from_center_size(at, vec2(16.0, 16.0) * z),
        id,
        Sense::hover(),
    )
    .on_hover_text(message);
}
