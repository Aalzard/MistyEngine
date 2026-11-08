//! Game view: display/aspect/scale toolbar and the camera output.

use egui::{Align2, Color32, FontId, Ui};
use misty_engine::render::RenderStats;
use misty_engine::render_scene::GameView;
use misty_engine::runtime::RuntimeStats;

use crate::state::EditorState;
use crate::viewport::{Gpu, ViewId};
use crate::widgets::{panel_toolbar, toggle_button};

const ASPECTS: [(&str, Option<f32>); 5] = [
    ("Free Aspect", None),
    ("16:9", Some(16.0 / 9.0)),
    ("16:10", Some(16.0 / 10.0)),
    ("4:3", Some(4.0 / 3.0)),
    ("5:4", Some(5.0 / 4.0)),
];
const PLAY_TARGETS: [&str; 3] = ["Play Focused", "Play Maximized", "Play Unfocused"];

pub fn show(ui: &mut Ui, state: &mut EditorState, gpu: Option<&mut Gpu>) {
    toolbar(ui, state);

    let area = ui.available_rect_before_wrap();
    ui.allocate_rect(area, egui::Sense::hover());
    ui.painter()
        .rect_filled(area, 0.0, Color32::from_rgb(32, 32, 32));

    // Letterbox to the chosen aspect ratio.
    let view = match ASPECTS[state.game_view.aspect].1 {
        Some(aspect) => fit_aspect(area, aspect),
        None => area,
    };
    state.game_view.view_rect = view;

    // Play mode merges Static objects, like a build.
    state
        .game_draws
        .set_static_batching(state.runtime.is_some());
    let game = GameView::cached(&state.scene, &state.assets, &mut state.game_draws);
    // Window ▸ Analysis ▸ Frame Debugger: capture (and replay up to the selected event).
    crate::panels::frame_debugger::sync(state, ui.ctx());
    match (game, gpu) {
        (Some(game), Some(gpu)) => {
            let debug = state.frame_debugger.request(ui.ctx(), Vec::new());
            let debugging = debug.is_some();
            if let Some(mut request) = debug {
                request.labels = state.game_draws.labels(&state.scene);
                gpu.renderer.begin_capture(request);
            }
            let frame = game.frame();
            // Quality ▸ Render Scale changes the pixel size the game renders at.
            let ppp = ui.ctx().pixels_per_point() * misty_engine::quality::active().render_scale;
            let size = [(view.width() * ppp) as u32, (view.height() * ppp) as u32];
            state.game_view.screen_px = glam::Vec2::new(size[0] as f32, size[1] as f32);
            let mode = misty_engine::player_settings::ScreenMode::Windowed;
            misty_engine::screen::set_size(size[0], size[1], mode);
            // The Game view has no window to resize: Screen.SetResolution is a no-op here.
            misty_engine::screen::take_requests();
            let texture = gpu.render(ViewId::Game, size, &frame, &game.effects);
            let render = gpu.renderer.stats();
            // Grass chunks, terrain textures or trees still streaming in: keep drawing so they
            // show up as they land.
            if render.grass_pending > 0 || render.loading > 0 {
                ui.ctx().request_repaint();
            }
            // The Profiler's Rendering and Memory modules report the Game view.
            if misty_engine::profiler::enabled() {
                misty_engine::profiler::set_render_stats(render);
                misty_engine::profiler::set_memory(misty_engine::profiler::MemoryStats {
                    entities: state.scene.world.len() as usize,
                    render: gpu.renderer.memory(),
                });
            }
            let interaction = state.runtime.as_ref().map(|r| r.ui_interaction());
            // Constant Physical Size canvases size themselves by the screen DPI.
            misty_engine::ui::layout::set_screen_dpi(96.0 * ui.ctx().pixels_per_point());
            if !debugging || ui_event(gpu, &state.scene) {
                gpu.render_ui(&state.scene, interaction);
            }
            if let Some(capture) = gpu.renderer.end_capture() {
                state.frame_debugger.set_capture(capture);
            }
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture, view, uv, Color32::WHITE);
            crate::gizmos::vcam_overlay(ui, state, view);
            let net = crate::multiplayer::overlay_lines(state);
            if !net.is_empty() {
                net_overlay(ui, view, &net);
                // The numbers refresh about once a second.
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_millis(500));
            }
            if state.game_view.stats {
                let gpu_ms = gpu.renderer.gpu_ms();
                let dt = ui.input(|i| i.stable_dt).max(1e-4);
                state.game_view.frame_history.push(dt * 1000.0);
                stats_overlay(ui, state, view, size, &render, gpu_ms);
                // Keep sampling frames while the overlay is up, idle or not.
                ui.ctx().request_repaint();
            } else {
                state.game_view.frame_history.clear();
            }
        }
        _ => {
            ui.painter().rect_filled(view, 0.0, Color32::BLACK);
            ui.painter().text(
                view.center(),
                Align2::CENTER_CENTER,
                "Display 1\nNo cameras rendering",
                FontId::proportional(16.0),
                Color32::from_gray(200),
            );
        }
    }
}

