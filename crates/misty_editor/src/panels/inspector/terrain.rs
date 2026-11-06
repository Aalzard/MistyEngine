//! Terrain and Terrain Collider Inspectors. The Terrain editor has a row of tool tabs (Raise or
//! Lower, Set Height, Smooth, Paint Texture, Settings) with brush settings, the layer list and
//! the terrain data settings; Paint Texture also holds the collapsible procedural sections of
//! `terrain_rules` (Auto Paint Rules, Generate, Grass & Details, Cliffs, Color Harmony). Data edits go through `terrain_tools` (one undo step each);
//! component fields return `true` like other editors.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Terrain, TerrainCollider};
use misty_engine::terrain::{
    ALPHAMAP_RESOLUTIONS, HEIGHTMAP_RESOLUTIONS, MAX_LAYERS, TerrainData, TerrainLayer, store,
};

use super::editors::grid;
use super::slots::texture_slot;
use crate::state::EditorState;
use crate::terrain_tools::{self, TerrainTool};
use crate::tutorials::targets;

pub const NAMES: [&str; 2] = ["Terrain", "Terrain Collider"];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Terrain" => "⛰",
        "Terrain Collider" => "⬡",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Terrain" => w.get::<&Terrain>(e).ok().map(|c| c.enabled),
        _ => w.get::<&TerrainCollider>(e).ok().map(|c| c.enabled),
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Terrain" => w.get::<&mut Terrain>(e).map(|mut c| c.enabled = on),
        _ => w.get::<&mut TerrainCollider>(e).map(|mut c| c.enabled = on),
    };
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Terrain" => terrain(ui, state, e),
        _ => collider(ui, state, e),
    }
}

