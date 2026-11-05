//! Inspector editor of the Occlusion Area component (box size and centre, View Volume) and the
//! Static Editor Flags dropdown of the header.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::{GameObject, OcclusionArea, StaticFlags};

use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Occlusion Area"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Occlusion Area").then_some("◩")
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&OcclusionArea>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut OcclusionArea>(e)
        .map(|mut c| c.enabled = on);
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut a) = state
        .scene
        .world
        .get::<&OcclusionArea>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let mut ch = vec3_row(ui, "Size", &mut a.size, 0.05);
    ch |= vec3_row(ui, "Center", &mut a.center, 0.05);
    ch |= ui
        .checkbox(&mut a.is_view_volume, "Is View Volume")
        .changed();
    ui.weak("The camera can be inside: the occlusion bake makes view cells here.");
    if ch {
        a.size = a.size.map(|s| s.max(0.0));
        let _ = state.scene.world.insert_one(e, a);
    }
    ch
}

/// The "▾" menu beside Static: Nothing, Everything and each static use. Returns true when `go`
/// changed.
pub fn static_menu(ui: &mut Ui, go: &mut GameObject) -> bool {
    let mut changed = false;
    ui.menu_button("▾", |ui| {
        if ui.button("Nothing").clicked() {
            go.is_static = false;
            go.static_flags = StaticFlags::ALL;
            changed = true;
        }
        if ui.button("Everything").clicked() {
            go.is_static = true;
            go.static_flags = StaticFlags::ALL;
            changed = true;
        }
        ui.separator();
        for (flag, name) in StaticFlags::LIST {
            let mut on = go.has_static(flag);
            if ui.checkbox(&mut on, name).changed() {
                go.set_static_flag(flag, on);
                changed = true;
            }
        }
    })
    .response
    .on_hover_text("Static Editor Flags");
    changed
}
