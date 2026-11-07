//! Details of the selected frame: the CPU Hierarchy (name, total, self, calls, ms) or
//! Timeline, and the Rendering / Memory counters.

use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};
use misty_engine::profiler::{FrameData, HierarchyTree};

use super::{CpuView, Module, ProfilerWindow, category_color};

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
            .gpu_ms
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
        Module::Rendering => counters(ui, "rendering", &rendering_rows(frame)),
        Module::Memory => counters(ui, "memory", &memory_rows(frame)),
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

fn counters(ui: &mut Ui, id: &str, rows: &[(&'static str, String)]) {
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
                            let arrow = match (n.children.is_empty(), open) {
                                (true, _) => "  ",
                                (false, true) => "▼",
                                (false, false) => "▶",
                            };
                            if ui
                                .add(egui::Label::new(arrow).sense(Sense::click()))
                                .clicked()
                                && !n.children.is_empty()
                            {
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

/// Samples as bars: x = time in the frame, rows = nesting depth.
fn timeline(ui: &mut Ui, frame: &FrameData) {
    let span = frame
        .samples
        .iter()
        .map(|s| s.start_ms + s.duration_ms)
        .fold(frame.cpu_ms, f32::max)
        .max(0.01);
    let depth = frame.samples.iter().map(|s| s.depth).max().unwrap_or(0) as f32 + 1.0;
    let row_h = 18.0;
    let area = ui.available_rect_before_wrap();
    let height = (depth * row_h + 20.0).max(area.height());
    let (rect, response) = ui.allocate_exact_size(vec2(area.width(), height), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 0.0, Color32::from_gray(30));
    let x = |ms: f32| rect.left() + ms / span * rect.width();
    // Millisecond ticks.
    let step = if span > 20.0 { 5.0 } else { 1.0 };
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
    let mut hovered = None;
    for s in &frame.samples {
        let top = rect.top() + 14.0 + s.depth as f32 * row_h;
        let bar = Rect::from_min_max(
            pos2(x(s.start_ms), top),
            pos2(
                x(s.start_ms + s.duration_ms).max(x(s.start_ms) + 1.0),
                top + row_h - 2.0,
            ),
        );
        painter.rect_filled(bar, 1.0, category_color(s.category).gamma_multiply(0.85));
        if bar.width() > 30.0 {
            painter.with_clip_rect(bar).text(
                bar.left_center() + vec2(3.0, 0.0),
                Align2::LEFT_CENTER,
                s.name.as_ref(),
                FontId::proportional(10.0),
                Color32::BLACK,
            );
        }
        if response.hover_pos().is_some_and(|p| bar.contains(p)) {
            hovered = Some(s);
        }
    }
    if let Some(s) = hovered {
        response.on_hover_text(format!("{}\n{:.3} ms", s.name, s.duration_ms));
    }
}
