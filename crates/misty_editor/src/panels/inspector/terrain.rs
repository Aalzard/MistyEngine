//! Terrain and Terrain Collider Inspectors. The Terrain editor has an icon tool bar (Raise or
//! Lower, Set Height, Smooth, Paint Texture, Holes, Paint Details, Paint Trees, Place Objects,
//! Settings), the brush section (shape picker, size, opacity, falloff), the texture layers as a
//! palette of thumbnail cards with the selected layer's settings, and the terrain data settings;
//! Paint Texture also holds the collapsible procedural sections of `terrain_rules` (Auto Paint
//! Rules, Generate, Grass & Details, Cliffs, Color Harmony). Data edits go through
//! `terrain_tools` (one undo step each); component fields return `true` like other editors.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Terrain, TerrainCollider};
use misty_engine::terrain::{
    ALPHAMAP_RESOLUTIONS, HEIGHTMAP_RESOLUTIONS, LAYERS_PER_MAP, MAX_LAYERS, TerrainData,
    TerrainLayer, store,
};

use super::editors::grid;
use super::slots::texture_slot;
use crate::branding::icons::Icon;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::terrain_kit as kit;
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

/// `[` and `]` make the brush smaller and larger while a terrain brush tool is open (not while
/// typing in a field).
fn brush_shortcuts(ui: &Ui, state: &mut EditorState) {
    if !state.terrain_tools.tool.is_brush() || ui.ctx().egui_wants_keyboard_input() {
        return;
    }
    let (smaller, larger) = ui.input(|i| {
        (
            i.key_pressed(egui::Key::OpenBracket),
            i.key_pressed(egui::Key::CloseBracket),
        )
    });
    let size = &mut state.terrain_tools.size;
    if smaller {
        *size = (*size / 1.2).max(0.5);
    }
    if larger {
        *size = (*size * 1.2).min(300.0);
    }
}

/// The tool's icon in the tool bar.
pub fn tool_icon(tool: TerrainTool) -> Icon {
    match tool {
        TerrainTool::RaiseLower => Icon::TerrainRaise,
        TerrainTool::SetHeight => Icon::TerrainSetHeight,
        TerrainTool::Smooth => Icon::TerrainSmooth,
        TerrainTool::Paint => Icon::TerrainPaint,
        TerrainTool::Holes => Icon::TerrainHoles,
        TerrainTool::Details => Icon::TerrainGrass,
        TerrainTool::Trees => Icon::TerrainTree,
        TerrainTool::Place => Icon::TerrainPlace,
        TerrainTool::Settings => Icon::TerrainSettings,
    }
}

