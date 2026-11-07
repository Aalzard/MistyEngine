//! Post-process Volume Inspector: priority, then one override section per effect with an
//! "active" checkbox in its header (fields are greyed out while the override is off).
//! The Fog and Sun Shafts (Screen) bodies live in `fog_editors`, Volumetric Clouds in `cloud_editors`.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::{PostProcessVolume, TonemapLook, TonemapMode};

use super::editors::{combo, grid};
use crate::state::EditorState;
use crate::tutorials::targets;

pub fn post_process_volume(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut v) = state
        .scene
        .world
        .get::<&PostProcessVolume>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = grid(ui, "ppv_grid", |ui| {
        ui.label("Mode");
        ui.weak("Global");
        ui.end_row();
        ui.label("Priority");
        let ch = ui
            .add(egui::DragValue::new(&mut v.priority).speed(0.1))
            .changed();
        ui.end_row();
        ch
    });
    ui.weak("The enabled volume with the highest priority is used.");

    let t = &mut v.tonemapping;
    ch |= section(ui, "Tonemapping", &mut t.active, |ui| {
        let mut ch = row(ui, "Mode", |ui| {
            combo(
                ui,
                "ppv_tonemap",
                &mut t.mode,
                &TonemapMode::ALL,
                TonemapMode::label,
            )
        });
        if t.mode == TonemapMode::Agx {
            ch |= row(ui, "Look", |ui| {
                combo(
                    ui,
                    "ppv_tonemap_look",
                    &mut t.look,
                    &TonemapLook::ALL,
                    TonemapLook::label,
                )
            });
        }
        ch
    });
    let b = &mut v.bloom;
    ch |= section(ui, "Bloom", &mut b.active, |ui| {
        let mut ch = row(ui, "Threshold", |ui| drag(ui, &mut b.threshold, 0.0..=10.0));
        ch |= row(ui, "Intensity", |ui| drag(ui, &mut b.intensity, 0.0..=20.0));
        ch |= row(ui, "Scatter", |ui| slider(ui, &mut b.scatter, 0.0..=1.0));
        ch |= row(ui, "Tint", |ui| color(ui, &mut b.tint));
        ch
    });
    let c = &mut v.color_adjustments;
    ch |= section(ui, "Color Adjustments", &mut c.active, |ui| {
        let mut ch = row(ui, "Post Exposure", |ui| {
            drag(ui, &mut c.post_exposure, -10.0..=10.0)
        });
        ch |= row(ui, "Contrast", |ui| {
            slider(ui, &mut c.contrast, -100.0..=100.0)
        });
        ch |= row(ui, "Temperature", |ui| {
            slider(ui, &mut c.temperature, -100.0..=100.0)
        });
        ch |= row(ui, "Tint", |ui| slider(ui, &mut c.tint, -100.0..=100.0));
        ch |= row(ui, "Color Filter", |ui| color(ui, &mut c.color_filter));
        ch |= row(ui, "Saturation", |ui| {
            slider(ui, &mut c.saturation, -100.0..=100.0)
        });
        ch
    });
    let g = &mut v.vignette;
    ch |= section(ui, "Vignette", &mut g.active, |ui| {
        let mut ch = row(ui, "Color", |ui| color(ui, &mut g.color));
        ch |= row(ui, "Intensity", |ui| {
            slider(ui, &mut g.intensity, 0.0..=1.0)
        });
        ch |= row(ui, "Smoothness", |ui| {
            slider(ui, &mut g.smoothness, 0.01..=1.0)
        });
        ch
    });
    // The bodies edit whole overrides, so the header toggles a copy of the flag.
    let mut active = v.fog.active;
    ch |= section(ui, "Fog", &mut active, |ui| {
        super::fog_editors::fog_override(ui, &mut v.fog)
    });
    v.fog.active = active;
    let mut active = v.sun_shafts.active;
    ch |= section(ui, "Sun Shafts (Screen)", &mut active, |ui| {
        super::fog_editors::sun_shafts_override(ui, &mut v.sun_shafts)
    });
    v.sun_shafts.active = active;
    let mut active = v.clouds.active;
    ch |= section(ui, "Volumetric Clouds", &mut active, |ui| {
        super::cloud_editors::clouds_override(ui, &mut v.clouds)
    });
    v.clouds.active = active;
    ch |= super::post_volume_screen::sections(ui, &mut v);
    ch |= super::post_volume_trace::sections(ui, &mut v);
    ch |= super::post_volume_fx::sections(ui, &mut v);
    if ch {
        let _ = state.scene.world.insert_one(e, v);
    }
    ch
}

/// Collapsible override with its active checkbox; the body is disabled while inactive.
pub(super) fn section(
    ui: &mut Ui,
    title: &str,
    active: &mut bool,
    body: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    let id = ui.make_persistent_id(("ppv_section", title));
    let state =
        egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, true);
    let mut ch = false;
    ui.add_space(2.0);
    let header = state.show_header(ui, |ui| {
        let check = ui
            .checkbox(active, "")
            .on_hover_text("Override this effect");
        ch |= check.changed();
        let label = ui.label(crate::theme::bold(ui.ctx(), title));
        // Tutorial target `…/<Section>`: checkbox and title.
        targets::row(ui, title, check.rect.union(label.rect));
    });
    let on = *active;
    header.body(|ui| {
        let _scope = targets::scope(title);
        ui.add_enabled_ui(on, |ui| {
            ch |= grid(ui, &format!("ppv_{title}"), body);
        });
    });
    ch
}

pub(super) fn row(ui: &mut Ui, label: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
    targets::label(ui, label);
    let ch = widget(ui);
    ui.end_row();
    ch
}

pub(super) fn drag(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(egui::DragValue::new(v).speed(0.01).range(range))
        .changed()
}

pub(super) fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(egui::Slider::new(v, range)).changed()
}

fn color(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut srgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed =
        crate::panels::inspector::card::wide(ui, |ui| ui.color_edit_button_srgb(&mut srgb))
            .changed();
    if changed {
        *c = srgb.map(|v| v as f32 / 255.0);
    }
    changed
}
