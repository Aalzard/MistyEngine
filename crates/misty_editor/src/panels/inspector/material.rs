//! Material Properties laid out as one aligned list: Shader and Rendering Mode, then Main Maps
//! (a square texture slot at the left of each map's name, its colour or slider in the value
//! column), Emission, Tiling and Offset, Secondary Maps, Advanced Surface, Rendering Options
//! and Advanced Options; simpler lists for the Unlit shaders; the preview at the bottom.

use egui::Ui;
use misty_engine::assets::{Material, SmoothnessSource};
use misty_engine::render::{RenderingMode, ShaderKind};

use super::editors::{color_rgb, combo, grid};
use super::texture_square::{map_label, normal_map_fix_row, row_label};
use crate::i18n::tr;
use crate::material_preview::PreviewShape;
use crate::state::EditorState;
use crate::theme;

/// Bold section header row ("Main Maps"). It is painted over the row, so a long title never
/// widens the label column. As tall as every other row: rows of one height keep the grid still
/// when a row appears above them.
pub(super) fn section(ui: &mut Ui, title: &str) {
    let w = crate::widgets::label_width_or(ui, 110.0);
    let (rect, resp) =
        ui.allocate_exact_size(egui::vec2(w, super::card::ROW_HEIGHT), egui::Sense::hover());
    let text: egui::WidgetText = theme::bold(ui.ctx(), title).into();
    let galley = text.into_galley(
        ui,
        Some(egui::TextWrapMode::Extend),
        f32::INFINITY,
        egui::TextStyle::Body,
    );
    let x = if crate::i18n::is_rtl() {
        rect.right() - galley.size().x
    } else {
        rect.left()
    };
    let pos = egui::pos2(x, rect.bottom() - galley.size().y - 1.0);
    ui.painter().galley(pos, galley, theme::TEXT);
    resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, true, title));
    ui.end_row();
}

/// Albedo-style colour with alpha: a swatch filling the value column.
fn color_rgba(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut srgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = crate::panels::inspector::card::wide(ui, |ui| {
        ui.color_edit_button_srgba_unmultiplied(&mut srgba)
    })
    .changed();
    if changed {
        *c = srgba.map(|v| v as f32 / 255.0);
    }
    changed
}

/// Width of the value column (the property grid sizes dropdowns to fill it).
fn value_width(ui: &Ui) -> f32 {
    ui.spacing().combo_width
}

/// Slider with its number field, together filling the value column.
pub(super) fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.scope(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let number = ui.spacing().interact_size.x;
        ui.spacing_mut().slider_width = (value_width(ui) - number - gap).max(30.0);
        ui.add(egui::Slider::new(v, range)).changed()
    })
    .inner
}

/// A lone number field filling the value column.
pub(super) fn number(ui: &mut Ui, field: egui::DragValue<'_>, tip: &str) -> bool {
    let size = [value_width(ui), ui.spacing().interact_size.y];
    let r = ui.add_sized(size, field);
    if tip.is_empty() {
        r.changed()
    } else {
        r.on_hover_text(tip).changed()
    }
}

/// `X [..] Y [..]`: two equal number fields filling the value column.
pub(super) fn pair(ui: &mut Ui, v: &mut [f32; 2]) -> bool {
    let mut ch = false;
    let size = egui::vec2(value_width(ui), super::card::ROW_HEIGHT);
    let layout = egui::Layout::left_to_right(egui::Align::Center);
    ui.allocate_ui_with_layout(size, layout, |ui| {
        let gap = ui.spacing().item_spacing.x;
        let h = ui.spacing().interact_size.y;
        let letter = 9.0;
        let field = ((size.x - 2.0 * letter - 3.0 * gap) / 2.0).max(30.0);
        for (axis, c) in ["X", "Y"].into_iter().zip(v.iter_mut()) {
            let (_, at) = ui.allocate_space(egui::vec2(letter, h));
            ui.painter().text(
                at.left_center(),
                egui::Align2::LEFT_CENTER,
                axis,
                egui::TextStyle::Body.resolve(ui.style()),
                theme::TEXT,
            );
            ch |= ui
                .add_sized(
                    [field, h],
                    egui::DragValue::new(c).speed(0.01).max_decimals(3),
                )
                .changed();
        }
    });
    ch
}

/// A whole checkbox row: the label (with its tooltip) and the checkbox in the value column.
pub(super) fn toggle_row(ui: &mut Ui, label: &str, tip: &str, level: u8, on: &mut bool) -> bool {
    let r = row_label(ui, label, level);
    if !tip.is_empty() {
        r.on_hover_text(tip);
    }
    let changed = crate::a11y::checkbox(ui, on, label).changed();
    ui.end_row();
    changed
}

