//! Light Probe Group Inspector: Edit Light Probes toggle, probe count, Add / Select All /
//! Delete / Duplicate, the selected probe's position and the Fill Box helper.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::light_probes::LightProbeGroup;

use super::card::{grid, sub_header};
use crate::light_probe_tools as tools;
use crate::state::EditorState;
use crate::widgets::vec3_row;

pub const NAMES: [&str; 1] = ["Light Probe Group"];

pub fn icon(name: &str) -> Option<&'static str> {
    NAMES.contains(&name).then_some("⁙")
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    (name == NAMES[0])
        .then(|| {
            state
                .scene
                .world
                .get::<&LightProbeGroup>(e)
                .ok()
                .map(|g| g.enabled)
        })
        .flatten()
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    if name == NAMES[0] {
        let _ = state
            .scene
            .world
            .get::<&mut LightProbeGroup>(e)
            .map(|mut g| g.enabled = on);
    }
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut g) = state
        .scene
        .world
        .get::<&LightProbeGroup>(e)
        .map(|g| (*g).clone())
    else {
        return false;
    };
    let editing = state.light_probes.editing && state.light_probes.entity == Some(e);
    ui.horizontal(|ui| {
        let label = if editing {
            "✔ Done Editing"
        } else {
            "✏ Edit Light Probes"
        };
        if ui
            .selectable_label(editing, label)
            .on_hover_text("Select, move, duplicate and delete probes in the Scene view")
            .clicked()
        {
            state.light_probes.set_editing(!editing);
            state.light_probes.entity = Some(e);
        }
    });
    grid(ui, "light_probe_group_info", |ui| {
        ui.label("Probes");
        ui.label(g.probes.len().to_string());
        ui.end_row();
        if editing {
            ui.label("Selected");
            ui.label(state.light_probes.selected.len().to_string());
            ui.end_row();
        }
        false
    });
    let mut ch = false;
    if editing {
        ui.horizontal_wrapped(|ui| {
            if ui
                .button("Add Probe")
                .on_hover_text("At the centre of the Scene view")
                .clicked()
            {
                tools::add_probe_at_view_center(state, e);
            }
            if ui.button("Select All").clicked() {
                state.light_probes.selected = (0..g.probes.len()).collect();
            }
            let any = !state.light_probes.selected.is_empty();
            if ui
                .add_enabled(any, egui::Button::new("Delete Selected"))
                .clicked()
            {
                tools::delete_selected(state, e);
            }
            if ui
                .add_enabled(any, egui::Button::new("Duplicate Selected"))
                .clicked()
            {
                tools::duplicate_selected(state, e);
            }
        });
        // Re-read: the buttons may have changed the probes.
        if let Ok(now) = state.scene.world.get::<&LightProbeGroup>(e) {
            g = (*now).clone();
        }
        let selected: Vec<usize> = state
            .light_probes
            .selected
            .iter()
            .copied()
            .filter(|i| *i < g.probes.len())
            .collect();
        if let [i] = selected[..] {
            sub_header(ui, &format!("Probe {i}"));
            let mut v = g.probes[i].to_array();
            if vec3_row(ui, "Position", &mut v, 0.05) {
                g.probes[i] = v.into();
                ch = true;
            }
        }
    }
    sub_header(ui, "Fill Box");
    let tools_state = &mut state.light_probes;
    let mut size = tools_state.fill_size.to_array();
    if vec3_row(ui, "Size", &mut size, 0.1) {
        tools_state.fill_size = glam::Vec3::from(size).max(glam::Vec3::ZERO);
    }
    grid(ui, "light_probe_fill", |ui| {
        ui.label("Spacing");
        ui.add(
            egui::DragValue::new(&mut tools_state.fill_spacing)
                .speed(0.05)
                .range(0.25..=50.0)
                .suffix(" m"),
        );
        ui.end_row();
        false
    });
    if ui
        .button("Fill Box")
        .on_hover_text("Replace the probes with a grid over the box, centred on this object")
        .clicked()
    {
        tools::fill_box(state, e);
        return false;
    }
    if ch {
        let _ = state.scene.world.insert_one(e, g);
    }
    ch
}
