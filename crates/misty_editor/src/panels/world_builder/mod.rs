//! World Builder window (Window ▸ Terrain ▸ World Builder): one scrolling window of collapsible
//! sections — Base Heightmap (with erosion), Stamps & Areas, Biomes and Generate. It works on the
//! World Builder object of the open scene (or creates one, with a terrain), regenerates a
//! low-resolution preview while you edit and runs full generations on a worker thread.

mod biome_ui;
pub mod gizmos;
mod job;
mod stamp_ui;
#[cfg(test)]
mod tests;

use std::time::Instant;

use egui::{CollapsingHeader, Grid, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Terrain, TerrainCollider, Transform};
use misty_engine::terrain::{HEIGHTMAP_RESOLUTIONS, TerrainData, store};
use misty_engine::world_gen::{
    self, Generator, ScatterSet, ScatterStorage, Stages, WorldBuilder, WorldStamp,
};

use crate::state::EditorState;
use crate::terrain_tools::TerrainSnapshot;

/// Components with editors here (Inspector).
pub const NAMES: [&str; 3] = ["World Builder", "World Stamp", "Scatter"];

/// Window state (kept while the window is closed).
#[derive(Default)]
pub struct WorldBuilderWindow {
    /// egui frame the window was last drawn in.
    pub seen_frame: Option<u64>,
    /// Last result or problem.
    pub status: String,
    /// Stage timings of the last run (ms).
    pub timings: Vec<(&'static str, f32)>,
    /// Biome asset open in the Biomes section.
    pub biome: Option<String>,
    job: Option<job::Job>,
    /// Terrain data from before the first preview (undo and Revert go back to it).
    preview_base: Option<TerrainSnapshot>,
    /// Settings + stamps last generated or previewed (change detection).
    signature: Option<String>,
    changed_at: Option<Instant>,
    builder: Option<Entity>,
}

impl WorldBuilderWindow {
    pub fn busy(&self) -> bool {
        self.job.as_ref().is_some_and(|j| !j.preview())
    }
}

/// Whether the window was drawn this frame or the one before (gizmos follow it).
pub fn is_open(state: &EditorState, ctx: &egui::Context) -> bool {
    let now = ctx.cumulative_frame_nr();
    state.world_builder.seen_frame.is_some_and(|f| f + 1 >= now)
}

/// The builder the window edits: the selection's, else the last one, else the first.
pub fn current_builder(state: &EditorState) -> Option<Entity> {
    let scene = &state.scene;
    state
        .selection
        .active()
        .and_then(|e| world_gen::builder_of(scene, e))
        .or(state
            .world_builder
            .builder
            .filter(|b| scene.contains(*b) && scene.world.get::<&WorldBuilder>(*b).is_ok()))
        .or_else(|| world_gen::builders(scene).first().copied())
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.world_builder.seen_frame = Some(ui.ctx().cumulative_frame_nr());
    job::poll(state);
    if state.world_builder.job.is_some() {
        ui.ctx().request_repaint();
    }
    let Some(builder) = current_builder(state) else {
        empty(ui, state);
        return;
    };
    job::preview_tick(state, builder);
    if state.world_builder.changed_at.is_some() {
        ui.ctx().request_repaint();
    }
    header(ui, state, builder);
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            CollapsingHeader::new(RichText::new("1  Base Heightmap").strong())
                .default_open(true)
                .show(ui, |ui| base_section(ui, state, builder));
            CollapsingHeader::new(RichText::new("2  Stamps & Areas").strong())
                .default_open(true)
                .show(ui, |ui| stamps_section(ui, state, builder));
            CollapsingHeader::new(RichText::new("3  Biomes").strong())
                .default_open(true)
                .show(ui, |ui| biome_ui::show(ui, state, builder));
            CollapsingHeader::new(RichText::new("4  Generate").strong())
                .default_open(true)
                .show(ui, |ui| generate_section(ui, state, builder));
        });
}

/// No World Builder in the scene yet.
fn empty(ui: &mut Ui, state: &mut EditorState) {
    ui.add_space(8.0);
    ui.label("Build a whole terrain from a few settings: a base landform, erosion, stamps (mountains, rivers, lakes…) and biomes that texture it and scatter trees and rocks.");
    ui.add_space(6.0);
    let terrain = state
        .selection
        .active()
        .filter(|e| state.scene.world.get::<&Terrain>(*e).is_ok());
    let label = if terrain.is_some() {
        "Create World Builder for the Selected Terrain"
    } else {
        "Create World Builder (with a new Terrain)"
    };
    if ui.button(label).clicked() {
        create_builder(state, terrain);
    }
    if !state.world_builder.status.is_empty() {
        ui.weak(&state.world_builder.status);
    }
}

