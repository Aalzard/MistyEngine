//! Details of the selected frame: the CPU Hierarchy (name, total, self, calls, ms) or
//! Timeline (a row per thread and the GPU's passes), the GPU Hierarchy (views and their
//! passes), and the Rendering / Memory / Physics / Audio / UI counters.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::profiler::{
    Counter, CounterGroup, FrameData, GpuGroup, GpuPass, GpuTiming, HierarchyTree, Sample,
    ThreadKind,
};

use super::{CpuView, Module, ProfilerWindow, category_color, gpu_color};

pub fn show(ui: &mut Ui, w: &mut ProfilerWindow, frame: &FrameData) {
    ui.horizontal(|ui| {
        if w.module == Module::Cpu {
            egui::ComboBox::from_id_salt("profiler_cpu_view")
                .selected_text(match w.cpu_view {
                    CpuView::Hierarchy => "Hierarchy",
                    CpuView::Timeline => "Timeline",
                })
                .width(90.0)
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut w.cpu_view, CpuView::Hierarchy, "Hierarchy");
                    ui.selectable_value(&mut w.cpu_view, CpuView::Timeline, "Timeline");
                });
        }
        let gpu = frame
            .gpu_total_ms()
            .map_or("GPU: n/a".to_owned(), |g| format!("GPU: {g:.2}ms"));
        ui.label(format!(
            "Frame {}   CPU: {:.2}ms   {gpu}",
            frame.index, frame.cpu_ms
        ));
    });
    match w.module {
        Module::Cpu => match w.cpu_view {
            CpuView::Hierarchy => hierarchy(ui, w, frame),
            CpuView::Timeline => timeline(ui, frame),
        },
        Module::Gpu => gpu_hierarchy(ui, w, frame),
        Module::Rendering => counters(ui, "rendering", &rendering_rows(frame)),
        Module::Memory => counters(ui, "memory", &memory_rows(frame)),
        Module::Physics => counters(ui, "physics", &module_rows(frame, CounterGroup::Physics)),
        Module::Audio => counters(ui, "audio", &module_rows(frame, CounterGroup::Audio)),
        Module::Ui => counters(ui, "ui", &module_rows(frame, CounterGroup::Ui)),
    }
}

pub fn rendering_rows(f: &FrameData) -> Vec<(&'static str, String)> {
    let r = &f.render;
    vec![
        ("Batches (draw calls)", r.batches.to_string()),
        ("Saved by batching", r.saved_by_batching.to_string()),
        ("SetPass calls", r.set_pass.to_string()),
        ("Triangles", r.triangles.to_string()),
        ("Vertices", r.vertices.to_string()),
        ("Material texture binds", r.texture_binds.to_string()),
        ("Shadow casters", r.shadow_casters.to_string()),
        ("Visible meshes", r.visible.to_string()),
        ("Culled meshes", r.culled.to_string()),
        ("Culled shadow draws", r.shadow_culled.to_string()),
        ("Virtual geometry objects", r.vgeo_instances.to_string()),
        ("Virtual geometry clusters", r.vgeo_clusters.to_string()),
        ("Virtual geometry triangles", r.vgeo_triangles.to_string()),
        (
            "Virtual geometry shadow triangles",
            r.vgeo_shadow_triangles.to_string(),
        ),
        ("Render CPU", format!("{:.2} ms", r.cpu_ms)),
    ]
}

fn mb(bytes: u64) -> String {
    format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
}

pub fn memory_rows(f: &FrameData) -> Vec<(&'static str, String)> {
    let m = &f.memory;
    vec![
        ("GameObjects", m.entities.to_string()),
        ("Meshes", m.render.meshes.to_string()),
        ("Textures", m.render.textures.to_string()),
        ("GPU buffers (estimate)", mb(m.render.buffer_bytes)),
        ("GPU textures (estimate)", mb(m.render.texture_bytes)),
        (
            "Virtual geometry pool + lists",
            mb(m.render.virtual_geometry_bytes),
        ),
    ]
}