fn tiling_offset(ui: &mut Ui, m: &mut Material) -> bool {
    let mut ch = false;
    row_label(ui, &tr!("Tiling"), 0);
    ch |= pair(ui, &mut m.tiling);
    ui.end_row();
    row_label(ui, &tr!("Offset"), 0);
    ch |= pair(ui, &mut m.offset);
    ui.end_row();
    ch
}

/// "Render Queue": the queue the shader gives, shown as a locked "From Shader" choice and its
/// number.
pub(super) fn render_queue_row(ui: &mut Ui, queue: u32, tip: &str) {
    row_label(ui, &tr!("Render Queue"), 0).on_hover_text(tip);
    ui.add_enabled_ui(false, |ui| {
        ui.horizontal(|ui| {
            let gap = ui.spacing().item_spacing.x;
            let number = ui.spacing().interact_size.x;
            let h = ui.spacing().interact_size.y;
            let w = (value_width(ui) - number - gap).max(40.0);
            ui.add_sized([w, h], egui::Button::new(tr!("From Shader")).truncate());
            let mut q = queue;
            ui.add_sized([number, h], egui::DragValue::new(&mut q));
        });
    });
    ui.end_row();
}

/// Shader dropdown: built-in shaders and the project's `.shader` assets (switching keeps every
/// value, so switching back restores them).
fn shader_row(ui: &mut Ui, id: &str, m: &mut Material, shaders: &[String]) -> bool {
    row_label(ui, &tr!("Shader"), 0);
    let ch = super::shader_material::shader_combo(ui, id, m, shaders);
    ui.end_row();
    ch
}

fn standard(ui: &mut Ui, state: &mut EditorState, id: &str, m: &mut Material) -> bool {
    let mut ch = false;
    row_label(ui, &tr!("Rendering Mode"), 0);
    ch |= combo(
        ui,
        &format!("{id}_mode"),
        &mut m.rendering_mode,
        &RenderingMode::ALL,
        RenderingMode::label,
    );
    ui.end_row();

    section(ui, &tr!("Main Maps"));
    ch |= map_label(
        ui,
        state,
        &format!("{id}_albedo"),
        &tr!("Albedo"),
        &tr!("Base colour (RGB) and transparency (A): the texture times the colour"),
        &mut m.albedo_map,
    );
    ch |= color_rgba(ui, &mut m.color);
    ui.end_row();
    if m.rendering_mode == RenderingMode::Cutout {
        row_label(ui, &tr!("Alpha Cutoff"), 1).on_hover_text(tr!("Alpha below this is cut away"));
        ch |= slider(ui, &mut m.alpha_cutoff, 0.0..=1.0);
        ui.end_row();
    }
    ch |= map_label(
        ui,
        state,
        &format!("{id}_metallic"),
        &tr!("Metallic"),
        &tr!("Metallic (R) and smoothness (A); a map replaces the slider"),
        &mut m.metallic_map,
    );
    // A metallic map replaces the slider.
    if m.metallic_map.is_none() {
        ch |= slider(ui, &mut m.metallic, 0.0..=1.0);
    }
    ui.end_row();
    row_label(ui, &tr!("Smoothness"), 1);
    ch |= slider(ui, &mut m.smoothness, 0.0..=1.0);
    ui.end_row();
    row_label(ui, &tr!("Source"), 2)
        .on_hover_text(tr!("The texture channel smoothness is read from"));
    ch |= combo(
        ui,
        &format!("{id}_source"),
        &mut m.smoothness_source,
        &SmoothnessSource::ALL,
        SmoothnessSource::label,
    );
    ui.end_row();
    ch |= map_label(
        ui,
        state,
        &format!("{id}_normal"),
        &tr!("Normal Map"),
        "",
        &mut m.normal_map,
    );
    if m.normal_map.is_some() {
        ch |= number(
            ui,
            egui::DragValue::new(&mut m.normal_scale)
                .speed(0.01)
                .range(-4.0..=4.0),
            &tr!("Normal map strength"),
        );
    }
    ui.end_row();
    normal_map_fix_row(ui, state, m.normal_map.as_deref());
    ch |= super::material_features::height_rows(ui, state, id, &mut m.features.parallax);
    ch |= map_label(
        ui,
        state,
        &format!("{id}_occlusion"),
        &tr!("Occlusion"),
        &tr!("Ambient occlusion (G)"),
        &mut m.occlusion_map,
    );
    if m.occlusion_map.is_some() {
        ch |= slider(ui, &mut m.occlusion_strength, 0.0..=1.0);
    }
    ui.end_row();
    ch |= super::material_features::detail_mask_row(ui, state, id, &mut m.features.detail);

    ch |= toggle_row(
        ui,
        &tr!("Emission"),
        &tr!("The surface gives off light of its own"),
        0,
        &mut m.emission_enabled,
    );
    if m.emission_enabled {
        ch |= map_label(
            ui,
            state,
            &format!("{id}_emission"),
            &tr!("Color"),
            &tr!("Emission colour, multiplied by the texture"),
            &mut m.emission_map,
        );
        ch |= color_rgb(ui, &mut m.emission);
        ui.end_row();
        row_label(ui, &tr!("Intensity"), 1)
            .on_hover_text(tr!("Brightness of the emission (above 1 glows with Bloom)"));
        ch |= number(
            ui,
            egui::DragValue::new(&mut m.emission_intensity)
                .speed(0.05)
                .range(0.0..=100.0),
            "",
        );
        ui.end_row();
    }
    ch |= tiling_offset(ui, m);
    row_label(ui, &tr!("Rotation"), 0).on_hover_text(tr!("Turns the textures about their centre"));
    ch |= number(
        ui,
        egui::DragValue::new(&mut m.uv_rotation)
            .speed(1.0)
            .range(-180.0..=180.0)
            .suffix("°"),
        "",
    );
    ui.end_row();

    ch |= super::material_features::secondary_maps(ui, state, id, &mut m.features.detail);
    ch |= super::material_features::advanced_surface(ui, state, id, &mut m.features);

    section(ui, &tr!("Rendering Options"));
    ch |= super::material_features::specular_rows(ui, &mut m.features);

    section(ui, &tr!("Advanced Options"));
    render_queue_row(
        ui,
        m.rendering_mode.render_queue(),
        &tr!("Set by the Rendering Mode"),
    );
    ch |= toggle_row(
        ui,
        &tr!("Double Sided"),
        &tr!("Draw both faces"),
        0,
        &mut m.double_sided,
    );
    ch
}