/// Add a "World Builder" object for `terrain` (or a new terrain) and make sure the biome presets
/// exist.
pub fn create_builder(state: &mut EditorState, terrain: Option<Entity>) -> Option<Entity> {
    let root = state.project.root.clone();
    if let Err(e) = biome_ui::ensure_presets(&root) {
        state.console.warn(format!("World Builder presets: {e}"));
    }
    let mut settings = WorldBuilder::default();
    state.record("Create World Builder");
    match terrain {
        Some(t) => {
            let path = state
                .scene
                .world
                .get::<&Terrain>(t)
                .ok()?
                .terrain_data
                .clone();
            if let Some((d, _)) = store::get(&path) {
                settings.size = d.size.to_array();
                settings.heightmap_resolution = d.heightmap_resolution;
                settings.alphamap_resolution = d.alphamap_resolution;
            }
            settings.terrain_data = path;
        }
        None => {
            let dir = root.join(world_gen::biome::FOLDER);
            let _ = std::fs::create_dir_all(&dir);
            let file =
                crate::asset_ops::unique_path(&dir, "World", misty_engine::terrain::EXTENSION);
            let rel = misty_engine::assets::relative(&file);
            let rel = file
                .strip_prefix(&root)
                .map(|r| r.to_string_lossy().replace('\\', "/"))
                .unwrap_or(rel);
            let size = Vec3::from(settings.size);
            let data = TerrainData::new(
                settings.heightmap_resolution,
                size,
                settings.alphamap_resolution,
            );
            if let Err(e) = data.save(&file) {
                state.console.warn(format!("Could not create {rel}: {e}"));
            }
            store::insert(&rel, data);
            let t = state.scene.spawn("Terrain", None);
            state.scene.set_transform(
                t,
                Transform::from_position(Vec3::new(-size.x * 0.5, 0.0, -size.z * 0.5)),
            );
            state.scene.add(
                t,
                Terrain {
                    terrain_data: rel.clone(),
                    ..Default::default()
                },
            );
            state.scene.add(t, TerrainCollider::default());
            settings.terrain_data = rel;
        }
    }
    let b = state.scene.spawn("World Builder", None);
    state.scene.add(b, settings);
    state.select(b);
    state.world_builder.builder = Some(b);
    state.world_builder.signature = None;
    state.refresh_assets();
    Some(b)
}

fn header(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    let path = state
        .scene
        .world
        .get::<&WorldBuilder>(builder)
        .map(|s| s.terrain_data.clone())
        .unwrap_or_default();
    ui.horizontal(|ui| {
        if ui
            .selectable_label(
                state.selection.contains(builder),
                format!("⛰ {}", state.scene.name(builder)),
            )
            .on_hover_text("Select the World Builder object")
            .clicked()
        {
            state.select(builder);
        }
        ui.weak(format!("→ {}", biome_ui::display_name(&path)));
        if world_gen::terrain_of(&state.scene, builder).is_none() {
            ui.colored_label(crate::theme::AXIS_X, "no terrain uses this asset");
        }
    });
}