/// Every `.terrain` file under the project's Assets folder (project-relative). The GUID
/// database already lists every asset, so this costs no directory walk per frame (the
/// Inspector asks every frame a terrain is selected); without one the folder is walked.
fn terrain_assets(state: &EditorState) -> Vec<String> {
    let ext = format!(".{}", misty_engine::terrain::EXTENSION);
    let listed = misty_engine::asset_db::with(|db| {
        db.paths()
            .filter(|p| p.ends_with(&ext))
            .map(str::to_owned)
            .collect::<Vec<String>>()
    });
    if let Some(mut out) = listed {
        out.sort();
        return out;
    }
    fn walk(dir: &std::path::Path, root: &std::path::Path, out: &mut Vec<String>) {
        for entry in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let p = entry.path();
            if p.is_dir() {
                walk(&p, root, out);
            } else if p
                .extension()
                .is_some_and(|x| x == misty_engine::terrain::EXTENSION)
                && let Ok(rel) = p.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    let root = &state.project.root;
    let mut out = Vec::new();
    walk(&root.join("Assets"), root, &mut out);
    out.sort();
    out
}

/// Picker for a terrain data asset; `none` = label for the empty value (if allowed).
fn data_picker(
    ui: &mut Ui,
    id: &str,
    value: &mut String,
    assets: &[String],
    none: Option<&str>,
) -> bool {
    let name = |p: &str| {
        std::path::Path::new(p)
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default()
    };
    let shown = if value.is_empty() {
        none.unwrap_or("None").to_owned()
    } else {
        name(value)
    };
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(format!("⛰ {shown}"))
        .show_ui(ui, |ui| {
            if let Some(label) = none {
                ch |= ui.selectable_value(value, String::new(), label).changed();
            }
            for a in assets {
                ch |= ui
                    .selectable_value(value, a.clone(), name(a))
                    .on_hover_text(a)
                    .changed();
            }
        });
    ch
}

fn terrain(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut t) = state.scene.world.get::<&Terrain>(e).map(|c| (*c).clone()) else {
        return false;
    };
    // Tool tabs.
    ui.horizontal(|ui| {
        for tool in TerrainTool::ALL {
            let selected = state.terrain_tools.tool == tool;
            let b = egui::Button::new(RichText::new(tool.icon()).size(16.0))
                .selected(selected)
                .min_size(egui::vec2(34.0, 26.0));
            let b = ui.add(b);
            targets::header(ui, &format!("tool/{}", tool.label()), b.rect);
            if b.on_hover_text(tool.label()).clicked() {
                state.terrain_tools.tool = tool;
            }
        }
        // The World Builder generates whole terrains: make it easy to find from here.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let b = ui
                .add(egui::Button::new("⛰ World Builder").min_size(egui::vec2(0.0, 26.0)))
                .on_hover_text(
                    "Generate this terrain from a template, noise, erosion, stamps and biomes                      (opens the World Builder; creates one for this terrain if needed)",
                );
            targets::header(ui, "World Builder", b.rect);
            if b.clicked() {
                crate::panels::world_builder::open_for_terrain(state, e);
            }
        });
    });
    let tool = state.terrain_tools.tool;
    super::card::sub_header(ui, tool.label());
    ui.weak(tool.help());
    ui.add_space(4.0);

    let path = t.terrain_data.clone();
    let data = store::get(&path).map(|(d, _)| d);
    if data.is_none() && tool != TerrainTool::Settings {
        ui.colored_label(
            egui::Color32::from_rgb(230, 180, 60),
            match store::error(&path) {
                Some(err) if !path.is_empty() => format!("Terrain data could not be loaded: {err}"),
                _ => "Assign Terrain Data in the Settings tab.".into(),
            },
        );
        return false;
    }
    let mut ch = false;
    match tool {
        TerrainTool::Settings => ch = settings(ui, state, &mut t, data.as_deref()),
        _ => {
            brush_settings(ui, state, data.as_deref().map(|d| d.size.y).unwrap_or(1.0));
            if tool == TerrainTool::Paint
                && let Some(d) = data.as_deref()
            {
                layers(ui, state, &path, d);
                // Procedural painting: rules of the selected layer, Generate, grass, look.
                let layer = state.terrain_tools.layer;
                crate::terrain_rules::layer_rules(ui, state, &path, d, layer);
                ui.add_space(4.0);
                crate::terrain_rules::generate_row(ui, state, &path, d);
                ui.add_space(4.0);
                crate::terrain_rules::details_section(ui, state, &path, d);
                crate::terrain_rules::cliffs_section(ui, state, &path, d);
                crate::terrain_rules::harmony_section(ui, state, &path, d);
            }
            if tool == TerrainTool::SetHeight
                && let Some(d) = data.as_deref()
                && ui
                    .button("Flatten All")
                    .on_hover_text("Set the whole terrain to the target height")
                    .clicked()
            {
                let h = (state.terrain_tools.target_height / d.size.y).clamp(0.0, 1.0);
                terrain_tools::begin_edit(state, &path, "Flatten Terrain");
                store::modify(&path, |d| d.heights.fill(h));
            }
        }
    }
    if ch {
        let _ = state.scene.world.insert_one(e, t);
    }
    ch
}

fn brush_settings(ui: &mut Ui, state: &mut EditorState, max_height: f32) {
    let tools = &mut state.terrain_tools;
    grid(ui, "terrain_brush", |ui| {
        targets::label(ui, "Brush Size");
        ui.add(egui::Slider::new(&mut tools.size, 0.5..=200.0).logarithmic(true));
        ui.end_row();
        ui.label("Opacity");
        ui.add(egui::Slider::new(&mut tools.strength, 0.0..=1.0));
        ui.end_row();
        ui.label("Falloff");
        ui.add(egui::Slider::new(&mut tools.falloff, 0.0..=1.0))
            .on_hover_text("0 = hard edge, 1 = fades from the centre");
        ui.end_row();
        if tools.tool == TerrainTool::SetHeight {
            ui.label("Height");
            ui.add(
                egui::DragValue::new(&mut tools.target_height)
                    .speed(0.1)
                    .range(0.0..=max_height)
                    .suffix(" m"),
            );
            ui.end_row();
        }
        false
    });
}