fn unlit(
    ui: &mut Ui,
    state: &mut EditorState,
    id: &str,
    kind: ShaderKind,
    m: &mut Material,
) -> bool {
    let mut ch = false;
    if kind == ShaderKind::UnlitColor {
        row_label(ui, &tr!("Main Color"), 0);
        ch |= color_rgba(ui, &mut m.color);
        ui.end_row();
    } else {
        let label = if kind == ShaderKind::UnlitTransparent {
            tr!("Base (RGB) Trans (A)")
        } else {
            tr!("Base (RGB)")
        };
        let id = format!("{id}_albedo");
        ch |= map_label(ui, state, &id, &label, "", &mut m.albedo_map);
        ui.end_row();
        ch |= tiling_offset(ui, m);
    }
    section(ui, &tr!("Advanced Options"));
    render_queue_row(
        ui,
        m.params().mode.render_queue(),
        &tr!("Set by the shader"),
    );
    ch |= toggle_row(
        ui,
        &tr!("Double Sided"),
        &tr!("Draw both faces"),
        0,
        &mut m.double_sided,
    );
    ch
}

/// Material properties. Texture slots take the project's textures; the Shader dropdown lists
/// its `.shader` assets.
pub fn material_fields(ui: &mut Ui, state: &mut EditorState, id: &str, m: &mut Material) -> bool {
    let shaders = state.assets.shaders.clone();
    grid(ui, id, |ui| {
        let mut ch = shader_row(ui, id, m, &shaders);
        ch |= match m.shader_kind() {
            _ if m.is_custom() => super::shader_material::material_fields(ui, state, id, m),
            ShaderKind::Standard => standard(ui, state, id, m),
            kind => unlit(ui, state, id, kind, m),
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
    let before = m.clone();
    if !material_fields(ui, state, &format!("mat_{rel}"), &mut m) {
        return;
    }
    // One undo step per interaction, holding the material from before it.
    let name = crate::assets::AssetDatabase::material_name(Some(rel));
    let label = crate::undo_names::struct_change(&name, &before, &m);
    state.undo.note_material(rel, &before);
    state.undo.mark_edited(&label);
    if let Err(err) = state.assets.update_material(rel, m) {
        state.console.warn(format!("Could not save {rel}: {err}"));
    }
}

/// Preview at the bottom of the material Inspector: drag to rotate, sphere, cube or plane.
pub fn preview(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let Some(m) = state.assets.materials.get(rel).cloned() else {
        return;
    };
    let previews = &mut state.project.thumbnails.materials;
    ui.add_space(6.0);
    ui.separator();
    ui.horizontal(|ui| {
        ui.label(crate::theme::bold(ui.ctx(), "Preview"));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right to left: shown as Sphere, Cube, Plane.
            for (shape, icon, tip) in [
                (PreviewShape::Plane, "▱", "Plane"),
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
