//! Window ▸ Analysis ▸ Memory Profiler: Capture takes a snapshot of what the engine holds
//! (GPU textures, meshes, render targets and buffers, audio clips, the Lua heap, scene
//! components, particles, lightmaps) with sizes estimated from formats and dimensions. Views:
//! Summary (totals by category), Table (largest items, sortable), Tree Map, and Compare (the
//! difference between two snapshots).

#[cfg(test)]
mod tests;
pub mod treemap;

use egui::{Align2, Color32, FontId, RichText, Sense, Ui, vec2};
use misty_engine::memory_snapshot::{
    self, Change, MemCategory, MemItem, MemorySnapshot, format_bytes, format_delta,
};

use crate::state::EditorState;
use crate::theme;
use crate::viewport::Gpu;
use crate::widgets::{panel_toolbar, search_field, toggle_button};

pub const ICON: &str = "🧠";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum View {
    #[default]
    Summary,
    Table,
    TreeMap,
    Compare,
}

impl View {
    const ALL: [Self; 4] = [Self::Summary, Self::Table, Self::TreeMap, Self::Compare];

    fn label(self) -> &'static str {
        match self {
            Self::Summary => "Summary",
            Self::Table => "Table",
            Self::TreeMap => "Tree Map",
            Self::Compare => "Compare",
        }
    }
}

/// Table columns.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Column {
    Name,
    Type,
    Category,
    #[default]
    Size,
    Refs,
}

#[derive(Default)]
pub struct MemoryProfilerWindow {
    pub snapshots: Vec<MemorySnapshot>,
    /// Snapshot shown (index into `snapshots`).
    pub selected: usize,
    /// Older snapshot to compare `selected` with.
    pub baseline: Option<usize>,
    pub view: View,
    pub sort: Column,
    pub ascending: bool,
    pub filter: String,
    /// Snapshots still to take once the views have drawn (screenshot hook).
    pub pending_captures: u32,
}

impl MemoryProfilerWindow {
    /// Pick a view by its toolbar label ("Tree Map").
    pub fn set_view_named(&mut self, name: &str) {
        if let Some(v) = View::ALL.into_iter().find(|v| v.label() == name) {
            self.view = v;
        }
    }

    /// Add a snapshot and show it (the previous one becomes the comparison baseline).
    pub fn push(&mut self, snap: MemorySnapshot) {
        if !self.snapshots.is_empty() {
            self.baseline = Some(self.snapshots.len() - 1);
        }
        self.snapshots.push(snap);
        self.selected = self.snapshots.len() - 1;
    }
}

/// Chart colour of a category.
pub fn category_color(c: MemCategory) -> Color32 {
    match c {
        MemCategory::Textures => Color32::from_rgb(92, 160, 230),
        MemCategory::Meshes => Color32::from_rgb(110, 190, 120),
        MemCategory::RenderTargets => Color32::from_rgb(170, 120, 220),
        MemCategory::Buffers => Color32::from_rgb(120, 130, 150),
        MemCategory::Audio => Color32::from_rgb(230, 200, 80),
        MemCategory::Scripts => Color32::from_rgb(70, 190, 200),
        MemCategory::SceneObjects => Color32::from_rgb(230, 140, 70),
        MemCategory::Particles => Color32::from_rgb(230, 100, 150),
        MemCategory::Lightmaps => Color32::from_rgb(240, 225, 150),
    }
}

/// Everything the editor can account for right now.
pub fn capture(s: &EditorState, gpu: Option<&Gpu>, label: String) -> MemorySnapshot {
    let mut items: Vec<MemItem> = Vec::new();
    if let Some(gpu) = gpu {
        items.extend(gpu.renderer.memory_items());
        items.extend(gpu.view_target_items());
    }
    items.extend(memory_snapshot::audio_items(&s.scene));
    if let Some(rt) = &s.runtime {
        items.push(memory_snapshot::lua_item(rt.lua_memory()));
    }
    items.extend(memory_snapshot::scene_items(&s.scene));
    MemorySnapshot::new(label, items)
}

