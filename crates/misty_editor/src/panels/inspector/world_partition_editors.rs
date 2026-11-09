//! Inspectors of the World Partition components: World Partition (grid, radii, budget, cells,
//! terrains to split into tiles, a button to the window), Streaming Source (radii) and Always
//! Loaded (a note).

use egui::{DragValue, Ui};
use misty_engine::hecs::Entity;
use misty_engine::world_partition::{StreamingSource, WorldPartition};

use super::editors::grid;
use crate::state::EditorState;

pub const NAMES: [&str; 3] = ["World Partition", "Streaming Source", "Always Loaded"];

pub fn icon(name: &str) -> Option<&'static str> {
    match name {
        "World Partition" => Some(crate::panels::world_partition::ICON),
        "Streaming Source" => Some("◎"),
        "Always Loaded" => Some("📌"),
        _ => None,
    }
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "World Partition" => w.get::<&WorldPartition>(e).ok().map(|c| c.enabled),
        "Streaming Source" => w.get::<&StreamingSource>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    match name {
        "World Partition" => {
            if let Ok(mut c) = w.get::<&mut WorldPartition>(e) {
                c.enabled = on;
            }
        }
        "Streaming Source" => {
            if let Ok(mut c) = w.get::<&mut StreamingSource>(e) {
                c.enabled = on;
            }
        }
        _ => {}
    }
}

fn metres(ui: &mut Ui, label: &str, tip: &str, v: &mut f32, min: f32) -> bool {
    ui.label(label).on_hover_text(tip);
    let c = ui
        .add(
            DragValue::new(v)
                .range(min..=100_000.0)
                .suffix(" m")
                .speed(1.0),
        )
        .on_hover_text(tip)
        .changed();
    ui.end_row();
    c
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "World Partition" => partition(ui, state, e),
        "Streaming Source" => source(ui, state, e),
        _ => {
            ui.weak(
                "Stays in the persistent scene when the world is partitioned: never streamed out.",
            );
            false
        }
    }
}

fn partition(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut wp) = state
        .scene
        .world
        .get::<&WorldPartition>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let ch = grid(ui, "world_partition", |ui| {
        let mut ch = false;
        ch |= metres(
            ui,
            "Cell Size",
            "Side of a grid cell. Convert Scene again after changing it.",
            &mut wp.cell_size,
            1.0,
        );
        ui.label("Grid Origin")
            .on_hover_text("World X / Z of the corner of cell (0, 0).");
        ui.horizontal(|ui| {
            ch |= ui
                .add(DragValue::new(&mut wp.origin[0]).prefix("X "))
                .changed();
            ch |= ui
                .add(DragValue::new(&mut wp.origin[1]).prefix("Z "))
                .changed();
        });
        ui.end_row();
        ui.label("Folder").on_hover_text(
            "Project folder of the cell sub-scenes (Cells), terrain tiles (Terrain) and HLODs.",
        );
        ch |= ui.text_edit_singleline(&mut wp.folder).changed();
        ui.end_row();
        ch |= metres(
            ui,
            "Load Radius",
            "Cells within this distance of a streaming source load.",
            &mut wp.load_radius,
            0.0,
        );
        ch |= metres(
            ui,
            "Unload Radius",
            "Cells unload once every source is farther than this (keep it above the load \
             radius, so a cell at the border doesn't load and unload over and over).",
            &mut wp.unload_radius,
            0.0,
        );
        ch |= metres(
            ui,
            "HLOD Distance",
            "Proxies of unloaded cells are drawn up to this distance (0 = always).",
            &mut wp.hlod_distance,
            0.0,
        );
        ui.label("Frame Budget").on_hover_text(
            "Main-thread time streaming may spend per frame adding and removing objects.",
        );
        ch |= ui
            .add(
                DragValue::new(&mut wp.budget_ms)
                    .range(0.1..=50.0)
                    .suffix(" ms")
                    .speed(0.05),
            )
            .changed();
        ui.end_row();
        ui.label("Parallel Loads")
            .on_hover_text("Cells read and parsed on background threads at once.");
        ch |= ui
            .add(DragValue::new(&mut wp.max_loads).range(1..=32))
            .changed();
        ui.end_row();
        ch
    });
    if wp.unload_radius < wp.load_radius {
        ui.colored_label(
            egui::Color32::from_rgb(220, 170, 70),
            "Unload Radius is below Load Radius: the load radius is used for both.",
        );
    }
    let with_hlod = wp.cells.iter().filter(|c| c.hlod).count();
    ui.label(format!(
        "{} cells · {} with an HLOD proxy · {} objects",
        wp.cells.len(),
        with_hlod,
        wp.cells.iter().map(|c| c.objects as u64).sum::<u64>()
    ));
    // Terrains too big for one cell can be split into tiles that stream like the rest.
    let big: Vec<(Entity, String)> = misty_engine::terrain::terrains(&state.scene)
        .into_iter()
        .filter(|(_, t)| {
            misty_engine::terrain::store::get(&t.terrain_data)
                .is_some_and(|(d, _)| d.size.x > wp.cell_size() * 1.01)
        })
        .map(|(t, _)| (t, state.scene.name(t)))
        .collect();
    for (t, name) in big {
        ui.horizontal(|ui| {
            ui.label(format!("'{name}' is larger than a cell."));
            if ui
                .button("Split into Tiles")
                .on_hover_text(
                    "One stitched tile terrain per cell, written under <Folder>/Terrain.",
                )
                .clicked()
            {
                crate::panels::world_partition::split_terrain(state, t);
            }
        });
    }
    if ui.button("Open World Streaming Window").clicked() {
        state.push(crate::actions::Action::FocusTab(
            crate::layout::Tab::WorldPartition,
        ));
    }
    if ch {
        let _ = state.scene.world.insert_one(e, wp);
    }
    ch
}

fn source(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut s) = state
        .scene
        .world
        .get::<&StreamingSource>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let ch = grid(ui, "streaming_source", |ui| {
        let mut ch = false;
        ch |= metres(
            ui,
            "Load Radius",
            "Cells within this distance load (0 = the World Streaming setting).",
            &mut s.load_radius,
            0.0,
        );
        ch |= metres(
            ui,
            "Unload Radius",
            "Cells beyond this distance unload (0 = the World Streaming setting).",
            &mut s.unload_radius,
            0.0,
        );
        ch
    });
    ui.weak("Without any Streaming Source, cells load around the main camera.");
    if ch {
        let _ = state.scene.world.insert_one(e, s);
    }
    ch
}
