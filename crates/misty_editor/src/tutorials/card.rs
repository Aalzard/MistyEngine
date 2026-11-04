//! The running tutorial's overlay: a light dim with a hole around the highlighted element, a
//! soft outline on it, and the floating step card next to it. Only the card takes clicks.

use egui::{
    Align, Area, Color32, CornerRadius, Frame, Id, LayerId, Layout, Order, Rect, RichText, Sense,
    Stroke, Ui, pos2, vec2,
};

use crate::state::EditorState;
use crate::theme;

/// Outline and accent colour.
const GLOW: Color32 = Color32::from_rgb(255, 196, 84);
const CARD_WIDTH: f32 = 300.0;
/// How long the outline pulses when a step starts.
const PULSE_SECS: f64 = 1.6;

pub enum Nav {
    Back,
    Next,
    Skip,
    Exit,
}

/// Draw the overlay of the running tutorial; returns the button pressed.
pub fn show(ctx: &egui::Context, s: &EditorState, now: f64) -> Option<Nav> {
    let run = s.tutorials.run.as_ref()?;
    let step = run.step();
    let target = step
        .targets
        .iter()
        .find_map(|id| super::targets::rect(ctx, id).map(|r| (id.as_str(), r)));
    let age = run.started_at.map_or(0.0, |t| now - t);
    if let Some((_, rect)) = target {
        dim(ctx, rect);
        outline(ctx, rect, age);
    }
    let met = super::condition_met(ctx, s) == Some(true);
    let advancing = run.done_at.is_some();
    let info = Info {
        index: run.step,
        count: run.tutorial.steps.len(),
        title: &step.title,
        text: &step.text,
        done: met,
        advancing,
    };
    card(ctx, &info, target)
}

/// Darken everything but `hole` a little (painted above panels and windows, below menus and
/// popups).
fn dim(ctx: &egui::Context, hole: Rect) {
    let screen = ctx.content_rect();
    let hole = hole.expand(6.0).intersect(screen);
    let painter = ctx.layer_painter(LayerId::new(Order::Middle, Id::new("tutorial_dim")));
    let shade = Color32::from_rgba_unmultiplied(8, 8, 12, 105);
    let bands = [
        Rect::from_min_max(screen.min, pos2(screen.max.x, hole.min.y)),
        Rect::from_min_max(pos2(screen.min.x, hole.max.y), screen.max),
        Rect::from_min_max(pos2(screen.min.x, hole.min.y), pos2(hole.min.x, hole.max.y)),
        Rect::from_min_max(pos2(hole.max.x, hole.min.y), pos2(screen.max.x, hole.max.y)),
    ];
    for band in bands {
        if band.is_positive() {
            painter.rect_filled(band, 0.0, shade);
        }
    }
}

/// Soft rounded outline around `rect`; it breathes for a moment when the step starts.
fn outline(ctx: &egui::Context, rect: Rect, age: f64) {
    let painter = ctx.layer_painter(LayerId::new(Order::Tooltip, Id::new("tutorial_outline")));
    let pulse = if age < PULSE_SECS {
        ctx.request_repaint();
        ((age * std::f64::consts::TAU / 0.8).sin() * 0.5 + 0.5) as f32
    } else {
        0.0
    };
    let r = rect
        .expand(4.0 + pulse * 3.0)
        .intersect(ctx.content_rect().shrink(4.0));
    let round = CornerRadius::same(5);
    for (grow, alpha) in [(6.0, 22), (3.5, 45)] {
        painter.rect_stroke(
            r.expand(grow),
            round,
            Stroke::new(3.0, GLOW.gamma_multiply(alpha as f32 / 255.0)),
            egui::StrokeKind::Outside,
        );
    }
    painter.rect_stroke(r, round, Stroke::new(2.0, GLOW), egui::StrokeKind::Outside);
}

struct Info<'a> {
    index: usize,
    count: usize,
    title: &'a str,
    text: &'a str,
    /// The step's condition holds.
    done: bool,
    /// It will move on by itself in a moment.
    advancing: bool,
}

fn card(ctx: &egui::Context, info: &Info, target: Option<(&str, Rect)>) -> Option<Nav> {
    let id = Id::new("tutorial_card");
    let size = ctx
        .memory(|m| m.area_rect(id))
        .map_or(vec2(CARD_WIDTH + 24.0, 150.0), |r| r.size());
    let pos = place(ctx.content_rect(), size, target);
    let mut nav = None;
    Area::new(id)
        .order(Order::Foreground)
        .current_pos(pos)
        .constrain(true)
        .show(ctx, |ui| {
            Frame::popup(ui.style())
                .fill(theme::PANEL_BG)
                .stroke(Stroke::new(1.0, GLOW.gamma_multiply(0.7)))
                .corner_radius(8)
                .inner_margin(12)
                .show(ui, |ui| {
                    ui.set_width(CARD_WIDTH);
                    nav = body(ui, info);
                });
        });
    nav
}