pub fn show(ui: &mut Ui, s: &mut EditorState, gpu: Option<&Gpu>) {
    let mut auto = false;
    if s.memory_profiler.pending_captures > 0 && ui.ctx().cumulative_pass_nr() > 20 {
        s.memory_profiler.pending_captures -= 1;
        auto = true;
        ui.ctx().request_repaint();
    }
    if toolbar(ui, &mut s.memory_profiler) || auto {
        let label = format!("Snapshot {}", s.memory_profiler.snapshots.len() + 1);
        let snap = capture(s, gpu, label);
        s.memory_profiler.push(snap);
    }
    let w = &mut s.memory_profiler;
    let Some(snap) = w.snapshots.get(w.selected).cloned() else {
        ui.add_space(40.0);
        ui.vertical_centered(|ui| {
            ui.label(RichText::new(ICON).size(34.0).color(theme::TEXT_DIM));
            ui.add_space(6.0);
            ui.label(
                RichText::new("Press Capture to take a snapshot of the editor's memory.")
                    .color(theme::TEXT_DIM),
            );
        });
        return;
    };
    ui.add_space(4.0);
    header(ui, &snap);
    ui.add_space(4.0);
    match w.view {
        View::Summary => summary(ui, &snap),
        View::Table => table(ui, w, &snap),
        View::TreeMap => tree_map(ui, &snap),
        View::Compare => compare(ui, w, &snap),
    }
}

/// Toolbar; true = Capture was pressed.
fn toolbar(ui: &mut Ui, w: &mut MemoryProfilerWindow) -> bool {
    let mut capture = false;
    panel_toolbar(ui, |ui| {
        capture = ui
            .button("⏺ Capture")
            .on_hover_text("Take a memory snapshot")
            .clicked();
        if !w.snapshots.is_empty() {
            egui::ComboBox::from_id_salt("memory_snapshot")
                .selected_text(w.snapshots[w.selected].label.as_str())
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, s) in w.snapshots.iter().enumerate() {
                        ui.selectable_value(&mut w.selected, i, s.label.as_str());
                    }
                });
            if ui
                .button("Clear")
                .on_hover_text("Forget all snapshots")
                .clicked()
            {
                *w = MemoryProfilerWindow {
                    view: w.view,
                    ..Default::default()
                };
            }
        }
        ui.separator();
        for v in View::ALL {
            if toggle_button(ui, w.view == v, v.label(), v.label()).clicked() {
                w.view = v;
            }
        }
        if w.view == View::Table {
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                search_field(ui, &mut w.filter, 140.0);
            });
        }
    });
    capture
}

fn header(ui: &mut Ui, snap: &MemorySnapshot) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(&snap.label).strong().size(14.0));
        ui.label(
            RichText::new(format!(
                "Tracked {}   ·   {} items",
                format_bytes(snap.total()),
                snap.items.len()
            ))
            .color(theme::TEXT_DIM),
        );
        if let Some(ws) = snap.working_set {
            ui.label(
                RichText::new(format!("·   Process working set {}", format_bytes(ws)))
                    .color(theme::TEXT_DIM),
            );
        }
    });
}

