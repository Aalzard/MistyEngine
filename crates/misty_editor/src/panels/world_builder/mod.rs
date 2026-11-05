//! World Builder window (Window ▸ Terrain ▸ World Builder, or the Terrain Inspector's World
//! Builder button): a Preview card (live image of the world while you edit), Templates, then
//! collapsible sections — Base Heightmap (with erosion), Stamps & Areas, Biomes and Generate. It
//! works on the World Builder object of the open scene (or creates one, with a terrain) and runs
//! full generations on a worker thread.

mod biome_ui;
pub mod gizmos;
mod job;
pub mod preview;
mod stamp_ui;
pub mod templates;
#[cfg(test)]
mod tests;

use std::sync::{Arc, Mutex};

use egui::{CollapsingHeader, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Terrain, TerrainCollider, Transform};
use misty_engine::terrain::{HEIGHTMAP_RESOLUTIONS, TerrainData, store};
use misty_engine::world_gen::generate::Cache;
use misty_engine::world_gen::{
    self, Generator, ScatterSet, ScatterStorage, Stages, WorldBuilder, WorldStamp,
};

use crate::state::EditorState;
use crate::terrain_tools::TerrainSnapshot;
use crate::tutorials::targets;

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
    /// Stages the last run took from the cache.
    pub reused: Vec<&'static str>,
    /// Biome asset open in the Biomes section.
    pub biome: Option<String>,
    job: Option<job::Job>,
    /// Terrain data from before the first preview (undo and Revert go back to it).
    preview_base: Option<TerrainSnapshot>,
    builder: Option<Entity>,
    /// The Preview card.
    pub preview: preview::PreviewState,
    /// Stage cache of full generations (incremental Generate).
    full_cache: Arc<Mutex<Cache>>,
    /// Template waiting for "Replace the current setup?".
    pub confirm_template: Option<world_gen::templates::Template>,
}

impl WorldBuilderWindow {
    /// A full generation is running.
    pub fn busy(&self) -> bool {
        self.job.is_some()
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
    // A selected terrain picks the builder that generates it.
    let for_terrain = |e: Entity| {
        world_gen::builders(scene)
            .into_iter()
            .find(|b| world_gen::terrain_of(scene, *b) == Some(e))
    };
    state
        .selection
        .active()
        .and_then(|e| world_gen::builder_of(scene, e).or_else(|| for_terrain(e)))
        .or(state
            .world_builder
            .builder
            .filter(|b| scene.contains(*b) && scene.world.get::<&WorldBuilder>(*b).is_ok()))
        .or_else(|| world_gen::builders(scene).first().copied())
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    state.world_builder.seen_frame = Some(ui.ctx().cumulative_frame_nr());
    // Tutorial targets `World Builder/<Section>/<Field>`.
    let _scope = targets::scope("World Builder");
    job::poll(state);
    if state.world_builder.job.is_some() {
        ui.ctx().request_repaint();
    }
    let Some(builder) = current_builder(state) else {
        empty(ui, state);
        return;
    };
    state.world_builder.builder = Some(builder);
    preview::tick(state, builder, ui.ctx());
    header(ui, state, builder);
    templates::confirm_dialog(ui, state, builder);
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            card(ui, "Preview", true, |ui| preview::show(ui, state, builder));
            let fresh = !templates::is_edited(state, builder);
            card(ui, "Templates", fresh, |ui| {
                templates::picker(ui, state, Some(builder))
            });
            section(ui, 1, "Base Heightmap", |ui| {
                base_section(ui, state, builder)
            });
            section(ui, 2, "Stamps & Areas", |ui| {
                stamps_section(ui, state, builder)
            });
            section(ui, 3, "Biomes", |ui| biome_ui::show(ui, state, builder));
            section(ui, 4, "Generate", |ui| generate_section(ui, state, builder));
        });
}

/// An unnumbered, collapsible card above the sections (a tutorial target too).
fn card(ui: &mut Ui, title: &str, open: bool, body: impl FnOnce(&mut Ui)) {
    let r = CollapsingHeader::new(RichText::new(title).strong())
        .id_salt(("wb_card", title))
        .default_open(open)
        .show(ui, |ui| {
            let _scope = targets::scope(title);
            body(ui)
        });
    targets::row(ui, title, r.header_response.rect);
}

/// A numbered, collapsible section (a tutorial target too).
fn section(ui: &mut Ui, n: usize, title: &str, body: impl FnOnce(&mut Ui)) {
    let r = CollapsingHeader::new(RichText::new(format!("{n}  {title}")).strong())
        .default_open(true)
        .show(ui, |ui| {
            let _scope = targets::scope(title);
            body(ui)
        });
    targets::row(ui, title, r.header_response.rect);
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
    let create = ui.button(label);
    targets::header(ui, "Create", create.rect);
    if create.clicked() {
        create_builder(state, terrain);
    }
    if terrain.is_none() {
        ui.add_space(8.0);
        ui.label(RichText::new("Or start from a template").strong());
        ui.weak("Makes a terrain and a World Builder set up for it.");
        ui.add_space(4.0);
        templates::picker(ui, state, None);
    }
    if !state.world_builder.status.is_empty() {
        ui.weak(&state.world_builder.status);
    }
}

