//! World Partition window (Window ▸ World ▸ World Partition): a minimap of the grid where cells
//! are loaded and unloaded by hand, the Convert / Build HLODs / Split Terrain actions, and the
//! streaming stats. Also the Scene view's streaming gizmos (cells by state, the sources' load
//! and unload radii) and the stats overlay drawn in the Scene and Game views.
//!
//! In Edit mode the window drives its own streamer (cells load by hand, or around the Scene
//! view camera with "Stream Around Scene Camera"); in Play mode it shows and steers the game's.

use egui::{Align2, Color32, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, vec2};
use glam::Vec3;
use misty_engine::world_partition::streamer::NoHooks;
use misty_engine::world_partition::{
    self, CellCoord, CellPhase, Source, Streamer, StreamingStats, WorldPartition,
};

use crate::gizmos::Lines;
use crate::state::EditorState;

pub const ICON: &str = "▦";

/// Window state (kept while the window is closed).
pub struct WorldPartitionWindow {
    /// Edit-mode streaming.
    pub streamer: Streamer,
    /// Edit mode: cells also load around the Scene view camera.
    pub stream_around_camera: bool,
    /// Cells and sources in the Scene view.
    pub show_gizmos: bool,
    /// Streaming stats box in the Scene and Game views.
    pub show_overlay: bool,
    pub selected: Option<CellCoord>,
    /// Samples per side of the tiles Split Terrain makes.
    pub tile_resolution: usize,
    pub status: String,
}

impl Default for WorldPartitionWindow {
    fn default() -> Self {
        let mut streamer = Streamer::new();
        streamer.auto = false;
        Self {
            streamer,
            stream_around_camera: false,
            show_gizmos: true,
            show_overlay: true,
            selected: None,
            tile_resolution: 129,
            status: String::new(),
        }
    }
}

/// The streamer in charge: the game's while playing, else the window's.
fn streamer(state: &EditorState) -> &Streamer {
    match state.runtime.as_ref() {
        Some(r) => r.streaming(),
        None => &state.world_partition.streamer,
    }
}

fn streamer_mut(state: &mut EditorState) -> &mut Streamer {
    match state.runtime.as_mut() {
        Some(r) => r.streaming_mut(),
        None => &mut state.world_partition.streamer,
    }
}

/// Edit-mode streaming, once per editor frame (cells by hand, or around the Scene camera).
/// Returns whether anything is in flight (keep repainting).
pub fn tick(state: &mut EditorState) -> bool {
    if state.play.in_play_mode() || world_partition::find(&state.scene).is_none() {
        return false;
    }
    let wp = world_partition::find(&state.scene).map(|(_, w)| w);
    let extra: Vec<Source> = match (&wp, state.world_partition.stream_around_camera) {
        (Some(wp), true) => vec![Source {
            name: "Scene Camera".into(),
            position: state.scene_view.camera.camera().position,
            load_radius: wp.load_radius,
            unload_radius: wp.unload_radius(),
        }],
        _ => Vec::new(),
    };
    let w = &mut state.world_partition;
    w.streamer.update(&mut state.scene, &extra, &mut NoHooks);
    for log in w.streamer.take_logs() {
        state.console.warn(log);
    }
    !state.world_partition.streamer.is_idle()
}

fn phase_color(p: CellPhase) -> Color32 {
    match p {
        CellPhase::Unloaded => Color32::from_rgb(70, 72, 78),
        CellPhase::Reading | CellPhase::Waiting => Color32::from_rgb(200, 160, 60),
        CellPhase::Adding => Color32::from_rgb(140, 190, 80),
        CellPhase::Loaded => Color32::from_rgb(70, 160, 90),
        CellPhase::Removing => Color32::from_rgb(170, 110, 70),
        CellPhase::Failed => Color32::from_rgb(200, 70, 60),
    }
}

fn mb(bytes: u64) -> String {
    format!("{:.1} MB", bytes as f64 / (1024.0 * 1024.0))
}

