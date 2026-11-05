//! Wind Zone Inspector: multipliers of the terrain grass's own wind (the object's blue axis is
//! where the wind blows).

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::WindZone;

use super::editors::grid;
use crate::state::EditorState;

pub const NAMES: [&str; 1] = ["Wind Zone"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Wind Zone").then_some("🌬")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&WindZone>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    if let Ok(mut c) = state.scene.world.get::<&mut WindZone>(e) {
        c.enabled = on;
    }
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut z) = state.scene.world.get::<&WindZone>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let ch = grid(ui, "wind_zone", |ui| {
        let mut ch = false;
        let row = |ui: &mut Ui, label: &str, tip: &str, v: &mut f32, max: f32| {
            ui.label(label).on_hover_text(tip);
            let c = ui
                .add(egui::Slider::new(v, 0.0..=max))
                .on_hover_text(tip)
                .changed();
            ui.end_row();
            c
        };
        ch |= row(
            ui,
            "Main",
            "× the grass wind strength (how far blades bend).",
            &mut z.main,
            4.0,
        );
        ch |= row(
            ui,
            "Turbulence",
            "× the fast flutter of single blades.",
            &mut z.turbulence,
            4.0,
        );
        ch |= row(
            ui,
            "Pulse Magnitude",
            "× the strength of the gust waves rolling across the field.",
            &mut z.pulse_magnitude,
            4.0,
        );
        ch |= row(
            ui,
            "Pulse Frequency",
            "× the speed the gust waves travel at.",
            &mut z.pulse_frequency,
            4.0,
        );
        ch
    });
    ui.weak("Blows along the object's blue (Z) axis; scales each terrain's grass wind.");
    if ch {
        let _ = state.scene.world.insert_one(e, z);
    }
    ch
}