/// Totals by category as soft bars.
fn summary(ui: &mut Ui, snap: &MemorySnapshot) {
    let totals = snap.totals();
    let max = totals.iter().map(|t| t.1).max().unwrap_or(1).max(1);
    egui::ScrollArea::vertical()
        .id_salt("memory_summary")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            // Stacked strip of the whole snapshot.
            let (strip, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 14.0), Sense::hover());
            let total = snap.total().max(1) as f32;
            let mut x = strip.left();
            for (c, b) in &totals {
                let w = strip.width() * *b as f32 / total;
                if w > 0.0 {
                    let r = egui::Rect::from_min_size(
                        egui::pos2(x, strip.top()),
                        vec2(w, strip.height()),
                    );
                    ui.painter().rect_filled(r, 0.0, category_color(*c));
                    x += w;
                }
            }
            ui.add_space(10.0);
            for (c, bytes) in totals {
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), 26.0), Sense::hover());
                resp.widget_info(|| {
                    egui::WidgetInfo::labeled(egui::WidgetType::Label, true, c.label())
                });
                let p = ui.painter_at(row);
                p.rect_filled(row, 6.0, theme::CARD_BG);
                let dot = egui::Rect::from_center_size(
                    row.left_center() + vec2(14.0, 0.0),
                    vec2(10.0, 10.0),
                );
                p.rect_filled(dot, 3.0, category_color(c));
                p.text(
                    row.left_center() + vec2(28.0, 0.0),
                    Align2::LEFT_CENTER,
                    c.label(),
                    FontId::proportional(13.0),
                    theme::TEXT,
                );
                let bar_left = row.left() + 190.0;
                let bar_right = row.right() - 110.0;
                if bar_right > bar_left {
                    let full = egui::Rect::from_min_max(
                        egui::pos2(bar_left, row.center().y - 4.0),
                        egui::pos2(bar_right, row.center().y + 4.0),
                    );
                    p.rect_filled(full, 4.0, Color32::from_gray(52));
                    let fill = full.with_max_x(bar_left + full.width() * bytes as f32 / max as f32);
                    p.rect_filled(fill, 4.0, category_color(c));
                }
                p.text(
                    row.right_center() - vec2(10.0, 0.0),
                    Align2::RIGHT_CENTER,
                    format_bytes(bytes),
                    FontId::monospace(12.0),
                    theme::TEXT,
                );
                let count = snap.items.iter().filter(|i| i.category == c).count();
                resp.on_hover_text(format!("{count} items"));
                ui.add_space(3.0);
            }
        });
}

/// Item indices of `snap` for the table: filtered, then sorted by `col`.
pub fn sorted(snap: &MemorySnapshot, col: Column, ascending: bool, filter: &str) -> Vec<usize> {
    let filter = filter.trim().to_lowercase();
    let mut idx: Vec<usize> = (0..snap.items.len())
        .filter(|i| {
            let it = &snap.items[*i];
            filter.is_empty()
                || it.name.to_lowercase().contains(&filter)
                || it.kind.to_lowercase().contains(&filter)
        })
        .collect();
    let items = &snap.items;
    idx.sort_by(|a, b| {
        let (x, y) = (&items[*a], &items[*b]);
        let o = match col {
            Column::Name => x.name.cmp(&y.name),
            Column::Type => x.kind.cmp(&y.kind),
            Column::Category => x.category.cmp(&y.category),
            Column::Size => x.bytes.cmp(&y.bytes),
            Column::Refs => x.refs.cmp(&y.refs),
        };
        if ascending { o } else { o.reverse() }
    });
    idx
}

const COLUMNS: [(Column, &str, f32); 5] = [
    (Column::Name, "Name", 0.0),
    (Column::Type, "Type", 150.0),
    (Column::Category, "Category", 130.0),
    (Column::Size, "Size", 90.0),
    (Column::Refs, "Refs", 50.0),
];