/// The canvas UI as a Frame Debugger event (when the scene has canvases); true = draw it.
fn ui_event(gpu: &Gpu, scene: &misty_engine::scene::Scene) -> bool {
    use misty_engine::render::{EventKind, FrameEvent};
    let canvases = scene.active_with::<misty_engine::scene::Canvas>().len();
    canvases == 0
        || gpu.renderer.capture_event(
            &["UI"],
            FrameEvent {
                shader: "ui.wgsl".to_owned(),
                blend: "SrcAlpha OneMinusSrcAlpha".to_owned(),
                depth: "Off".to_owned(),
                cull: "Off".to_owned(),
                ..FrameEvent::new(
                    EventKind::Ui,
                    format!("Draw Canvas UI ({canvases} canvases)"),
                )
            },
        )
}

/// Text of the Stats box: frame rate, CPU/GPU times, draw counters, audio voices.
pub fn stats_lines(
    dt: f32,
    render: &RenderStats,
    gpu_ms: Option<f32>,
    size: [u32; 2],
    runtime: Option<RuntimeStats>,
) -> Vec<String> {
    let gpu = gpu_ms.map_or("n/a".to_owned(), |g| format!("{g:.1}ms"));
    vec![
        "Statistics".to_owned(),
        match runtime {
            Some(r) => format!("Audio:  {} voices playing", r.voices),
            None => "Audio:  -".to_owned(),
        },
        format!("Graphics:  {:.1} FPS ({:.1}ms)", 1.0 / dt, dt * 1000.0),
        format!(
            "  CPU: main {:.1}ms  render thread {:.1}ms",
            dt * 1000.0,
            render.cpu_ms
        ),
        format!("  GPU: {gpu}"),
        format!(
            "  Batches: {}   Saved by batching: {}",
            render.batches, render.saved_by_batching
        ),
        format!(
            "  Tris: {}   Verts: {}",
            fmt_k(render.triangles as usize),
            fmt_k(render.vertices as usize)
        ),
        format!("  Screen: {}x{}", size[0], size[1]),
        format!(
            "  SetPass calls: {}   Shadow casters: {}",
            render.set_pass, render.shadow_casters
        ),
        format!(
            "  Visible meshes: {}   Culled: {}",
            render.visible, render.culled
        ),
        format!("  Occlusion culled: {}", render.occlusion_culled),
        format!(
            "  Virtual geometry: {} objects  {} clusters  {} tris (shadows {})  {:.1} MB",
            render.vgeo_instances,
            fmt_k(render.vgeo_clusters as usize),
            fmt_k(render.vgeo_triangles as usize),
            fmt_k(render.vgeo_shadow_triangles as usize),
            render.vgeo_memory as f64 / (1024.0 * 1024.0)
        ),
        format!(
            "  Grass chunks pending: {}   upload {:.1}ms   Shaders built: {}",
            render.grass_pending,
            render.grass_upload_ms,
            misty_engine::render::pipelines_built()
        ),
        match runtime {
            Some(r) => format!(
                "Physics: {} bodies, {} colliders   Scripts: {}",
                r.bodies, r.colliders, r.scripts
            ),
            None => "Physics: -   Scripts: -".to_owned(),
        },
    ]
}