fn terrain(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut t) = state.scene.world.get::<&Terrain>(e).map(|c| (*c).clone()) else {
        return false;
    };
    misty_engine::profile_scope!("Inspector.Terrain");
    brush_shortcuts(ui, state);
    // The tool bar: one icon per tool (they wrap in a narrow Inspector).
    let current = state.terrain_tools.tool;
    let tools: Vec<kit::Tool> = TerrainTool::ALL
        .iter()
        .map(|tool| kit::Tool {
            icon: tool_icon(*tool),
            label: tool.label(),
            tip: tr(tool.help()),
            selected: *tool == current,
        })
        .collect();
    if let Some(i) = kit::tool_bar(ui, "terrain_tools", &tools, |ui, label, rect| {
        targets::header(ui, &format!("tool/{label}"), rect)
    }) {
        state.terrain_tools.tool = TerrainTool::ALL[i];
    }
    let tool = state.terrain_tools.tool;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        let accent = kit::accent(ui);
        crate::branding::icons::image(ui, tool_icon(tool), 18.0, accent);
        ui.label(
            egui::RichText::new(tr(tool.label()))
                .size(14.0)
                .strong()
                .color(ui.visuals().strong_text_color()),
        );
        // The World Builder generates whole terrains: make it easy to find from here.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let b = kit::icon_button(
                ui,
                Icon::TerrainRaise,
                &tr!("World Builder"),
                &tr!("Generate this terrain from a template, noise, erosion, stamps and biomes (opens the World Builder; creates one for this terrain if needed)"),
            );
            targets::header(ui, "World Builder", b.rect);
            if b.clicked() {
                crate::panels::world_builder::open_for_terrain(state, e);
            }
        });
    });
    ui.add(egui::Label::new(egui::RichText::new(tr(tool.help())).weak()).wrap());
    ui.add_space(2.0);

    let path = t.terrain_data.clone();
    let data = store::get(&path).map(|(d, _)| d);
    if data.is_none() && tool != TerrainTool::Settings {
        ui.colored_label(
            egui::Color32::from_rgb(230, 180, 60),
            match store::error(&path) {
                Some(err) if !path.is_empty() => {
                    tr!("Terrain data could not be loaded: {error}", error = err)
                }
                _ => tr!("Assign Terrain Data in the Settings tab."),
            },
        );
        return false;
    }
    let mut ch = false;
    match tool {
        TerrainTool::Settings => ch = settings(ui, state, &mut t, data.as_deref()),
        TerrainTool::Trees => {
            if let Some(d) = data.as_deref() {
                crate::terrain_trees_ui::trees_tab(ui, state, &path, d);
            }
        }
        TerrainTool::Place => {
            if let Some(d) = data.as_deref() {
                crate::terrain_place_ui::place_tab(ui, state, e, d);
            }
        }
        _ => {
            let d = data.as_deref();
            if tool == TerrainTool::Paint
                && let Some(d) = d
            {
                layers(ui, state, &path, d);
            }
            if tool == TerrainTool::Details
                && let Some(d) = d
            {
                crate::terrain_details_ui::details_palette(ui, state, &path, d);
            }
            brush_settings(ui, state, &path, d);
            if tool == TerrainTool::Details
                && let Some(d) = d
            {
                crate::terrain_details_ui::details_settings(ui, state, &path, d);
            }
            if tool == TerrainTool::Paint
                && let Some(d) = d
            {
                layer_settings(ui, state, &path, d);
                // Procedural painting: rules of the selected layer, Generate, grass, look.
                ui.add_space(kit::SECTION_GAP);
                let layer = state.terrain_tools.layer;
                crate::terrain_rules::layer_rules(ui, state, &path, d, layer);
                ui.add_space(4.0);
                crate::terrain_rules::generate_row(ui, state, &path, d);
                ui.add_space(4.0);
                crate::terrain_rules::details_section(ui, state, &path, d);
                crate::terrain_flowers_ui::flowers_section(ui, state, &path, d);
                crate::terrain_rules::cliffs_section(ui, state, &path, d);
                crate::terrain_rules::harmony_section(ui, state, &path, d);
            }
            if tool == TerrainTool::Holes
                && let Some(d) = d
            {
                holes_section(ui, state, &path, d);
            }
        }
    }
    if ch {
        let _ = state.scene.world.insert_one(e, t);
    }
    ch
}

/// Paint Holes: how much is cut out, and filling every hole.
fn holes_section(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let cut = d.holes.iter().filter(|h| **h != 0).count();
    kit::section(ui, "holes", Icon::TerrainHoles, &tr!("Holes"), |ui| {
        ui.weak(tr!(
            "{cut} of {total} cells cut out (holes are saved in the terrain data).",
            cut = cut,
            total = d.cells() * d.cells()
        ));
        if kit::confirm_button(
            ui,
            Icon::Fill,
            &tr!("Fill All Holes"),
            &tr!("Close every hole of this terrain"),
            &tr!("Fill all {count} holes of this terrain?", count = cut),
            &tr!("Fill"),
            cut > 0,
        ) {
            terrain_tools::begin_edit(state, path, "Fill Terrain Holes");
            store::modify(path, |d| d.holes.clear());
        }
    });
}

