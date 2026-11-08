//! The Terrain Inspector's Place Objects tab: a palette of prefabs (rocks, bushes, props) to
//! scatter with the brush as scene objects under the terrain's "Placed Objects" child (left
//! click places, Shift erases), with spacing, random scale and turn, align-to-normal and slope
//! and height limits (`terrain::placement`). Each stroke is one undo step.

use egui::Ui;
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::terrain::placement::{self, ObjectPlacement};
use misty_engine::terrain::{TerrainData, store};

use crate::state::EditorState;

/// Icon of the tab and its list.
pub const ICON: &str = "🪨";

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body);
}

fn stem(p: &str) -> String {
    std::path::Path::new(p)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Every prefab asset of the project (the GUID database lists them).
fn prefab_assets() -> Vec<String> {
    let mut out = misty_engine::asset_db::with(|db| {
        db.paths()
            .filter(|p| p.ends_with(".prefab"))
            .map(str::to_owned)
            .collect::<Vec<String>>()
    })
    .unwrap_or_default();
    out.sort();
    out
}

fn range(ui: &mut Ui, v: &mut [f32; 2], lo: f32, hi: f32, speed: f64, suffix: &str) {
    ui.horizontal(|ui| {
        let (a, b) = (v[0], v[1]);
        ui.add(
            egui::DragValue::new(&mut v[0])
                .speed(speed)
                .range(lo..=b)
                .suffix(suffix),
        );
        ui.label("–");
        ui.add(
            egui::DragValue::new(&mut v[1])
                .speed(speed)
                .range(a..=hi)
                .suffix(suffix),
        );
    });
}

pub fn place_tab(ui: &mut Ui, state: &mut EditorState, e: Entity, d: &TerrainData) {
    let tools = &mut state.terrain_tools;
    if tools.prefab >= tools.prefabs.len() {
        tools.prefab = tools.prefabs.len().saturating_sub(1);
    }
    ui.add_space(4.0);
    ui.label(crate::theme::bold(ui.ctx(), "Prefabs"));
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        if tools.prefabs.is_empty() {
            ui.weak("No prefabs yet. Add a rock, bush or prop prefab to place it.");
        }
        for (i, p) in tools.prefabs.iter().enumerate() {
            if ui
                .selectable_label(i == tools.prefab, format!("{ICON} {}", stem(p)))
                .on_hover_text(p)
                .clicked()
            {
                tools.prefab = i;
            }
        }
    });
    ui.horizontal(|ui| {
        let mut add = None;
        egui::ComboBox::from_id_salt("terrain_add_prefab")
            .selected_text("+ Add Prefab")
            .show_ui(ui, |ui| {
                let assets = prefab_assets();
                if assets.is_empty() {
                    ui.weak("No prefabs in the project");
                }
                for a in &assets {
                    if ui
                        .selectable_label(false, stem(a))
                        .on_hover_text(a)
                        .clicked()
                    {
                        add = Some(a.clone());
                    }
                }
            });
        let tools = &mut state.terrain_tools;
        if let Some(a) = add
            && !tools.prefabs.contains(&a)
        {
            tools.prefabs.push(a);
            tools.prefab = tools.prefabs.len() - 1;
        }
        if ui
            .add_enabled(!tools.prefabs.is_empty(), egui::Button::new("− Remove"))
            .on_hover_text("Take this prefab off the palette (placed objects stay)")
            .clicked()
        {
            tools.prefabs.remove(tools.prefab);
            tools.prefab = tools.prefab.saturating_sub(1);
        }
    });

    ui.add_space(6.0);
    let max_h = d.size.y;
    let tools = &mut state.terrain_tools;
    grid(ui, "terrain_place_brush", |ui| {
        ui.label("Brush Size");
        ui.add(
            egui::Slider::new(&mut tools.size, 1.0..=200.0)
                .logarithmic(true)
                .suffix(" m"),
        );
        ui.end_row();
        let p: &mut ObjectPlacement = &mut tools.object_placement;
        ui.label("Density")
            .on_hover_text("Objects per 100 m² the brush fills up to");
        ui.add(egui::Slider::new(&mut p.density, 0.05..=50.0).logarithmic(true));
        ui.end_row();
        ui.label("Spacing")
            .on_hover_text("Closest two objects stand, centre to centre");
        ui.add(egui::Slider::new(&mut p.spacing, 0.1..=40.0).suffix(" m"));
        ui.end_row();
        ui.label("Scale")
            .on_hover_text("Random uniform scale of new objects");
        range(ui, &mut p.scale, 0.05, 10.0, 0.01, "");
        ui.end_row();
        ui.label("Random Rotation");
        ui.checkbox(&mut p.random_rotation, "");
        ui.end_row();
        ui.label("Align to Normal")
            .on_hover_text("Tilt with the ground instead of standing upright");
        ui.checkbox(&mut p.align_to_normal, "");
        ui.end_row();
        ui.label("Slope")
            .on_hover_text("Ground steepness objects go on");
        range(ui, &mut p.slope, 0.0, 90.0, 0.5, "°");
        ui.end_row();
        ui.label("Height")
            .on_hover_text("Terrain heights objects go at (above the terrain's base)");
        p.height[1] = p.height[1].min(max_h);
        range(ui, &mut p.height, 0.0, max_h, 0.1, " m");
        ui.end_row();
        ui.label("Erase Selected Only")
            .on_hover_text("Shift + click removes only instances of the selected prefab");
        ui.checkbox(&mut tools.erase_selected_only, "");
        ui.end_row();
    });

    let count = placement::placed(&state.scene, e).len();
    ui.horizontal(|ui| {
        ui.weak(format!("{count} objects placed on this terrain."));
        if ui
            .add_enabled(count > 0, egui::Button::new("Clear All"))
            .on_hover_text("Delete every placed object of this terrain")
            .clicked()
        {
            state.record("Clear Placed Objects");
            if let Some(h) = placement::holder(&mut state.scene, e, false) {
                state.scene.despawn(h);
            }
        }
    });
}