/// Lines of the stats box.
pub fn stats_lines(s: &StreamingStats) -> Vec<String> {
    vec![
        format!("World Partition   {} / {} cells loaded", s.loaded, s.cells),
        format!(
            "Pending: {} (reading {}, waiting {}, adding {})   Unloading: {}",
            s.pending(),
            s.reading,
            s.waiting,
            s.adding,
            s.removing
        ),
        format!(
            "Memory: {}   Loads {} / Unloads {}",
            mb(s.memory_bytes),
            s.loads,
            s.unloads
        ),
        format!(
            "Main thread: {:.2} ms (max {:.2}, budget {:.1}, {} over)",
            s.last_ms, s.max_ms, s.budget_ms, s.over_budget
        ),
        match &s.slowest {
            Some((what, ms)) => format!("Slowest step: {what} ({ms:.2} ms)"),
            None => "Slowest step: -".to_owned(),
        },
    ]
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let Some((owner, wp)) = world_partition::find(&state.scene) else {
        no_partition(ui, state);
        return;
    };
    if !streamer(state).is_idle() {
        ui.ctx().request_repaint();
    }
    let playing = state.play.in_play_mode();
    ui.horizontal_wrapped(|ui| {
        ui.add_enabled_ui(!playing, |ui| {
            if ui
                .button("Convert Scene")
                .on_hover_text(
                    "Split the scene's objects into grid cells: each cell becomes its own sub-scene \
                     file. Objects marked Always Loaded, cameras, lights and UI stay.",
                )
                .clicked()
            {
                convert(state, owner);
            }
            if ui
                .button("Build HLODs")
                .on_hover_text(
                    "Merge each cell into one cheap proxy mesh, drawn while the cell is not loaded.",
                )
                .clicked()
            {
                build_hlods(state, owner);
            }
            if ui
                .button("Save Cells")
                .on_hover_text("Write the loaded cells back to their sub-scene files.")
                .clicked()
            {
                save_cells(state);
            }
        });
    });
    ui.horizontal_wrapped(|ui| {
        if ui.button("Load All").clicked() {
            streamer_mut(state).load_all(&wp);
        }
        if ui.button("Unload All").clicked() {
            streamer_mut(state).unload_all();
        }
        if !playing {
            ui.checkbox(
                &mut state.world_partition.stream_around_camera,
                "Stream Around Scene Camera",
            )
            .on_hover_text(
                "Edit mode: cells within the load radius of the Scene view camera load.",
            );
        }
    });
    ui.horizontal(|ui| {
        ui.checkbox(&mut state.world_partition.show_gizmos, "Gizmos");
        ui.checkbox(&mut state.world_partition.show_overlay, "Stats Overlay");
    });
    ui.separator();
    let avail = ui.available_size();
    let side = avail.x.min((avail.y - 150.0).max(160.0)).max(120.0);
    minimap(ui, state, &wp, side);
    ui.separator();
    selected_cell(ui, state, &wp);
    ui.separator();
    let stats = streamer(state).stats().clone();
    for line in stats_lines(&stats) {
        ui.label(RichText::new(line).monospace().size(11.0));
    }
    history_graph(ui, streamer(state), stats.budget_ms);
    if !state.world_partition.status.is_empty() {
        ui.separator();
        ui.weak(state.world_partition.status.clone());
    }
}

fn no_partition(ui: &mut Ui, state: &mut EditorState) {
    ui.weak("This scene has no World Partition.");
    ui.label(
        "Add one to split a large world into grid cells that stream in and out around the \
         camera (or Streaming Source objects).",
    );
    if ui
        .add_enabled(
            !state.play.in_play_mode(),
            egui::Button::new("Add World Partition"),
        )
        .clicked()
    {
        state.record("Add World Partition");
        let e = state.scene.spawn("World Partition", None);
        let folder = state
            .scene
            .active_path()
            .map(|p| p.trim_end_matches(".scene").to_owned())
            .unwrap_or_else(|| format!("Assets/Scenes/{}", state.scene.name));
        state.scene.add(
            e,
            WorldPartition {
                folder,
                ..Default::default()
            },
        );
        state.selection.set(e);
    }
}

fn convert(state: &mut EditorState, owner: misty_engine::hecs::Entity) {
    state.record("Convert to World Partition");
    state.world_partition.streamer.reset();
    let root = state.project.root.clone();
    match world_partition::convert::convert(&mut state.scene, owner, &root) {
        Ok(r) => {
            let mut msg = format!(
                "Converted: {} objects into {} cells, {} kept loaded.",
                r.moved, r.cells, r.kept
            );
            for w in &r.warnings {
                state.console.warn(w.clone());
                msg.push_str(&format!(" {w}."));
            }
            state.console.info(msg.clone());
            state.world_partition.status = msg;
            state.selection.retain_existing(&state.scene);
            state.refresh_assets();
        }
        Err(e) => {
            state.console.warn(format!("Convert: {e}"));
            state.world_partition.status = e;
        }
    }
}

