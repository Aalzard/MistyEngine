//! The Terrain Inspector's Flowers section (Paint Texture tab, under Grass & Details): the
//! terrain's flower kinds (like the reference engine's detail prototypes: textures, how
//! common, size), where patches grow, and a Paint Flowers brush (Shift thins them out). Every
//! change is one undo step. Turning flowers on for a terrain without kinds adds the four
//! sample wildflowers (and writes their textures into `Assets/Flowers` when the project lacks
//! them).

use egui::{Grid, RichText, Ui};
use misty_engine::terrain::flowers::{self, FlowerKind, FlowerSettings};
use misty_engine::terrain::{TerrainData, store};

use crate::state::EditorState;
use crate::terrain_tools;

/// Where the sample wildflowers live in a project.
pub const FLOWER_DIR: &str = "Assets/Flowers";

macro_rules! flower_files {
    ($($name:literal),* $(,)?) => {
        [$(
            ($name, include_bytes!(concat!("../../../SampleProject/Assets/Flowers/", $name)) as &[u8]),
        )*]
    };
}

/// The sample wildflowers' textures, written into a project that lacks them.
const FLOWER_TEXTURES: [(&str, &[u8]); 16] = flower_files!(
    "Knapweed.png",
    "KnapweedNormal.png",
    "KnapweedRoughness.png",
    "KnapweedTranslucency.png",
    "Poppy.png",
    "PoppyNormal.png",
    "PoppyRoughness.png",
    "PoppyTranslucency.png",
    "Daisy.png",
    "DaisyNormal.png",
    "DaisyRoughness.png",
    "DaisyTranslucency.png",
    "Buttercup.png",
    "ButtercupNormal.png",
    "ButtercupRoughness.png",
    "ButtercupTranslucency.png",
);

/// Write the sample wildflower textures into the open project's `Assets/Flowers` (files that
/// exist are kept).
fn ensure_sample_textures(state: &mut EditorState) {
    let dir = misty_engine::assets::absolute(FLOWER_DIR);
    let mut wrote = 0;
    for (name, bytes) in FLOWER_TEXTURES {
        let path = dir.join(name);
        if path.exists() {
            continue;
        }
        let ok = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, bytes));
        match ok {
            Ok(()) => wrote += 1,
            Err(e) => state.console.warn(format!("{}: {e}", path.display())),
        }
    }
    if wrote > 0 {
        state.console.info(format!(
            "Added the sample wildflowers' textures to {FLOWER_DIR}."
        ));
    }
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

/// Flowers: kinds, patches and the Paint Flowers brush.
pub fn flowers_section(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let mut s = d.flowers.clone();
    let mut ch = false;
    let mut cleared = false;
    let textures = state.assets.textures.clone();
    egui::CollapsingHeader::new(RichText::new("Flowers").strong())
        .id_salt("terrain_flowers")
        .show(ui, |ui| {
            if !d.details.enabled {
                ui.weak("Flowers grow in the grass: enable Grass & Details first.");
            }
            if ui.checkbox(&mut s.enabled, "Enabled").changed() {
                ch = true;
                if s.enabled && s.kinds.is_empty() {
                    ensure_sample_textures(state);
                    s.kinds = FlowerSettings::wildflowers(FLOWER_DIR);
                }
            }
            ui.add_enabled_ui(s.enabled, |ui| {
                ch |= kinds_list(ui, &mut s, &textures);
                ch |= grid(ui, "terrain_flowers_grid", |ui| {
                    let mut ch = false;
                    let mut slider = |ui: &mut Ui,
                                      label: &str,
                                      tip: &str,
                                      v: &mut f32,
                                      range: std::ops::RangeInclusive<f32>,
                                      suffix: &str| {
                        ui.label(label).on_hover_text(tip);
                        ch |= ui
                            .add(egui::Slider::new(v, range).suffix(suffix))
                            .on_hover_text(tip)
                            .changed();
                        ui.end_row();
                    };
                    slider(
                        ui,
                        "Density",
                        "Flowers per square metre in the middle of a patch.",
                        &mut s.density,
                        0.5..=flowers::MAX_DENSITY,
                        " /m²",
                    );
                    slider(
                        ui,
                        "Patches",
                        "Share of the grass covered by flower patches.",
                        &mut s.coverage,
                        0.0..=0.5,
                        "",
                    );
                    slider(
                        ui,
                        "Patch Size",
                        "Size of the patches (each kind gathers in its own).",
                        &mut s.patch_scale,
                        2.0..=60.0,
                        " m",
                    );
                    slider(
                        ui,
                        "Lone Flowers",
                        "Single flowers between the patches.",
                        &mut s.scattered,
                        0.0..=0.05,
                        "",
                    );
                    slider(
                        ui,
                        "Meadow Edges",
                        "More flowers around trees and where the grass thins out.",
                        &mut s.edges,
                        0.0..=1.0,
                        "",
                    );
                    slider(
                        ui,
                        "LOD Distance",
                        "All flowers are drawn up to here; further away fewer, bigger ones.",
                        &mut s.lod_distance,
                        5.0..=100.0,
                        " m",
                    );
                    ch
                });
                ui.horizontal(|ui| {
                    let tools = &mut state.terrain_tools;
                    ui.checkbox(&mut tools.paint_flowers, "Paint Flowers").on_hover_text(
                        "The brush paints where flowers may grow (Shift + click thins them out) \
                         instead of the selected layer",
                    );
                    if !s.map.is_empty()
                        && ui
                            .button("Clear Paint")
                            .on_hover_text("Forget the painted flower mask (the rules alone)")
                            .clicked()
                    {
                        s.map.clear();
                        cleared = true;
                    }
                });
            });
        });
    if cleared {
        // The mask is not part of the settings the grass compares: re-scatter everything.
        terrain_tools::begin_edit(state, path, "Clear Flower Paint");
        store::modify(path, |d| d.flowers = s);
    } else if ch {
        terrain_tools::begin_edit(state, path, "Flowers");
        store::modify_settings(path, |d| d.flowers = s);
    }
}