fn table(ui: &mut Ui, w: &mut MemoryProfilerWindow, snap: &MemorySnapshot) {
    let fixed: f32 = COLUMNS.iter().map(|c| c.2).sum();
    let name_w = (ui.available_width() - fixed - 16.0).max(120.0);
    let width = |c: f32| if c == 0.0 { name_w } else { c };
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 0.0;
        for (col, title, cw) in COLUMNS {
            let arrow = match (w.sort == col, w.ascending) {
                (true, true) => " ⏶",
                (true, false) => " ⏷",
                _ => "",
            };
            let b = egui::Button::new(RichText::new(format!("{title}{arrow}")).strong())
                .frame(false)
                .min_size(vec2(width(cw), 20.0));
            if ui.add(b).clicked() {
                if w.sort == col {
                    w.ascending = !w.ascending;
                } else {
                    w.sort = col;
                    w.ascending = matches!(col, Column::Name | Column::Type | Column::Category);
                }
            }
        }
    });
    let order = sorted(snap, w.sort, w.ascending, &w.filter);
    let row_h = 20.0;
    egui::ScrollArea::vertical()
        .id_salt("memory_table")
        .auto_shrink([false, false])
        .show_rows(ui, row_h, order.len(), |ui, range| {
            for (n, i) in order[range.clone()].iter().enumerate() {
                let it = &snap.items[*i];
                let (row, resp) =
                    ui.allocate_exact_size(vec2(ui.available_width(), row_h), Sense::hover());
                let p = ui.painter_at(row);
                if (range.start + n) % 2 == 1 {
                    p.rect_filled(row, 0.0, Color32::from_gray(60));
                }
                if resp.hovered() {
                    p.rect_filled(row, 4.0, Color32::from_gray(70));
                }
                let mut x = row.left();
                let cells = [
                    it.name.clone(),
                    it.kind.clone(),
                    it.category.label().to_owned(),
                    format_bytes(it.bytes),
                    if it.refs > 0 {
                        it.refs.to_string()
                    } else {
                        "–".to_owned()
                    },
                ];
                for ((_, _, cw), text) in COLUMNS.iter().zip(cells) {
                    let cw = width(*cw);
                    let clip =
                        egui::Rect::from_min_size(egui::pos2(x, row.top()), vec2(cw - 6.0, row_h));
                    ui.painter_at(clip).text(
                        egui::pos2(x + 4.0, row.center().y),
                        Align2::LEFT_CENTER,
                        text,
                        FontId::proportional(12.5),
                        theme::TEXT,
                    );
                    x += cw;
                }
                let dot = egui::Rect::from_center_size(
                    egui::pos2(row.left() + name_w + 150.0 - 2.0, row.center().y),
                    vec2(4.0, 12.0),
                );
                p.rect_filled(dot, 2.0, category_color(it.category));
            }
        });
}

/// Categories, then each category's items, as nested squarified rectangles.
fn tree_map(ui: &mut Ui, snap: &MemorySnapshot) {
    let area = ui.available_rect_before_wrap().shrink(4.0);
    let resp = ui.allocate_rect(area, Sense::hover());
    let totals: Vec<(MemCategory, u64)> = {
        let mut t = snap.totals();
        t.retain(|c| c.1 > 0);
        t.sort_by_key(|a| std::cmp::Reverse(a.1));
        t
    };
    let sizes: Vec<f64> = totals.iter().map(|t| t.1 as f64).collect();
    let rects = treemap::squarify(&sizes, area);
    let painter = ui.painter_at(area);
    let hover = resp.hover_pos();
    let mut tip: Option<String> = None;
    for ((cat, bytes), rect) in totals.iter().zip(rects) {
        let color = category_color(*cat);
        painter.rect_filled(rect.shrink(1.0), 6.0, color.gamma_multiply(0.35));
        let title_h = if rect.height() > 40.0 { 18.0 } else { 0.0 };
        if title_h > 0.0 {
            painter.text(
                rect.left_top() + vec2(6.0, 3.0),
                Align2::LEFT_TOP,
                format!("{}  {}", cat.label(), format_bytes(*bytes)),
                FontId::proportional(12.0),
                Color32::WHITE,
            );
        }
        let inner = egui::Rect::from_min_max(
            rect.min + vec2(3.0, title_h + 2.0),
            rect.max - vec2(3.0, 3.0),
        );
        let mut items: Vec<&MemItem> = snap
            .items
            .iter()
            .filter(|i| i.category == *cat && i.bytes > 0)
            .collect();
        items.sort_by_key(|a| std::cmp::Reverse(a.bytes));
        let item_sizes: Vec<f64> = items.iter().map(|i| i.bytes as f64).collect();
        for (it, r) in items.iter().zip(treemap::squarify(&item_sizes, inner)) {
            if r.width() < 1.0 || r.height() < 1.0 {
                continue;
            }
            let hovered = hover.is_some_and(|p| r.contains(p));
            let fill = if hovered {
                color
            } else {
                color.gamma_multiply(0.75)
            };
            painter.rect_filled(r.shrink(1.0), 3.0, fill);
            if r.width() > 60.0 && r.height() > 16.0 {
                let label = it.name.rsplit('/').next().unwrap_or(&it.name);
                painter.with_clip_rect(r.shrink(2.0)).text(
                    r.left_top() + vec2(4.0, 2.0),
                    Align2::LEFT_TOP,
                    label,
                    FontId::proportional(11.0),
                    Color32::from_gray(20),
                );
            }
            if hovered {
                tip = Some(format!(
                    "{}\n{} · {}\n{}",
                    it.name,
                    it.kind,
                    cat.label(),
                    format_bytes(it.bytes)
                ));
            }
        }
    }
    if let Some(t) = tip {
        resp.on_hover_text(t);
    }
}