fn build_hlods(state: &mut EditorState, owner: misty_engine::hecs::Entity) {
    let root = state.project.root.clone();
    state.record("Build HLODs");
    match world_partition::hlod::build_all(&mut state.scene, owner, &root) {
        Ok(r) => {
            let msg = format!(
                "Built {} HLOD proxies: {} triangles, {:.0} kB.",
                r.cells,
                r.triangles,
                r.bytes as f64 / 1024.0
            );
            for w in &r.warnings {
                state.console.warn(w.clone());
            }
            state.console.info(msg.clone());
            state.world_partition.status = msg;
            state.refresh_assets();
        }
        Err(e) => {
            state.console.warn(format!("Build HLODs: {e}"));
            state.world_partition.status = e;
        }
    }
}

fn save_cells(state: &mut EditorState) {
    let root = state.project.root.clone();
    match world_partition::convert::save_loaded_cells(&mut state.scene, &root) {
        Ok(n) => {
            let msg = format!("Saved {n} cells.");
            state.console.info(msg.clone());
            state.world_partition.status = msg;
        }
        Err(e) => state.console.warn(format!("Save Cells: {e}")),
    }
}

/// Split the selected terrain into tiles (Inspector of a Terrain with a World Partition).
pub fn split_terrain(state: &mut EditorState, e: misty_engine::hecs::Entity) {
    let Some((_, wp)) = world_partition::find(&state.scene) else {
        return;
    };
    let root = state.project.root.clone();
    let res = state.world_partition.tile_resolution;
    state.record("Split Terrain into Tiles");
    match world_partition::tiles::split_object(&mut state.scene, e, &wp, res, &root) {
        Ok(tiles) => {
            let msg = format!("Split the terrain into {} tiles.", tiles.len());
            state.console.info(msg.clone());
            state.world_partition.status = msg;
            state.selection.retain_existing(&state.scene);
            state.refresh_assets();
        }
        Err(e) => state.console.warn(format!("Split Terrain: {e}")),
    }
}

/// Grid of cells by state; click selects, double-click loads / unloads, the sources (and the
/// Scene camera) as dots with their load radius.
fn minimap(ui: &mut Ui, state: &mut EditorState, wp: &WorldPartition, side: f32) {
    let Some((lo, hi)) = wp.bounds() else {
        ui.weak("No cells yet: press Convert Scene.");
        return;
    };
    let (w, h) = ((hi.0 - lo.0 + 1) as f32, (hi.1 - lo.1 + 1) as f32);
    let cell_px = (side / w.max(h)).max(4.0);
    let size = vec2(w * cell_px, h * cell_px);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgb(30, 31, 34));
    // North (+z) up.
    let cell_rect = |c: CellCoord| {
        let x = rect.left() + (c.0 - lo.0) as f32 * cell_px;
        let y = rect.bottom() - (c.1 - lo.1 + 1) as f32 * cell_px;
        Rect::from_min_size(Pos2::new(x, y), vec2(cell_px, cell_px)).shrink(1.0)
    };
    let st = streamer(state);
    for info in &wp.cells {
        let c = info.coord();
        let r = cell_rect(c);
        painter.rect_filled(r, 1.0, phase_color(st.phase(c)));
        if info.hlod && st.phase(c) != CellPhase::Loaded {
            painter.rect_stroke(
                r.shrink(1.0),
                1.0,
                Stroke::new(1.0, Color32::from_rgb(90, 130, 200)),
                egui::StrokeKind::Inside,
            );
        }
        if st.is_pinned(c) {
            painter.circle_filled(r.right_top() + vec2(-4.0, 4.0), 2.5, Color32::WHITE);
        }
        if let Some((done, all)) = st.progress(c) {
            let f = done as f32 / all.max(1) as f32;
            let bar =
                Rect::from_min_size(r.left_bottom() - vec2(0.0, 3.0), vec2(r.width() * f, 3.0));
            painter.rect_filled(bar, 0.0, Color32::WHITE);
        }
    }
    if let Some(sel) = state.world_partition.selected {
        painter.rect_stroke(
            cell_rect(sel).expand(1.0),
            1.0,
            Stroke::new(2.0, Color32::from_rgb(255, 170, 40)),
            egui::StrokeKind::Outside,
        );
    }
    // World point → minimap point.
    let s = wp.cell_size();
    let to_px = |p: Vec3| {
        let fx = (p.x - wp.origin[0]) / s - lo.0 as f32;
        let fz = (p.z - wp.origin[1]) / s - lo.1 as f32;
        Pos2::new(rect.left() + fx * cell_px, rect.bottom() - fz * cell_px)
    };
    let mut sources: Vec<Source> = st.sources().to_vec();
    if sources.is_empty() {
        sources = world_partition::sources(&state.scene, wp);
    }
    if !state.play.in_play_mode() {
        let p = state.scene_view.camera.camera().position;
        painter.circle_filled(to_px(p), 3.5, Color32::from_rgb(120, 200, 255));
    }
    for src in &sources {
        let c = to_px(src.position);
        painter.circle_stroke(
            c,
            src.load_radius / s * cell_px,
            Stroke::new(1.0, Color32::from_rgb(120, 230, 120)),
        );
        painter.circle_stroke(
            c,
            src.unload_radius / s * cell_px,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(230, 160, 80, 160)),
        );
        painter.circle_filled(c, 4.0, Color32::from_rgb(250, 250, 120));
    }
    let hovered = response.hover_pos().map(|p| {
        let x = lo.0 + ((p.x - rect.left()) / cell_px).floor() as i32;
        let z = lo.1 + ((rect.bottom() - p.y) / cell_px).floor() as i32;
        (x, z)
    });
    if let Some(c) = hovered.filter(|c| wp.cell(*c).is_some()) {
        let phase = st.phase(c);
        let info = wp.cell(c).cloned().unwrap_or_default();
        response.clone().on_hover_text(format!(
            "{}: {} · {} objects{}",
            world_partition::cell_name(c),
            phase.name(),
            info.objects,
            if info.hlod { " · HLOD" } else { "" }
        ));
        if response.double_clicked() {
            toggle(state, c);
        } else if response.clicked() {
            state.world_partition.selected = Some(c);
        }
    }
    painter.text(
        rect.right_top() + vec2(-4.0, 2.0),
        Align2::RIGHT_TOP,
        "N",
        FontId::proportional(11.0),
        Color32::from_gray(160),
    );
}

