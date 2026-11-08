//! Module charts, newest frame on the right: CPU Usage stacked by category (with the GPU time
//! as a line), GPU Usage stacked by pass group, then one line chart per module (Rendering
//! batches, Memory, Physics / Audio / UI time). Clicking a chart selects that frame.

use egui::{Align2, Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::profiler::{Category, Counter, FrameData, GpuGroup, GpuTiming, HISTORY};
use misty_engine::render::RenderStats;

use super::{Module, ProfilerWindow, category_color, gpu_color};

/// What the charts need from a frame.
pub struct Summary {
    pub index: u64,
    /// Per category (Others includes untracked time).
    pub categories: [f32; Category::COUNT],
    pub gpu_ms: Option<f32>,
    /// Per GPU pass group (all in Other without per-pass times).
    pub gpu_groups: [f32; GpuGroup::COUNT],
    pub gpu_timing: GpuTiming,
    pub render: RenderStats,
    pub memory_mb: f32,
    pub entities: usize,
    pub physics_ms: f32,
    pub audio_ms: f32,
    pub ui_ms: f32,
    pub bodies: f32,
    pub contacts: f32,
    pub voices: f32,
    pub dsp_load: f32,
    pub ui_elements: f32,
    pub ui_batches: f32,
}

impl Summary {
    pub fn of(f: &FrameData) -> Self {
        Self {
            index: f.index,
            categories: Category::ALL.map(|c| f.category_ms(c)),
            gpu_ms: f.gpu_total_ms(),
            gpu_groups: f.gpu_groups(),
            gpu_timing: f.gpu_timing,
            render: f.render,
            memory_mb: (f.memory.render.buffer_bytes + f.memory.render.texture_bytes) as f32
                / (1024.0 * 1024.0),
            entities: f.memory.entities,
            physics_ms: f.category_ms(Category::Physics),
            // Main thread (Audio.Update) and the audio thread's mixing.
            audio_ms: f.category_ms(Category::Audio) + f.counter(Counter::AudioMixMs),
            // The game's canvases (UI.* scopes), not the editor's panels.
            ui_ms: f.time_in("UI."),
            bodies: f.counter(Counter::PhysicsBodies),
            contacts: f.counter(Counter::PhysicsContacts),
            voices: f.counter(Counter::AudioPlayingSources),
            dsp_load: f.counter(Counter::AudioDspLoad),
            ui_elements: f.counter(Counter::UiElements),
            ui_batches: f.counter(Counter::UiBatches),
        }
    }
}

const LEGEND_W: f32 = 170.0;

/// Height of a module row: CPU and GPU have a legend of swatches, the others one line.
pub fn row_height(m: Module) -> f32 {
    match m {
        Module::Cpu => 96.0,
        Module::Gpu => 76.0,
        _ => 30.0,
    }
}

/// The frame "Current" shows: the newest one whose GPU times have arrived (they come a frame
/// or two late), else the newest.
pub fn current(frames: &[Summary]) -> Option<u64> {
    frames
        .iter()
        .rev()
        .take(6)
        .find(|f| f.gpu_ms.is_some())
        .or(frames.last())
        .map(|f| f.index)
}

/// Height of every module row together.
pub fn content_height() -> f32 {
    Module::ALL.iter().map(|m| row_height(*m)).sum()
}

pub fn modules(ui: &mut Ui, w: &mut ProfilerWindow, frames: &[Summary]) {
    let (total, _) =
        ui.allocate_exact_size(vec2(ui.available_width(), content_height()), Sense::hover());
    let selected = w.selected.or(current(frames));
    let shown = frames
        .iter()
        .find(|f| Some(f.index) == selected)
        .or(frames.last());
    let mut top = total.top();
    for module in Module::ALL {
        let h = row_height(module);
        if module == w.module && std::mem::take(&mut w.reveal_module) {
            let row = Rect::from_min_size(pos2(total.left(), top), vec2(total.width(), h));
            ui.scroll_to_rect(row, Some(egui::Align::Max));
        }
        let legend = Rect::from_min_size(pos2(total.left(), top), vec2(LEGEND_W, h));
        let plot = Rect::from_min_max(
            pos2(total.left() + LEGEND_W, top),
            pos2(total.right(), top + h),
        );
        top += h;
        let latest = frames.iter().find(|f| Some(f.index) == selected);
        module_legend(ui, w, module, legend, latest);
        let painter = ui.painter_at(plot);
        painter.rect_filled(plot.shrink(1.0), 0.0, Color32::from_gray(28));
        match module {
            Module::Cpu => cpu_chart(&painter, w, frames, plot),
            Module::Gpu => gpu_chart(&painter, w, frames, plot),
            Module::Rendering => line_chart(&painter, frames, shown, plot, |f| {
                (
                    f.render.batches as f32,
                    format!(
                        "{} batches   SetPass {}",
                        f.render.batches, f.render.set_pass
                    ),
                )
            }),
            Module::Memory => line_chart(&painter, frames, shown, plot, |f| {
                (
                    f.memory_mb,
                    format!("{:.1} MB   {} objects", f.memory_mb, f.entities),
                )
            }),
            Module::Physics => line_chart(&painter, frames, shown, plot, |f| {
                (
                    f.physics_ms,
                    format!(
                        "{:.2} ms   Bodies {}   Contacts {}",
                        f.physics_ms, f.bodies, f.contacts
                    ),
                )
            }),
            Module::Audio => line_chart(&painter, frames, shown, plot, |f| {
                (
                    f.audio_ms,
                    format!(
                        "{:.2} ms   Voices {}   DSP {:.1}%",
                        f.audio_ms, f.voices, f.dsp_load
                    ),
                )
            }),
            Module::Ui => line_chart(&painter, frames, shown, plot, |f| {
                (
                    f.ui_ms,
                    format!(
                        "{:.2} ms   Elements {}   Batches {}",
                        f.ui_ms, f.ui_elements, f.ui_batches
                    ),
                )
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

/// Two compact columns of colour swatches; click one to hide its series.
fn swatches(
    ui: &mut Ui,
    rect: Rect,
    top: f32,
    items: &[(String, Color32)],
    hidden: &mut [bool],
    id: &str,
) {
    let col_w = (rect.width() - 12.0) / 2.0;
    for (i, (label, color)) in items.iter().enumerate() {
        let shown = !hidden[i];
        let at = pos2(
            rect.left() + 6.0 + (i % 2) as f32 * col_w,
            top + (i / 2) as f32 * 13.0,
        );
        if at.y + 12.0 > rect.bottom() {
            break;
        }
        let item = Rect::from_min_size(at, vec2(col_w, 12.0));
        let swatch = Rect::from_min_size(at + vec2(0.0, 2.0), vec2(8.0, 8.0));
        if shown {
            ui.painter().rect_filled(swatch, 1.0, *color);
        } else {
            let stroke = Stroke::new(1.0, *color);
            ui.painter()
                .rect_stroke(swatch, 1.0, stroke, egui::StrokeKind::Inside);
        }
        ui.painter().with_clip_rect(item).text(
            at + vec2(11.0, 0.0),
            Align2::LEFT_TOP,
            label,
            FontId::proportional(9.5),
            Color32::from_gray(if shown { 210 } else { 120 }),
        );
        let rid = ui.id().with((id, i));
        if ui.interact(item, rid, Sense::click()).clicked() {
            hidden[i] = shown;
        }
    }
}

/// Module name (click to show its details) and its key values for the selected frame.
fn module_legend(
    ui: &mut Ui,
    w: &mut ProfilerWindow,
    module: Module,
    rect: Rect,
    frame: Option<&Summary>,
) {
    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect.shrink2(vec2(6.0, 2.0))));
    let ui = &mut child;
    if ui
        .selectable_label(
            w.module == module,
            egui::RichText::new(module.label()).strong().size(12.0),
        )
        .clicked()
    {
        w.module = module;
    }
    match module {
        Module::Cpu => {
            let top = ui.cursor().top();
            let items: Vec<(String, Color32)> = Category::ALL
                .iter()
                .map(|c| {
                    let ms = frame.map_or(0.0, |f| f.categories[c.index()]);
                    (format!("{} {ms:.1}", c.label()), category_color(*c))
                })
                .collect();
            swatches(ui, rect, top, &items, &mut w.hidden, "profiler legend");
            if let Some(gpu) = frame.and_then(|f| f.gpu_ms) {
                ui.painter().text(
                    pos2(rect.left() + 6.0, top + 4.0 * 13.0),
                    Align2::LEFT_TOP,
                    format!("GPU {gpu:.2}ms"),
                    FontId::proportional(10.0),
                    GPU_COLOR,
                );
            }
        }
        Module::Gpu => {
            let top = ui.cursor().top();
            let items: Vec<(String, Color32)> = GpuGroup::ALL
                .iter()
                .map(|g| {
                    let ms = frame.map_or(0.0, |f| f.gpu_groups[g.index()]);
                    let label = g.label().split([' ', '/']).next().unwrap_or("");
                    (format!("{label} {ms:.2}"), gpu_color(*g))
                })
                .collect();
            swatches(
                ui,
                rect,
                top,
                &items,
                &mut w.hidden_gpu,
                "profiler gpu legend",
            );
        }
        Module::Rendering | Module::Memory | Module::Physics | Module::Audio | Module::Ui => {}
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
pub fn nice_max(max: f32) -> f32 {
    let max = max.max(1e-3);
    let p = 10f32.powf(max.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * p)
        .find(|v| *v >= max)
        .unwrap_or(10.0 * p)
}

/// Stacked columns of `values` (one array per frame), shown series only, scaled to the 95th
/// percentile so one hitch doesn't flatten the rest. Returns the scale's top.
fn stacked(
    painter: &egui::Painter,
    frames: &[Summary],
    plot: Rect,
    values: impl Fn(&Summary) -> Vec<(f32, Color32)>,
    extra: impl Fn(&Summary) -> f32,
    min_top: f32,
) -> f32 {
    let mut totals: Vec<f32> = frames
        .iter()
        .map(|f| values(f).iter().map(|v| v.0).sum::<f32>().max(extra(f)))
        .collect();
    totals.sort_by(f32::total_cmp);
    let peak = totals[(totals.len() * 95 / 100).min(totals.len() - 1)];
    let top = nice_max(peak.max(min_top) * 1.1);
    let y = |ms: f32| plot.bottom() - (ms / top).min(1.0) * plot.height();
    let col = plot.width() / HISTORY as f32;
    for (i, f) in frames.iter().enumerate() {
        let x1 = plot.right() - (frames.len() - 1 - i) as f32 * col;
        let x0 = x1 - col;
        let mut base = 0.0;
        for (ms, color) in values(f) {
            if ms <= 0.0 {
                continue;
            }
            let r = Rect::from_min_max(pos2(x0, y(base + ms)), pos2(x1 + 0.5, y(base)));
            painter.rect_filled(r, 0.0, color);
            base += ms;
        }
    }
    top
}

/// Frame-rate guides that fit under `top`.
fn ms_guides(painter: &egui::Painter, plot: Rect, top: f32, guides: &[(f32, &str)]) {
    let y = |ms: f32| plot.bottom() - (ms / top).min(1.0) * plot.height();
    for (ms, label) in guides {
        if *ms < top {
            let yy = y(*ms);
            painter.hline(
                plot.x_range(),
                yy,
                Stroke::new(1.0, Color32::from_white_alpha(40)),
            );
            painter.text(
                pos2(plot.left() + 4.0, yy - 1.0),
                Align2::LEFT_BOTTOM,
                *label,
                FontId::proportional(10.0),
                Color32::from_gray(170),
            );
        }
    }
}

fn cpu_chart(painter: &egui::Painter, w: &ProfilerWindow, frames: &[Summary], plot: Rect) {
    let shown = |c: Category| !w.hidden[c.index()];
    // Stacked like the reference: Rendering at the bottom. Always room for the 60 FPS line.
    let top = stacked(
        painter,
        frames,
        plot,
        |f| {
            Category::ALL
                .into_iter()
                .filter(|c| shown(*c))
                .map(|c| (f.categories[c.index()], category_color(c)))
                .collect()
        },
        |f| f.gpu_ms.unwrap_or(0.0),
        16.7,
    );
    let y = |ms: f32| plot.bottom() - (ms / top).min(1.0) * plot.height();
    let col = plot.width() / HISTORY as f32;
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
    ms_guides(
        painter,
        plot,
        top,
        &[
            (16.7, "16ms (60FPS)"),
            (33.3, "33ms (30FPS)"),
            (66.7, "66ms (15FPS)"),
        ],
    );
}

fn gpu_chart(painter: &egui::Painter, w: &ProfilerWindow, frames: &[Summary], plot: Rect) {
    let timing = frames
        .last()
        .map_or(GpuTiming::Unsupported, |f| f.gpu_timing);
    if timing == GpuTiming::Unsupported && frames.iter().all(|f| f.gpu_ms.is_none()) {
        painter.text(
            plot.center(),
            Align2::CENTER_CENTER,
            "GPU profiling is not supported by this device (no timestamp queries)",
            FontId::proportional(11.0),
            Color32::from_gray(150),
        );
        return;
    }
    let shown = |g: GpuGroup| !w.hidden_gpu[g.index()];
    let top = stacked(
        painter,
        frames,
        plot,
        |f| {
            GpuGroup::ALL
                .into_iter()
                .filter(|g| shown(*g))
                .map(|g| (f.gpu_groups[g.index()], gpu_color(g)))
                .collect()
        },
        |_| 0.0,
        4.0,
    );
    ms_guides(
        painter,
        plot,
        top,
        &[(4.0, "4ms"), (8.3, "8ms (120FPS)"), (16.7, "16ms (60FPS)")],
    );
    if timing == GpuTiming::FrameOnly {
        painter.text(
            plot.right_top() + vec2(-4.0, 2.0),
            Align2::RIGHT_TOP,
            "per-pass timings unsupported: whole renders only",
            FontId::proportional(10.0),
            Color32::from_gray(160),
        );
    }
}

/// One value per frame as filled columns, with the selected frame's values at the top left.
fn line_chart(
    painter: &egui::Painter,
    frames: &[Summary],
    shown: Option<&Summary>,
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
    if let Some(f) = shown {
        painter.text(
            plot.left_top() + vec2(4.0, 2.0),
            Align2::LEFT_TOP,
            value(f).1,
            FontId::proportional(10.0),
            Color32::from_gray(190),
        );
    }
}
