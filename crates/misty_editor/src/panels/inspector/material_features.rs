//! The Standard material's optional features in the Properties view (Standard Lit v2), folded
//! into the material's sections:
//!
//! - Main Maps gets the Height Map (parallax) and the Detail Mask;
//! - Secondary Maps holds the detail albedo and normal with their own tiling;
//! - Advanced Surface holds Clear Coat, Translucency and Anisotropy, each a checkbox that
//!   opens its rows (off keeps the values and compiles nothing);
//! - Rendering Options holds the dielectric Specular and its tint.
//!
//! The height map and the detail maps have no checkbox: assigning a map turns the feature on
//! and clearing it turns it off. A file that has maps with the feature switched off (written
//! by an older editor) shows an "Enabled" row so it can be switched on.

use egui::Ui;
use misty_engine::material_features::{AnisotropyDirection, DetailMaps, Features, Parallax};

use super::editors::{color_rgb, combo};
use super::material::{number, pair, section, slider, toggle_row};
use super::texture_square::{map_label, normal_map_fix_row, row_label};
use crate::i18n::tr;
use crate::state::EditorState;

fn normal_scale(ui: &mut Ui, v: &mut f32) -> bool {
    number(
        ui,
        egui::DragValue::new(v).speed(0.01).range(-4.0..=4.0),
        &tr!("Normal map strength"),
    )
}

/// "Enabled" row of a feature whose maps are assigned while it is switched off.
fn enabled_row(ui: &mut Ui, on: &mut bool, tip: &str) -> bool {
    toggle_row(ui, &tr!("Enabled"), tip, 1, on)
}

/// Main Maps ▸ Height Map: the slot, the relief depth when a map is set, and the step count.
pub fn height_rows(ui: &mut Ui, state: &mut EditorState, id: &str, p: &mut Parallax) -> bool {
    let mut ch = false;
    let tip =
        tr!("Parallax occlusion mapping: the height map (white = high) gives the surface depth");
    if map_label(
        ui,
        state,
        &format!("{id}_height"),
        &tr!("Height Map"),
        &tip,
        &mut p.height_map,
    ) {
        p.enabled = p.height_map.is_some();
        ch = true;
    }
    if p.height_map.is_some() {
        ch |= slider(ui, &mut p.scale, 0.0..=0.1);
    }
    ui.end_row();
    if p.height_map.is_none() {
        return ch;
    }
    if !p.enabled {
        ch |= enabled_row(ui, &mut p.enabled, &tip);
    }
    row_label(ui, &tr!("Steps"), 1).on_hover_text(tr!(
        "Most steps along the view ray (more: smoother at grazing angles, costlier)"
    ));
    ch |= number(ui, egui::DragValue::new(&mut p.steps).range(4..=64), "");
    ui.end_row();
    ch
}

/// Main Maps ▸ Detail Mask (R masks the Secondary Maps; white where unassigned).
pub fn detail_mask_row(ui: &mut Ui, state: &mut EditorState, id: &str, d: &mut DetailMaps) -> bool {
    let ch = map_label(
        ui,
        state,
        &format!("{id}_dmask"),
        &tr!("Detail Mask"),
        &tr!("Where the Secondary Maps show (R; everywhere without a mask)"),
        &mut d.mask,
    );
    ui.end_row();
    ch
}

/// The Secondary Maps section: detail albedo and normal on their own tiling and offset.
pub fn secondary_maps(ui: &mut Ui, state: &mut EditorState, id: &str, d: &mut DetailMaps) -> bool {
    let mut ch = false;
    section(ui, &tr!("Secondary Maps"));
    let tip = tr!("A detail albedo (×2: grey changes nothing) and normal map on their own tiling");
    let mut maps = map_label(
        ui,
        state,
        &format!("{id}_dalbedo"),
        &tr!("Detail Albedo x2"),
        &tip,
        &mut d.albedo_map,
    );
    ui.end_row();
    maps |= map_label(
        ui,
        state,
        &format!("{id}_dnormal"),
        &tr!("Normal Map"),
        &tip,
        &mut d.normal_map,
    );
    if d.normal_map.is_some() {
        ch |= normal_scale(ui, &mut d.normal_scale);
    }
    ui.end_row();
    normal_map_fix_row(ui, state, d.normal_map.as_deref());
    let assigned = d.albedo_map.is_some() || d.normal_map.is_some();
    if maps {
        // Assigning a detail map turns the detail maps on; clearing both turns them off.
        d.enabled = assigned;
        ch = true;
    }
    if assigned && !d.enabled {
        ch |= enabled_row(ui, &mut d.enabled, &tip);
    }
    row_label(ui, &tr!("Tiling"), 0);
    ch |= pair(ui, &mut d.tiling);
    ui.end_row();
    row_label(ui, &tr!("Offset"), 0);
    ch |= pair(ui, &mut d.offset);
    ui.end_row();
    ch
}