/// Paint Texture layer list: pick the layer to paint, add/remove layers, edit the selected one.
fn layers(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    ui.add_space(6.0);
    let title = ui.label(crate::theme::bold(ui.ctx(), "Layers"));
    let selected = state.terrain_tools.layer;
    let list = egui::Frame::group(ui.style()).show(ui, |ui| {
        if d.layers.is_empty() {
            ui.weak("No layers yet. Add one to paint it.");
        }
        for (i, l) in d.layers.iter().enumerate() {
            let tex = l
                .diffuse
                .as_deref()
                .and_then(|p| std::path::Path::new(p).file_stem())
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "No texture".into());
            let text = format!("{}  {}   ({tex})", i + 1, l.name);
            if ui.selectable_label(selected == i, text).clicked() {
                state.terrain_tools.layer = i;
            }
        }
    });
    targets::header(ui, "Layers", title.rect.union(list.response.rect));
    let textures = state.assets.textures.clone();
    ui.horizontal(|ui| {
        let full = d.layers.len() >= MAX_LAYERS;
        let add = ui.add_enabled(!full, egui::Button::new("+ Add Layer"));
        targets::header(ui, "+ Add Layer", add.rect);
        if add
            .on_hover_text(format!("At most {MAX_LAYERS} layers"))
            .clicked()
        {
            terrain_tools::begin_edit(state, path, "Add Terrain Layer");
            store::modify_settings(path, |d| {
                d.layers.push(TerrainLayer {
                    name: format!("Layer {}", d.layers.len()),
                    ..Default::default()
                })
            });
            state.terrain_tools.layer = d.layers.len();
        }
        if ui
            .add_enabled(selected < d.layers.len(), egui::Button::new("− Remove"))
            .clicked()
        {
            terrain_tools::begin_edit(state, path, "Remove Terrain Layer");
            store::modify(path, |d| remove_layer(d, selected));
            state.terrain_tools.layer = selected.saturating_sub(1);
        }
    });
    let Some(mut layer) = d.layers.get(selected).cloned() else {
        return;
    };
    ui.add_space(4.0);
    let changed = grid(ui, "terrain_layer", |ui| {
        let mut ch = false;
        ui.label("Name");
        ch |= ui.text_edit_singleline(&mut layer.name).changed();
        ui.end_row();
        targets::label(ui, "Diffuse");
        ch |= texture_slot(ui, "terrain_layer_diffuse", &mut layer.diffuse, &textures);
        ui.end_row();
        ui.label("Normal Map");
        ui.horizontal(|ui| {
            ch |= texture_slot(ui, "terrain_layer_normal", &mut layer.normal_map, &textures);
            if layer.normal_map.is_some() {
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut layer.normal_scale)
                            .speed(0.01)
                            .range(0.0..=4.0),
                    )
                    .on_hover_text("Normal map strength")
                    .changed();
            }
        });
        ui.end_row();
        ui.label("Height Map");
        ch |= texture_slot(ui, "terrain_layer_height", &mut layer.height_map, &textures);
        ui.end_row();
        for (label, v, min) in [
            ("Tile Size", &mut layer.tile_size, 0.1),
            ("Tile Offset", &mut layer.tile_offset, f32::MIN),
        ] {
            ui.label(label);
            ui.horizontal(|ui| {
                for (axis, c) in ["X", "Z"].into_iter().zip(v.iter_mut()) {
                    ui.label(axis);
                    ch |= ui
                        .add(egui::DragValue::new(c).speed(0.05).range(min..=f32::MAX))
                        .changed();
                }
            });
            ui.end_row();
        }
        ui.label("Metallic");
        ch |= ui
            .add(egui::Slider::new(&mut layer.metallic, 0.0..=1.0))
            .changed();
        ui.end_row();
        ui.label("Smoothness");
        ch |= ui
            .add(egui::Slider::new(&mut layer.smoothness, 0.0..=1.0))
            .changed();
        ui.end_row();
        ch
    });
    if changed {
        terrain_tools::begin_edit(state, path, "Change Terrain Layer");
        store::modify_settings(path, |d| d.layers[selected] = layer);
    }
}

/// Drop a layer and move its splat channel's weight to the remaining layers.
fn remove_layer(d: &mut TerrainData, index: usize) {
    if index >= d.layers.len() {
        return;
    }
    d.layers.remove(index);
    let count = d.layers.len();
    for px in &mut d.alphamap {
        let mut w: Vec<u8> = px.to_vec();
        w.remove(index);
        w.push(0);
        let sum: u32 = w.iter().take(count).map(|v| *v as u32).sum();
        let mut out = [0u8; 4];
        // Remaining weights rescaled to 255; nothing left goes to the first layer.
        if let Some(scale) = (255 * 256u32).checked_div(sum) {
            for (o, v) in out.iter_mut().zip(&w).take(count) {
                *o = (*v as u32 * scale / 256) as u8;
            }
        }
        let total: u32 = out.iter().map(|v| *v as u32).sum();
        out[0] = (out[0] as u32 + 255 - total.min(255)) as u8;
        *px = out;
    }
}

