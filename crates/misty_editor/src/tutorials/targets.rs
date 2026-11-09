//! Stable IDs for UI elements a tutorial can point at.
//!
//! Panels call [`mark`] (or the helpers) with an ID and the screen rect of a widget; the
//! tutorial overlay looks the rect up later in the frame. Nothing is recorded unless a
//! tutorial is running, so the calls cost one thread-local read otherwise.
//!
//! IDs are plain strings:
//! - `menu.<Menu>` top menu buttons, `menuitem.<Label>` entries and submenus;
//! - `toolbar.play`, `scene.effects`, `scene.effects_menu`, `panel.<Tab title>`;
//! - `hierarchy.<object name>` Hierarchy rows;
//! - `inspector.<Component>` component headers; nested sections and field rows add
//!   `/<Section>` and `/<Label>` from the [`scope`] stack (e.g.
//!   `inspector.Post-process Volume/Fog/Anisotropy`);
//! - `World Builder/<Section>/<Label>` in the World Builder window.

use std::cell::RefCell;
use std::collections::HashMap;

use egui::{Rect, Response, Ui};

#[derive(Default)]
struct Registry {
    enabled: bool,
    /// Each ID's last drawing.
    rects: HashMap<String, Mark>,
    /// Section names pushed by [`scope`].
    scope: Vec<String>,
    /// Scroll this target into view the next time it is drawn.
    scroll_to: Option<String>,
}

#[derive(Clone, Copy)]
struct Mark {
    /// The part inside the clip rect (the whole rect when none of it is visible).
    rect: Rect,
    /// Some of it is on screen (not scrolled away or clipped by its panel).
    visible: bool,
    frame: u64,
}

thread_local! {
    static REGISTRY: RefCell<Registry> = RefCell::new(Registry::default());
}

fn with<R>(f: impl FnOnce(&mut Registry) -> R) -> R {
    REGISTRY.with(|r| f(&mut r.borrow_mut()))
}

/// Turn recording on or off (on while a tutorial runs).
pub fn set_enabled(on: bool) {
    with(|r| {
        r.enabled = on;
        if !on {
            r.rects.clear();
            r.scroll_to = None;
        }
    });
}

pub fn enabled() -> bool {
    with(|r| r.enabled)
}

/// Record `rect` for `id` this frame (clipped to what `ui` shows).
pub fn mark(ui: &Ui, id: &str, rect: Rect) {
    let frame = ui.ctx().cumulative_frame_nr();
    let shown = rect.intersect(ui.clip_rect());
    let visible = shown.width() >= 4.0 && shown.height() >= 4.0;
    let m = Mark {
        rect: if visible { shown } else { rect },
        visible,
        frame,
    };
    let scroll = with(|r| {
        if !r.enabled {
            return false;
        }
        // Drop IDs not drawn for a while (closed windows, other scenes).
        if r.rects.len() > 1024 {
            r.rects.retain(|_, m| m.frame + 2 >= frame);
        }
        r.rects.insert(id.to_owned(), m);
        if r.scroll_to.as_deref() == Some(id) {
            r.scroll_to = None;
            return true;
        }
        false
    });
    if scroll {
        ui.scroll_to_rect(rect, Some(egui::Align::Center));
    }
}

/// Record a response's rect for `id`.
pub fn mark_response(ui: &Ui, id: &str, response: &Response) {
    mark(ui, id, response.rect);
}

/// The ID of a field or header inside the current scope: `<scope>/<name>`.
pub fn scoped(name: &str) -> String {
    with(|r| {
        if r.scope.is_empty() {
            name.to_owned()
        } else {
            format!("{}/{name}", r.scope.join("/"))
        }
    })
}

/// Mark a field row: its label rect widened to the right edge of the visible area, so the
/// highlight covers the label and its value.
pub fn row(ui: &Ui, label: &str, label_rect: Rect) {
    if !enabled() {
        return;
    }
    let right = ui.clip_rect().right() - 10.0;
    let rect = Rect::from_min_max(
        label_rect.min,
        egui::pos2(right.max(label_rect.right()), label_rect.max.y),
    );
    mark(ui, &scoped(label), rect.expand2(egui::vec2(3.0, 1.0)));
}

/// `ui.label(text)` that also marks its row as `<scope>/<text>`.
pub fn label(ui: &mut Ui, text: &str) -> Response {
    // Shown in the editor's language; the target id stays English.
    let r = crate::panels::inspector::card::prop_label(ui, crate::i18n::tr(text));
    row(ui, text, r.rect);
    r
}

/// Mark a section header as `<scope>/<name>`.
pub fn header(ui: &Ui, name: &str, rect: Rect) {
    if enabled() {
        mark(ui, &scoped(name), rect);
    }
}

/// Push a section name for the IDs marked until the guard drops.
#[must_use = "the scope ends when the guard drops"]
pub fn scope(name: &str) -> ScopeGuard {
    let pushed = with(|r| {
        if r.enabled {
            r.scope.push(name.to_owned());
        }
        r.enabled
    });
    ScopeGuard { pushed }
}

pub struct ScopeGuard {
    pushed: bool,
}

impl Drop for ScopeGuard {
    fn drop(&mut self) {
        if self.pushed {
            with(|r| {
                r.scope.pop();
            });
        }
    }
}

/// Where `id` is visible, if it was drawn this frame or the one before (widgets drawn after
/// the overlay).
pub fn rect(ctx: &egui::Context, id: &str) -> Option<Rect> {
    let frame = ctx.cumulative_frame_nr();
    with(|r| {
        r.rects
            .get(id)
            .filter(|m| m.visible && m.frame + 1 >= frame)
            .map(|m| m.rect)
    })
}

/// Every ID drawn this frame or the one before (visible or not), sorted.
#[cfg(test)]
pub fn recorded(ctx: &egui::Context) -> Vec<String> {
    let frame = ctx.cumulative_frame_nr();
    with(|r| {
        let mut v: Vec<String> = r
            .rects
            .iter()
            .filter(|(_, m)| m.frame + 1 >= frame)
            .map(|(id, _)| id.clone())
            .collect();
        v.sort();
        v
    })
}

/// Ask the scroll area holding `id` to bring it into view when it is next drawn.
pub fn request_scroll(id: Option<&str>) {
    with(|r| r.scroll_to = id.map(str::to_owned));
}
