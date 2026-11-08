//! The Terrain Inspector's Paint Details tab: the terrain grass and the terrain's extra grass
//! species and detail meshes (`terrain::detail_layers`) in one list, a Target Density for the
//! brush (left click paints towards it, Shift erases), Fill / Clear, and the selected layer's
//! settings. Every change is one undo step.

use egui::Ui;
use misty_engine::scene::{MeshRef, PrimitiveMesh};
use misty_engine::terrain::detail_layers::{
    self, DetailKind, DetailLayer, DetailTarget, MAX_DETAIL_LAYERS, MAX_MESH_DENSITY,
};
use misty_engine::terrain::details::MAX_DENSITY;
use misty_engine::terrain::{TerrainData, store};

use crate::state::EditorState;
use crate::terrain_tools;

/// The brush's target: list row 0 is the terrain grass, row `i + 1` is detail layer `i`.
pub fn target(row: usize) -> DetailTarget {
    match row {
        0 => DetailTarget::TerrainGrass,
        r => DetailTarget::Layer(r - 1),
    }
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui)) {
    egui::Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body);
}

fn edit(state: &mut EditorState, path: &str, label: &str, f: impl FnOnce(&mut TerrainData)) {
    terrain_tools::begin_edit(state, path, label);
    store::modify(path, f);
}

fn range(ui: &mut Ui, v: &mut [f32; 2], lo: f32, hi: f32, speed: f64) -> bool {
    ui.horizontal(|ui| {
        let (a, b) = (v[0], v[1]);
        let mut ch = ui
            .add(egui::DragValue::new(&mut v[0]).speed(speed).range(lo..=b))
            .changed();
        ui.label("–");
        ch |= ui
            .add(egui::DragValue::new(&mut v[1]).speed(speed).range(a..=hi))
            .changed();
        ch
    })
    .inner
}

fn icon(kind: DetailKind) -> &'static str {
    match kind {
        DetailKind::Grass => "🌾",
        DetailKind::Mesh => "▦",
    }
}

