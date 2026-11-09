//! The custom title bar of the frameless window (the editor and the Hub).
//!
//! The window is created without OS decorations (`main.rs`), so this bar does their job:
//! menus on the left, the logo and window title in the centre, and flat minimise /
//! maximise (restore) / close buttons on the right. Dragging the empty bar moves the window,
//! double-clicking it maximises or restores, and [`resize_edges`] puts thin resize zones
//! along the window border.

use egui::text::LayoutJob;
use egui::{
    Align, Color32, Context, CursorIcon, FontId, Layout, Pos2, Rect, Sense, Stroke, TextFormat, Ui,
    UiBuilder, Vec2, ViewportCommand, WidgetInfo, WidgetType, pos2, vec2,
};

/// Height of the bar in points.
pub const HEIGHT: f32 = 30.0;
/// Width of one window button (as on Windows).
const BUTTON_WIDTH: f32 = 46.0;
/// The close button's hover colour (the familiar Windows red).
const CLOSE_RED: Color32 = Color32::from_rgb(196, 43, 28);
/// Thickness of the resize zones along the window border.
const RESIZE_MARGIN: f32 = 5.0;

/// Colours of the bar.
#[derive(Clone, Copy)]
pub struct BarStyle {
    pub bg: Color32,
    /// The product name and the window-button glyphs.
    pub text: Color32,
    /// The rest of the title (project and scene).
    pub dim: Color32,
    /// Window-button hover fill.
    pub hover: Color32,
}

/// The centred title: `strong` brighter, then `rest` dimmer ("Misty Engine — Project — Scene").
pub fn title_job(strong: &str, rest: &str, style: &BarStyle) -> LayoutJob {
    let mut job = LayoutJob::default();
    let format = |color| TextFormat {
        font_id: FontId::proportional(13.0),
        color,
        valign: Align::Center,
        ..Default::default()
    };
    job.append(strong, 0.0, format(style.text));
    job.append(rest, 0.0, format(style.dim));
    job
}

/// Draw the bar filling `ui` (a top panel of [`HEIGHT`]); `menus` go on the left and return
/// the right edge of what they drew. In a right-to-left UI the bar is mirrored: window buttons
/// on the left, `menus` laid out right to left from the right end (returning their left edge).
pub fn show(ui: &mut Ui, style: &BarStyle, title: LayoutJob, menus: impl FnOnce(&mut Ui) -> f32) {
    let rect = ui.max_rect();
    ui.painter().rect_filled(rect, 0.0, style.bg);
    let title_text = title.text.clone();
    // The empty bar: registered first, so the menus and buttons drawn on top take priority.
    let bar = ui.interact(
        rect,
        ui.id().with("title_bar_drag"),
        Sense::click_and_drag(),
    );
    bar.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, &title_text));

    let rtl = crate::i18n::is_rtl();
    // Window buttons from the far end; what is left between them and the near end.
    let buttons_edge = window_buttons(ui, rect, style, rtl);
    let (menus_rect, layout) = if rtl {
        (
            Rect::from_min_max(pos2(buttons_edge, rect.min.y), rect.max - vec2(4.0, 0.0)),
            Layout::right_to_left(Align::Center),
        )
    } else {
        (
            Rect::from_min_max(rect.min + vec2(4.0, 0.0), pos2(buttons_edge, rect.max.y)),
            Layout::left_to_right(Align::Center),
        )
    };
    let menus_edge = ui
        .scope_builder(UiBuilder::new().max_rect(menus_rect).layout(layout), menus)
        .inner;

    // Logo and title in the centre, nudged aside if a narrow window brings the menus close.
    let logo_size = 18.0;
    let gap = 6.0;
    let galley = ui.painter().layout_job(title);
    let width = logo_size + gap + galley.size().x;
    let (x, clip) = if rtl {
        let x = (rect.center().x - width / 2.0).min(menus_edge - 16.0 - width);
        (
            x,
            Rect::from_min_max(pos2(buttons_edge, rect.min.y), pos2(menus_edge, rect.max.y)),
        )
    } else {
        let x = (rect.center().x - width / 2.0).max(menus_edge + 16.0);
        (
            x,
            Rect::from_min_max(pos2(menus_edge, rect.min.y), pos2(buttons_edge, rect.max.y)),
        )
    };
    let painter = ui.painter().with_clip_rect(clip);
    let logo_rect = Rect::from_min_size(
        pos2(x, rect.center().y - logo_size / 2.0),
        Vec2::splat(logo_size),
    );
    let logo = crate::branding::small_logo_texture(ui.ctx());
    egui::Image::new(&logo).paint_at(ui, logo_rect);
    let text_pos = pos2(
        logo_rect.right() + gap,
        rect.center().y - galley.size().y / 2.0,
    );
    painter.galley(text_pos, galley, style.text);

    // Move by dragging, maximise / restore by double-clicking.
    if bar.double_clicked() {
        toggle_maximized(ui.ctx());
    } else if bar.drag_started_by(egui::PointerButton::Primary) {
        ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
    }
}