/// Settings tab: data asset, shadows, size and resolutions.
fn settings(
    ui: &mut Ui,
    state: &mut EditorState,
    t: &mut Terrain,
    data: Option<&TerrainData>,
) -> bool {
    let assets = terrain_assets(state);
    let ch = grid(ui, "terrain_settings", |ui| {
        let mut ch = false;
        ui.label("Terrain Data");
        ch |= data_picker(ui, "terrain_data", &mut t.terrain_data, &assets, None);
        ui.end_row();
        ui.label("Cast Shadows");
        ch |= ui.checkbox(&mut t.cast_shadows, "").changed();
        ui.end_row();
        ch
    });
    let Some(d) = data else {
        ui.weak("Pick a .terrain asset (GameObject ▸ 3D Object ▸ Terrain makes one).");
        return ch;
    };
    let path = t.terrain_data.clone();
    ui.add_space(6.0);
    super::card::sub_header(ui, "Mesh Resolution (on Terrain Data)");
    let mut size = d.size;
    let mut res = d.heightmap_resolution;
    let mut ares = d.alphamap_resolution;
    let data_changed = grid(ui, "terrain_data_settings", |ui| {
        let mut ch = false;
        for (label, v) in [
            ("Terrain Width", &mut size.x),
            ("Terrain Length", &mut size.z),
            ("Terrain Height", &mut size.y),
        ] {
            ui.label(label);
            ch |= ui
                .add(
                    egui::DragValue::new(v)
                        .speed(0.5)
                        .range(1.0..=10000.0)
                        .suffix(" m"),
                )
                .changed();
            ui.end_row();
        }
        ui.label("Heightmap Resolution");
        ch |= resolution_combo(ui, "terrain_hres", &mut res, &HEIGHTMAP_RESOLUTIONS);
        ui.end_row();
        ui.label("Control Texture Resolution");
        ch |= resolution_combo(ui, "terrain_ares", &mut ares, &ALPHAMAP_RESOLUTIONS);
        ui.end_row();
        ch
    });
    if data_changed {
        terrain_tools::begin_edit(state, &path, "Change Terrain Settings");
        store::modify(&path, |d| {
            d.set_size(size);
            d.set_heightmap_resolution(res);
            d.set_alphamap_resolution(ares);
        });
    }
    ui.weak(format!(
        "{0}×{0} heights, {1}×{1} splat texels, {2} layer(s).",
        d.heightmap_resolution,
        d.alphamap_resolution,
        d.layers.len()
    ));
    ch
}

fn resolution_combo(ui: &mut Ui, id: &str, value: &mut usize, options: &[usize]) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(value.to_string())
        .show_ui(ui, |ui| {
            for o in options {
                ch |= ui.selectable_value(value, *o, o.to_string()).changed();
            }
        });
    ch
}

fn collider(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut c) = state
        .scene
        .world
        .get::<&TerrainCollider>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let assets = terrain_assets(state);
    let ch = grid(ui, "terrain_collider", |ui| {
        let mut ch = false;
        ui.label("Terrain Data");
        ch |= data_picker(
            ui,
            "terrain_col_data",
            &mut c.terrain_data,
            &assets,
            Some("Same as Terrain"),
        );
        ui.end_row();
        ui.label("Friction");
        ch |= ui
            .add(
                egui::DragValue::new(&mut c.surface.friction)
                    .speed(0.01)
                    .range(0.0..=10.0),
            )
            .changed();
        ui.end_row();
        ui.label("Bounciness");
        ch |= ui
            .add(egui::Slider::new(&mut c.surface.bounciness, 0.0..=1.0))
            .changed();
        ui.end_row();
        ch
    });
    if ch {
        let _ = state.scene.world.insert_one(e, c);
    }
    ch
}