/// The Advanced Surface section: Clear Coat, Translucency and Anisotropy.
pub fn advanced_surface(ui: &mut Ui, state: &mut EditorState, id: &str, f: &mut Features) -> bool {
    let mut ch = false;
    section(ui, &tr!("Advanced Surface"));

    let c = &mut f.clear_coat;
    ch |= toggle_row(
        ui,
        &tr!("Clear Coat"),
        &tr!("A clear lacquer over the surface (car paint, varnish)"),
        0,
        &mut c.enabled,
    );
    if c.enabled {
        row_label(ui, &tr!("Strength"), 1);
        ch |= slider(ui, &mut c.strength, 0.0..=1.0);
        ui.end_row();
        row_label(ui, &tr!("Smoothness"), 1);
        ch |= slider(ui, &mut c.smoothness, 0.0..=1.0);
        ui.end_row();
        ch |= map_label(
            ui,
            state,
            &format!("{id}_coatn"),
            &tr!("Normal Map"),
            &tr!("The coat's own normal map (none: the coat follows the geometry)"),
            &mut c.normal_map,
        );
        if c.normal_map.is_some() {
            ch |= normal_scale(ui, &mut c.normal_scale);
        }
        ui.end_row();
        normal_map_fix_row(ui, state, c.normal_map.as_deref());
    }

    let t = &mut f.translucency;
    ch |= toggle_row(
        ui,
        &tr!("Translucency"),
        &tr!("Light through thin or soft surfaces (leaves, skin, wax)"),
        0,
        &mut t.enabled,
    );
    if t.enabled {
        row_label(ui, &tr!("Color"), 1);
        ch |= color_rgb(ui, &mut t.color);
        ui.end_row();
        row_label(ui, &tr!("Strength"), 1).on_hover_text(tr!("Light coming through from behind"));
        ch |= slider(ui, &mut t.strength, 0.0..=1.0);
        ui.end_row();
        row_label(ui, &tr!("Wrap"), 1)
            .on_hover_text(tr!("How far light wraps round the shadow side"));
        ch |= slider(ui, &mut t.wrap, 0.0..=1.0);
        ui.end_row();
        ch |= map_label(
            ui,
            state,
            &format!("{id}_thick"),
            &tr!("Thickness"),
            &tr!("Thickness (R, white = thick: no light through), times the slider"),
            &mut t.thickness_map,
        );
        ch |= slider(ui, &mut t.thickness, 0.0..=1.0);
        ui.end_row();
    }

    let a = &mut f.anisotropy;
    ch |= toggle_row(
        ui,
        &tr!("Anisotropy"),
        &tr!("Highlights stretched along a direction (brushed metal, hair, satin)"),
        0,
        &mut a.enabled,
    );
    if a.enabled {
        row_label(ui, &tr!("Strength"), 1)
            .on_hover_text(tr!("Negative: stretched across the direction"));
        ch |= slider(ui, &mut a.strength, -1.0..=1.0);
        ui.end_row();
        row_label(ui, &tr!("Direction"), 1);
        ch |= combo(
            ui,
            &format!("{id}_aniso_dir"),
            &mut a.direction,
            &AnisotropyDirection::ALL,
            AnisotropyDirection::label,
        );
        ui.end_row();
        row_label(ui, &tr!("Rotation"), 1);
        ch |= number(
            ui,
            egui::DragValue::new(&mut a.rotation)
                .speed(1.0)
                .range(-180.0..=180.0)
                .suffix("°"),
            "",
        );
        ui.end_row();
    }
    ch
}

/// Rendering Options ▸ Specular: dielectric reflectance and its tint.
pub fn specular_rows(ui: &mut Ui, f: &mut Features) -> bool {
    let mut ch = false;
    row_label(ui, &tr!("Specular"), 0)
        .on_hover_text(tr!("Reflectance of non-metals (1 = the usual 4%)"));
    ch |= slider(ui, &mut f.specular, 0.0..=2.0);
    ui.end_row();
    row_label(ui, &tr!("Specular Tint"), 0)
        .on_hover_text(tr!("Colour of the reflections of non-metals"));
    ch |= color_rgb(ui, &mut f.specular_color);
    ui.end_row();
    ch
}