fn toggle(state: &mut EditorState, c: CellCoord) {
    let st = streamer_mut(state);
    if matches!(st.phase(c), CellPhase::Unloaded | CellPhase::Failed) {
        st.load(c);
    } else {
        st.unload(c);
    }
}

fn selected_cell(ui: &mut Ui, state: &mut EditorState, wp: &WorldPartition) {
    let Some(c) = state
        .world_partition
        .selected
        .filter(|c| wp.cell(*c).is_some())
    else {
        ui.weak("Click a cell to select it; double-click loads or unloads it.");
        return;
    };
    let info = wp.cell(c).cloned().unwrap_or_default();
    let st = streamer(state);
    let phase = st.phase(c);
    ui.horizontal(|ui| {
        ui.label(RichText::new(world_partition::cell_name(c)).strong());
        ui.colored_label(phase_color(phase), phase.name());
        if st.is_pinned(c) {
            ui.weak("(loaded by hand)");
        }
    });
    ui.label(format!(
        "{} objects · height {:.0}–{:.0} m · {}",
        info.objects,
        info.height[0],
        info.height[1],
        if info.hlod { "has HLOD" } else { "no HLOD" }
    ));
    if let Some(e) = st.error(c) {
        ui.colored_label(Color32::from_rgb(220, 90, 80), e.to_owned());
    }
    ui.horizontal(|ui| {
        if ui.button("Load").clicked() {
            streamer_mut(state).load(c);
        }
        if ui.button("Unload").clicked() {
            streamer_mut(state).unload(c);
        }
    });
}

fn history_graph(ui: &mut Ui, st: &Streamer, budget: f32) {
    let (rect, _) =
        ui.allocate_exact_size(vec2(ui.available_width().min(320.0), 40.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
    let times: Vec<f32> = st.history().collect();
    let top = times.iter().copied().fold(budget * 2.0, f32::max).max(0.5);
    let bar_w = rect.width() / 240.0;
    let x0 = rect.right() - times.len() as f32 * bar_w;
    for (i, ms) in times.iter().enumerate() {
        let h = ms / top * rect.height();
        let x = x0 + i as f32 * bar_w;
        let color = if *ms > budget {
            Color32::from_rgb(230, 70, 60)
        } else {
            Color32::from_rgb(110, 190, 120)
        };
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x, rect.bottom() - h),
                Pos2::new(x + bar_w.max(1.0), rect.bottom()),
            ),
            0.0,
            color,
        );
    }
    let y = rect.bottom() - budget / top * rect.height();
    painter.line_segment(
        [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
        Stroke::new(1.0, Color32::from_rgb(230, 200, 90)),
    );
}

