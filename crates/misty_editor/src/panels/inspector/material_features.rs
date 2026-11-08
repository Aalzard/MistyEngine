//! The Standard material's optional features in the Inspector (Standard Lit v2): Secondary
//! Maps (detail), Parallax, Clear Coat, Translucency and Anisotropy sections, each switched on
//! by a checkbox (off keeps the values and compiles nothing), plus Specular under Advanced.

use egui::Ui;
use misty_engine::material_features::{AnisotropyDirection, Features};

use super::editors::{color_rgb, combo};
use super::slots::texture_slot;

fn section(ui: &mut Ui, title: &str) {
    super::card::grid_sub_header(ui, title);
}

/// Slider over `range` filling the rest of the row.
fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.scope(|ui| {
        let gap = ui.spacing().item_spacing.x;
        let mut number = ui.spacing().interact_size.x;
        let mut w = ui.available_width() - number - gap;
        if w < 60.0 {
            number = number.min(44.0);
            w = ui.available_width() - number - gap;
        }
        let s = ui.spacing_mut();
        s.interact_size.x = number;
        s.slider_width = w.min(s.slider_width).max(30.0);
        ui.add(egui::Slider::new(v, range)).changed()
    })
    .inner
}

/// "Enabled" checkbox row; true when the section's fields should show.
fn toggle(ui: &mut Ui, label: &str, on: &mut bool, ch: &mut bool, tip: &str) -> bool {
    ui.label(label).on_hover_text(tip);
    *ch |= ui.checkbox(on, "").on_hover_text(tip).changed();
    ui.end_row();
    *on
}

fn map_row(
    ui: &mut Ui,
    label: &str,
    id: String,
    map: &mut Option<String>,
    textures: &[String],
    extra: impl FnOnce(&mut Ui, bool) -> bool,
) -> bool {
    ui.label(label);
    let mut ch = false;
    ui.horizontal(|ui| {
        ch |= texture_slot(ui, &id, map, textures);
        ch |= extra(ui, map.is_some());
    });
    ui.end_row();
    ch
}

fn normal_scale(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(egui::DragValue::new(v).speed(0.01).range(-4.0..=4.0))
        .on_hover_text("Normal map strength")
        .changed()
}

fn pair(ui: &mut Ui, label: &str, v: &mut [f32; 2]) -> bool {
    ui.label(label);
    let mut ch = false;
    ui.horizontal(|ui| {
        for (axis, c) in ["X", "Y"].into_iter().zip(v.iter_mut()) {
            ui.label(axis);
            ch |= ui
                .add(egui::DragValue::new(c).speed(0.01).max_decimals(3))
                .changed();
        }
    });
    ui.end_row();
    ch
}