/// Rendering statistics box in the top-right corner of the Game view.
fn stats_overlay(
    ui: &Ui,
    state: &EditorState,
    view: egui::Rect,
    size: [u32; 2],
    render: &RenderStats,
    gpu_ms: Option<f32>,
) {
    let dt = ui.input(|i| i.stable_dt).max(1e-4);
    let runtime = state.runtime.as_ref().map(|r| r.stats());
    let mut lines = stats_lines(dt, render, gpu_ms, size, runtime);
    let history = &state.game_view.frame_history;
    lines.push(hitch_line(history));
    let font = FontId::monospace(11.0);
    let graph_h = 36.0;
    let height = 12.0 + lines.len() as f32 * 14.0 + graph_h + 6.0;
    let rect = egui::Rect::from_min_size(
        view.right_top() + egui::vec2(-318.0, 8.0),
        egui::vec2(310.0, height),
    );
    let painter = ui.painter().with_clip_rect(view);
    painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(20, 20, 20, 200));
    for (i, line) in lines.iter().enumerate() {
        painter.text(
            rect.left_top() + egui::vec2(8.0, 6.0 + i as f32 * 14.0),
            Align2::LEFT_TOP,
            line,
            font.clone(),
            Color32::from_gray(if i == 0 { 255 } else { 210 }),
        );
    }
    // Frame-time graph: one bar per frame, hitches (> 2× the median) in red, the median as
    // a line. The scale is 3× the median, so a hitch shows as a bar past the line's height.
    let graph = egui::Rect::from_min_size(
        rect.left_top() + egui::vec2(8.0, 8.0 + lines.len() as f32 * 14.0),
        egui::vec2(294.0, graph_h),
    );
    painter.rect_filled(graph, 2.0, Color32::from_rgba_unmultiplied(0, 0, 0, 120));
    let median = history.median_ms().max(1e-3);
    let scale = graph_h / (median * 3.0);
    let n = history.times().len().max(1) as f32;
    let bar_w = graph.width() / misty_engine::render::stats::FRAME_HISTORY as f32;
    let x0 = graph.right() - n * bar_w;
    for (i, ms) in history.times().enumerate() {
        let h = (ms * scale).min(graph_h);
        let x = x0 + i as f32 * bar_w;
        let bar = egui::Rect::from_min_max(
            egui::pos2(x, graph.bottom() - h),
            egui::pos2(x + bar_w.max(1.0), graph.bottom()),
        );
        let color = if history.is_hitch(ms) {
            Color32::from_rgb(230, 70, 60)
        } else {
            Color32::from_rgb(90, 170, 110)
        };
        painter.rect_filled(bar, 0.0, color);
    }
    let y = graph.bottom() - (median * scale).min(graph_h);
    painter.line_segment(
        [egui::pos2(graph.left(), y), egui::pos2(graph.right(), y)],
        egui::Stroke::new(1.0, Color32::from_gray(200)),
    );
}

/// The network session's round trip, packets and bandwidth, bottom left of the Game view.
fn net_overlay(ui: &Ui, view: egui::Rect, lines: &[String]) {
    let font = FontId::monospace(11.0);
    let painter = ui.painter().with_clip_rect(view);
    let width = lines
        .iter()
        .map(|l| {
            painter
                .layout_no_wrap(l.clone(), font.clone(), Color32::WHITE)
                .size()
                .x
        })
        .fold(0.0, f32::max)
        + 16.0;
    let height = 8.0 + lines.len() as f32 * 14.0;
    let rect = egui::Rect::from_min_size(
        view.left_bottom() + egui::vec2(8.0, -8.0 - height),
        egui::vec2(width, height),
    );
    painter.rect_filled(rect, 4.0, Color32::from_rgba_unmultiplied(20, 20, 20, 190));
    for (i, line) in lines.iter().enumerate() {
        painter.text(
            rect.left_top() + egui::vec2(8.0, 4.0 + i as f32 * 14.0),
            Align2::LEFT_TOP,
            line,
            font.clone(),
            Color32::from_gray(if i == 0 { 240 } else { 200 }),
        );
    }
}

