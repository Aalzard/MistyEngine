//! Window ▸ Analysis ▸ Frame Debugger: Enable pauses the Game view on one frame and lists its
//! render events grouped by pass; selecting an event replays the frame up to it (the Game view
//! shows that event's render target, or the depth / normals / GI / emission view picked in the
//! toolbar) and the right pane shows its details (shader, bindings, state, textures). The
//! slider steps through every event or, with Draws only, through the draw calls; GPU-driven
//! keeps GPU culling on so indirect draws are listed as they are issued.
//!
//! The Game view installs the renderer's capture each frame while the window is enabled and
//! visible (see [`FrameDebuggerWindow::request`]); with the window closed nothing is recorded.

mod details;
#[cfg(test)]
mod tests;
pub mod tree;

use std::collections::HashSet;

use egui::{Color32, RichText, Ui};
use misty_engine::render::{CaptureRequest, DrawLabel, FrameCapture, TargetView};

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
    /// The buffer the Game view shows.
    pub view: TargetView,
    /// Capture with GPU culling on (indirect draws as events).
    pub gpu_driven: bool,
    /// The slider and arrow keys step through draw calls only.
    pub draws_only: bool,
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
            view: self.view,
            gpu_driven: self.gpu_driven,
        })
    }

    /// Store a new capture; the first one selects its last event (the whole frame).
    pub fn set_capture(&mut self, capture: FrameCapture) {
        let n = capture.events.len();
        if self.capture.is_none() {
            self.selected = n.saturating_sub(1);
            self.scroll_to_selected = true;
        }
        // The screenshot hook's event: the first capture that has it (grass and other
        // streamed work may only show up a few frames in).
        if let Some(want) = self.pending_select.as_deref() {
            let found = match want.parse::<usize>() {
                Ok(i) => Some(i),
                Err(_) => capture.events.iter().position(|e| e.name.contains(want)),
            };
            if let Some(i) = found {
                self.selected = i;
                self.scroll_to_selected = true;
                self.pending_select = None;
            }
        }
        self.selected = self.selected.min(n.saturating_sub(1));
        self.capture = Some(capture);
    }

    pub fn event_count(&self) -> usize {
        self.capture.as_ref().map_or(0, |c| c.events.len())
    }

    /// Move the selection by `delta` events, or draw calls with Draws only (clamped).
    pub fn step(&mut self, delta: i64) {
        let n = self.event_count();
        if n == 0 {
            return;
        }
        if self.draws_only {
            let draws = self.draw_events();
            if draws.is_empty() {
                return;
            }
            // Position among the draws: the selected one, or where it would be.
            let at = draws.partition_point(|&i| i < self.selected) as i64;
            let on_draw = draws.get(at as usize) == Some(&self.selected);
            let base = if on_draw || delta < 0 { at } else { at - 1 };
            let to = (base + delta).clamp(0, draws.len() as i64 - 1) as usize;
            self.selected = draws[to];
        } else {
            self.selected = (self.selected as i64 + delta).clamp(0, n as i64 - 1) as usize;
        }
        self.scroll_to_selected = true;
    }

    /// Indices of the draw-call events (geometry, indirect and fullscreen draws).
    pub fn draw_events(&self) -> Vec<usize> {
        self.capture.as_ref().map_or(Vec::new(), |c| {
            (0..c.events.len())
                .filter(|&i| c.events[i].is_draw())
                .collect()
        })
    }

    /// Pick another view or GPU-driven mode: the next capture starts over (the event list
    /// changes).
    pub fn set_mode(&mut self, view: TargetView, gpu_driven: bool) {
        if (view, gpu_driven) != (self.view, self.gpu_driven) {
            self.view = view;
            self.gpu_driven = gpu_driven;
            self.capture = None;
        }
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
            // Event space, or draw-call space with Draws only.
            let draws = w.draw_events();
            let by_draw = w.draws_only && !draws.is_empty();
            let (count, mut shown) = match by_draw {
                true => (
                    draws.len(),
                    draws
                        .partition_point(|&i| i < w.selected)
                        .min(draws.len() - 1)
                        + 1,
                ),
                false => (n, w.selected + 1),
            };
            let slider = egui::Slider::new(&mut shown, 1..=count.max(1))
                .show_value(false)
                .handle_shape(egui::style::HandleShape::Rect { aspect_ratio: 0.5 });
            let tip = match by_draw {
                true => "Replay the frame up to this draw call",
                false => "Replay the frame up to this event",
            };
            if ui
                .add_sized([160.0, 18.0], slider)
                .on_hover_text(tip)
                .changed()
            {
                w.selected = match by_draw {
                    true => draws[shown - 1],
                    false => shown - 1,
                };
                w.scroll_to_selected = true;
            }
            if ui.button("▶").on_hover_text("Next event (→)").clicked() {
                w.step(1);
            }
            let text = match by_draw {
                true => format!("draw {shown} of {count}"),
                false => format!("{} of {}", (w.selected + 1).min(n), n),
            };
            ui.label(RichText::new(text).monospace().color(theme::TEXT_DIM));
            if toggle_button(
                ui,
                w.draws_only,
                "Draws only",
                "Step through draw calls only (skip clears, copies and compute)",
            )
            .clicked()
            {
                w.draws_only = !w.draws_only;
            }
        });
        ui.separator();
        let (mut view, mut gpu_driven) = (w.view, w.gpu_driven);
        egui::ComboBox::from_id_salt("frame_debugger_view")
            .selected_text(format!("View: {}", view.label()))
            .width(110.0)
            .show_ui(ui, |ui| {
                for v in TargetView::ALL {
                    ui.selectable_value(&mut view, v, v.label());
                }
            })
            .response
            .on_hover_text(
                "What the Game view shows: the event's target, depth, normals, GI or emission",
            );
        if toggle_button(
            ui,
            gpu_driven,
            "GPU-driven",
            "Keep GPU culling on: opaque meshes draw indirectly, one event per multi-draw",
        )
        .clicked()
        {
            gpu_driven = !gpu_driven;
        }
        w.set_mode(view, gpu_driven);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            search_field(ui, &mut s.frame_debugger.filter, 140.0);
            if let Some(c) = s.frame_debugger.capture.as_ref().filter(|_| enabled) {
                if !c.view_available {
                    ui.label(
                        RichText::new(format!("{} not in this frame", c.view.label()))
                            .color(Color32::from_rgb(230, 170, 80)),
                    )
                    .on_hover_text(
                        "GI needs Misty GI on; Depth and Normals need a view the prepass fits",
                    );
                }
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