/// One brush step of Place Objects at terrain-local `at` (Shift = erase).
pub fn apply(state: &mut EditorState, e: Entity, path: &str, at: Vec2, erase: bool) {
    let Some((d, _)) = store::get(path) else {
        return;
    };
    let t = &state.terrain_tools;
    let radius = t.size * 0.5;
    let selected = t.prefabs.get(t.prefab).cloned();
    if erase {
        let only = selected.as_deref().filter(|_| t.erase_selected_only);
        let near = placement::placed(&state.scene, e)
            .iter()
            .any(|q| q.distance(at) <= radius);
        if !near {
            return;
        }
        if !state.terrain_tools.place_recorded {
            state.record("Erase Placed Objects");
            state.terrain_tools.place_recorded = true;
        }
        placement::erase(&mut state.scene, e, at, radius, only);
        return;
    }
    let Some(rel) = selected else {
        return;
    };
    let prefab = match state.assets.prefab(&rel) {
        Ok(p) => p,
        Err(err) => {
            state.console.warn(format!("Could not place {rel}: {err}"));
            state.terrain_tools.prefabs.retain(|p| *p != rel);
            return;
        }
    };
    let p = state.terrain_tools.object_placement;
    let step = state.terrain_tools.place_step;
    state.terrain_tools.place_step = step.wrapping_add(1);
    let existing = placement::placed(&state.scene, e);
    if placement::spots(&d, at, radius, &existing, &p, step).is_empty() {
        return;
    }
    if !state.terrain_tools.place_recorded {
        state.record("Place Objects");
        state.terrain_tools.place_recorded = true;
    }
    placement::place(&mut state.scene, e, &d, &prefab, &rel, at, radius, &p, step);
}
