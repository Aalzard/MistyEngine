//! Game view: display/aspect/scale toolbar and the camera output.

use egui::{Align2, Color32, FontId, Ui};
use misty_engine::render_scene::GameView;

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

    let game = GameView::new(&state.scene, &state.assets);
    match (game, gpu) {
        (Some(game), Some(gpu)) => {
            let frame = game.frame();
            let ppp = ui.ctx().pixels_per_point();
            let size = [(view.width() * ppp) as u32, (view.height() * ppp) as u32];
            let started = std::time::Instant::now();
            let texture = gpu.render(ViewId::Game, size, &frame);
            let render_ms = started.elapsed().as_secs_f32() * 1000.0;
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture, view, uv, Color32::WHITE);
            if state.game_view.stats {
                stats_overlay(ui, state, view, &game.meshes, size, render_ms);
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

/// Rendering statistics box in the top-right corner of the Game view.
fn stats_overlay(
    ui: &Ui,
    state: &EditorState,
    view: egui::Rect,
    meshes: &[misty_engine::render::MeshDraw],
    size: [u32; 2],
    render_ms: f32,
) {
    use misty_engine::render::mesh::primitive;
    let (tris, verts) = meshes.iter().fold((0, 0), |(t, v), m| {
        let data = primitive(m.mesh);
        (t + data.indices.len() / 3, v + data.vertices.len())
    });
    let dt = ui.input(|i| i.stable_dt).max(1e-4);
    let lines = [
        "Statistics".to_owned(),
        format!("Graphics:  {:.1} FPS ({:.1}ms)", 1.0 / dt, dt * 1000.0),
        format!("  Render thread:  {render_ms:.2}ms"),
        format!(
            "  Batches: {}   Tris: {}   Verts: {}",
            meshes.len(),
            fmt_k(tris),
            fmt_k(verts)
        ),
        format!("  Screen: {}x{}", size[0], size[1]),
        format!(
            "Play time: {:.2}s   Frame: {}",
            state.time.time, state.time.frame
        ),
        match state.runtime.as_ref().map(|r| r.stats()) {
            Some(r) => format!(
                "Physics: {} bodies, {} colliders   Scripts: {}   Audio: {}",
                r.bodies, r.colliders, r.scripts, r.voices
            ),
            None => "Physics: -   Scripts: -   Audio: -".to_owned(),
        },
    ];
    let font = FontId::monospace(11.0);
    let rect = egui::Rect::from_min_size(
        view.right_top() + egui::vec2(-300.0, 8.0),
        egui::vec2(292.0, 110.0),
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
}

fn fmt_k(n: usize) -> String {
    if n >= 1000 {
        format!("{:.1}k", n as f32 / 1000.0)
    } else {
        n.to_string()
    }
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
            if toggle_button(ui, gv.mute, "🔇", "Mute Audio").clicked() {
                gv.mute = !gv.mute;
            }
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
