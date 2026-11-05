//! Post-process Volume sections of the screen-space traced lighting: Screen Space Global
//! Illumination and Screen Space Reflections.

use egui::Ui;
use misty_engine::scene::{PostProcessVolume, ScreenSpaceQuality};

use super::card::grid_sub_header;
use super::editors::combo;
use super::post_volume::{drag, row, section, slider};

fn quality(ui: &mut Ui, id: &str, q: &mut ScreenSpaceQuality) -> bool {
    combo(
        ui,
        id,
        q,
        &ScreenSpaceQuality::ALL,
        ScreenSpaceQuality::label,
    )
}

pub fn sections(ui: &mut Ui, v: &mut PostProcessVolume) -> bool {
    let g = &mut v.screen_space_gi;
    let mut ch = section(
        ui,
        "Screen Space Global Illumination",
        &mut g.active,
        |ui| {
            let mut ch = row(ui, "Intensity", |ui| {
                slider(ui, &mut g.intensity, 0.0..=4.0)
            });
            ch |= row(ui, "Radius", |ui| drag(ui, &mut g.radius, 0.1..=20.0));
            ch |= row(ui, "Thickness", |ui| drag(ui, &mut g.thickness, 0.01..=5.0));
            ch |= row(ui, "Quality", |ui| {
                quality(ui, "ssgi_quality", &mut g.quality)
            });
            ch |= row(ui, "Full Resolution", |ui| {
                ui.checkbox(&mut g.full_resolution, "").changed()
            });
            grid_sub_header(ui, "Denoising");
            ch |= row(ui, "Temporal Accumulation", |ui| {
                ui.checkbox(&mut g.temporal, "").changed()
            });
            ch |= row(ui, "Spatial Denoise", |ui| {
                ui.checkbox(&mut g.denoise, "").changed()
            });
            grid_sub_header(ui, "Baked Lighting");
            ch |= row(ui, "Lightmapped Surfaces", |ui| {
                ui.checkbox(&mut g.lightmapped_surfaces, "")
                .on_hover_text("Also add the screen-space bounce on lightmapped surfaces (their lightmap already holds baked bounce light)")
                .changed()
            });
            ch
        },
    );
    if g.active {
        ui.weak("Bounces light seen on screen; off-screen light falls back to the ambient, light probes and reflection probes.");
    }
    let r = &mut v.screen_space_reflections;
    ch |= section(ui, "Screen Space Reflections", &mut r.active, |ui| {
        let mut ch = row(ui, "Intensity", |ui| {
            slider(ui, &mut r.intensity, 0.0..=1.0)
        });
        ch |= row(ui, "Max Roughness", |ui| {
            slider(ui, &mut r.max_roughness, 0.0..=1.0)
        });
        ch |= row(ui, "Thickness", |ui| drag(ui, &mut r.thickness, 0.01..=5.0));
        ch |= row(ui, "Max Distance", |ui| {
            drag(ui, &mut r.max_distance, 0.5..=1000.0)
        });
        ch |= row(ui, "Edge Fade", |ui| {
            slider(ui, &mut r.edge_fade, 0.0..=0.5)
        });
        ch |= row(ui, "Quality", |ui| {
            quality(ui, "ssr_quality", &mut r.quality)
        });
        ch |= row(ui, "Full Resolution", |ui| {
            ui.checkbox(&mut r.full_resolution, "").changed()
        });
        ch |= row(ui, "Temporal Stabilization", |ui| {
            ui.checkbox(&mut r.temporal, "").changed()
        });
        ch
    });
    ch
}
