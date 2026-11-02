//! Game view: display/aspect/scale toolbar and the camera output.

use egui::{Align2, Color32, FontId, Ui};
use misty_engine::camera::{Camera, Projection};
use misty_engine::render::Frame;
use misty_engine::scene::{Camera as CameraComponent, ClearFlags};

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

    let camera = game_camera(state);
    match (camera, gpu) {
        (Some((camera, settings)), Some(gpu)) => {
            let bg = settings.background;
            let frame = Frame {
                camera: &camera,
                grid: None,
                skybox: settings.clear_flags == ClearFlags::Skybox,
                background: [bg[0], bg[1], bg[2]],
                lines: &[],
            };
            let ppp = ui.ctx().pixels_per_point();
            let size = [(view.width() * ppp) as u32, (view.height() * ppp) as u32];
            let texture = gpu.render(ViewId::Game, size, &frame);
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(texture, view, uv, Color32::WHITE);
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

/// The enabled, active camera with the highest Depth renders the game.
fn game_camera(state: &EditorState) -> Option<(Camera, CameraComponent)> {
    let scene = &state.scene;
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let c = scene.world.get::<&CameraComponent>(e).ok()?;
            c.enabled.then(|| (e, (*c).clone()))
        })
        .max_by(|a, b| a.1.depth.total_cmp(&b.1.depth))
        .map(|(e, c)| {
            let (_, rotation, position) = scene.world_matrix(e).to_scale_rotation_translation();
            let projection = if c.orthographic {
                Projection::Orthographic { size: c.ortho_size }
            } else {
                Projection::Perspective { fov_y: c.fov }
            };
            let camera = Camera {
                position,
                rotation,
                projection,
                near: c.near,
                far: c.far,
            };
            (camera, c)
        })
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
