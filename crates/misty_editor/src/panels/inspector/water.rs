//! Water Surface and Floater Inspectors. The Water Surface editor starts with the shape, the
//! presets and a Calm ↔ Stormy slider, then collapsible sections of plain sliders (each with a
//! tooltip): Waves, Color & Clarity, Foam, Reflections & Refraction, Caustics, Underwater.

use std::ops::RangeInclusive;

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::water::{
    Floater, SpectrumKind, WaterKind, WaterPreset, WaterSettings, WaterSurface,
};

use super::editors::{color_rgb, combo, grid};
use crate::state::EditorState;

pub const NAMES: [&str; 2] = ["Water Surface", "Floater"];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Water Surface" => "🌊",
        "Floater" => "⚓",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Water Surface" => w.get::<&WaterSurface>(e).ok().map(|c| c.enabled),
        _ => w.get::<&Floater>(e).ok().map(|c| c.enabled),
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Water Surface" => w.get::<&mut WaterSurface>(e).map(|mut c| c.enabled = on),
        _ => w.get::<&mut Floater>(e).map(|mut c| c.enabled = on),
    };
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Water Surface" => edit_copy::<WaterSurface>(ui, state, e, water_surface),
        _ => edit_copy::<Floater>(ui, state, e, floater),
    }
}

fn edit_copy<T: misty_engine::scene::SceneComponent>(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    draw: fn(&mut Ui, &mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = draw(ui, &mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

/// Label + slider row with the same tooltip on both.
fn slider(ui: &mut Ui, label: &str, tip: &str, v: &mut f32, range: RangeInclusive<f32>) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = ui
        .add(egui::Slider::new(v, range))
        .on_hover_text(tip)
        .changed();
    ui.end_row();
    ch
}

fn color(ui: &mut Ui, label: &str, tip: &str, c: &mut [f32; 3]) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = color_rgb(ui, c);
    ui.end_row();
    ch
}

fn toggle(ui: &mut Ui, label: &str, tip: &str, v: &mut bool) -> bool {
    ui.label(label).on_hover_text(tip);
    let ch = ui.checkbox(v, "").on_hover_text(tip).changed();
    ui.end_row();
    ch
}

/// A collapsible section holding a property grid.
fn section(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), title))
        .id_salt(("water_section", title))
        .default_open(open)
        .show(ui, |ui| {
            ch = grid(ui, &format!("water_{title}"), body);
        });
    ch
}

fn water_surface(ui: &mut Ui, w: &mut WaterSurface) -> bool {
    let mut ch = grid(ui, "water_top", |ui| {
        let mut ch = false;
        ui.label("Type");
        ch |= combo(
            ui,
            "water_kind",
            &mut w.kind,
            &WaterKind::ALL,
            WaterKind::label,
        );
        ui.end_row();
        if w.kind != WaterKind::Ocean {
            ui.label("Size")
                .on_hover_text("Width (X) and length (Z) in metres");
            ui.horizontal(|ui| {
                for (axis, v) in ["X", "Z"].into_iter().zip(w.size.iter_mut()) {
                    ui.label(axis);
                    ch |= ui
                        .add(egui::DragValue::new(v).speed(0.2).range(0.5..=10_000.0))
                        .changed();
                }
            });
            ui.end_row();
        }
        let mut storm = w.settings.storminess();
        ui.label("Calm ↔ Stormy")
            .on_hover_text("Sets wind, wave height, chop, swell and foam together");
        if ui
            .add(egui::Slider::new(&mut storm, 0.0..=1.0).show_value(false))
            .on_hover_text("Sets wind, wave height, chop, swell and foam together")
            .changed()
        {
            w.settings.set_storminess(storm);
            ch = true;
        }
        ui.end_row();
        ch
    });
    ui.horizontal_wrapped(|ui| {
        ui.label("Presets");
        for p in WaterPreset::ALL {
            if ui
                .button(p.label())
                .on_hover_text("Replace the look and waves with this preset")
                .clicked()
            {
                w.settings = p.settings();
                ch = true;
            }
        }
    });
    ch | settings(ui, &mut w.settings, w.kind)
}