/// Whether the window is maximised (as last reported by the OS).
fn maximized(ctx: &Context) -> bool {
    ctx.input(|i| i.viewport().maximized.unwrap_or(false))
}

fn toggle_maximized(ctx: &Context) {
    let max = maximized(ctx);
    ctx.send_viewport_cmd(ViewportCommand::Maximized(!max));
}

#[derive(Clone, Copy, PartialEq)]
enum Glyph {
    Minimize,
    Maximize,
    Restore,
    Close,
}

/// Minimise, maximise / restore and close at the right end of `bar` (close outermost), or at
/// the left end when `mirrored`. Returns their inner edge.
fn window_buttons(ui: &mut Ui, bar: Rect, style: &BarStyle, mirrored: bool) -> f32 {
    let ctx = ui.ctx().clone();
    let max_glyph = if maximized(&ctx) {
        Glyph::Restore
    } else {
        Glyph::Maximize
    };
    let mut edge = if mirrored { bar.min.x } else { bar.max.x };
    for glyph in [Glyph::Close, max_glyph, Glyph::Minimize] {
        let rect = if mirrored {
            let r = Rect::from_min_max(pos2(edge, bar.min.y), pos2(edge + BUTTON_WIDTH, bar.max.y));
            edge += BUTTON_WIDTH;
            r
        } else {
            let r = Rect::from_min_max(pos2(edge - BUTTON_WIDTH, bar.min.y), pos2(edge, bar.max.y));
            edge -= BUTTON_WIDTH;
            r
        };
        if window_button(ui, rect, glyph, style).clicked() {
            match glyph {
                Glyph::Minimize => ctx.send_viewport_cmd(ViewportCommand::Minimized(true)),
                Glyph::Maximize | Glyph::Restore => toggle_maximized(&ctx),
                // Goes through `close_requested`, so the unsaved-changes prompt still asks.
                Glyph::Close => ctx.send_viewport_cmd(ViewportCommand::Close),
            }
        }
    }
    edge
}

fn window_button(ui: &mut Ui, rect: Rect, glyph: Glyph, style: &BarStyle) -> egui::Response {
    let name = match glyph {
        Glyph::Minimize => "Minimize",
        Glyph::Maximize => "Maximize",
        Glyph::Restore => "Restore Down",
        Glyph::Close => "Close",
    };
    let resp = ui.interact(rect, ui.id().with(("window_button", name)), Sense::click());
    resp.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, name));
    let hovered = resp.hovered();
    let mut color = style.text;
    if hovered {
        let fill = if glyph == Glyph::Close {
            color = Color32::WHITE;
            if resp.is_pointer_button_down_on() {
                CLOSE_RED.gamma_multiply(0.8)
            } else {
                CLOSE_RED
            }
        } else {
            style.hover
        };
        ui.painter().rect_filled(rect, 0.0, fill);
    }
    paint_glyph(ui, rect.center(), glyph, color);
    resp
}

/// Thin 10-point glyphs, pixel-snapped so their 1 px lines stay sharp.
fn paint_glyph(ui: &Ui, center: Pos2, glyph: Glyph, color: Color32) {
    let p = ui.painter();
    use egui::emath::GuiRounding as _;
    let ppp = ui.ctx().pixels_per_point();
    // One physical pixel wide (but at least 1 pt on low-DPI screens).
    let stroke = Stroke::new((1.0 / ppp).max(1.0), color);
    let c = center.round_to_pixel_center(ppp);
    let h = 5.0;
    match glyph {
        Glyph::Minimize => {
            p.line_segment([c + vec2(-h, 0.0), c + vec2(h, 0.0)], stroke);
        }
        Glyph::Maximize => {
            p.rect_stroke(
                Rect::from_center_size(c, Vec2::splat(2.0 * h)),
                1.0,
                stroke,
                egui::StrokeKind::Middle,
            );
        }
        Glyph::Restore => {
            // The front window, and the back one peeking out above and to the right.
            let front = Rect::from_min_size(c + vec2(-h, -h + 2.0), Vec2::splat(2.0 * h - 2.0));
            p.rect_stroke(front, 1.0, stroke, egui::StrokeKind::Middle);
            let (a, b, d) = (
                c + vec2(-h + 2.0, -h),
                c + vec2(h, -h),
                c + vec2(h, h - 2.0),
            );
            p.line_segment([a, b], stroke);
            p.line_segment([b, d], stroke);
        }
        Glyph::Close => {
            let h = h - 0.5;
            p.line_segment([c + vec2(-h, -h), c + vec2(h, h)], stroke);
            p.line_segment([c + vec2(-h, h), c + vec2(h, -h)], stroke);
        }
    }
}