/// Brush shape, size, opacity and falloff (and the Set Height target), with the shape
/// picker's thumbnails drawn from the brush's own falloff curve.
fn brush_settings(ui: &mut Ui, state: &mut EditorState, path: &str, d: Option<&TerrainData>) {
    let max_height = d.map(|d| d.size.y).unwrap_or(1.0);
    let defaults = terrain_tools::TerrainTools::default();
    let mut flatten = false;
    kit::section(ui, "brush", Icon::Brush, &tr!("Brush"), |ui| {
        let tools = &mut state.terrain_tools;
        let strength = tools.strength;
        kit::row(
            ui,
            &tr!("Brush Shape"),
            &tr!("How the brush fades towards its rim, from a hard edge to a feathered one"),
            None,
            |ui| kit::brush_picker(ui, &mut tools.falloff, strength),
        );
        kit::slider(
            ui,
            &tr!("Brush Size"),
            &tr!("Brush diameter in metres. [ makes it smaller, ] larger."),
            &mut tools.size,
            0.5..=200.0,
            defaults.size,
            " m",
            true,
        );
        kit::mark_last_row(ui, "Brush Size");
        kit::slider(
            ui,
            &tr!("Opacity"),
            &tr!("How strongly and how fast the brush works"),
            &mut tools.strength,
            0.0..=1.0,
            defaults.strength,
            "",
            false,
        );
        kit::slider(
            ui,
            &tr!("Falloff"),
            &tr!("0 = hard edge, 1 = fades from the centre to the rim"),
            &mut tools.falloff,
            0.0..=1.0,
            defaults.falloff,
            "",
            false,
        );
        if tools.tool == TerrainTool::SetHeight {
            kit::slider(
                ui,
                &tr!("Height"),
                &tr!("The height Set Height moves the ground to"),
                &mut tools.target_height,
                0.0..=max_height,
                defaults.target_height.min(max_height),
                " m",
                false,
            );
            ui.add_space(4.0);
            flatten = kit::confirm_button(
                ui,
                Icon::TerrainSetHeight,
                &tr!("Flatten All"),
                &tr!("Set the whole terrain to the target height"),
                &tr!("Flatten the whole terrain to the target height?"),
                &tr!("Flatten"),
                d.is_some(),
            );
        }
    });
    if flatten && let Some(d) = d {
        let h = (state.terrain_tools.target_height / d.size.y).clamp(0.0, 1.0);
        terrain_tools::begin_edit(state, path, "Flatten Terrain");
        store::modify(path, |d| d.heights.fill(h));
    }
}

/// A texture layer's albedo thumbnail with its mean colour as a swatch.
fn layer_thumb(ui: &Ui, state: &mut EditorState, l: &TerrainLayer) -> kit::Thumb {
    let Some(rel) = l.diffuse.as_deref() else {
        return kit::Thumb::Icon {
            icon: Icon::Layers,
            pending: false,
        };
    };
    let abs = state.assets.absolute(rel);
    let tex = state.project.thumbnails.get(ui.ctx(), &abs);
    let swatch = state.project.thumbnails.terrain.swatch(ui.ctx(), &abs);
    match tex {
        Some(t) => kit::Thumb::Image(t.id(), swatch),
        None => kit::Thumb::Icon {
            icon: Icon::Layers,
            pending: false,
        },
    }
}

/// Move layer `from` to `to`: its splat channel moves with it.
pub(crate) fn move_layer(d: &mut TerrainData, from: usize, to: usize) {
    let n = d.layers.len();
    if from >= n || to >= n || from == to {
        return;
    }
    let texels = d.alphamap_resolution * d.alphamap_resolution;
    for t in 0..texels {
        let mut w = d.layer_weights(t);
        let v = w[..n].to_vec();
        let mut v2 = v.clone();
        let x = v2.remove(from);
        v2.insert(to, x);
        w[..n].copy_from_slice(&v2);
        d.set_layer_weights(t, &w);
    }
    let l = d.layers.remove(from);
    d.layers.insert(to, l);
    // The grass's "grow on" layer follows its layer.
    if let Some(on) = d.details.on_layer {
        let mut order: Vec<usize> = (0..n).collect();
        let m = order.remove(from);
        order.insert(to, m);
        d.details.on_layer = order.iter().position(|o| *o == on);
    }
}

