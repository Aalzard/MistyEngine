//! Inspector editors for light glows: the Light's collapsible Halo section and the
//! Lens Flare component.

use egui::{CollapsingHeader, DragValue, Slider, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::light_glow::MAX_GHOSTS;
use misty_engine::scene::{LensFlare, Light, LightType};

use super::editors::{color_rgb, grid};
use super::mixed;
use crate::state::EditorState;

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Lens Flare"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Lens Flare").then_some("✴")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&LensFlare>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut LensFlare>(e)
        .map(|mut c| c.enabled = on);
}

/// Light ▸ Halo (point and spot lights), folded away until the halo is on.
pub fn halo_section(ui: &mut Ui, l: &mut Light) -> bool {
    if l.kind == LightType::Directional {
        return false;
    }
    let light_color = l.color;
    let h = &mut l.halo;
    CollapsingHeader::new("Halo")
        .id_salt("light_halo")
        .default_open(h.enabled)
        .show(ui, |ui| {
            grid(ui, "light_halo_grid", |ui| {
                ui.label("Enabled");
                let mut ch = mixed::checkbox(ui, "halo/enabled", &mut h.enabled, "").changed();
                ui.end_row();
                let on = h.enabled;
                ch |= row(ui, on, "Size", "Radius of the glow in metres.", |ui| {
                    let dv = DragValue::new(&mut h.size)
                        .speed(0.01)
                        .range(0.01..=100.0)
                        .suffix(" m");
                    ui.add(mixed::drag("halo/size", dv)).changed()
                });
                ch |= row(
                    ui,
                    on,
                    "Intensity",
                    "Multiplies the light's brightness.",
                    |ui| {
                        let dv = DragValue::new(&mut h.intensity)
                            .speed(0.01)
                            .range(0.0..=100.0);
                        ui.add(mixed::drag("halo/intensity", dv)).changed()
                    },
                );
                ch |= row(
                    ui,
                    on,
                    "Color",
                    "The light's colour, or a tint of its own.",
                    |ui| {
                        ui.horizontal(|ui| {
                            let mut own = h.color.is_some();
                            let mut c =
                                mixed::checkbox(ui, "halo/color", &mut own, "Custom").changed();
                            match (own, h.color.as_mut()) {
                                (true, Some(color)) => c |= color_rgb(ui, color),
                                (true, None) => h.color = Some(light_color),
                                (false, _) => h.color = None,
                            }
                            c
                        })
                        .inner
                    },
                );
                ch |= row(
                    ui,
                    on,
                    "Softness",
                    "0 = tight bright core, 1 = wide soft glow.",
                    |ui| {
                        let s = Slider::new(&mut h.softness, 0.0..=1.0);
                        ui.add(mixed::slider("halo/softness", s)).changed()
                    },
                );
                ch |= row(
                    ui,
                    on,
                    "Fog Response",
                    "How much fog at the light widens and brightens the halo.",
                    |ui| {
                        let s = Slider::new(&mut h.fog_response, 0.0..=2.0);
                        ui.add(mixed::slider("halo/fog_response", s)).changed()
                    },
                );
                ch
            })
        })
        .body_returned
        .unwrap_or(false)
}

/// A labelled grid row, greyed out while `on` is false.
fn row(
    ui: &mut Ui,
    on: bool,
    label: &str,
    tip: &str,
    widget: impl FnOnce(&mut Ui) -> bool,
) -> bool {
    ui.add_enabled_ui(on, |ui| ui.label(label).on_hover_text(tip));
    let ch = ui.add_enabled_ui(on, widget).inner;
    ui.end_row();
    ch
}

/// The Lens Flare component.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut f) = state.scene.world.get::<&LensFlare>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let ch = grid(ui, "lens_flare_grid", |ui| {
        let mut ch = row(
            ui,
            true,
            "Intensity",
            "Brightness of every element.",
            |ui| {
                let dv = DragValue::new(&mut f.intensity)
                    .speed(0.01)
                    .range(0.0..=50.0);
                ui.add(mixed::drag("intensity", dv)).changed()
            },
        );
        ch |= row(ui, true, "Scale", "Size of every element.", |ui| {
            let dv = DragValue::new(&mut f.scale).speed(0.01).range(0.05..=10.0);
            ui.add(mixed::drag("scale", dv)).changed()
        });
        ch |= row(
            ui,
            true,
            "Tint",
            "Multiplied with the light's colour.",
            |ui| color_rgb(ui, &mut f.tint),
        );
        ch |= row(
            ui,
            true,
            "Elements",
            "Which parts of the flare draw.",
            |ui| {
                ui.horizontal(|ui| {
                    let mut c = mixed::checkbox(ui, "ring", &mut f.ring, "Ring").changed();
                    c |= mixed::checkbox(ui, "streak", &mut f.streak, "Streak").changed();
                    c
                })
                .inner
            },
        );
        ch |= row(
            ui,
            true,
            "Ghosts",
            "Discs along the line through the screen centre.",
            |ui| {
                let s = Slider::new(&mut f.ghosts, 0..=MAX_GHOSTS);
                ui.add(mixed::slider("ghosts", s)).changed()
            },
        );
        ch
    });
    let lit = state.scene.world.get::<&Light>(e).is_ok();
    ui.weak(if lit {
        "Takes its colour from the Light. Off on Quality levels without Lens Flares."
    } else {
        "No Light on this object: the flare is white."
    });
    if ch {
        let _ = state.scene.world.insert_one(e, f);
    }
    ch
}