/// Times of a module (from its scopes) then its counters.
pub fn module_rows(f: &FrameData, group: CounterGroup) -> Vec<(&'static str, String)> {
    let ms = |v: f32| format!("{v:.3} ms");
    let mut rows: Vec<(&'static str, String)> = match group {
        CounterGroup::Physics => vec![
            ("Physics time", ms(f.time_in("Physics."))),
            ("  Simulation steps", ms(f.time_in("Physics.Step"))),
            ("  Sync transforms", ms(f.time_in("Physics.SyncTransforms"))),
            ("  Ragdolls and cloth", ms(f.time_in("Physics.Ragdoll"))),
        ],
        CounterGroup::Audio => vec![
            ("Main thread (Audio.Update)", ms(f.time_in("Audio."))),
            ("Audio thread mixing", ms(f.counter(Counter::AudioMixMs))),
        ],
        CounterGroup::Ui => vec![
            (
                "UI time",
                ms(f.category_ms(misty_engine::profiler::Category::Ui)),
            ),
            ("  Layout", ms(f.time_in("UI.Layout"))),
            ("  Render", ms(f.time_in("UI.Render"))),
            ("  Events", ms(f.time_in("UI.Events"))),
        ],
    };
    for c in Counter::of(group) {
        if c == Counter::AudioMixMs {
            continue;
        }
        let v = f.counter(c);
        let text = match c {
            Counter::AudioDspLoad => format!("{v:.2} %"),
            _ => format!("{}", v.round() as i64),
        };
        rows.push((c.label(), text));
    }
    rows
}

fn counters(ui: &mut Ui, id: &str, rows: &[(&'static str, String)]) {
    egui::ScrollArea::vertical()
        .id_salt(("profiler counters scroll", id))
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new(("profiler counters", id))
                .striped(true)
                .num_columns(2)
                .min_col_width(160.0)
                .show(ui, |ui| {
                    for (name, value) in rows {
                        ui.label(*name);
                        ui.monospace(value);
                        ui.end_row();
                    }
                });
        });
}

/// One visible Hierarchy row.
pub struct Row {
    pub node: usize,
    pub depth: usize,
    pub path: String,
}

/// Rows in display order: children of expanded nodes, longest first.
pub fn visible_rows(
    tree: &HierarchyTree,
    expanded: &std::collections::HashSet<String>,
) -> Vec<Row> {
    let mut out = Vec::new();
    let mut stack: Vec<(usize, usize, String)> = tree.nodes[0]
        .children
        .iter()
        .rev()
        .map(|&c| (c, 0, tree.nodes[c].name.clone()))
        .collect();
    while let Some((node, depth, path)) = stack.pop() {
        if expanded.contains(&path) {
            for &c in tree.nodes[node].children.iter().rev() {
                stack.push((c, depth + 1, format!("{path}/{}", tree.nodes[c].name)));
            }
        }
        out.push(Row { node, depth, path });
    }
    out
}

/// Expand / collapse arrow of a tree row.
fn arrow(ui: &mut Ui, has_children: bool, open: bool) -> bool {
    let text = match (has_children, open) {
        (false, _) => "  ",
        (true, true) => "▼",
        (true, false) => "▶",
    };
    ui.add(egui::Label::new(text).sense(Sense::click()))
        .clicked()
        && has_children
}

fn hierarchy(ui: &mut Ui, w: &mut ProfilerWindow, frame: &FrameData) {
    let tree = HierarchyTree::build(frame);
    let rows = visible_rows(&tree, &w.expanded);
    let frame_ms = frame.cpu_ms.max(1e-6);
    let header = ["Overview", "Total", "Self", "Calls", "Time ms", "Self ms"];
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("profiler hierarchy")
                .striped(true)
                .num_columns(header.len())
                .min_col_width(56.0)
                .show(ui, |ui| {
                    for h in header {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for row in &rows {
                        let n = &tree.nodes[row.node];
                        ui.horizontal(|ui| {
                            ui.add_space(row.depth as f32 * 14.0);
                            let open = w.expanded.contains(&row.path);
                            if arrow(ui, !n.children.is_empty(), open) {
                                if open {
                                    w.expanded.remove(&row.path);
                                } else {
                                    w.expanded.insert(row.path.clone());
                                }
                            }
                            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                            ui.painter()
                                .rect_filled(dot, 1.0, category_color(n.category));
                            ui.label(&n.name);
                        });
                        ui.monospace(format!("{:.1}%", n.total_ms / frame_ms * 100.0));
                        ui.monospace(format!("{:.1}%", n.self_ms / frame_ms * 100.0));
                        ui.monospace(n.calls.to_string());
                        ui.monospace(format!("{:.2}", n.total_ms));
                        ui.monospace(format!("{:.2}", n.self_ms));
                        ui.end_row();
                    }
                });
        });
}

/// The GPU passes of a frame per view, in frame order: (view, its passes).
pub fn gpu_views(frame: &FrameData) -> Vec<(String, Vec<&GpuPass>)> {
    let mut out: Vec<(String, Vec<&GpuPass>)> = Vec::new();
    for p in &frame.gpu_passes {
        // A new render of the same view starts again at "start" time 0.
        let new_render = out.last().is_none_or(|(v, passes)| {
            *v != p.view || p.start_ms < passes.last().map_or(0.0, |l| l.start_ms)
        });
        if new_render {
            out.push((p.view.to_string(), Vec::new()));
        }
        if let Some(last) = out.last_mut() {
            last.1.push(p);
        }
    }
    out
}