pub fn details_tab(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let rows = d.detail_layers.len() + 1;
    if state.terrain_tools.detail >= rows {
        state.terrain_tools.detail = rows - 1;
    }
    let selected = state.terrain_tools.detail;
    ui.add_space(4.0);
    ui.label(crate::theme::bold(ui.ctx(), "Details"));
    let counts = detail_layers::counts(d);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        let grass = if d.details.enabled {
            "🌱 Terrain Grass"
        } else {
            "🌱 Terrain Grass (off: Paint Texture ▸ Grass & Details)"
        };
        if ui
            .selectable_label(selected == 0, grass)
            .on_hover_text(
                "The terrain's own grass (Grass & Details); Generate fills it from its rules",
            )
            .clicked()
        {
            state.terrain_tools.detail = 0;
        }
        for (i, l) in d.detail_layers.iter().enumerate() {
            let text = format!("{} {}   ({})", icon(l.kind), l.name, counts[i]);
            if ui.selectable_label(selected == i + 1, text).clicked() {
                state.terrain_tools.detail = i + 1;
            }
        }
    });
    let full = d.detail_layers.len() >= MAX_DETAIL_LAYERS;
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!full, egui::Button::new("+ Add Grass"))
            .on_hover_text("Another grass species with its own height, colour and density")
            .clicked()
        {
            let n = d.detail_layers.len();
            edit(state, path, "Add Detail Grass", |d| {
                d.detail_layers
                    .push(DetailLayer::grass(&format!("Grass {}", n + 1)));
                // Species draw with the terrain grass.
                d.details.enabled = true;
            });
            state.terrain_tools.detail = n + 1;
        }
        if ui
            .add_enabled(!full, egui::Button::new("+ Add Detail Mesh"))
            .on_hover_text("Small meshes (pebbles, ferns, stones) painted with a density")
            .clicked()
        {
            let n = d.detail_layers.len();
            edit(state, path, "Add Detail Mesh", |d| {
                d.detail_layers.push(DetailLayer::mesh(
                    &format!("Detail Mesh {}", n + 1),
                    MeshRef::Primitive(PrimitiveMesh::Sphere),
                    None,
                ));
            });
            state.terrain_tools.detail = n + 1;
        }
        if ui
            .add_enabled(selected > 0, egui::Button::new("− Remove"))
            .on_hover_text("Remove this detail layer and what is painted with it")
            .clicked()
        {
            edit(state, path, "Remove Detail Layer", |d| {
                if selected - 1 < d.detail_layers.len() {
                    d.detail_layers.remove(selected - 1);
                }
            });
            state.terrain_tools.detail = selected - 1;
        }
    });

    // Brush target and whole-map actions.
    ui.add_space(6.0);
    let which = target(selected);
    grid(ui, "terrain_details_brush", |ui| {
        ui.label("Target Density")
            .on_hover_text("The density the brush paints towards (Shift + click erases)");
        ui.add(egui::Slider::new(
            &mut state.terrain_tools.detail_target,
            0.0..=1.0,
        ));
        ui.end_row();
    });
    ui.horizontal(|ui| {
        let target = state.terrain_tools.detail_target;
        if ui
            .button("Fill")
            .on_hover_text("Paint the target density over the whole terrain")
            .clicked()
        {
            edit(state, path, "Fill Details", |d| {
                detail_layers::fill(d, which, target)
            });
        }
        if ui
            .button("Clear")
            .on_hover_text("Erase this layer everywhere")
            .clicked()
        {
            edit(state, path, "Clear Details", |d| {
                detail_layers::fill(d, which, 0.0)
            });
        }
    });

    // The selected layer's settings.
    let Some(mut l) = d.detail_layers.get(selected.wrapping_sub(1)).cloned() else {
        ui.add_space(4.0);
        ui.weak(
            "The terrain grass's look and rules are under Paint Texture ▸ Grass & Details. \
             Add grass species or detail meshes to paint more kinds.",
        );
        return;
    };
    ui.add_space(6.0);
    let mut ch = false;
    grid(ui, "terrain_detail_layer", |ui| {
        ui.label("Name");
        ch |= ui.text_edit_singleline(&mut l.name).changed();
        ui.end_row();
        match l.kind {
            DetailKind::Grass => {
                ui.label("Density")
                    .on_hover_text("Blades per m² at full painted density");
                ch |= ui
                    .add(
                        egui::Slider::new(&mut l.density, 0.0..=MAX_DENSITY)
                            .logarithmic(true)
                            .suffix(" /m²"),
                    )
                    .changed();
                ui.end_row();
                ui.label("Height");
                ch |= range(ui, &mut l.height, 0.02, 3.0, 0.01);
                ui.end_row();
                ui.label("Blade Width");
                ch |= ui
                    .add(egui::Slider::new(&mut l.width, 0.005..=0.2).suffix(" m"))
                    .changed();
                ui.end_row();
                ui.label("Bend");
                ch |= ui.add(egui::Slider::new(&mut l.bend, 0.0..=1.5)).changed();
                ui.end_row();
                ui.label("Healthy ↔ Dry")
                    .on_hover_text("Between the terrain grass's healthy and dry colours");
                ch |= ui
                    .add(egui::Slider::new(&mut l.dryness, 0.0..=1.0))
                    .changed();
                ui.end_row();
                ui.label("Brightness");
                ch |= ui.add(egui::Slider::new(&mut l.tint, -1.0..=1.0)).changed();
                ui.end_row();
                ui.label("Seed Heads");
                ch |= ui
                    .add(egui::Slider::new(&mut l.seed_heads, 0.0..=1.0))
                    .changed();
                ui.end_row();
            }
            DetailKind::Mesh => {
                ui.label("Mesh");
                let choices = crate::panels::mesh_choices(state);
                let current = misty_engine::models::display_name(&l.mesh);
                egui::ComboBox::from_id_salt("terrain_detail_mesh")
                    .selected_text(format!("▦ {current}"))
                    .show_ui(ui, |ui| {
                        for (i, (r, label)) in choices.iter().enumerate() {
                            if i == PrimitiveMesh::ALL.len() {
                                ui.separator();
                            }
                            ch |= ui.selectable_value(&mut l.mesh, r.clone(), label).changed();
                        }
                    });
                ui.end_row();
                ui.label("Material");
                let materials: Vec<String> = state.assets.materials.keys().cloned().collect();
                let shown = l
                    .material
                    .as_deref()
                    .and_then(|p| std::path::Path::new(p).file_stem())
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "Default-Material".into());
                egui::ComboBox::from_id_salt("terrain_detail_material")
                    .selected_text(format!("◉ {shown}"))
                    .show_ui(ui, |ui| {
                        ch |= ui
                            .selectable_value(&mut l.material, None, "Default-Material")
                            .changed();
                        for m in &materials {
                            let name = std::path::Path::new(m)
                                .file_stem()
                                .map(|s| s.to_string_lossy().into_owned())
                                .unwrap_or_default();
                            ch |= ui
                                .selectable_value(&mut l.material, Some(m.clone()), name)
                                .on_hover_text(m)
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label("Density")
                    .on_hover_text("Meshes per m² at full painted density");
                ch |= ui
                    .add(
                        egui::Slider::new(&mut l.density, 0.001..=MAX_MESH_DENSITY)
                            .logarithmic(true)
                            .suffix(" /m²"),
                    )
                    .changed();
                ui.end_row();
                ui.label("Scale");
                ch |= range(ui, &mut l.scale, 0.05, 10.0, 0.01);
                ui.end_row();
                ui.label("Random Rotation");
                ch |= ui.checkbox(&mut l.random_rotation, "").changed();
                ui.end_row();
                ui.label("Align to Normal")
                    .on_hover_text("Tilt with the ground instead of standing upright");
                ch |= ui.checkbox(&mut l.align_to_normal, "").changed();
                ui.end_row();
                ui.label("Draw Distance");
                ch |= ui
                    .add(egui::Slider::new(&mut l.draw_distance, 5.0..=500.0).suffix(" m"))
                    .changed();
                ui.end_row();
                ui.label("Cast Shadows");
                ch |= ui.checkbox(&mut l.cast_shadows, "").changed();
                ui.end_row();
            }
        }
    });
    if ch {
        let i = selected - 1;
        terrain_tools::begin_edit(state, path, "Change Detail Layer");
        // Look only: no map changes, so no re-upload of the terrain's maps.
        store::modify_settings(path, |d| {
            if let Some(slot) = d.detail_layers.get_mut(i) {
                let map = std::mem::take(&mut slot.map);
                *slot = DetailLayer { map, ..l };
            }
        });
    }
}
