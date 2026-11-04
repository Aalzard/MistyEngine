//! Material Inspector laid out like the Standard shader's (Shader, Rendering Mode, Main Maps,
//! Advanced Options), simpler panels for the Unlit shaders, and the preview at the bottom.

use egui::{RichText, Ui};
use misty_engine::assets::{Material, SmoothnessSource};
use misty_engine::render::{RenderingMode, ShaderKind};

use super::editors::{color_rgb, combo, grid};
use super::slots::texture_slot;
use crate::material_preview::PreviewShape;
use crate::state::EditorState;
use crate::theme;

fn section(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong());
    ui.label("");
    ui.end_row();
}

/// Albedo-style colour with alpha.
fn color_rgba(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut srgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = ui
        .color_edit_button_srgba_unmultiplied(&mut srgba)
        .changed();
    if changed {
        *c = srgba.map(|v| v as f32 / 255.0);
    }
    changed
}

fn slider(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(egui::Slider::new(v, 0.0..=1.0)).changed()
}

fn tiling_offset(ui: &mut Ui, m: &mut Material) -> bool {
    let mut ch = false;
    for (label, v) in [("Tiling", &mut m.tiling), ("Offset", &mut m.offset)] {
        ui.label(label);
        ui.horizontal(|ui| {
            for (axis, c) in ["X", "Y"].into_iter().zip(v.iter_mut()) {
                ui.label(axis);
                ch |= ui
                    .add(egui::DragValue::new(c).speed(0.01).max_decimals(3))
                    .changed();
            }
        });
        ui.end_row();
    }
    ch
}

/// Shader dropdown: built-in shaders and the project's `.shader` assets (switching keeps every
/// value, so switching back restores them).
fn shader_row(ui: &mut Ui, id: &str, m: &mut Material, shaders: &[String]) -> bool {
    ui.label("Shader");
    let ch = super::shader_material::shader_combo(ui, id, m, shaders);
    ui.end_row();
    ch
}

fn standard(ui: &mut Ui, id: &str, m: &mut Material, textures: &[String]) -> bool {
    let mut ch = false;
    ui.label("Rendering Mode");
    ch |= combo(
        ui,
        &format!("{id}_mode"),
        &mut m.rendering_mode,
        &RenderingMode::ALL,
        RenderingMode::label,
    );
    ui.end_row();

    section(ui, "Main Maps");
    ui.label("Albedo");
    ui.horizontal(|ui| {
        ch |= texture_slot(ui, &format!("{id}_albedo"), &mut m.albedo_map, textures);
        ch |= color_rgba(ui, &mut m.color);
    });
    ui.end_row();
    if m.rendering_mode == RenderingMode::Cutout {
        ui.label("    Alpha Cutoff");
        ch |= slider(ui, &mut m.alpha_cutoff);
        ui.end_row();
    }
    ui.label("Metallic");
    ui.horizontal(|ui| {
        ch |= texture_slot(ui, &format!("{id}_metallic"), &mut m.metallic_map, textures);
        // A metallic map replaces the slider.
        if m.metallic_map.is_none() {
            ch |= slider(ui, &mut m.metallic);
        }
    });
    ui.end_row();
    ui.label("    Smoothness");
    ch |= slider(ui, &mut m.smoothness);
    ui.end_row();
    ui.label("    Source");
    ch |= combo(
        ui,
        &format!("{id}_source"),
        &mut m.smoothness_source,
        &SmoothnessSource::ALL,
        SmoothnessSource::label,
    );
    ui.end_row();
    ui.label("Normal Map");
    ui.horizontal(|ui| {
        ch |= texture_slot(ui, &format!("{id}_normal"), &mut m.normal_map, textures);
        if m.normal_map.is_some() {
            ch |= ui
                .add(
                    egui::DragValue::new(&mut m.normal_scale)
                        .speed(0.01)
                        .range(-4.0..=4.0),
                )
                .on_hover_text("Normal map strength")
                .changed();
        }
    });
    ui.end_row();
    ui.label("Occlusion");
    ui.horizontal(|ui| {
        ch |= texture_slot(
            ui,
            &format!("{id}_occlusion"),
            &mut m.occlusion_map,
            textures,
        );
        if m.occlusion_map.is_some() {
            ch |= slider(ui, &mut m.occlusion_strength);
        }
    });
    ui.end_row();
    ui.label("Emission");
    ch |= ui.checkbox(&mut m.emission_enabled, "").changed();
    ui.end_row();
    if m.emission_enabled {
        ui.label("    Color");
        ui.horizontal(|ui| {
            ch |= texture_slot(ui, &format!("{id}_emission"), &mut m.emission_map, textures);
            ch |= color_rgb(ui, &mut m.emission);
        });
        ui.end_row();
        ui.label("    Intensity");
        ch |= ui
            .add(
                egui::DragValue::new(&mut m.emission_intensity)
                    .speed(0.05)
                    .range(0.0..=100.0),
            )
            .changed();
        ui.end_row();
    }
    ch |= tiling_offset(ui, m);

    section(ui, "Advanced Options");
    ui.label("Render Queue");
    ui.label(
        RichText::new(format!("From Shader  {}", m.rendering_mode.render_queue()))
            .color(theme::TEXT_DIM),
    );
    ui.end_row();
    ui.label("Double Sided");
    ch |= ui.checkbox(&mut m.double_sided, "").changed();
    ui.end_row();
    ch
}

