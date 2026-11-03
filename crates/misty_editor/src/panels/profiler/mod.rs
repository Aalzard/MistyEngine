//! Window ▸ Analysis ▸ Profiler: record toggle, module charts of the last frames (CPU Usage
//! stacked by category, Rendering, Memory), and details of the selected frame.

mod chart;
mod details;
#[cfg(test)]
mod tests;

use egui::{Color32, Ui};
use misty_engine::profiler::{self, Category, FrameData};

use crate::state::EditorState;
use crate::widgets::{panel_toolbar, toggle_button};

/// Chart modules; the selected one decides the details pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Module {
    #[default]
    Cpu,
    Rendering,
    Memory,
}

/// CPU Usage details: merged call tree or per-sample bars.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuView {
    #[default]
    Hierarchy,
    Timeline,
}

#[derive(Default)]
pub struct ProfilerWindow {
    /// Selected frame (profiler index); `None` follows the latest one.
    pub selected: Option<u64>,
    pub module: Module,
    pub cpu_view: CpuView,
    /// Hierarchy rows the user expanded, by path ("Render/Render.Culling").
    pub expanded: std::collections::HashSet<String>,
    /// Categories hidden from the CPU chart (legend toggles).
    pub hidden: [bool; Category::COUNT],
    /// The window has been shown (recording starts then, like the reference editor).
    opened: bool,
}

/// Chart colour of a category.
pub fn category_color(c: Category) -> Color32 {
    match c {
        Category::Rendering => Color32::from_rgb(110, 190, 80),
        Category::Scripts => Color32::from_rgb(70, 140, 230),
        Category::Physics => Color32::from_rgb(240, 150, 50),
        Category::Animation => Color32::from_rgb(60, 190, 180),
        Category::Navigation => Color32::from_rgb(170, 110, 220),
        Category::Audio => Color32::from_rgb(230, 210, 70),
        Category::Ui => Color32::from_rgb(230, 100, 150),
        Category::Others => Color32::from_rgb(120, 120, 120),
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let w = &mut s.profiler;
    if !w.opened {
        w.opened = true;
        profiler::set_enabled(true);
    }
    toolbar(ui, w);
    // Chart summaries of every frame; only the selected frame's samples are copied.
    let frames: Vec<chart::Summary> =
        profiler::with_frames(|f| f.iter().map(chart::Summary::of).collect());
    // A selection that scrolled out of the history follows the latest frame again.
    if w.selected
        .is_some_and(|i| !frames.iter().any(|f| f.index == i))
    {
        w.selected = None;
    }
    if frames.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.weak(if profiler::enabled() {
                "Recording… frames appear here."
            } else {
                "Press Record to capture frames (enter Play mode to profile the game)."
            });
        });
        return;
    }
    let chart_h = (ui.available_height() * 0.5).clamp(170.0, 320.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), chart_h), |ui| {
        chart::modules(ui, w, &frames);
    });
    ui.separator();
    let selected = w.selected;
    let frame: Option<FrameData> = profiler::with_frames(|f| match selected {
        Some(i) => f.iter().find(|f| f.index == i).cloned(),
        None => f.back().cloned(),
    });
    if let Some(frame) = frame {
        details::show(ui, w, &frame);
    }
}

fn toolbar(ui: &mut Ui, w: &mut ProfilerWindow) {
    panel_toolbar(ui, |ui| {
        let recording = profiler::enabled();
        let label = egui::RichText::new("⏺ Record").color(if recording {
            Color32::from_rgb(230, 70, 60)
        } else {
            Color32::from_gray(200)
        });
        if toggle_button(ui, recording, label, "Record profiling information").clicked() {
            profiler::set_enabled(!recording);
        }
        if ui
            .button("Clear")
            .on_hover_text("Forget recorded frames")
            .clicked()
        {
            profiler::clear();
            w.selected = None;
        }
        ui.separator();
        let (first, last) =
            profiler::with_frames(|f| (f.front().map(|f| f.index), f.back().map(|f| f.index)));
        let current = w.selected.or(last);
        if ui.button("◀").on_hover_text("Previous frame").clicked()
            && let (Some(c), Some(first)) = (current, first)
        {
            w.selected = Some(c.saturating_sub(1).max(first));
        }
        if ui.button("▶").on_hover_text("Next frame").clicked()
            && let (Some(c), Some(last)) = (current, last)
        {
            w.selected = (c + 1 < last).then_some(c + 1);
        }
        if toggle_button(
            ui,
            w.selected.is_none(),
            "Current",
            "Follow the latest frame",
        )
        .clicked()
        {
            w.selected = None;
        }
        if let Some(c) = current {
            ui.label(format!("Frame: {c}"));
        }
    });
}