/// Views (expandable) and their passes: name, group, GPU ms and share of the frame's GPU time.
fn gpu_hierarchy(ui: &mut Ui, w: &mut ProfilerWindow, frame: &FrameData) {
    let views = gpu_views(frame);
    if views.is_empty() {
        ui.add_space(8.0);
        ui.weak(match frame.gpu_timing {
            GpuTiming::Passes => "No GPU timings for this frame (they arrive a frame or two late).",
            GpuTiming::FrameOnly => {
                "This device can't write timestamps between passes: only the whole render \
                 is timed (see the CPU Usage chart's GPU line)."
            }
            GpuTiming::Unsupported => "GPU profiling is not supported by this device.",
        });
        return;
    }
    let total = frame.gpu_total_ms().unwrap_or(0.0).max(1e-6);
    let header = ["Pass", "Group", "Total", "GPU ms"];
    egui::ScrollArea::vertical()
        .id_salt("profiler gpu hierarchy")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("profiler gpu grid")
                .striped(true)
                .num_columns(header.len())
                .min_col_width(60.0)
                .show(ui, |ui| {
                    for h in header {
                        ui.strong(h);
                    }
                    ui.end_row();
                    for (i, (view, passes)) in views.iter().enumerate() {
                        let key = format!("gpu/{i}/{view}");
                        let open = !w.expanded.contains(&key);
                        let view_ms: f32 = passes.iter().map(|p| p.ms).sum();
                        ui.horizontal(|ui| {
                            if arrow(ui, true, open) {
                                // Views start open; the set remembers collapsed ones.
                                if open {
                                    w.expanded.insert(key.clone());
                                } else {
                                    w.expanded.remove(&key);
                                }
                            }
                            ui.strong(format!("{view} view"));
                        });
                        ui.label("");
                        ui.monospace(format!("{:.1}%", view_ms / total * 100.0));
                        ui.monospace(format!("{view_ms:.3}"));
                        ui.end_row();
                        if !open {
                            continue;
                        }
                        for p in passes {
                            let g = p.group();
                            ui.horizontal(|ui| {
                                ui.add_space(22.0);
                                let (dot, _) =
                                    ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                                ui.painter().rect_filled(dot, 1.0, gpu_color(g));
                                ui.label(p.name.as_ref());
                            });
                            ui.weak(g.label());
                            ui.monospace(format!("{:.1}%", p.ms / total * 100.0));
                            ui.monospace(format!("{:.3}", p.ms));
                            ui.end_row();
                        }
                    }
                    // Totals per group.
                    ui.end_row();
                    let groups = frame.gpu_groups();
                    for g in GpuGroup::ALL {
                        let ms = groups[g.index()];
                        if ms <= 0.0 {
                            continue;
                        }
                        ui.horizontal(|ui| {
                            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                            ui.painter().rect_filled(dot, 1.0, gpu_color(g));
                            ui.label(format!("All {}", g.label()));
                        });
                        ui.label("");
                        ui.monospace(format!("{:.1}%", ms / total * 100.0));
                        ui.monospace(format!("{ms:.3}"));
                        ui.end_row();
                    }
                });
        });
}

/// One Timeline section: a label and samples drawn on `depth` rows.
pub struct Lane<'a> {
    pub name: String,
    pub kind: ThreadKind,
    pub samples: Vec<LaneBar<'a>>,
}

/// A bar of a lane.
pub struct LaneBar<'a> {
    pub name: &'a str,
    pub color: Color32,
    pub depth: u16,
    pub start_ms: f32,
    pub ms: f32,
}

impl Lane<'_> {
    pub fn rows(&self) -> u16 {
        self.samples
            .iter()
            .map(|s| s.depth)
            .max()
            .map_or(1, |d| d + 1)
    }
}

fn sample_bar(s: &Sample) -> LaneBar<'_> {
    LaneBar {
        name: s.name.as_ref(),
        color: category_color(s.category),
        depth: s.depth,
        // Began in an earlier frame: drawn from the frame's start.
        start_ms: s.start_ms.max(0.0),
        ms: s.duration_ms + s.start_ms.min(0.0),
    }
}