/// Paint Texture palette: pick the layer to paint, add / duplicate / remove / reorder layers.
fn layers(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let n = d.layers.len();
    if state.terrain_tools.layer >= n && n > 0 {
        state.terrain_tools.layer = n - 1;
    }
    let selected = state.terrain_tools.layer;
    let version = store::get(path).map(|(_, v)| v).unwrap_or(0);
    let counts = state.project.thumbnails.terrain.counts(path, d, version);
    let full = n >= MAX_LAYERS;
    let section = kit::section(ui, "layers", Icon::Layers, &tr!("Layers"), |ui| {
        let top = ui.cursor().min;
        let cards: Vec<kit::Card> = (d.layers.iter().enumerate())
            .map(|(i, l)| {
                let cover = counts.layers.get(i).copied().unwrap_or(0.0);
                let tex = l
                    .diffuse
                    .as_deref()
                    .map(|p| p.to_owned())
                    .unwrap_or_else(|| tr!("No texture"));
                kit::Card {
                    name: l.name.clone(),
                    thumb: layer_thumb(ui, state, l),
                    badge: Some(format!("{:.0}%", cover * 100.0)),
                    tip: format!(
                        "{} {}: {tex}\n{}",
                        tr!("Layer"),
                        i + 1,
                        tr!(
                            "Covers {share}% of the terrain. Drag to reorder.",
                            share = format!("{:.0}", cover * 100.0)
                        )
                    ),
                    asset: l.diffuse.clone(),
                    can_duplicate: !full,
                    can_remove: true,
                    dimmed: false,
                }
            })
            .collect();
        let out = kit::palette(
            ui,
            "layers",
            &cards,
            Some(selected),
            Some(kit::AddCard {
                label: &tr!("Add Layer"),
                tip: &tr!(
                    "Add a texture layer (at most {max}); drop textures here to add them as layers",
                    max = MAX_LAYERS
                ),
                enabled: !full,
                target: "+ Add Layer",
            }),
            true,
            &|rel: &str| misty_engine::assets::is_texture(rel),
            &tr!("No layers yet: add one to paint it."),
            targets::header,
        );
        if out.add.as_ref().is_some_and(|r| r.clicked()) {
            add_layer(
                state,
                path,
                d,
                TerrainLayer {
                    name: format!("Layer {n}"),
                    ..Default::default()
                },
            );
        }
        for ev in out.events {
            match ev {
                kit::CardEvent::Select(i) => state.terrain_tools.layer = i,
                kit::CardEvent::Duplicate(i) => {
                    if let Some(l) = d.layers.get(i) {
                        let mut copy = l.clone();
                        copy.name = tr!("{name} Copy", name = copy.name);
                        add_layer(state, path, d, copy);
                    }
                }
                kit::CardEvent::Remove(i) => {
                    terrain_tools::begin_edit(state, path, "Remove Terrain Layer");
                    store::modify(path, |d| remove_layer(d, i));
                    if state.terrain_tools.layer >= i {
                        state.terrain_tools.layer = state.terrain_tools.layer.saturating_sub(1);
                    }
                }
                kit::CardEvent::Move { from, to } => {
                    terrain_tools::begin_edit(state, path, "Reorder Terrain Layers");
                    store::modify(path, |d| move_layer(d, from, to));
                    state.terrain_tools.layer = to;
                }
                kit::CardEvent::Locate(i) => {
                    if let Some(p) = d.layers.get(i).and_then(|l| l.diffuse.clone()) {
                        crate::terrain_details_ui::locate_asset(state, &p);
                    }
                }
                kit::CardEvent::Open(_) => {}
                kit::CardEvent::Drop(rel) => {
                    if !full {
                        add_layer(
                            state,
                            path,
                            d,
                            TerrainLayer {
                                name: crate::terrain_details_ui::stem(&rel),
                                diffuse: Some(rel),
                                ..Default::default()
                            },
                        );
                    }
                }
            }
        }
        egui::Rect::from_min_max(top, ui.cursor().min)
    });
    if let Some(rect) = section {
        targets::header(ui, "Layers", rect);
    }
}

