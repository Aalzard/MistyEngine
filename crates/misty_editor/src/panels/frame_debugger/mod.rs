//! Window ▸ Analysis ▸ Frame Debugger: Enable pauses the Game view on one frame and lists its
//! render events grouped by pass; selecting an event replays the frame up to it (the Game view
//! shows that event's render target) and the right pane shows its details.
//!
//! The Game view installs the renderer's capture each frame while the window is enabled and
//! visible (see [`FrameDebuggerWindow::request`]); with the window closed nothing is recorded.

mod details;
#[cfg(test)]
mod tests;
pub mod tree;

use std::collections::HashSet;

use egui::{Color32, RichText, Ui};
use misty_engine::render::{CaptureRequest, DrawLabel, FrameCapture};

use crate::state::{EditorState, PlayState};
use crate::theme;
use crate::widgets::{panel_toolbar, search_field, toggle_button};

pub const ICON: &str = "🎞";

#[derive(Default)]
pub struct FrameDebuggerWindow {
    /// Enable pressed: the Game view is frozen on one frame and replayed up to `selected`.
    pub enabled: bool,
    /// The last captured frame's events.
    pub capture: Option<FrameCapture>,
    /// Selected event (replay stops after it).
    pub selected: usize,
    /// Collapsed pass groups by path ("Shadows ▸ Cascade 0").
    pub collapsed: HashSet<String>,
    /// Event name filter.
    pub filter: String,
    /// Play mode was running when Enable was pressed (resumed on Disable).
    resume_play: bool,
    /// UI pass the window was last drawn in (the capture runs only while it is on screen).
    seen_pass: Option<u64>,
    /// Scroll the tree to the selection (after stepping with the keyboard).
    scroll_to_selected: bool,
    /// Event to select in the first capture: an index or text in its name (screenshot hook).
    pub pending_select: Option<String>,
}

impl FrameDebuggerWindow {
    /// Enabled and drawn this or the previous UI pass.
    pub fn active(&self, ctx: &egui::Context) -> bool {
        self.enabled
            && self
                .seen_pass
                .is_some_and(|p| p + 1 >= ctx.cumulative_pass_nr())
    }

    /// The capture the Game view should install this frame (`None` = not debugging).
    pub fn request(&self, ctx: &egui::Context, labels: Vec<DrawLabel>) -> Option<CaptureRequest> {
        self.active(ctx).then(|| CaptureRequest {
            // First frame after Enable: capture everything, then select the last event.
            stop_after: self.capture.as_ref().map(|_| self.selected),
            labels,
        })
    }

    /// Store a new capture; the first one selects its last event (the whole frame).
    pub fn set_capture(&mut self, capture: FrameCapture) {
        let n = capture.events.len();
        if self.capture.is_none() {
            self.selected = n.saturating_sub(1);
            self.scroll_to_selected = true;
            if let Some(want) = self.pending_select.take() {
                let found = match want.parse::<usize>() {
                    Ok(i) => Some(i),
                    Err(_) => capture.events.iter().position(|e| e.name.contains(&want)),
                };
                self.selected = found.unwrap_or(self.selected);
            }
        }
        self.selected = self.selected.min(n.saturating_sub(1));
        self.capture = Some(capture);
    }

    pub fn event_count(&self) -> usize {
        self.capture.as_ref().map_or(0, |c| c.events.len())
    }

    /// Move the selection by `delta` events (clamped).
    pub fn step(&mut self, delta: i64) {
        let n = self.event_count();
        if n == 0 {
            return;
        }
        self.selected = (self.selected as i64 + delta).clamp(0, n as i64 - 1) as usize;
        self.scroll_to_selected = true;
    }
}

/// Enable / disable: pauses Play mode while debugging and resumes it afterwards.
pub fn set_enabled(s: &mut EditorState, on: bool) {
    let w = &mut s.frame_debugger;
    if on == w.enabled {
        return;
    }
    w.enabled = on;
    w.capture = None;
    if on {
        w.resume_play = s.play == PlayState::Playing;
        if w.resume_play {
            s.play = PlayState::Paused;
        }
    } else if std::mem::take(&mut w.resume_play) && s.play == PlayState::Paused {
        s.play = PlayState::Playing;
    }
}