/// The Timeline's sections: Main Thread, the GPU's passes per view (placed after each
/// render's submit), then every other thread that recorded something.
pub fn lanes(frame: &FrameData) -> Vec<Lane<'_>> {
    let mut out = vec![Lane {
        name: ThreadKind::Main.label().to_owned(),
        kind: ThreadKind::Main,
        samples: frame.samples.iter().map(sample_bar).collect(),
    }];
    // The n-th render's GPU work starts when its commands were submitted (approximately).
    let submits: Vec<f32> = frame
        .samples
        .iter()
        .filter(|s| s.name == "Render.Submit")
        .map(|s| s.start_ms + s.duration_ms)
        .collect();
    for (i, (view, passes)) in gpu_views(frame).into_iter().enumerate() {
        let offset = submits.get(i).copied().unwrap_or(0.0);
        out.push(Lane {
            name: format!("{} · {view}", ThreadKind::Render.label()),
            kind: ThreadKind::Render,
            samples: passes
                .into_iter()
                .map(|p| LaneBar {
                    name: p.name.as_ref(),
                    color: gpu_color(p.group()),
                    depth: 0,
                    start_ms: offset + p.start_ms,
                    ms: p.ms,
                })
                .collect(),
        });
    }
    for t in &frame.threads {
        out.push(Lane {
            name: t.name.clone(),
            kind: t.kind,
            samples: t.samples.iter().map(sample_bar).collect(),
        });
    }
    out
}

/// Every thread as a section of bars: x = time in the frame, rows = nesting depth.
fn timeline(ui: &mut Ui, frame: &FrameData) {
    let lanes = lanes(frame);
    let span = lanes
        .iter()
        .flat_map(|l| &l.samples)
        .map(|s| s.start_ms + s.ms)
        .fold(frame.cpu_ms, f32::max)
        .max(0.01);
    let row_h = 16.0;
    let label_w = 120.0;
    let gap = 6.0;
    let content_h: f32 = lanes
        .iter()
        .map(|l| l.rows() as f32 * row_h + gap)
        .sum::<f32>()
        + 16.0;
    egui::ScrollArea::vertical()
        .id_salt("profiler timeline")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            let width = ui.available_width();
            let height = content_h.max(ui.available_height());
            let (rect, response) = ui.allocate_exact_size(vec2(width, height), Sense::hover());
            let painter = ui.painter_at(rect);
            painter.rect_filled(rect, 0.0, Color32::from_gray(30));
            let plot_left = rect.left() + label_w;
            let x = |ms: f32| plot_left + ms / span * (rect.right() - plot_left);
            // Millisecond ticks.
            let step = if span > 40.0 {
                10.0
            } else if span > 20.0 {
                5.0
            } else if span > 4.0 {
                1.0
            } else {
                0.5
            };
            let mut t = 0.0;
            while t <= span {
                painter.vline(
                    x(t),
                    rect.y_range(),
                    Stroke::new(1.0, Color32::from_white_alpha(18)),
                );
                painter.text(
                    pos2(x(t) + 2.0, rect.top()),
                    Align2::LEFT_TOP,
                    format!("{t}ms"),
                    FontId::proportional(9.0),
                    Color32::from_gray(140),
                );
                t += step;
            }
            let mut hovered: Option<(&str, f32, String)> = None;
            let mut top = rect.top() + 14.0;
            for lane in &lanes {
                let h = lane.rows() as f32 * row_h;
                let band = Rect::from_min_max(pos2(rect.left(), top), pos2(rect.right(), top + h));
                painter.rect_filled(
                    Rect::from_min_max(band.min, pos2(plot_left - 2.0, band.max.y)),
                    0.0,
                    Color32::from_gray(40),
                );
                painter.with_clip_rect(band).text(
                    band.left_top() + vec2(4.0, 2.0),
                    Align2::LEFT_TOP,
                    &lane.name,
                    FontId::proportional(10.5),
                    match lane.kind {
                        ThreadKind::Main => Color32::WHITE,
                        ThreadKind::Render => Color32::from_rgb(250, 220, 90),
                        _ => Color32::from_gray(185),
                    },
                );
                for s in &lane.samples {
                    let y0 = top + s.depth as f32 * row_h;
                    let bar = Rect::from_min_max(
                        pos2(x(s.start_ms), y0 + 1.0),
                        pos2(
                            x(s.start_ms + s.ms).max(x(s.start_ms) + 1.0),
                            y0 + row_h - 1.0,
                        ),
                    );
                    painter.rect_filled(bar, 1.0, s.color.gamma_multiply(0.85));
                    if bar.width() > 30.0 {
                        painter.with_clip_rect(bar).text(
                            bar.left_center() + vec2(3.0, 0.0),
                            Align2::LEFT_CENTER,
                            s.name,
                            FontId::proportional(9.5),
                            Color32::BLACK,
                        );
                    }
                    if response.hover_pos().is_some_and(|p| bar.contains(p)) {
                        hovered = Some((s.name, s.ms, lane.name.clone()));
                    }
                }
                painter.hline(
                    rect.x_range(),
                    top + h + gap * 0.5,
                    Stroke::new(1.0, Color32::from_gray(55)),
                );
                top += h + gap;
            }
            if let Some((name, ms, lane)) = hovered {
                response.on_hover_text(format!("{name}\n{ms:.3} ms\n{lane}"));
            }
        });
}