fn compare(ui: &mut Ui, w: &mut MemoryProfilerWindow, snap: &MemorySnapshot) {
    if w.snapshots.len() < 2 {
        ui.add_space(20.0);
        ui.vertical_centered(|ui| {
            ui.label(
                RichText::new("Capture a second snapshot to compare the two.")
                    .color(theme::TEXT_DIM),
            );
        });
        return;
    }
    let base_i = w
        .baseline
        .filter(|b| *b != w.selected && *b < w.snapshots.len())
        .unwrap_or(if w.selected == 0 { 1 } else { 0 });
    ui.horizontal(|ui| {
        ui.label("Compare with");
        let mut b = base_i;
        egui::ComboBox::from_id_salt("memory_baseline")
            .selected_text(w.snapshots[b].label.as_str())
            .width(110.0)
            .show_ui(ui, |ui| {
                for (i, s) in w.snapshots.iter().enumerate() {
                    if i != w.selected {
                        ui.selectable_value(&mut b, i, s.label.as_str());
                    }
                }
            });
        w.baseline = Some(b);
    });
    let base = &w.snapshots[base_i];
    let net = snap.total() as i64 - base.total() as i64;
    ui.label(
        RichText::new(format!(
            "{} → {}: {} ({} → {})",
            base.label,
            snap.label,
            format_delta(net),
            format_bytes(base.total()),
            format_bytes(snap.total())
        ))
        .color(delta_color(net)),
    );
    ui.add_space(4.0);
    egui::Grid::new("memory_category_diff")
        .num_columns(4)
        .striped(true)
        .spacing([18.0, 3.0])
        .show(ui, |ui| {
            for h in ["Category", "Before", "After", "Change"] {
                ui.strong(h);
            }
            ui.end_row();
            for (c, before, after) in memory_snapshot::category_diff(base, snap) {
                ui.label(RichText::new(c.label()).color(category_color(c)));
                ui.monospace(format_bytes(before));
                ui.monospace(format_bytes(after));
                let d = after as i64 - before as i64;
                ui.label(
                    RichText::new(format_delta(d))
                        .monospace()
                        .color(delta_color(d)),
                );
                ui.end_row();
            }
        });
    ui.add_space(6.0);
    ui.strong("Largest changes");
    let rows: Vec<_> = memory_snapshot::diff(base, snap)
        .into_iter()
        .filter(|r| r.change() != Change::Same)
        .collect();
    egui::ScrollArea::vertical()
        .id_salt("memory_diff")
        .auto_shrink([false, false])
        .show_rows(ui, 20.0, rows.len(), |ui, range| {
            for r in &rows[range] {
                ui.horizontal(|ui| {
                    let tag = match r.change() {
                        Change::Added => "new",
                        Change::Removed => "gone",
                        Change::Grew => "grew",
                        Change::Shrank => "shrank",
                        Change::Same => "",
                    };
                    ui.add_sized(
                        [52.0, 18.0],
                        egui::Label::new(RichText::new(tag).color(delta_color(r.delta()))),
                    );
                    ui.add_sized(
                        [90.0, 18.0],
                        egui::Label::new(
                            RichText::new(format_delta(r.delta()))
                                .monospace()
                                .color(delta_color(r.delta())),
                        ),
                    );
                    ui.label(RichText::new(&r.name).color(theme::TEXT));
                    ui.label(RichText::new(&r.kind).color(theme::TEXT_DIM));
                });
            }
        });
}

fn delta_color(d: i64) -> Color32 {
    match d.signum() {
        1 => Color32::from_rgb(235, 130, 110),
        -1 => Color32::from_rgb(120, 205, 140),
        _ => theme::TEXT_DIM,
    }
}