/// The feature sections (after Main Maps). Returns true when something changed.
pub fn feature_sections(ui: &mut Ui, id: &str, f: &mut Features, textures: &[String]) -> bool {
    let mut ch = false;

    section(ui, "Secondary Maps");
    let d = &mut f.detail;
    if toggle(
        ui,
        "Detail Maps",
        &mut d.enabled,
        &mut ch,
        "A detail albedo (×2: grey changes nothing) and normal map on their own tiling",
    ) {
        ch |= map_row(
            ui,
            "    Detail Mask",
            format!("{id}_dmask"),
            &mut d.mask,
            textures,
            |_, _| false,
        );
        ch |= map_row(
            ui,
            "    Detail Albedo x2",
            format!("{id}_dalbedo"),
            &mut d.albedo_map,
            textures,
            |_, _| false,
        );
        ch |= map_row(
            ui,
            "    Normal Map",
            format!("{id}_dnormal"),
            &mut d.normal_map,
            textures,
            |ui, has| has && normal_scale(ui, &mut d.normal_scale),
        );
        ch |= pair(ui, "    Tiling", &mut d.tiling);
        ch |= pair(ui, "    Offset", &mut d.offset);
    }

    section(ui, "Parallax");
    let p = &mut f.parallax;
    if toggle(
        ui,
        "Height Map",
        &mut p.enabled,
        &mut ch,
        "Parallax occlusion mapping: the height map (white = high) gives the surface depth",
    ) {
        ch |= map_row(
            ui,
            "    Height",
            format!("{id}_height"),
            &mut p.height_map,
            textures,
            |ui, _| slider(ui, &mut p.scale, 0.0..=0.1),
        );
        ui.label("    Steps");
        ch |= ui
            .add(egui::DragValue::new(&mut p.steps).range(4..=64))
            .on_hover_text(
                "Most steps along the view ray (more: smoother at grazing angles, costlier)",
            )
            .changed();
        ui.end_row();
    }

    section(ui, "Clear Coat");
    let c = &mut f.clear_coat;
    if toggle(
        ui,
        "Clear Coat",
        &mut c.enabled,
        &mut ch,
        "A clear lacquer over the surface (car paint, varnish)",
    ) {
        ui.label("    Strength");
        ch |= slider(ui, &mut c.strength, 0.0..=1.0);
        ui.end_row();
        ui.label("    Smoothness");
        ch |= slider(ui, &mut c.smoothness, 0.0..=1.0);
        ui.end_row();
        ch |= map_row(
            ui,
            "    Normal Map",
            format!("{id}_coatn"),
            &mut c.normal_map,
            textures,
            |ui, has| has && normal_scale(ui, &mut c.normal_scale),
        );
    }

    section(ui, "Translucency");
    let t = &mut f.translucency;
    if toggle(
        ui,
        "Translucency",
        &mut t.enabled,
        &mut ch,
        "Light through thin or soft surfaces (leaves, skin, wax)",
    ) {
        ui.label("    Color");
        ch |= color_rgb(ui, &mut t.color);
        ui.end_row();
        ui.label("    Strength")
            .on_hover_text("Light coming through from behind");
        ch |= slider(ui, &mut t.strength, 0.0..=1.0);
        ui.end_row();
        ui.label("    Wrap")
            .on_hover_text("How far light wraps round the shadow side");
        ch |= slider(ui, &mut t.wrap, 0.0..=1.0);
        ui.end_row();
        ch |= map_row(
            ui,
            "    Thickness",
            format!("{id}_thick"),
            &mut t.thickness_map,
            textures,
            |ui, _| slider(ui, &mut t.thickness, 0.0..=1.0),
        );
    }

    section(ui, "Anisotropy");
    let a = &mut f.anisotropy;
    if toggle(
        ui,
        "Anisotropy",
        &mut a.enabled,
        &mut ch,
        "Highlights stretched along a direction (brushed metal, hair, satin)",
    ) {
        ui.label("    Strength")
            .on_hover_text("Negative: stretched across the direction");
        ch |= slider(ui, &mut a.strength, -1.0..=1.0);
        ui.end_row();
        ui.label("    Direction");
        ch |= combo(
            ui,
            &format!("{id}_aniso_dir"),
            &mut a.direction,
            &AnisotropyDirection::ALL,
            AnisotropyDirection::label,
        );
        ui.end_row();
        ui.label("    Rotation");
        ch |= ui
            .add(
                egui::DragValue::new(&mut a.rotation)
                    .speed(1.0)
                    .range(-180.0..=180.0)
                    .suffix("°"),
            )
            .changed();
        ui.end_row();
    }
    ch
}

/// Specular row of Advanced Options (dielectric reflectance and its tint).
pub fn specular_row(ui: &mut Ui, f: &mut Features) -> bool {
    let mut ch = false;
    ui.label("Specular")
        .on_hover_text("Reflectance of non-metals (1 = the usual 4%) and its tint");
    ui.horizontal(|ui| {
        ch |= ui
            .add(
                egui::DragValue::new(&mut f.specular)
                    .speed(0.01)
                    .range(0.0..=2.0),
            )
            .changed();
        ch |= color_rgb(ui, &mut f.specular_color);
    });
    ui.end_row();
    ch
}
