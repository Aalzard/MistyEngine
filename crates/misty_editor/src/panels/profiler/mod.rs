//! Window ▸ Analysis ▸ Profiler: record toggle, module charts of the last frames (CPU Usage
//! stacked by category, GPU Usage stacked by pass group, Rendering, Memory, Physics, Audio,
//! UI), and details of the selected frame (CPU Hierarchy / Timeline with a row per thread, GPU
//! Hierarchy, module counters).
//!
//! The frames come from the editor itself, a capture file (Load) or a Player connected over
//! the network (Target ▸ Player). Recording stops while the window isn't shown and starts again
//! when it is.

mod chart;
mod details;
#[cfg(test)]
mod tests;

use std::collections::VecDeque;
use std::path::PathBuf;

use egui::{Color32, Ui};
use misty_engine::profiler::{self, Category, FrameData, GpuGroup, remote};

use crate::state::EditorState;
use crate::widgets::{panel_toolbar, toggle_button};

/// Chart modules; the selected one decides the details pane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Module {
    #[default]
    Cpu,
    Gpu,
    Rendering,
    Memory,
    Physics,
    Audio,
    Ui,
}

impl Module {
    pub const ALL: [Self; 7] = [
        Self::Cpu,
        Self::Gpu,
        Self::Rendering,
        Self::Memory,
        Self::Physics,
        Self::Audio,
        Self::Ui,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Cpu => "CPU Usage",
            Self::Gpu => "GPU Usage",
            Self::Rendering => "Rendering",
            Self::Memory => "Memory",
            Self::Physics => "Physics",
            Self::Audio => "Audio",
            Self::Ui => "UI",
        }
    }

    /// By label or a short name ("GPU", "gpu usage").
    pub fn named(name: &str) -> Option<Self> {
        let n = name.trim().to_ascii_lowercase();
        Self::ALL.into_iter().find(|m| {
            let l = m.label().to_ascii_lowercase();
            l == n || l.split(' ').next() == Some(n.as_str())
        })
    }
}

/// CPU Usage details: merged call tree or per-sample bars.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum CpuView {
    #[default]
    Hierarchy,
    Timeline,
}

/// Where the frames come from.
#[derive(Default)]
pub enum Source {
    /// This editor (Play mode profiles the game).
    #[default]
    Editor,
    /// A capture file loaded from disk.
    File {
        path: PathBuf,
        frames: VecDeque<FrameData>,
    },
    /// A Player streaming its frames (Target ▸ Player).
    Player(remote::Listener),
}

impl Source {
    pub fn with_frames<R>(&self, f: impl FnOnce(&VecDeque<FrameData>) -> R) -> R {
        match self {
            Self::Editor => profiler::with_frames(f),
            Self::File { frames, .. } => f(frames),
            Self::Player(l) => l.with_frames(f),
        }
    }

    pub fn is_editor(&self) -> bool {
        matches!(self, Self::Editor)
    }
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
    /// GPU groups hidden from the GPU chart.
    pub hidden_gpu: [bool; GpuGroup::COUNT],
    pub source: Source,
    /// Last Load / Save problem (shown in the toolbar).
    pub message: Option<String>,
    /// The window has been shown (recording starts then, like the reference editor).
    opened: bool,
    /// UI pass the window was last drawn in.
    seen_pass: Option<u64>,
    /// Recording was stopped because the window went out of sight (resumed when shown).
    paused_hidden: bool,
}

impl ProfilerWindow {
    /// Show a capture file instead of the live frames.
    pub fn load(&mut self, path: PathBuf) -> std::io::Result<()> {
        let frames = profiler::capture::load(&path)?;
        self.selected = None;
        self.source = Source::File {
            path,
            frames: frames.into(),
        };
        set_live(false);
        Ok(())
    }

    /// Write the frames shown to `path`.
    pub fn save(&self, path: &std::path::Path) -> std::io::Result<usize> {
        self.source.with_frames(|frames| {
            profiler::capture::save(path, frames.iter()).map(|_| frames.len())
        })
    }

