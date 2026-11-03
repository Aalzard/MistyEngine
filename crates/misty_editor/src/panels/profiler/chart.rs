//! Module charts: CPU Usage stacked by category (with the GPU time as a line), Rendering
//! batches and Memory, newest frame on the right. Clicking a chart selects that frame.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::profiler::{Category, FrameData, HISTORY};
use misty_engine::render::RenderStats;

use super::{Module, ProfilerWindow, category_color};

/// What the charts need from a frame.
pub struct Summary {
    pub index: u64,
    /// Per category (Others includes untracked time).
    pub categories: [f32; Category::COUNT],
    pub gpu_ms: Option<f32>,
    pub render: RenderStats,
    pub memory_mb: f32,
    pub entities: usize,
}

impl Summary {
    pub fn of(f: &FrameData) -> Self {
        Self {
            index: f.index,
            categories: Category::ALL.map(|c| f.category_ms(c)),
            gpu_ms: f.gpu_ms,
            render: f.render,
            memory_mb: (f.memory.render.buffer_bytes + f.memory.render.texture_bytes) as f32
                / (1024.0 * 1024.0),
            entities: f.memory.entities,
        }
    }
}

const LEGEND_W: f32 = 170.0;

pub fn modules(ui: &mut Ui, w: &mut ProfilerWindow, frames: &[Summary]) {
    let total = ui.available_rect_before_wrap();
    ui.allocate_rect(total, Sense::hover());
    let cpu_h = total.height() * 0.6;
    let row_h = (total.height() - cpu_h) / 2.0;
    let rows = [
        (Module::Cpu, total.top(), cpu_h),
        (Module::Rendering, total.top() + cpu_h, row_h),
        (Module::Memory, total.top() + cpu_h + row_h, row_h),
    ];
    let selected = w.selected.or(frames.last().map(|f| f.index));
    for (module, top, h) in rows {
        let legend = Rect::from_min_size(pos2(total.left(), top), vec2(LEGEND_W, h));
        let plot = Rect::from_min_max(
            pos2(total.left() + LEGEND_W, top),
            pos2(total.right(), top + h),
        );
        let latest = frames.iter().find(|f| Some(f.index) == selected);
        module_legend(ui, w, module, legend, latest);
        let painter = ui.painter_at(plot);
        painter.rect_filled(plot.shrink(1.0), 0.0, Color32::from_gray(28));
        match module {
            Module::Cpu => cpu_chart(&painter, w, frames, plot),
            Module::Rendering => line_chart(&painter, frames, plot, |f| {
                (f.render.batches as f32, format!("{}", f.render.batches))
            }),
            Module::Memory => line_chart(&painter, frames, plot, |f| {
                (f.memory_mb, format!("{:.1} MB", f.memory_mb))
            }),
        }
        // Click or drag to pick a frame.
        let response = ui.interact(
            plot,
            ui.id().with(("profiler chart", module as u8)),
            Sense::click_and_drag(),
        );
        if let Some(p) = response.interact_pointer_pos()
            && (response.clicked() || response.dragged())
            && let Some(i) = frame_at(frames, plot, p)
        {
            w.selected = Some(i);
            w.module = module;
        }
        if let Some(i) = selected
            && let Some(x) = frame_x(frames, plot, i)
        {
            painter.vline(x, plot.y_range(), Stroke::new(1.0, Color32::WHITE));
        }
        let border = if w.module == module {
            Color32::from_gray(150)
        } else {
            Color32::from_gray(60)
        };
        ui.painter().rect_stroke(
            plot.shrink(1.0),
            0.0,
            Stroke::new(1.0, border),
            egui::StrokeKind::Inside,
        );
    }
}

/// Module name (click to show its details) and, for CPU Usage, the category legend.
fn module_legend(
    ui: &mut Ui,
    w: &mut ProfilerWindow,
    module: Module,
    rect: Rect,
    frame: Option<&Summary>,
) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 4.0))));
    let ui = &mut child;
    let title = match module {
        Module::Cpu => "CPU Usage",
        Module::Rendering => "Rendering",
        Module::Memory => "Memory",
    };
    if ui
        .selectable_label(w.module == module, egui::RichText::new(title).strong())
        .clicked()
    {
        w.module = module;
    }
    match module {
        Module::Cpu => {
            for c in Category::ALL {
                let ms = frame.map_or(0.0, |f| f.categories[c.index()]);
                let shown = !w.hidden[c.index()];
                ui.horizontal(|ui| {
                    ui.spacing_mut().item_spacing.x = 4.0;
                    let (r, resp) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::click());
                    let color = category_color(c);
                    if shown {
                        ui.painter().rect_filled(r, 1.0, color);
                    } else {
                        ui.painter().rect_stroke(
                            r,
                            1.0,
                            Stroke::new(1.0, color),
                            egui::StrokeKind::Inside,
                        );
                    }
                    let label = ui.add(
                        egui::Label::new(
                            egui::RichText::new(format!("{} {ms:.2}ms", c.label())).size(11.0),
                        )
                        .sense(Sense::click()),
                    );
                    if resp.clicked() || label.clicked() {
                        w.hidden[c.index()] = shown;
                    }
                });
            }
            if let Some(gpu) = frame.and_then(|f| f.gpu_ms) {
                ui.label(
                    egui::RichText::new(format!("— GPU {gpu:.2}ms"))
                        .size(11.0)
                        .color(GPU_COLOR),
                );
            }
        }
        Module::Rendering => {
            if let Some(f) = frame {
                ui.label(
                    egui::RichText::new(format!(
                        "Batches {}  SetPass {}",
                        f.render.batches, f.render.set_pass
                    ))
                    .size(11.0),
                );
            }
        }
        Module::Memory => {
            if let Some(f) = frame {
                ui.label(egui::RichText::new(format!("{} objects", f.entities)).size(11.0));
            }
        }
    }
}

