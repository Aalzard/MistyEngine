//! Biomes section: the default biome, the biome assets in `Assets/WorldBuilder/`, and an editor
//! for one biome (feature textures with tiling, scatter list, colour tone). Edits save the asset.

use std::path::Path;

use egui::{Grid, Ui};
use misty_engine::world_gen::biome::{self, Biome, Feature, Preset, ScatterItem, ScatterShape};
use misty_engine::world_gen::textures;

use crate::panels::AssetPayload;
use crate::state::EditorState;

/// Project paths of every `.biome` asset in the World Builder folder.
pub fn biome_paths(state: &EditorState) -> Vec<String> {
    let dir = state.project.root.join(biome::FOLDER);
    let mut out: Vec<String> = std::fs::read_dir(&dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().extension().is_some_and(|x| x == biome::EXTENSION))
        .map(|e| format!("{}/{}", biome::FOLDER, e.file_name().to_string_lossy()))
        .collect();
    out.sort();
    out
}

/// "Assets/WorldBuilder/Forest.biome" → "Forest".
pub fn display_name(path: &str) -> String {
    Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_owned())
}

/// Project paths of every prefab under Assets.
fn prefab_files(root: &Path) -> Vec<String> {
    fn walk(dir: &Path, root: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p.extension().is_some_and(|x| x == "prefab")
                && let Ok(rel) = p.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let mut out = Vec::new();
    walk(&root.join("Assets"), root, &mut out);
    out.sort();
    out
}

/// Write the six presets and their textures (existing files are kept).
pub fn ensure_presets(root: &Path) -> Result<usize, String> {
    let mut written = 0;
    for stem in biome::TEXTURES {
        let path = root.join(biome::texture_path(stem));
        if !path.exists() {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
            }
            if let Some(img) = textures::generate(stem, 256) {
                img.save(&path).map_err(|e| e.to_string())?;
                written += 1;
            }
        }
    }
    for p in Preset::ALL {
        let path = root.join(p.path());
        if !path.exists() {
            p.biome().save(&path)?;
            written += 1;
        }
    }
    Ok(written)
}

pub fn show(ui: &mut Ui, state: &mut EditorState, builder: misty_engine::hecs::Entity) {
    let paths = biome_paths(state);
    if paths.is_empty() {
        ui.weak("No biomes yet.");
    }
    let Ok(mut settings) = state
        .scene
        .world
        .get::<&misty_engine::world_gen::WorldBuilder>(builder)
        .map(|s| (*s).clone())
    else {
        return;
    };
    let mut changed = false;
    Grid::new("wb_biome_defaults")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, |ui| {
            ui.label("Default Biome");
            egui::ComboBox::from_id_salt("wb_default_biome")
                .selected_text(display_name(&settings.default_biome))
                .show_ui(ui, |ui| {
                    for p in &paths {
                        changed |= ui
                            .selectable_value(
                                &mut settings.default_biome,
                                p.clone(),
                                display_name(p),
                            )
                            .changed();
                    }
                });
            ui.end_row();
            ui.label("Edge Breakup");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut settings.biome_breakup)
                        .speed(0.2)
                        .range(0.0..=200.0)
                        .suffix(" m"),
                )
                .on_hover_text("Noise that breaks up biome and area edges")
                .changed();
            ui.end_row();
        });
    if changed {
        let _ = state.scene.world.insert_one(builder, settings);
        state.mark_edited("World Builder");
    }
    ui.horizontal_wrapped(|ui| {
        for p in &paths {
            let selected = state.world_builder.biome.as_deref() == Some(p.as_str());
            if ui.selectable_label(selected, display_name(p)).clicked() {
                state.world_builder.biome = (!selected).then(|| p.clone());
            }
        }
        if ui
            .small_button("+ Presets")
            .on_hover_text("Write the built-in biomes and their textures into Assets/WorldBuilder")
            .clicked()
        {
            match ensure_presets(&state.project.root) {
                Ok(n) => state.world_builder.status = format!("Wrote {n} preset files."),
                Err(e) => state.world_builder.status = e,
            }
            state.refresh_assets();
        }
    });
    if let Some(path) = state.world_builder.biome.clone() {
        ui.separator();
        edit_biome(ui, state, &path);
    }
}