    /// Listen for a Player (Target ▸ Player).
    pub fn connect_player(&mut self, addr: &str) -> std::io::Result<()> {
        let listener = remote::Listener::bind(addr)?;
        self.selected = None;
        self.source = Source::Player(listener);
        set_live(false);
        Ok(())
    }
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

/// Chart colour of a GPU pass group.
pub fn gpu_color(g: GpuGroup) -> Color32 {
    match g {
        GpuGroup::Shadows => Color32::from_rgb(120, 110, 200),
        GpuGroup::Depth => Color32::from_rgb(90, 150, 210),
        GpuGroup::Opaque => Color32::from_rgb(110, 190, 80),
        GpuGroup::Vegetation => Color32::from_rgb(60, 160, 110),
        GpuGroup::Transparent => Color32::from_rgb(80, 200, 210),
        GpuGroup::Lighting => Color32::from_rgb(235, 180, 70),
        GpuGroup::PostProcess => Color32::from_rgb(220, 110, 150),
        GpuGroup::Other => Color32::from_rgb(120, 120, 120),
    }
}

/// Record the editor's own frames (off while a file or a Player is shown; environment captures
/// keep recording).
fn set_live(on: bool) {
    if on || !profiler::capture::is_active() {
        profiler::set_enabled(on);
    }
}

/// Stop recording while the window is out of sight (closed or behind another tab) so a closed
/// Profiler costs nothing; start again when it is shown. Captures started from the
/// environment (`MISTY_PROFILE`) keep recording.
pub fn sync(s: &mut EditorState, ctx: &egui::Context) {
    let w = &mut s.profiler;
    let visible = w
        .seen_pass
        .is_some_and(|p| p + 1 >= ctx.cumulative_pass_nr());
    if !visible && profiler::enabled() && !profiler::capture::is_active() {
        profiler::set_enabled(false);
        w.paused_hidden = true;
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    let w = &mut s.profiler;
    w.seen_pass = Some(ui.ctx().cumulative_pass_nr());
    if !w.opened || std::mem::take(&mut w.paused_hidden) {
        w.opened = true;
        set_live(w.source.is_editor());
    }
    toolbar(ui, w);
    // Chart summaries of every frame; only the selected frame's samples are copied.
    let frames: Vec<chart::Summary> = w
        .source
        .with_frames(|f| f.iter().map(chart::Summary::of).collect());
    // A selection that scrolled out of the history follows the latest frame again.
    if w.selected
        .is_some_and(|i| !frames.iter().any(|f| f.index == i))
    {
        w.selected = None;
    }
    if frames.is_empty() {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.weak(match &w.source {
                Source::Editor if profiler::enabled() => {
                    "Recording… frames appear here.".to_owned()
                }
                Source::Editor => {
                    "Press Record to capture frames (enter Play mode to profile the game)."
                        .to_owned()
                }
                Source::File { .. } => "The capture has no frames.".to_owned(),
                Source::Player(l) => format!(
                    "Waiting for a Player on {}. Start it with {}={}",
                    l.addr,
                    remote::ENV,
                    l.addr
                ),
            });
        });
        return;
    }
    let chart_h = (ui.available_height() * 0.55).clamp(240.0, 440.0);
    ui.allocate_ui(egui::vec2(ui.available_width(), chart_h), |ui| {
        chart::modules(ui, w, &frames);
    });
    ui.separator();
    let selected = w.selected;
    let frame: Option<FrameData> = w.source.with_frames(|f| match selected {
        Some(i) => f.iter().find(|f| f.index == i).cloned(),
        None => f.back().cloned(),
    });
    if let Some(frame) = frame {
        details::show(ui, w, &frame);
    }
}

fn toolbar(ui: &mut Ui, w: &mut ProfilerWindow) {
    panel_toolbar(ui, |ui| {
        let live = w.source.is_editor();
        ui.add_enabled_ui(live, |ui| {
            let recording = profiler::enabled();
            let label = egui::RichText::new("⏺ Record").color(if recording {
                Color32::from_rgb(230, 70, 60)
            } else {
                Color32::from_gray(200)
            });
            if toggle_button(ui, recording, label, "Record profiling information").clicked() {
                profiler::set_enabled(!recording);
            }
        });
        if ui
            .button("Clear")
            .on_hover_text("Forget recorded frames")
            .clicked()
        {
            match &mut w.source {
                Source::Editor => profiler::clear(),
                Source::File { frames, .. } => frames.clear(),
                Source::Player(l) => l.clear(),
            }
            w.selected = None;
        }
        target_menu(ui, w);
        ui.separator();
        let (first, last) = w
            .source
            .with_frames(|f| (f.front().map(|f| f.index), f.back().map(|f| f.index)));
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if ui
                .button("Load…")
                .on_hover_text("Open a profiler capture (.mprof)")
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Profiler capture", &[profiler::capture::EXTENSION, "jsonl"])
                    .pick_file()
            {
                w.message = w.load(path).err().map(|e| format!("Load failed: {e}"));
            }
            if ui
                .button("Save…")
                .on_hover_text("Save the frames shown to a capture file")
                .clicked()
                && let Some(path) = rfd::FileDialog::new()
                    .add_filter("Profiler capture", &[profiler::capture::EXTENSION])
                    .set_file_name(format!("capture.{}", profiler::capture::EXTENSION))
                    .save_file()
            {
                w.message = Some(match w.save(&path) {
                    Ok(n) => format!("Saved {n} frames"),
                    Err(e) => format!("Save failed: {e}"),
                });
            }
            if let Some(m) = &w.message {
                ui.weak(m);
            }
        });
    });
}

/// Target ▸ Editor / Player / the loaded file.
fn target_menu(ui: &mut Ui, w: &mut ProfilerWindow) {
    let current = match &w.source {
        Source::Editor => "Editor".to_owned(),
        Source::File { path, .. } => path
            .file_name()
            .map_or("Capture".to_owned(), |n| n.to_string_lossy().into_owned()),
        Source::Player(l) => match l.peer() {
            Some(p) => format!("Player ({p})"),
            None => "Player (waiting…)".to_owned(),
        },
    };
    let mut pick: Option<u8> = None;
    egui::ComboBox::from_id_salt("profiler_target")
        .selected_text(current)
        .width(130.0)
        .show_ui(ui, |ui| {
            if ui
                .selectable_label(w.source.is_editor(), "Editor")
                .clicked()
            {
                pick = Some(0);
            }
            let player = matches!(w.source, Source::Player(_));
            if ui
                .selectable_label(player, format!("Player ({})", remote::DEFAULT_ADDR))
                .on_hover_text(format!(
                    "Listen for a Player started with {}={}",
                    remote::ENV,
                    remote::DEFAULT_ADDR
                ))
                .clicked()
            {
                pick = Some(1);
            }
        });
    match pick {
        Some(0) if !w.source.is_editor() => {
            w.source = Source::Editor;
            w.selected = None;
            set_live(true);
        }
        Some(1) if !matches!(w.source, Source::Player(_)) => {
            w.message = w
                .connect_player(remote::DEFAULT_ADDR)
                .err()
                .map(|e| format!("Can't listen on {}: {e}", remote::DEFAULT_ADDR));
        }
        _ => {}
    }
}