/// Turn debugging off when the window is no longer shown (closed or hidden behind a tab).
pub fn sync(s: &mut EditorState, ctx: &egui::Context) {
    if s.frame_debugger.enabled && !s.frame_debugger.active(ctx) {
        set_enabled(s, false);
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    s.frame_debugger.seen_pass = Some(ui.ctx().cumulative_pass_nr());
    toolbar(ui, s);
    let w = &mut s.frame_debugger;
    let hovered = ui.rect_contains_pointer(ui.max_rect());
    if w.enabled && hovered && !ui.ctx().egui_wants_keyboard_input() {
        let (left, right) = ui.input(|i| {
            (
                i.key_pressed(egui::Key::ArrowLeft),
                i.key_pressed(egui::Key::ArrowRight),
            )
        });
        match (left, right) {
            (true, false) => w.step(-1),
            (false, true) => w.step(1),
            _ => {}
        }
    }
    let Some(capture) = w.capture.as_ref().filter(|_| w.enabled) else {
        placeholder(ui, w.enabled);
        return;
    };
    let rows = tree::rows(&capture.events, &w.collapsed, &w.filter);
    let full = ui.available_rect_before_wrap();
    let split = (full.width() * 0.46).clamp(220.0, 520.0);
    let (left, right) = full.split_left_right_at_x(full.left() + split);
    let mut clicked = None;
    let mut toggled = None;
    ui.scope_builder(egui::UiBuilder::new().max_rect(left.shrink(4.0)), |ui| {
        let scroll = std::mem::take(&mut w.scroll_to_selected);
        (clicked, toggled) = tree::show(ui, capture, &rows, w.selected, &w.collapsed, scroll);
    });
    ui.painter().vline(
        left.right(),
        full.y_range(),
        egui::Stroke::new(1.0, theme::DIVIDER),
    );
    let selected = w.selected;
    let event = capture.events.get(selected).cloned();
    let total = capture.events.len();
    ui.scope_builder(egui::UiBuilder::new().max_rect(right.shrink(6.0)), |ui| {
        if let Some(ev) = &event {
            details::show(ui, s, ev, selected, total);
        }
    });
    let w = &mut s.frame_debugger;
    if let Some(i) = clicked {
        w.selected = i;
    }
    if let Some(key) = toggled
        && !w.collapsed.remove(&key)
    {
        w.collapsed.insert(key);
    }
    ui.allocate_rect(full, egui::Sense::hover());
}

fn placeholder(ui: &mut Ui, enabled: bool) {
    ui.add_space(40.0);
    ui.vertical_centered(|ui| {
        ui.label(RichText::new(ICON).size(34.0).color(theme::TEXT_DIM));
        ui.add_space(6.0);
        let text = match enabled {
            true => "Waiting for the Game view to draw a frame…",
            false => "Press Enable to freeze the Game view and step through its draw calls.",
        };
        ui.label(RichText::new(text).color(theme::TEXT_DIM));
    });
}

fn toolbar(ui: &mut Ui, s: &mut EditorState) {
    let enabled = s.frame_debugger.enabled;
    let mut toggle = false;
    panel_toolbar(ui, |ui| {
        let label =
            RichText::new(if enabled { "● Disable" } else { "Enable" }).color(match enabled {
                true => Color32::from_rgb(120, 200, 255),
                false => theme::TEXT,
            });
        toggle = toggle_button(
            ui,
            enabled,
            label,
            "Freeze the Game view and capture its frame",
        )
        .clicked();
        let w = &mut s.frame_debugger;
        let n = w.event_count();
        ui.add_enabled_ui(enabled && n > 0, |ui| {
            if ui.button("◀").on_hover_text("Previous event (←)").clicked() {
                w.step(-1);
            }
            let mut shown = w.selected + 1;
            let slider = egui::Slider::new(&mut shown, 1..=n.max(1))
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 });
            if ui
                .add_sized([160.0, 18.0], slider)
                .on_hover_text("Replay the frame up to this event")
                .changed()
            {
                w.selected = shown - 1;
                w.scroll_to_selected = true;
            }
            if ui.button("▶").on_hover_text("Next event (→)").clicked() {
                w.step(1);
            }
            ui.label(
                RichText::new(format!("{} of {}", (w.selected + 1).min(n), n))
                    .monospace()
                    .color(theme::TEXT_DIM),
            );
        });
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut s.frame_debugger.filter, 140.0);
            if let Some(c) = s.frame_debugger.capture.as_ref().filter(|_| enabled) {
                ui.label(
                    RichText::new(format!(
                        "{} draw calls · {}×{}",
                        c.draw_calls(),
                        c.size.0,
                        c.size.1
                    ))
                    .color(theme::TEXT_DIM),
                );
            }
        });
    });
    if toggle {
        set_enabled(s, !enabled);
    }
}