/// Edit a copy of the builder's settings and write it back when changed.
fn with_settings(
    state: &mut EditorState,
    builder: Entity,
    f: impl FnOnce(&mut WorldBuilder) -> bool,
) {
    let Ok(mut s) = state
        .scene
        .world
        .get::<&WorldBuilder>(builder)
        .map(|s| (*s).clone())
    else {
        return;
    };
    if f(&mut s) {
        let _ = state.scene.world.insert_one(builder, s);
        state.mark_edited("World Builder");
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

fn base_section(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    with_settings(state, builder, |s| {
        let mut ch = grid(ui, "wb_base", |ui| {
            let mut ch = false;
            ui.label("Terrain Size");
            ui.horizontal(|ui| {
                for (axis, i) in [("W", 0), ("H", 1), ("L", 2)] {
                    ui.label(axis);
                    ch |= ui
                        .add(
                            egui::DragValue::new(&mut s.size[i])
                                .speed(1.0)
                                .range(10.0..=8000.0),
                        )
                        .changed();
                }
            });
            ui.end_row();
            ui.label("Resolution");
            egui::ComboBox::from_id_salt("wb_res")
                .selected_text(format!("{}²", s.heightmap_resolution))
                .show_ui(ui, |ui| {
                    for r in HEIGHTMAP_RESOLUTIONS.into_iter().filter(|r| *r >= 129) {
                        if ui
                            .selectable_value(&mut s.heightmap_resolution, r, format!("{r}²"))
                            .changed()
                        {
                            s.alphamap_resolution = (r - 1).min(512);
                            ch = true;
                        }
                    }
                });
            ui.end_row();
            let b = &mut s.base;
            ui.label("Generator");
            ch |= stamp_ui::combo(ui, "wb_gen", &mut b.generator, &Generator::ALL, |g| {
                g.label()
            });
            ui.end_row();
            ui.label("Height");
            ch |= ui
                .add(egui::Slider::new(&mut b.height, 0.0..=1.0))
                .changed();
            ui.end_row();
            ui.label("Scale");
            ch |= ui
                .add(
                    egui::Slider::new(&mut b.scale, 10.0..=3000.0)
                        .logarithmic(true)
                        .suffix(" m"),
                )
                .changed();
            ui.end_row();
            ui.label("Noise Amount");
            ch |= ui
                .add(egui::Slider::new(&mut b.noise_amount, 0.0..=1.5))
                .changed();
            ui.end_row();
            ui.label("Roughness");
            ch |= ui
                .add(egui::Slider::new(&mut b.octaves, 1..=10).suffix(" octaves"))
                .changed();
            ui.end_row();
            ui.label("Ridge Sharpness");
            ch |= ui
                .add(egui::Slider::new(&mut b.ridge_sharpness, 0.5..=5.0))
                .changed();
            ui.end_row();
            ui.label("Seed");
            ui.horizontal(|ui| {
                ch |= ui.add(egui::DragValue::new(&mut b.seed)).changed();
                if ui.button("🎲 Randomize").clicked() {
                    b.seed = random_seed(b.seed);
                    ch = true;
                }
            });
            ui.end_row();
            ch
        });
        CollapsingHeader::new("Erosion")
            .default_open(true)
            .show(ui, |ui| {
                ch |= grid(ui, "wb_erosion", |ui| {
                    let e = &mut s.erosion;
                    let mut ch = false;
                    ui.label("Hydraulic");
                    ch |= ui.checkbox(&mut e.hydraulic, "Water droplets").changed();
                    ui.end_row();
                    if e.hydraulic {
                        ui.label("Iterations");
                        ch |= ui
                            .add(egui::Slider::new(&mut e.hydraulic_iterations, 1..=30))
                            .on_hover_text(
                                "Droplet passes (one droplet per 8 heightmap samples each)",
                            )
                            .changed();
                        ui.end_row();
                        ui.label("Strength");
                        ch |= ui
                            .add(egui::Slider::new(&mut e.hydraulic_strength, 0.0..=1.0))
                            .changed();
                        ui.end_row();
                    }
                    ui.label("Thermal");
                    ch |= ui.checkbox(&mut e.thermal, "Slopes settle").changed();
                    ui.end_row();
                    if e.thermal {
                        ui.label("Iterations");
                        ch |= ui
                            .add(egui::Slider::new(&mut e.thermal_iterations, 1..=100))
                            .changed();
                        ui.end_row();
                        ui.label("Talus Angle");
                        ch |= ui
                            .add(egui::Slider::new(&mut e.talus_angle, 10.0..=80.0).suffix("°"))
                            .changed();
                        ui.end_row();
                        ui.label("Strength");
                        ch |= ui
                            .add(egui::Slider::new(&mut e.thermal_strength, 0.0..=1.0))
                            .changed();
                        ui.end_row();
                    }
                    ch
                });
            });
        ch
    });
}

/// A new seed from the clock (different from `old`).
fn random_seed(old: u32) -> u32 {
    let t = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u32);
    let s = world_gen::noise::hash(t as i32, old as i32, 0x5eed) % 100_000;
    if s == old { s + 1 } else { s }
}

fn stamps_section(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    ui.horizontal_wrapped(|ui| {
        ui.label("Add:");
        for (name, kind) in stamp_ui::kinds() {
            if ui
                .small_button(format!("{} {name}", stamp_ui::icon(&kind)))
                .on_hover_text(kind.label())
                .clicked()
            {
                stamp_ui::add(state, builder, name, kind);
            }
        }
    });
    let list = world_gen::stamps_of(&state.scene, builder);
    if list.is_empty() {
        ui.weak("No stamps yet. Added stamps go under the World Builder object at the Scene view's centre.");
        return;
    }
    let active = state.selection.active();
    for e in &list {
        let Ok(s) = state
            .scene
            .world
            .get::<&WorldStamp>(*e)
            .map(|s| (*s).clone())
        else {
            continue;
        };
        let name = state.scene.name(*e);
        let mut text = RichText::new(format!("{} {name}", stamp_ui::icon(&s.kind)));
        if !s.enabled || !state.scene.active_in_hierarchy(*e) {
            text = text.weak();
        }
        ui.horizontal(|ui| {
            if ui.selectable_label(active == Some(*e), text).clicked() {
                state.select(*e);
            }
            ui.weak(s.kind.label());
        });
    }
    if let Some(e) = active.filter(|e| list.contains(e)) {
        ui.add_space(4.0);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.label(RichText::new(state.scene.name(e)).strong());
            stamp_ui::edit(ui, state, e);
            if ui.small_button("Delete Stamp").clicked() {
                state.record("Delete Stamp");
                state.scene.despawn(e);
                state.selection.clear();
            }
        });
    }
}