/// Resize zones along the edges and corners of a frameless, non-maximised window. Call once a
/// frame after the rest of the UI (they sit in the foreground). Undecorated windows on Windows
/// get no OS resize border, so pressing in a zone starts the OS resize itself.
pub fn resize_edges(ctx: &Context) {
    let (maximized, fullscreen) = ctx.input(|i| {
        let v = i.viewport();
        (v.maximized.unwrap_or(false), v.fullscreen.unwrap_or(false))
    });
    if maximized || fullscreen {
        return;
    }
    use egui::ResizeDirection as D;
    let screen = ctx.content_rect();
    let (m, c) = (RESIZE_MARGIN, 2.0 * RESIZE_MARGIN);
    let (l, r, t, b) = (screen.min.x, screen.max.x, screen.min.y, screen.max.y);
    let zones = [
        (
            D::North,
            Rect::from_min_max(pos2(l + c, t), pos2(r - c, t + m)),
        ),
        (
            D::South,
            Rect::from_min_max(pos2(l + c, b - m), pos2(r - c, b)),
        ),
        (
            D::West,
            Rect::from_min_max(pos2(l, t + c), pos2(l + m, b - c)),
        ),
        (
            D::East,
            Rect::from_min_max(pos2(r - m, t + c), pos2(r, b - c)),
        ),
        (
            D::NorthWest,
            Rect::from_min_max(pos2(l, t), pos2(l + c, t + c)),
        ),
        (
            D::NorthEast,
            Rect::from_min_max(pos2(r - c, t), pos2(r, t + c)),
        ),
        (
            D::SouthWest,
            Rect::from_min_max(pos2(l, b - c), pos2(l + c, b)),
        ),
        (
            D::SouthEast,
            Rect::from_min_max(pos2(r - c, b - c), pos2(r, b)),
        ),
    ];
    for (dir, rect) in zones {
        let cursor = match dir {
            D::North => CursorIcon::ResizeNorth,
            D::South => CursorIcon::ResizeSouth,
            D::West => CursorIcon::ResizeWest,
            D::East => CursorIcon::ResizeEast,
            D::NorthWest => CursorIcon::ResizeNorthWest,
            D::NorthEast => CursorIcon::ResizeNorthEast,
            D::SouthWest => CursorIcon::ResizeSouthWest,
            D::SouthEast => CursorIcon::ResizeSouthEast,
        };
        egui::Area::new(egui::Id::new(("window_resize", format!("{dir:?}"))))
            .order(egui::Order::Foreground)
            .fixed_pos(rect.min)
            .movable(false)
            .constrain(false)
            .show(ctx, |ui| {
                let (_, resp) = ui.allocate_exact_size(rect.size(), Sense::drag());
                let resp = resp.on_hover_cursor(cursor);
                if resp.is_pointer_button_down_on() && ui.input(|i| i.pointer.primary_pressed()) {
                    ctx.send_viewport_cmd(ViewportCommand::BeginResize(dir));
                }
            });
    }
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;

    const STYLE: BarStyle = BarStyle {
        bg: Color32::from_rgb(30, 30, 30),
        text: Color32::WHITE,
        dim: Color32::GRAY,
        hover: Color32::from_rgba_premultiplied(20, 20, 20, 20),
    };

    #[test]
    fn shows_title_menus_and_window_buttons() {
        let mut h = Harness::builder().with_size([900.0, 60.0]).build_ui(|ui| {
            egui::Panel::top("bar")
                .exact_size(HEIGHT)
                .frame(egui::Frame::NONE)
                .show(ui, |ui| {
                    let title = title_job("Misty Engine", " — Game — Level", &STYLE);
                    show(ui, &STYLE, title, |ui| ui.button("File").rect.right());
                });
        });
        h.run();
        h.get_by_label("Misty Engine — Game — Level");
        h.get_by_label("File");
        h.get_by_label("Minimize");
        h.get_by_label("Maximize");
        h.get_by_label("Close").click();
        h.run();
    }
}