fn edit_biome(ui: &mut Ui, state: &mut EditorState, rel: &str) {
    let abs = state.project.root.join(rel);
    let Ok(mut b) = Biome::load(&abs) else {
        ui.weak(format!("Could not read {rel}."));
        return;
    };
    let textures = state.assets.textures.clone();
    let mut ch = false;
    Grid::new("wb_biome")
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, |ui| {
            ui.label("Colour Tone");
            ch |= ui.color_edit_button_rgb(&mut b.tone).changed();
            ui.end_row();
            for f in Feature::ALL {
                ui.label(f.label());
                ui.horizontal(|ui| {
                    let l = b.layer_mut(f);
                    ch |= texture_slot(ui, ("wb_tex", f.label()), &mut l.texture, &textures);
                    ch |= ui
                        .add(
                            egui::DragValue::new(&mut l.tiling)
                                .speed(0.1)
                                .range(0.1..=500.0)
                                .suffix(" m"),
                        )
                        .on_hover_text("Tiling: world size of one repeat")
                        .changed();
                });
                ui.end_row();
            }
            for (label, v, max) in [
                ("Slope Angle", &mut b.slope_angle, 90.0),
                ("Cliff Angle", &mut b.cliff_angle, 90.0),
                ("Shore Width", &mut b.shore_width, 100.0),
            ] {
                ui.label(label);
                ch |= ui
                    .add(egui::DragValue::new(v).speed(0.2).range(0.0..=max))
                    .changed();
                ui.end_row();
            }
            ui.label("Snow / Peak Line");
            ch |= ui
                .add(egui::Slider::new(&mut b.peak_height, 0.0..=1.0))
                .on_hover_text("Fraction of the terrain's height")
                .changed();
            ui.end_row();
        });
    ui.add_space(4.0);
    ui.label(egui::RichText::new("Scatter").strong());
    let prefabs = prefab_files(&state.project.root);
    let mut remove = None;
    for (i, item) in b.scatter.iter_mut().enumerate() {
        egui::CollapsingHeader::new(format!("{} ({:.1} / 1000 m²)", item.name, item.density))
            .id_salt(("wb_item", i))
            .show(ui, |ui| {
                ch |= scatter_item(ui, item, i, &prefabs);
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
    }
    if let Some(i) = remove {
        b.scatter.remove(i);
        ch = true;
    }
    if ui.small_button("+ Add Scatter Item").clicked() {
        b.scatter.push(ScatterItem::default());
        ch = true;
    }
    if ch && let Err(e) = b.save(&abs) {
        state.world_builder.status = e;
    }
}

fn scatter_item(ui: &mut Ui, item: &mut ScatterItem, i: usize, prefabs: &[String]) -> bool {
    let mut ch = false;
    Grid::new(("wb_item_grid", i))
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(100.0)
        .show(ui, |ui| {
            ui.label("Name");
            ch |= ui.text_edit_singleline(&mut item.name).changed();
            ui.end_row();
            ui.label("Prefab");
            let none = "None (primitive shape)";
            let r = egui::ComboBox::from_id_salt(("wb_prefab", i))
                .selected_text(
                    item.prefab
                        .as_deref()
                        .map(display_name)
                        .unwrap_or(none.into()),
                )
                .show_ui(ui, |ui| {
                    ch |= ui.selectable_value(&mut item.prefab, None, none).changed();
                    for p in prefabs {
                        ch |= ui
                            .selectable_value(&mut item.prefab, Some(p.clone()), display_name(p))
                            .changed();
                    }
                })
                .response;
            if let Some(p) = r.dnd_release_payload::<AssetPayload>()
                && p.0.ends_with(".prefab")
            {
                item.prefab = Some(p.0.clone());
                ch = true;
            }
            ui.end_row();
            if item.prefab.is_none() {
                ui.label("Shape");
                ch |= super::stamp_ui::combo(
                    ui,
                    ("wb_shape", i),
                    &mut item.shape,
                    &ScatterShape::ALL,
                    |s| s.label(),
                );
                ui.end_row();
                ui.label("Colour");
                ch |= ui.color_edit_button_rgb(&mut item.color).changed();
                ui.end_row();
            }
            ui.label("Density");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut item.density)
                        .speed(0.05)
                        .range(0.0..=500.0)
                        .suffix(" /1000 m²"),
                )
                .changed();
            ui.end_row();
            ui.label("Min Spacing");
            ch |= ui
                .add(
                    egui::DragValue::new(&mut item.min_spacing)
                        .speed(0.05)
                        .range(0.25..=100.0)
                        .suffix(" m"),
                )
                .changed();
            ui.end_row();
            ui.label("Scale");
            ui.horizontal(|ui| {
                for v in &mut item.scale {
                    ch |= ui
                        .add(egui::DragValue::new(v).speed(0.01).range(0.05..=20.0))
                        .changed();
                }
            });
            ui.end_row();
            ui.label("Max Slope");
            ch |= ui
                .add(egui::Slider::new(&mut item.max_slope, 0.0..=90.0).suffix("°"))
                .changed();
            ui.end_row();
            ui.label("Height Band");
            ui.horizontal(|ui| {
                for v in &mut item.height_range {
                    ch |= ui
                        .add(egui::DragValue::new(v).speed(0.005).range(0.0..=1.0))
                        .changed();
                }
            });
            ui.end_row();
            ui.label("Random Rotation");
            ch |= ui.checkbox(&mut item.random_rotation, "").changed();
            ui.end_row();
            ui.label("Align to Ground");
            ch |= ui.checkbox(&mut item.align_to_normal, "").changed();
            ui.end_row();
        });
    ch
}

/// Texture dropdown that also takes a texture dragged from the Project window.
fn texture_slot(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    value: &mut Option<String>,
    textures: &[String],
) -> bool {
    let mut ch = false;
    let name = value.as_deref().map(display_name);
    let r = egui::ComboBox::from_id_salt(id)
        .selected_text(format!("🖼 {}", name.as_deref().unwrap_or("None")))
        .width(120.0)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(value, None, "None").changed();
            for t in textures {
                ch |= ui
                    .selectable_value(value, Some(t.clone()), display_name(t))
                    .on_hover_text(t)
                    .changed();
            }
        })
        .response;
    if let Some(p) = r.dnd_release_payload::<AssetPayload>()
        && misty_engine::assets::is_texture(&p.0)
    {
        *value = Some(p.0.clone());
        ch = true;
    }
    ch
}