/// Add `layer` at the end (layers 5, 9, 13 start another splat map) and select it.
fn add_layer(state: &mut EditorState, path: &str, d: &TerrainData, layer: TerrainLayer) {
    if d.layers.len() >= MAX_LAYERS {
        return;
    }
    terrain_tools::begin_edit(state, path, "Add Terrain Layer");
    let new_map = d.layers.len().is_multiple_of(LAYERS_PER_MAP) && !d.layers.is_empty();
    let add = |d: &mut TerrainData| {
        d.layers.push(layer);
        d.fit_splat_maps();
    };
    if new_map {
        store::modify(path, add);
    } else {
        store::modify_settings(path, add);
    }
    state.terrain_tools.layer = d.layers.len();
}

/// The selected layer's textures, tiling and surface.
fn layer_settings(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let selected = state.terrain_tools.layer;
    let Some(mut layer) = d.layers.get(selected).cloned() else {
        return;
    };
    let def = TerrainLayer::default();
    let textures = state.assets.textures.clone();
    let mut ch = false;
    // The layer, large, with its name and colour.
    ui.add_space(kit::SECTION_GAP * 0.5);
    let thumb = layer_thumb(ui, state, &layer);
    ui.horizontal(|ui| {
        let (rect, _) = ui.allocate_exact_size(egui::vec2(56.0, 56.0), egui::Sense::hover());
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(rect));
        child.set_clip_rect(ui.clip_rect().intersect(rect));
        kit::paint_thumb(&child, rect, &thumb);
        ui.vertical(|ui| {
            ui.label(crate::theme::bold(
                ui.ctx(),
                tr!("Layer {n}", n = selected + 1),
            ));
            ui.horizontal(|ui| {
                ch |= ui
                    .add(egui::TextEdit::singleline(&mut layer.name).desired_width(140.0))
                    .on_hover_text(tr!("Layer name"))
                    .changed();
            });
        });
    });
    kit::section(
        ui,
        "layer_textures",
        Icon::TerrainPaint,
        &tr!("Textures"),
        |ui| {
            ch |= kit::row(
                ui,
                &tr!("Diffuse"),
                &tr!("Albedo texture of this ground (drop a texture here)"),
                None,
                |ui| texture_slot(ui, "terrain_layer_diffuse", &mut layer.diffuse, &textures),
            )
            .0;
            kit::mark_last_row(ui, "Diffuse");
            ch |= kit::row(
                ui,
                &tr!("Normal Map"),
                &tr!("Bumps and detail lit by the sun; the number is its strength"),
                None,
                |ui| {
                    let mut c =
                        texture_slot(ui, "terrain_layer_normal", &mut layer.normal_map, &textures);
                    if layer.normal_map.is_some() {
                        c |= ui
                            .add(
                                egui::DragValue::new(&mut layer.normal_scale)
                                    .speed(0.01)
                                    .range(0.0..=4.0),
                            )
                            .on_hover_text(tr!("Normal map strength"))
                            .changed();
                    }
                    c
                },
            )
            .0;
            ch |= kit::row(
                ui,
                &tr!("Height Map"),
                &tr!("Greyscale heights used to blend layers (none = the albedo's brightness)"),
                None,
                |ui| texture_slot(ui, "terrain_layer_height", &mut layer.height_map, &textures),
            )
            .0;
        },
    );
    kit::section_closed(ui, "layer_tiling", Icon::Layers, &tr!("Tiling"), |ui| {
        for (label, tip, v, min, d0) in [
            (
                tr!("Tile Size"),
                tr!("World size of one texture repeat (metres along X and Z)"),
                &mut layer.tile_size,
                0.1,
                def.tile_size,
            ),
            (
                tr!("Tile Offset"),
                tr!("Shifts the tiling (metres along X and Z)"),
                &mut layer.tile_offset,
                f32::MIN,
                def.tile_offset,
            ),
        ] {
            let (c, reset) = kit::row(ui, &label, &tip, Some(*v == d0), |ui| {
                let mut c = false;
                for (axis, x) in ["X", "Z"].into_iter().zip(v.iter_mut()) {
                    ui.label(axis);
                    c |= ui
                        .add(
                            egui::DragValue::new(x)
                                .speed(0.05)
                                .range(min..=f32::MAX)
                                .suffix(" m"),
                        )
                        .on_hover_text(&tip)
                        .changed();
                }
                c
            });
            if reset {
                *v = d0;
            }
            ch |= c || reset;
        }
    });
    kit::section_closed(
        ui,
        "layer_surface",
        Icon::TerrainSettings,
        &tr!("Surface"),
        |ui| {
            ch |= kit::slider(
                ui,
                &tr!("Metallic"),
                &tr!("0 for ground and rock; 1 only for bare metal"),
                &mut layer.metallic,
                0.0..=1.0,
                def.metallic,
                "",
                false,
            );
            ch |= kit::slider(
                ui,
                &tr!("Smoothness"),
                &tr!("Rough (0) to glossy (1): wet rock, ice"),
                &mut layer.smoothness,
                0.0..=1.0,
                def.smoothness,
                "",
                false,
            );
        },
    );
    if ch {
        terrain_tools::begin_edit(state, path, "Change Terrain Layer");
        store::modify_settings(path, |d| d.layers[selected] = layer);
    }
}