fn body(ui: &mut Ui, info: &Info) -> Option<Nav> {
    let mut nav = None;
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!("Step {} of {}", info.index + 1, info.count))
                .color(theme::TEXT_DIM),
        );
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let close = ui.add(egui::Button::new(RichText::new("✕").small()).frame(false));
            if close.on_hover_text("Exit the tutorial").clicked() {
                nav = Some(Nav::Exit);
            }
        });
    });
    progress_bar(ui, info);
    ui.add_space(4.0);
    ui.label(
        RichText::new(info.title)
            .strong()
            .size(15.0)
            .color(Color32::WHITE),
    );
    ui.add_space(2.0);
    ui.label(RichText::new(info.text).color(theme::TEXT));
    if info.done {
        ui.add_space(2.0);
        let msg = if info.advancing {
            "✓ Done!"
        } else {
            "✓ Done. Press Next."
        };
        ui.label(RichText::new(msg).color(theme::AXIS_Y).strong());
    }
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let back = ui.add_enabled(info.index > 0, egui::Button::new("◀ Back"));
        if back.clicked() {
            nav = Some(Nav::Back);
        }
        let last = info.index + 1 == info.count;
        let label = if last { "Finish" } else { "Next ▶" };
        let next = egui::Button::new(RichText::new(label).strong().color(Color32::BLACK))
            .fill(GLOW)
            .min_size(vec2(72.0, 0.0));
        if ui.add(next).clicked() {
            nav = Some(Nav::Next);
        }
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let exit = ui
                .add(egui::Button::new(RichText::new("Exit").color(theme::TEXT_DIM)).frame(false));
            if exit.on_hover_text("Close the tutorial").clicked() {
                nav = Some(Nav::Exit);
            }
            let skip = ui
                .add(egui::Button::new(RichText::new("Skip").color(theme::TEXT_DIM)).frame(false));
            if skip
                .on_hover_text("Skip the rest and mark this tutorial as done")
                .clicked()
            {
                nav = Some(Nav::Skip);
            }
        });
    });
    nav
}

/// Thin bar: how far through the tutorial this step is.
fn progress_bar(ui: &mut Ui, info: &Info) {
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 3.0), Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, 1.5, theme::FIELD_BG);
    let f = (info.index + 1) as f32 / info.count.max(1) as f32;
    let done = Rect::from_min_size(rect.min, vec2(rect.width() * f, rect.height()));
    p.rect_filled(done, 1.5, GLOW.gamma_multiply(0.8));
}

/// Where the card goes: beside the target (menus leave room for their submenus), inside large
/// targets such as a whole panel, or near the top of the window without a target.
fn place(screen: Rect, size: egui::Vec2, target: Option<(&str, Rect)>) -> egui::Pos2 {
    let gap = 14.0;
    let Some((id, t)) = target else {
        return pos2(screen.center().x - size.x * 0.5, screen.top() + 90.0);
    };
    let fits = |p: egui::Pos2| screen.contains_rect(Rect::from_min_size(p, size));
    if t.width() > size.x * 1.6 && t.height() > size.y * 1.8 {
        return pos2(t.right() - size.x - 16.0, t.top() + 36.0);
    }
    // Menus: clear of their submenus and below the menu bar.
    let menu = id.starts_with("menu.");
    let side = if menu { 220.0 } else { gap };
    let top = if menu {
        t.top().max(screen.top() + 44.0)
    } else {
        t.top() - 8.0
    };
    let candidates = [
        pos2(t.right() + side, top),
        pos2(t.left() - side - size.x, t.top() - 8.0),
        pos2(t.left(), t.bottom() + gap),
        pos2(t.left(), t.top() - gap - size.y),
    ];
    let clamp = |p: egui::Pos2| {
        pos2(
            p.x.clamp(
                screen.left() + 8.0,
                (screen.right() - size.x - 8.0).max(8.0),
            ),
            p.y.clamp(
                screen.top() + 8.0,
                (screen.bottom() - size.y - 8.0).max(8.0),
            ),
        )
    };
    // Fits after nudging along the edge it sits on, without covering the target.
    candidates
        .into_iter()
        .map(clamp)
        .find(|p| fits(*p) && !Rect::from_min_size(*p, size).intersects(t))
        .unwrap_or_else(|| clamp(candidates[0]))
}
