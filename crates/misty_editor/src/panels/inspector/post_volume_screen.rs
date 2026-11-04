//! Post-process Volume sections of the screen-space lighting effects: Ambient Occlusion and
//! Contact Shadows (per-light toggles are in the Light Inspector).

use egui::Ui;
use misty_engine::scene::PostProcessVolume;

use super::post_volume::{drag, row, section, slider};

pub fn sections(ui: &mut Ui, v: &mut PostProcessVolume) -> bool {
    let a = &mut v.ambient_occlusion;
    let mut ch = section(ui, "Ambient Occlusion", &mut a.active, |ui| {
        let mut ch = row(ui, "Intensity", |ui| {
            slider(ui, &mut a.intensity, 0.0..=4.0)
        });
        ch |= row(ui, "Radius", |ui| drag(ui, &mut a.radius, 0.05..=10.0));
        ch |= row(ui, "Direct Lighting Strength", |ui| {
            slider(ui, &mut a.direct_lighting_strength, 0.0..=1.0)
        });
        ch |= row(ui, "Quality (Steps)", |ui| {
            ui.add(egui::Slider::new(&mut a.step_count, 2..=16))
                .changed()
        });
        ch |= row(ui, "Full Resolution", |ui| {
            ui.checkbox(&mut a.full_resolution, "").changed()
        });
        ch |= row(ui, "Temporal Accumulation", |ui| {
            ui.checkbox(&mut a.temporal, "").changed()
        });
        ch
    });
    let c = &mut v.contact_shadows;
    ch |= section(ui, "Contact Shadows", &mut c.active, |ui| {
        let mut ch = row(ui, "Length", |ui| drag(ui, &mut c.length, 0.0..=5.0));
        ch |= row(ui, "Distance Fade", |ui| {
            drag(ui, &mut c.distance_fade, 0.1..=1000.0)
        });
        ch |= row(ui, "Opacity", |ui| slider(ui, &mut c.opacity, 0.0..=1.0));
        ch |= row(ui, "Sample Count", |ui| {
            ui.add(egui::Slider::new(&mut c.sample_count, 4..=64))
                .changed()
        });
        ch
    });
    if c.active {
        ui.weak("Lights cast contact shadows when their Contact Shadows toggle is on.");
    }
    ch
}