fn generate_section(ui: &mut Ui, state: &mut EditorState, builder: Entity) {
    let busy = state.world_builder.busy() || state.play.in_play_mode();
    ui.horizontal(|ui| {
        let big = egui::Button::new(RichText::new("▶ Generate").strong())
            .min_size(egui::vec2(110.0, 26.0));
        if ui
            .add_enabled(!busy, big)
            .on_hover_text("Height → erosion → stamps → water → textures → scatter")
            .clicked()
        {
            job::start(state, builder, Stages::All, false);
        }
        for (label, stages) in [
            ("Heights Only", Stages::Heights),
            ("Textures Only", Stages::Textures),
            ("Scatter Only", Stages::Scatter),
        ] {
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                job::start(state, builder, stages, false);
            }
        }
    });
    with_settings(state, builder, |s| {
        grid(ui, "wb_generate", |ui| {
            let mut ch = false;
            ui.label("Live Preview");
            ch |= ui
                .checkbox(&mut s.live_preview, "Low-res preview while editing")
                .changed();
            ui.end_row();
            ui.label("Scatter Storage");
            ch |= stamp_ui::combo(
                ui,
                "wb_storage",
                &mut s.scatter_storage,
                &ScatterStorage::ALL,
                |v| v.label(),
            );
            ui.end_row();
            ch
        })
    });
    if let Some(job) = &state.world_builder.job {
        let (stage, f) = job.progress.get();
        let preview = job.preview();
        let label = job.label;
        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(f)
                    .desired_width(220.0)
                    .text(format!("{label}: {stage}")),
            );
            if !preview && ui.button("Cancel").clicked() {
                job.progress.cancel();
            }
        });
    }
    if state.world_builder.preview_base.is_some() && !state.world_builder.busy() {
        ui.horizontal(|ui| {
            ui.weak("Showing a preview.");
            if ui.small_button("Revert Preview").clicked() {
                job::revert_preview(state);
            }
        });
    }
    if !state.world_builder.status.is_empty() {
        ui.weak(&state.world_builder.status);
    }
    if !state.world_builder.timings.is_empty() {
        let t: Vec<String> = state
            .world_builder
            .timings
            .iter()
            .map(|(n, ms)| format!("{n} {ms:.0} ms"))
            .collect();
        ui.weak(t.join(" · "));
    }
}

/// Inspector bodies of the World Builder components.
pub fn inspector(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "World Stamp" => stamp_ui::edit(ui, state, e),
        "Scatter" => {
            if let Ok(s) = state.scene.world.get::<&ScatterSet>(e) {
                ui.weak(format!(
                    "{} instances × {} meshes, drawn instanced. Regenerate from the World Builder.",
                    s.count,
                    s.parts.len()
                ));
            }
            false
        }
        _ => {
            ui.weak("Settings for the World Builder window.");
            if ui.button("Open World Builder").clicked() {
                state.push(crate::actions::Action::FocusTab(
                    crate::layout::Tab::WorldBuilder,
                ));
            }
            false
        }
    }
}