fn unlit(ui: &mut Ui, id: &str, kind: ShaderKind, m: &mut Material, textures: &[String]) -> bool {
    let mut ch = false;
    if kind == ShaderKind::UnlitColor {
        ui.label("Main Color");
        ch |= color_rgba(ui, &mut m.color);
        ui.end_row();
    } else {
        let label = if kind == ShaderKind::UnlitTransparent {
            "Base (RGB) Trans (A)"
        } else {
            "Base (RGB)"
        };
        ui.label(label);
        ch |= texture_slot(ui, &format!("{id}_albedo"), &mut m.albedo_map, textures);
        ui.end_row();
        ch |= tiling_offset(ui, m);
    }
    ui.label("Render Queue");
    let queue = m.params().mode.render_queue();
    ui.label(RichText::new(format!("From Shader  {queue}")).color(theme::TEXT_DIM));
    ui.end_row();
    ui.label("Double Sided");
    ch |= ui.checkbox(&mut m.double_sided, "").changed();
    ui.end_row();
    ch
}

/// Material properties; `textures` lists the project's textures for the map slots and
/// `shaders` its `.shader` assets.
pub fn material_fields(
    ui: &mut Ui,
    id: &str,
    m: &mut Material,
    textures: &[String],
    shaders: &[String],
) -> bool {
    grid(ui, id, |ui| {
        let mut ch = shader_row(ui, id, m, shaders);
        ch |= match m.shader_kind() {
            _ if m.is_custom() => super::shader_material::custom_fields(ui, id, m, textures),
            ShaderKind::Standard => standard(ui, id, m, textures),
            kind => unlit(ui, id, kind, m, textures),
        };
        ch
    })
}

/// Edit a material asset in place and save it. Used inline under Mesh Renderer and for Project selection.
pub fn material_asset(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let Some(mut m) = state.assets.materials.get(rel).cloned() else {
        ui.weak(format!("Missing material: {rel}"));
        return;
    };
    let textures = state.assets.textures.clone();
    let shaders = state.assets.shaders.clone();
    if material_fields(ui, &format!("mat_{rel}"), &mut m, &textures, &shaders)
        && let Err(err) = state.assets.update_material(rel, m)
    {
        state.console.warn(format!("Could not save {rel}: {err}"));
    }
}

/// Preview at the bottom of the material Inspector: drag to rotate, sphere or cube.
pub fn preview(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let Some(m) = state.assets.materials.get(rel).cloned() else {
        return;
    };
    let previews = &mut state.project.thumbnails.materials;
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(RichText::new("Preview").strong());
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            for (shape, icon, tip) in [
                (PreviewShape::Cube, "▣", "Cube"),
                (PreviewShape::Sphere, "◍", "Sphere"),
            ] {
                if ui
                    .selectable_label(previews.shape == shape, icon)
                    .on_hover_text(tip)
                    .clicked()
                {
                    previews.shape = shape;
                }
            }
        });
    });
    let width = ui.available_width().max(64.0);
    let height = (ui.available_height() - 4.0).clamp(140.0, 320.0);
    let ppp = ui.ctx().pixels_per_point();
    let size_px = [(width * ppp) as u32, (height * ppp) as u32];
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(width, height), egui::Sense::drag());
    if resp.dragged() {
        let d = resp.drag_delta();
        previews.yaw -= d.x * 0.6;
        previews.pitch = (previews.pitch - d.y * 0.6).clamp(-89.0, 89.0);
    }
    match previews.inspector(&m, size_px) {
        Some(id) => {
            let uv = egui::Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0));
            ui.painter().image(id, rect, uv, egui::Color32::WHITE);
        }
        None => {
            ui.painter().rect_filled(rect, 0.0, theme::BORDER);
        }
    }
}