/// Drop a layer and move its splat channel's weight to the remaining layers (later layers
/// move down a channel, across splat maps).
pub(crate) fn remove_layer(d: &mut TerrainData, index: usize) {
    if index >= d.layers.len() {
        return;
    }
    let n = d.alphamap_resolution * d.alphamap_resolution;
    let before: Vec<[u8; MAX_LAYERS]> = (0..n).map(|t| d.layer_weights(t)).collect();
    d.layers.remove(index);
    let count = d.layers.len();
    d.fit_splat_maps();
    for (t, px) in before.into_iter().enumerate() {
        let mut w: Vec<u8> = px.to_vec();
        w.remove(index);
        w.push(0);
        let sum: u32 = w.iter().take(count).map(|v| *v as u32).sum();
        let mut out = [0u8; MAX_LAYERS];
        // Remaining weights rescaled to 255; nothing left goes to the first layer.
        if let Some(scale) = (255 * 256u32).checked_div(sum) {
            for (o, v) in out.iter_mut().zip(&w).take(count) {
                *o = (*v as u32 * scale / 256) as u8;
            }
        }
        let total: u32 = out.iter().map(|v| *v as u32).sum();
        out[0] = (out[0] as u32 + 255 - total.min(255)) as u8;
        d.set_layer_weights(t, &out);
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
        ui.weak("Pick a .terrain asset (Object ▸ 3D Object ▸ Terrain makes one).");
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn removing_a_layer_moves_later_layers_down_across_maps() {
        let mut d = TerrainData::new(33, glam::Vec3::splat(32.0), 32);
        d.layers = (0..6).map(|_| TerrainLayer::default()).collect();
        d.fit_splat_maps();
        let mut w = [0u8; MAX_LAYERS];
        w[2] = 100;
        w[5] = 155;
        d.set_layer_weights(0, &w);
        // Layer 2 goes: layer 5 (second map) becomes layer 4.
        remove_layer(&mut d, 2);
        assert_eq!(d.layers.len(), 5);
        let after = d.layer_weights(0);
        assert!(after[4] >= 254, "{after:?}");
        assert_eq!(after.iter().map(|v| *v as u32).sum::<u32>(), 255);
        // Down to four layers: one map again.
        remove_layer(&mut d, 0);
        assert!(d.alphamap_extra.is_empty());
        assert!(d.layer_weights(0)[3] >= 254);
    }
}