/// Scene-view streaming gizmos: every cell outlined at its ground in its state's colour, and
/// each source with its load (green) and unload (orange) radius.
pub fn scene_lines(state: &EditorState, lines: &mut Lines) {
    if !state.world_partition.show_gizmos {
        return;
    }
    let Some((_, wp)) = world_partition::find(&state.scene) else {
        return;
    };
    let st = streamer(state);
    for info in &wp.cells {
        let c = info.coord();
        let col = phase_color(st.phase(c));
        let color = [
            col.r() as f32 / 255.0,
            col.g() as f32 / 255.0,
            col.b() as f32 / 255.0,
            0.9,
        ];
        let (a, b) = wp.cell_rect(c);
        let y = info.height[0];
        let inset = wp.cell_size() * 0.01;
        let p = |x: f32, z: f32| Vec3::new(x, y, z);
        let (x0, z0, x1, z1) = (a.x + inset, a.y + inset, b.x - inset, b.y - inset);
        lines.line(p(x0, z0), p(x1, z0), color);
        lines.line(p(x1, z0), p(x1, z1), color);
        lines.line(p(x1, z1), p(x0, z1), color);
        lines.line(p(x0, z1), p(x0, z0), color);
    }
    let mut sources: Vec<Source> = st.sources().to_vec();
    if sources.is_empty() {
        sources = world_partition::sources(&state.scene, &wp);
    }
    for s in &sources {
        for (r, color) in [
            (s.load_radius, [0.45, 0.95, 0.45, 0.9]),
            (s.unload_radius, [0.95, 0.65, 0.3, 0.7]),
        ] {
            let n = 96;
            for i in 0..n {
                let a0 = i as f32 / n as f32 * std::f32::consts::TAU;
                let a1 = (i + 1) as f32 / n as f32 * std::f32::consts::TAU;
                let at = |a: f32| s.position + Vec3::new(a.cos() * r, 0.0, a.sin() * r);
                lines.line(at(a0), at(a1), color);
            }
        }
    }
}

/// The streaming stats box in a view's top-left corner.
pub fn overlay(ui: &Ui, state: &EditorState, view: Rect) {
    if !state.world_partition.show_overlay || world_partition::find(&state.scene).is_none() {
        return;
    }
    let s = streamer(state).stats();
    let lines = stats_lines(s);
    let font = FontId::monospace(11.0);
    let graph_h = 26.0;
    // Bottom left: the Scene view's tool bar sits at the top.
    let size = vec2(360.0, 10.0 + lines.len() as f32 * 14.0 + graph_h + 6.0);
    let rect = Rect::from_min_size(view.left_bottom() + vec2(8.0, -size.y - 8.0), size);
    let painter = ui.painter().with_clip_rect(view);
    painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(20, 20, 20, 200));
    for (i, line) in lines.iter().enumerate() {
        painter.text(
            rect.left_top() + vec2(8.0, 5.0 + i as f32 * 14.0),
            Align2::LEFT_TOP,
            line,
            font.clone(),
            Color32::from_gray(if i == 0 { 255 } else { 210 }),
        );
    }
    let graph = Rect::from_min_size(
        rect.left_top() + vec2(8.0, 8.0 + lines.len() as f32 * 14.0),
        vec2(344.0, graph_h),
    );
    painter.rect_filled(graph, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
    let st = streamer(state);
    let times: Vec<f32> = st.history().collect();
    let budget = s.budget_ms.max(0.1);
    let top = times.iter().copied().fold(budget * 2.0, f32::max);
    let bar_w = graph.width() / 240.0;
    let x0 = graph.right() - times.len() as f32 * bar_w;
    for (i, ms) in times.iter().enumerate() {
        let h = ms / top * graph_h;
        let x = x0 + i as f32 * bar_w;
        let color = if *ms > budget {
            Color32::from_rgb(230, 70, 60)
        } else {
            Color32::from_rgb(110, 190, 120)
        };
        painter.rect_filled(
            Rect::from_min_max(
                Pos2::new(x, graph.bottom() - h),
                Pos2::new(x + bar_w.max(1.0), graph.bottom()),
            ),
            0.0,
            color,
        );
    }
    let y = graph.bottom() - budget / top * graph_h;
    painter.line_segment(
        [Pos2::new(graph.left(), y), Pos2::new(graph.right(), y)],
        Stroke::new(1.0, Color32::from_rgb(230, 200, 90)),
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stats_lines_read_well() {
        let s = StreamingStats {
            cells: 64,
            loaded: 12,
            reading: 2,
            waiting: 1,
            adding: 1,
            memory_bytes: 3 * 1024 * 1024,
            last_ms: 0.4,
            max_ms: 1.9,
            budget_ms: 2.0,
            ..Default::default()
        };
        let lines = stats_lines(&s);
        assert_eq!(lines[0], "World Partition   12 / 64 cells loaded");
        assert!(lines[1].starts_with("Pending: 4 (reading 2, waiting 1, adding 1)"));
        assert!(lines[2].starts_with("Memory: 3.0 MB"));
        assert!(lines[3].contains("max 1.90"));
    }
}