/// The flower kinds: one collapsible row each (name, how common, size, textures), + / −.
fn kinds_list(ui: &mut Ui, s: &mut FlowerSettings, textures: &[String]) -> bool {
    let mut ch = false;
    let mut remove = None;
    for (i, k) in s.kinds.iter_mut().enumerate() {
        let id = format!("terrain_flower_kind_{i}");
        egui::CollapsingHeader::new(format!("🌼 {}", k.name))
            .id_salt(&id)
            .show(ui, |ui| {
                ch |= grid(ui, &id, |ui| kind_rows(ui, &id, k, textures));
                if ui.small_button("− Remove").clicked() {
                    remove = Some(i);
                }
            });
    }
    if let Some(i) = remove {
        s.kinds.remove(i);
        ch = true;
    }
    if s.kinds.len() < flowers::MAX_KINDS
        && ui
            .button("+ Add Flower")
            .on_hover_text(format!(
                "Up to {} kinds; each needs an atlas (head from above in the top quarter, \
                 the plant from the side below)",
                flowers::MAX_KINDS
            ))
            .clicked()
    {
        s.kinds.push(FlowerKind::default());
        ch = true;
    }
    ch
}

fn kind_rows(ui: &mut Ui, id: &str, k: &mut FlowerKind, textures: &[String]) -> bool {
    let mut ch = false;
    ui.label("Name");
    ch |= ui.text_edit_singleline(&mut k.name).changed();
    ui.end_row();
    ui.label("Amount")
        .on_hover_text("How common, relative to the other kinds");
    ch |= ui
        .add(egui::Slider::new(&mut k.amount, 0.0..=2.0))
        .changed();
    ui.end_row();
    ui.label("Height");
    ch |= crate::terrain_rules::range_slider(ui, &mut k.height, 0.05..=1.5, " m");
    ui.end_row();
    ui.label("Head Size")
        .on_hover_text("Diameter of the flower head");
    ch |= ui
        .add(egui::Slider::new(&mut k.head_size, 0.005..=0.2).suffix(" m"))
        .changed();
    ui.end_row();
    let mut slot = |ui: &mut Ui, label: &str, suffix: &str, v: &mut String| {
        ui.label(label);
        let mut o = (!v.is_empty()).then(|| v.clone());
        if crate::panels::texture_slot(ui, &format!("{id}_{suffix}"), &mut o, textures) {
            *v = o.unwrap_or_default();
            ch = true;
        }
        ui.end_row();
    };
    slot(ui, "Texture", "tex", &mut k.texture);
    slot(ui, "Normal", "normal", &mut k.normal);
    slot(ui, "Roughness", "rough", &mut k.roughness);
    slot(ui, "Translucency", "trans", &mut k.translucency);
    ch
}