/// Add a "World Builder" object for `terrain` (or a new terrain) and make sure the biome presets
/// exist.
pub fn create_builder(state: &mut EditorState, terrain: Option<Entity>) -> Option<Entity> {
    create_builder_named(state, terrain, "World")
}

/// [`create_builder`] with a new terrain asset named after `name` ("Desert World.terrain").
pub fn create_builder_named(
    state: &mut EditorState,
    terrain: Option<Entity>,
    name: &str,
) -> Option<Entity> {
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
            let file = crate::asset_ops::unique_path(&dir, name, misty_engine::terrain::EXTENSION);
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
    state.refresh_assets();
    Some(b)
}

/// The Terrain Inspector's World Builder button: open the World Builder of `terrain`, creating
/// one for it first when there is none.
pub fn open_for_terrain(state: &mut EditorState, terrain: Entity) {
    let existing = world_gen::builders(&state.scene)
        .into_iter()
        .find(|b| world_gen::terrain_of(&state.scene, *b) == Some(terrain));
    let b = existing.or_else(|| create_builder(state, Some(terrain)));
    if let Some(b) = b {
        state.world_builder.builder = Some(b);
        // Keep the terrain selected: the window follows it to its builder.
        state.select(terrain);
    }
    state.push(crate::actions::Action::FocusTab(
        crate::layout::Tab::WorldBuilder,
    ));
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
    let before = s.clone();
    if f(&mut s) {
        let label = crate::undo_names::struct_change("World Builder", &before, &s);
        let _ = state.scene.world.insert_one(builder, s);
        state.mark_edited(&label);
    }
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::panels::inspector::card::PropertyGrid::new(id)
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
            targets::label(ui, "Generator");
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
            targets::label(ui, "Noise Amount");
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
        let erosion = CollapsingHeader::new("Erosion")
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
        targets::row(ui, "Erosion", erosion.header_response.rect);
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
    let add_row = ui.horizontal_wrapped(|ui| {
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
    targets::header(ui, "Add", add_row.response.rect);
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
        let big = ui.add_enabled(!busy, big);
        targets::header(ui, "Generate", big.rect);
        if big
            .on_hover_text(
                "Height → erosion → stamps → water → textures → scatter. \
                 Stages your edits did not touch are reused from the last run.",
            )
            .clicked()
        {
            job::start(state, builder, Stages::All);
        }
        for (label, stages) in [
            ("Heights Only", Stages::Heights),
            ("Textures Only", Stages::Textures),
            ("Scatter Only", Stages::Scatter),
        ] {
            if ui.add_enabled(!busy, egui::Button::new(label)).clicked() {
                job::start(state, builder, stages);
            }
        }
    });
    with_settings(state, builder, |s| {
        grid(ui, "wb_generate", |ui| {
            let mut ch = false;
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
        let f = job.progress.overall();
        let text = format!("{}: {}", job.label, job.eta_text());
        ui.horizontal(|ui| {
            ui.add(
                egui::ProgressBar::new(f)
                    .desired_width(ui.available_width().min(300.0) - 70.0)
                    .text(text),
            );
            if ui.button("Cancel").clicked() {
                job.progress.cancel();
            }
        });
    }
    if !state.world_builder.status.is_empty() {
        ui.weak(&state.world_builder.status);
    }
    if !state.world_builder.timings.is_empty() {
        let mut t: Vec<String> = state
            .world_builder
            .timings
            .iter()
            .map(|(n, ms)| format!("{n} {ms:.0} ms"))
            .collect();
        if !state.world_builder.reused.is_empty() {
            t.push(format!("reused {}", state.world_builder.reused.len()));
        }
        ui.weak(t.join(" · "))
            .on_hover_text(if state.world_builder.reused.is_empty() {
                "Every stage ran.".to_owned()
            } else {
                format!(
                    "Reused from the last run: {}",
                    state.world_builder.reused.join(", ")
                )
            });
    }
}

/// Add a stamp of the kind named like the window's Add buttons ("Mountain", "Water", …) under
/// `builder` (tutorial demos). Returns the new stamp object.
pub fn add_stamp(state: &mut EditorState, builder: Entity, name: &str) -> Option<Entity> {
    let (name, kind) = stamp_ui::kinds().into_iter().find(|(n, _)| *n == name)?;
    stamp_ui::add(state, builder, name, kind);
    state.selection.active()
}

/// Start a full generation, as the Generate button does (tutorial demos).
pub fn generate(state: &mut EditorState, builder: Entity) {
    job::start(state, builder, Stages::All);
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