const GPU_COLOR: Color32 = Color32::from_rgb(250, 220, 90);

/// X of a frame column (the newest at the right edge).
fn frame_x(frames: &[Summary], plot: Rect, index: u64) -> Option<f32> {
    let pos = frames.iter().position(|f| f.index == index)?;
    let col = plot.width() / HISTORY as f32;
    let from_right = frames.len() - 1 - pos;
    Some(plot.right() - (from_right as f32 + 0.5) * col)
}

fn frame_at(frames: &[Summary], plot: Rect, p: Pos2) -> Option<u64> {
    let col = plot.width() / HISTORY as f32;
    let from_right = ((plot.right() - p.x) / col).floor().max(0.0) as usize;
    let pos = frames.len().checked_sub(1 + from_right)?;
    Some(frames[pos].index)
}

/// A scale that fits `max`: 1, 2, 5 × 10ⁿ steps.
fn nice_max(max: f32) -> f32 {
    let max = max.max(1e-3);
    let p = 10f32.powf(max.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|v| *v >= max)
        .unwrap_or(10.0 * p)
}

fn cpu_chart(painter: &egui::Painter, w: &ProfilerWindow, frames: &[Summary], plot: Rect) {
    let shown = |c: Category| !w.hidden[c.index()];
    let stack = |f: &Summary| -> f32 {
        Category::ALL
            .iter()
            .filter(|c| shown(**c))
            .map(|c| f.categories[c.index()])
            .sum()
    };
    let peak = frames
        .iter()
        .map(|f| stack(f).max(f.gpu_ms.unwrap_or(0.0)))
        .fold(0.0, f32::max);
    // Always room for the 60 FPS line.
    let top = nice_max(peak.max(16.7) * 1.1);
    let y = |ms: f32| plot.bottom() - (ms / top).min(1.0) * plot.height();
    let col = plot.width() / HISTORY as f32;
    for (i, f) in frames.iter().enumerate() {
        let x1 = plot.right() - (frames.len() - 1 - i) as f32 * col;
        let x0 = x1 - col;
        let mut base = 0.0;
        // Stacked like the reference: Rendering at the bottom.
        for c in Category::ALL.into_iter().filter(|c| shown(*c)) {
            let ms = f.categories[c.index()];
            if ms <= 0.0 {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, y(base + ms)), pos2(x1 + 0.5, y(base)));
            painter.rect_filled(r, 0.0, category_color(c));
            base += ms;
        }
    }
    // GPU time as a line.
    let points: Vec<Pos2> = frames
        .iter()
        .enumerate()
        .filter_map(|(i, f)| {
            let x = plot.right() - (frames.len() - 1 - i) as f32 * col - col * 0.5;
            f.gpu_ms.map(|g| pos2(x, y(g)))
        })
        .collect();
    if points.len() > 1 {
        painter.add(egui::Shape::line(points, Stroke::new(1.5, GPU_COLOR)));
    }
    for (ms, label) in [
        (16.7, "16ms (60FPS)"),
        (33.3, "33ms (30FPS)"),
        (66.7, "66ms (15FPS)"),
    ] {
        if ms < top {
            let yy = y(ms);
            painter.hline(
                plot.x_range(),
                yy,
                Stroke::new(1.0, Color32::from_white_alpha(40)),
            );
            painter.text(
                pos2(plot.left() + 4.0, yy - 1.0),
                Align2::LEFT_BOTTOM,
                label,
                FontId::proportional(10.0),
                Color32::from_gray(170),
            );
        }
    }
}

/// One value per frame as a filled line, with the latest value at the top left.
fn line_chart(
    painter: &egui::Painter,
    frames: &[Summary],
    plot: Rect,
    value: impl Fn(&Summary) -> (f32, String),
) {
    let peak = frames.iter().map(|f| value(f).0).fold(0.0, f32::max);
    let top = nice_max(peak * 1.15);
    let col = plot.width() / HISTORY as f32;
    let color = Color32::from_rgb(110, 170, 230);
    for (i, f) in frames.iter().enumerate() {
        let x1 = plot.right() - (frames.len() - 1 - i) as f32 * col;
        let v = value(f).0;
        let r = Rect::from_min_max(
            pos2(x1 - col, plot.bottom() - v / top * plot.height()),
            pos2(x1 + 0.5, plot.bottom()),
        );
        painter.rect_filled(r, 0.0, color.gamma_multiply(0.6));
    }
    if let Some(last) = frames.last() {
        painter.text(
            plot.left_top() + vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            value(last).1,
            FontId::proportional(10.0),
            Color32::from_gray(190),
        );
    }
}