/// "Hitches: 3 (> 2× median 16.7ms)   worst 48.2ms".
pub fn hitch_line(history: &misty_engine::render::FrameHistory) -> String {
    format!(
        "Hitches: {} (> {:.0}x median {:.1}ms)   worst {:.1}ms",
        history.hitches(),
        misty_engine::render::FrameHistory::HITCH_FACTOR,
        history.median_ms(),
        history.worst_ms()
    )
}

fn fmt_k(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f32 / 1000.0)
    } else {
        n.to_string()
    }
}

/// Preview a Quality level in the Scene and Game views (and Play mode).
fn quality_dropdown(ui: &mut Ui, quality: &misty_engine::quality::QualitySettings) {
    let current = misty_engine::quality::level();
    let name = quality
        .levels
        .get(current)
        .map_or("Quality", |l| l.name.as_str());
    egui::ComboBox::from_id_salt("game_quality")
        .selected_text(name)
        .width(80.0)
        .show_ui(ui, |ui| {
            for (i, l) in quality.levels.iter().enumerate() {
                let label = if i == quality.default_level {
                    format!("{} (default)", l.name)
                } else {
                    l.name.clone()
                };
                if ui.selectable_label(i == current, label).clicked() {
                    misty_engine::quality::install(quality, i);
                }
            }
        })
        .response
        .on_hover_text("Quality level to preview (Project Settings ▸ Quality)");
}

fn fit_aspect(area: egui::Rect, aspect: f32) -> egui::Rect {
    let mut size = area.size();
    if size.x / size.y > aspect {
        size.x = size.y * aspect;
    } else {
        size.y = size.x / aspect;
    }
    egui::Rect::from_center_size(area.center(), size)
}

fn toolbar(ui: &mut Ui, state: &mut EditorState) {
    let quality = &state.project_settings.quality;
    let gv = &mut state.game_view;
    panel_toolbar(ui, |ui| {
        egui::ComboBox::from_id_salt("game_display")
            .selected_text("Display 1")
            .width(80.0)
            .show_ui(ui, |ui| {
                let _ = ui.selectable_label(true, "Display 1");
            });
        egui::ComboBox::from_id_salt("game_aspect")
            .selected_text(ASPECTS[gv.aspect].0)
            .width(100.0)
            .show_ui(ui, |ui| {
                for (i, (name, _)) in ASPECTS.iter().enumerate() {
                    ui.selectable_value(&mut gv.aspect, i, *name);
                }
            });
        ui.label("Scale");
        ui.add(
            egui::Slider::new(&mut gv.scale, 1.0..=5.0)
                .suffix("x")
                .max_decimals(1),
        );
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            if toggle_button(ui, gv.gizmos, "Gizmos", "Toggle gizmos").clicked() {
                gv.gizmos = !gv.gizmos;
            }
            if toggle_button(ui, gv.stats, "Stats", "Rendering statistics").clicked() {
                gv.stats = !gv.stats;
            }
            let vsync = *gv
                .vsync
                .get_or_insert_with(crate::hub::config::editor_vsync);
            let hover = "VSync: the editor waits for the display's refresh (the frame rate \
                stops at the refresh rate, and halves when a frame misses it).";
            if toggle_button(ui, vsync, "VSync", hover).clicked() {
                gv.vsync = Some(!vsync);
                if let Err(e) = crate::hub::config::set_editor_vsync(!vsync) {
                    eprintln!("could not save the VSync preference: {e}");
                }
            }
            if toggle_button(ui, gv.mute, "🔇", "Mute Audio").clicked() {
                gv.mute = !gv.mute;
            }
            quality_dropdown(ui, quality);
            egui::ComboBox::from_id_salt("game_play_target")
                .selected_text(PLAY_TARGETS[gv.play_mode_target])
                .width(110.0)
                .show_ui(ui, |ui| {
                    for (i, t) in PLAY_TARGETS.iter().enumerate() {
                        ui.selectable_value(&mut gv.play_mode_target, i, *t);
                    }
                });
        });
    });
}