fn settings(ui: &mut Ui, s: &mut WaterSettings, kind: WaterKind) -> bool {
    let mut ch = section(ui, "Waves", true, |ui| {
        let mut ch = slider(
            ui,
            "Wind Speed",
            "Wind over the water in m/s: stronger wind makes bigger, longer, choppier waves",
            &mut s.wind_speed,
            0.5..=30.0,
        );
        ch |= slider(
            ui,
            "Wind Direction",
            "Degrees clockwise from +Z the waves travel towards",
            &mut s.wind_direction,
            0.0..=360.0,
        );
        ch |= slider(
            ui,
            "Wave Height",
            "Height of the wind waves (1 = a fully developed sea for this wind)",
            &mut s.wave_height,
            0.0..=2.5,
        );
        ch |= slider(
            ui,
            "Choppiness",
            "Sharper, pointier crests (0 = smooth rolling waves)",
            &mut s.choppiness,
            0.0..=2.0,
        );
        ui.label("Spectrum")
            .on_hover_text("JONSWAP: coherent, peaky waves. Phillips: a busier wind sea");
        ch |= combo(
            ui,
            "water_spectrum",
            &mut s.spectrum,
            &SpectrumKind::ALL,
            SpectrumKind::label,
        );
        ui.end_row();
        if kind != WaterKind::River {
            ch |= slider(
                ui,
                "Swell Height",
                "Long, smooth waves from distant storms, in metres",
                &mut s.swell_height,
                0.0..=5.0,
            );
            ch |= slider(
                ui,
                "Swell Length",
                "Distance between swell crests in metres",
                &mut s.swell_length,
                10.0..=400.0,
            );
            ch |= slider(
                ui,
                "Swell Direction",
                "Degrees clockwise from +Z the swell travels towards",
                &mut s.swell_direction,
                0.0..=360.0,
            );
        }
        ch |= slider(
            ui,
            "Ripples",
            "Fine detail in the reflections and highlights",
            &mut s.ripples,
            0.0..=2.0,
        );
        if kind == WaterKind::River {
            ch |= slider(
                ui,
                "Flow Speed",
                "How fast the river flows along the object's forward (blue) axis, m/s",
                &mut s.flow_speed,
                0.0..=6.0,
            );
        }
        ch
    });
    ch |= section(ui, "Color & Clarity", false, |ui| {
        let mut ch = color(
            ui,
            "Shallow Color",
            "Colour of thin, shallow water",
            &mut s.shallow_color,
        );
        ch |= color(
            ui,
            "Deep Color",
            "Colour the water turns towards with depth",
            &mut s.deep_color,
        );
        ch |= slider(
            ui,
            "Clarity",
            "How many metres you can see into the water",
            &mut s.clarity,
            0.2..=60.0,
        );
        ch |= slider(
            ui,
            "Scattering",
            "Sunlight glowing through thin wave crests",
            &mut s.scattering,
            0.0..=3.0,
        );
        ch |= color(
            ui,
            "Scattering Color",
            "Colour of the light glowing through crests",
            &mut s.scattering_color,
        );
        ch |= slider(
            ui,
            "Smoothness",
            "Gloss of the surface: higher gives a tighter, brighter sun glitter",
            &mut s.smoothness,
            0.5..=1.0,
        );
        ch
    });
    ch |= section(ui, "Foam", false, |ui| {
        let mut ch = slider(
            ui,
            "Crest Foam",
            "White caps where waves break",
            &mut s.crest_foam,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Shore Foam",
            "Foam where the water meets land or objects",
            &mut s.shore_foam,
            0.0..=1.0,
        );
        ch |= slider(
            ui,
            "Shore Foam Width",
            "Width of the shore foam band in metres",
            &mut s.shore_foam_width,
            0.05..=6.0,
        );
        ch |= color(ui, "Foam Color", "Colour of the foam", &mut s.foam_color);
        ch
    });
    ch |= section(ui, "Reflections & Refraction", false, |ui| {
        let mut ch = slider(
            ui,
            "Reflections",
            "Strength of the sky and scene reflections",
            &mut s.reflections,
            0.0..=1.5,
        );
        ch |= toggle(
            ui,
            "Screen Space Reflections",
            "Also reflect what is on screen (islands, boats); otherwise only the sky",
            &mut s.screen_space_reflections,
        );
        ch |= slider(
            ui,
            "Refraction",
            "How much the waves bend the view of what is underwater",
            &mut s.refraction,
            0.0..=1.5,
        );
        ch
    });
    ch |= section(ui, "Caustics", false, |ui| {
        let mut ch = slider(
            ui,
            "Intensity",
            "Brightness of the dancing light on underwater surfaces (0 = off)",
            &mut s.caustics,
            0.0..=3.0,
        );
        ch |= slider(
            ui,
            "Scale",
            "Size of the caustic pattern in metres",
            &mut s.caustics_scale,
            0.3..=20.0,
        );
        ch |= slider(
            ui,
            "Depth",
            "Caustics fade out below this depth (m)",
            &mut s.caustics_depth,
            0.5..=40.0,
        );
        ch
    });
    ch |= section(ui, "Underwater", false, |ui| {
        let mut ch = toggle(
            ui,
            "Enabled",
            "Tint, fog and wobble the view when the camera dives below the surface",
            &mut s.underwater,
        );
        ch |= color(
            ui,
            "Fog Color",
            "Colour of the water around you",
            &mut s.underwater_color,
        );
        ch |= slider(
            ui,
            "Visibility",
            "How far you can see underwater, in metres",
            &mut s.underwater_visibility,
            1.0..=100.0,
        );
        ch |= slider(
            ui,
            "Distortion",
            "Wobble of the view underwater",
            &mut s.underwater_distortion,
            0.0..=2.0,
        );
        ch
    });
    ch
}

fn floater(ui: &mut Ui, f: &mut Floater) -> bool {
    let ch = grid(ui, "floater_grid", |ui| {
        let mut ch = slider(
            ui,
            "Buoyancy",
            "Upward push when fully under water, in multiples of the weight (2 ≈ floats half under)",
            &mut f.buoyancy,
            0.0..=6.0,
        );
        for (label, tip, v) in [
            (
                "Size",
                "Probe box in local metres: probes sit at its eight corners",
                &mut f.size,
            ),
            ("Center", "Probe box centre in local space", &mut f.center),
        ] {
            ui.label(label).on_hover_text(tip);
            ui.horizontal(|ui| {
                for (axis, c) in ["X", "Y", "Z"]
                    .into_iter()
                    .zip([&mut v.x, &mut v.y, &mut v.z])
                {
                    ui.label(axis);
                    ch |= ui.add(egui::DragValue::new(c).speed(0.02)).changed();
                }
            });
            ui.end_row();
        }
        ch |= slider(
            ui,
            "Water Drag",
            "How quickly the water slows the object down",
            &mut f.water_drag,
            0.0..=10.0,
        );
        ch |= slider(
            ui,
            "Water Angular Drag",
            "How quickly the water stops the object spinning",
            &mut f.water_angular_drag,
            0.0..=10.0,
        );
        ch
    });
    ui.weak("Needs a Rigidbody; floats on any Water Surface while playing.");
    ch
}
