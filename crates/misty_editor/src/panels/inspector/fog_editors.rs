//! Inspector editors for volumetric fog: the Fog Volume component, the Light's volumetric
//! rows, and the Post-process Volume's Fog and Sun Shafts (Screen) overrides.

use egui::{DragValue, Grid, Slider, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    FogOverride, FogQuality, FogVolume, Light, SunShaftsOverride, SunShaftsQuality,
};

use super::editors::{color_rgb, combo};
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Fog Volume"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Fog Volume").then_some("☁")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&FogVolume>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut FogVolume>(e)
        .map(|mut c| c.enabled = on);
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut v) = state.scene.world.get::<&FogVolume>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let mut ch = vec3_row(ui, "Size", &mut v.size, 0.05);
    ch |= grid(ui, "fog_volume_grid", |ui| {
        let mut ch = row(ui, "Fog Distance", |ui| {
            drag(ui, &mut v.fog_distance, 0.05..=1e4, 0.05)
        });
        ch |= row(ui, "Albedo", |ui| color_rgb(ui, &mut v.albedo));
        ch |= row(ui, "Blend Distance", |ui| {
            drag(ui, &mut v.blend_distance, 0.0..=100.0, 0.02)
        });
        ch
    });
    ui.weak("Seen when the active volume's Fog override is Volumetric.");
    if ch {
        v.size = v.size.map(|s| s.max(0.0));
        let _ = state.scene.world.insert_one(e, v);
    }
    ch
}

/// Volumetric rows of the Light Inspector (inside its grid).
pub fn light_rows(ui: &mut Ui, l: &mut Light) -> bool {
    let mut ch = row(ui, "Volumetric Multiplier", |ui| {
        drag(ui, &mut l.volumetric_multiplier, 0.0..=16.0, 0.01)
    });
    ch |= row(ui, "Volumetric Shadow Dimmer", |ui| {
        ui.add(Slider::new(&mut l.volumetric_shadow_dimmer, 0.0..=1.0))
            .changed()
    });
    ch
}

/// Body of the Post-process Volume ▸ Fog section.
pub fn fog_override(ui: &mut Ui, f: &mut FogOverride) -> bool {
    let mut ch = row(ui, "Enable", |ui| ui.checkbox(&mut f.enabled, "").changed());
    ch |= row(ui, "Max Fog Distance", |ui| {
        drag(ui, &mut f.max_fog_distance, 1.0..=1e5, 1.0)
    });
    ch |= row(ui, "Fog Attenuation Distance", |ui| {
        drag(ui, &mut f.attenuation_distance, 0.05..=1e5, 0.5)
    });
    ch |= row(ui, "Base Height", |ui| {
        drag(ui, &mut f.base_height, -1e4..=1e4, 0.1)
    });
    ch |= row(ui, "Maximum Height", |ui| {
        drag(ui, &mut f.maximum_height, -1e4..=1e4, 0.1)
    });
    ch |= row(ui, "Albedo", |ui| color_rgb(ui, &mut f.albedo));
    ch |= row(ui, "Anisotropy", |ui| {
        ui.add(Slider::new(&mut f.anisotropy, -0.95..=0.95))
            .changed()
    });
    ch |= row(ui, "Ambient Light Probe Dimmer", |ui| {
        ui.add(Slider::new(&mut f.ambient_dimmer, 0.0..=1.0))
            .changed()
    });
    ch |= row(ui, "Volumetric", |ui| {
        ui.checkbox(&mut f.volumetric, "").changed()
    });
    let on = f.volumetric;
    ui.add_enabled_ui(on, |ui| ui.label("Volumetric Distance"));
    ch |= ui
        .add_enabled_ui(on, |ui| {
            drag(ui, &mut f.volumetric_distance, 1.0..=1e4, 0.5)
        })
        .inner;
    ui.end_row();
    ui.add_enabled_ui(on, |ui| ui.label("Quality"));
    ch |= ui
        .add_enabled_ui(on, |ui| {
            combo(
                ui,
                "fog_quality",
                &mut f.quality,
                &FogQuality::ALL,
                FogQuality::label,
            )
        })
        .inner;
    ui.end_row();
    ui.add_enabled_ui(on, |ui| {
        ui.label("Sun Shafts Quality").on_hover_text(
            "Sharp: the sun is raymarched per pixel through its shadow map for crisp light              shafts. Off: the sun lights the froxel grid like other lights (softer). The              Quality settings can turn Sharp off.",
        )
    });
    ch |= ui
        .add_enabled_ui(on, |ui| {
            combo(
                ui,
                "fog_sun_shafts_quality",
                &mut f.sun_shafts_quality,
                &SunShaftsQuality::ALL,
                SunShaftsQuality::label,
            )
        })
        .inner;
    ui.end_row();
    ui.add_enabled_ui(on, |ui| ui.label("Denoising"));
    ch |= ui
        .add_enabled_ui(on, |ui| ui.checkbox(&mut f.denoising, "Temporal").changed())
        .inner;
    ui.end_row();
    ui.add_enabled_ui(on, |ui| ui.label("Noise Intensity"));
    ch |= ui
        .add_enabled_ui(on, |ui| {
            ui.add(Slider::new(&mut f.noise_intensity, 0.0..=1.0))
                .changed()
        })
        .inner;
    ui.end_row();
    let noisy = on && f.noise_intensity > 0.0;
    ui.add_enabled_ui(noisy, |ui| ui.label("Noise Scale"));
    ch |= ui
        .add_enabled_ui(noisy, |ui| drag(ui, &mut f.noise_scale, 0.05..=1e3, 0.05))
        .inner;
    ui.end_row();
    ui.add_enabled_ui(noisy, |ui| ui.label("Noise Scroll Speed"));
    ch |= ui
        .add_enabled_ui(noisy, |ui| {
            let mut c = false;
            ui.horizontal(|ui| {
                for v in &mut f.noise_scroll {
                    c |= ui.add(DragValue::new(v).speed(0.02)).changed();
                }
            });
            c
        })
        .inner;
    ui.end_row();
    ch
}

/// Body of the Post-process Volume ▸ Sun Shafts (Screen) section.
pub fn sun_shafts_override(ui: &mut Ui, s: &mut SunShaftsOverride) -> bool {
    let mut ch = row(ui, "Intensity", |ui| {
        drag(ui, &mut s.intensity, 0.0..=20.0, 0.01)
    });
    ch |= row(ui, "Threshold", |ui| {
        drag(ui, &mut s.threshold, 0.0..=20.0, 0.01)
    });
    ch |= row(ui, "Length", |ui| {
        ui.add(Slider::new(&mut s.length, 0.0..=1.0)).changed()
    });
    ch |= row(ui, "Radius", |ui| {
        ui.add(Slider::new(&mut s.radius, 0.05..=2.0)).changed()
    });
    ch |= row(ui, "Tint", |ui| color_rgb(ui, &mut s.tint));
    ch
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(130.0)
        .show(ui, body)
        .inner
}

fn row(ui: &mut Ui, label: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::tutorials::targets::label(ui, label);
    let ch = widget(ui);
    ui.end_row();
    ch
}

fn drag(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, speed: f64) -> bool {
    ui.add(DragValue::new(v).speed(speed).range(range))
        .changed()
}
