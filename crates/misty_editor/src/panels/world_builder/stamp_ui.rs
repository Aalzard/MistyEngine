//! Stamp and area fields (shown in the World Builder window and the Inspector), and creating
//! new stamps under the World Builder.

use egui::Ui;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::world_gen::stamps::{
    BiomeZone, BlendMode, Flatten, Mountain, MountainShape, River, RockArea, Water, WaterMode,
};
use misty_engine::world_gen::{self, StampKind, WorldStamp};

use crate::state::EditorState;

/// Stamp kinds the "Add" buttons create, with their default settings.
pub fn kinds() -> [(&'static str, StampKind); 6] {
    [
        ("Mountain", StampKind::Mountain(Mountain::default())),
        ("Flatten", StampKind::Flatten(Flatten::default())),
        ("River", StampKind::River(River::default())),
        ("Water", StampKind::Water(Water::default())),
        ("Rock Area", StampKind::RockArea(RockArea::default())),
        ("Biome Zone", StampKind::BiomeZone(BiomeZone::default())),
    ]
}

/// Icon for a stamp kind (list rows and the Inspector header).
pub fn icon(kind: &StampKind) -> &'static str {
    match kind {
        StampKind::Mountain(_) => "⛰",
        StampKind::Flatten(_) => "▭",
        StampKind::River(_) => "〰",
        StampKind::Water(_) => "💧",
        StampKind::RockArea(_) => "◆",
        StampKind::BiomeZone(_) => "🌲",
    }
}

/// Add a stamp under `builder` at the Scene view's pivot, sitting on the ground (flatten areas
/// take the ground height there: sample-on-place).
pub fn add(state: &mut EditorState, builder: Entity, name: &str, kind: StampKind) {
    let pivot = state.scene_view.camera.pivot();
    let ground = world_gen::terrain_of(&state.scene, builder)
        .and_then(|t| {
            let corner = state.scene.world_position(t);
            misty_engine::terrain::sample_height(&state.scene, t, pivot).map(|h| h + corner.y)
        })
        .unwrap_or(0.0);
    let y = match &kind {
        // Water sits a little above the ground so the basin fills.
        StampKind::Water(_) => ground + 4.0,
        _ => ground,
    };
    let radius = match &kind {
        StampKind::Flatten(_) => 30.0,
        StampKind::Water(_) => 20.0,
        _ => 60.0,
    };
    state.record(&format!("Add {name}"));
    let n = state.scene.unique_name(name, Some(builder));
    let e = state.scene.spawn(&n, Some(builder));
    world_gen::set_world_matrix(
        &mut state.scene,
        e,
        glam::Mat4::from_translation(Vec3::new(pivot.x, y, pivot.z)),
    );
    state.scene.add(
        e,
        WorldStamp {
            radius,
            kind,
            ..Default::default()
        },
    );
    state.select(e);
}

/// Fields of the stamp on `e`. Returns true when something changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut s) = state
        .scene
        .world
        .get::<&WorldStamp>(e)
        .map(|s| (*s).clone())
    else {
        return false;
    };
    let biomes = super::biome_ui::biome_paths(state);
    let mut height = state.scene.world_position(e).y;
    let mut ch = false;
    let mut moved = false;
    crate::panels::inspector::card::PropertyGrid::new(("stamp", e)).show(ui, |ui| {
        ui.label("Enabled");
        ch |= ui.checkbox(&mut s.enabled, "").changed();
        ui.end_row();
        let uses_radius = !matches!(s.kind, StampKind::River(_));
        if uses_radius {
            ui.label("Radius");
            ch |= drag(ui, &mut s.radius, 0.5, 1.0, 5000.0, " m");
            ui.end_row();
            ui.label("Falloff");
            ch |= ui
                .add(egui::Slider::new(&mut s.falloff, 0.0..=1.0))
                .changed();
            ui.end_row();
        }
        match &mut s.kind {
            StampKind::Mountain(m) => {
                ui.label("Shape");
                ch |= combo(ui, ("shape", e), &mut m.shape, &MountainShape::ALL, |v| {
                    v.label()
                });
                ui.end_row();
                ui.label("Height");
                ch |= drag(ui, &mut m.height, 0.5, -1000.0, 1000.0, " m");
                ui.end_row();
                ui.label("Blend");
                ch |= combo(ui, ("blend", e), &mut m.blend, &BlendMode::ALL, |v| {
                    v.label()
                });
                ui.end_row();
                ui.label("Roughness");
                ch |= ui
                    .add(egui::Slider::new(&mut m.roughness, 0.0..=1.0))
                    .changed();
                ui.end_row();
                if m.blend != BlendMode::Add {
                    ui.label("Base Height");
                    moved |= drag(ui, &mut height, 0.25, -1000.0, 5000.0, " m");
                    ui.end_row();
                }
            }
            StampKind::Flatten(f) => {
                ui.label("Sample Ground");
                ch |= ui
                    .checkbox(&mut f.sample, "")
                    .on_hover_text("Use the ground height under the centre when generating")
                    .changed();
                ui.end_row();
                if !f.sample {
                    ui.label("Target Height");
                    moved |= drag(ui, &mut height, 0.25, -1000.0, 5000.0, " m");
                    ui.end_row();
                }
            }
            StampKind::River(r) => {
                ui.label("Width");
                ch |= drag(ui, &mut r.width, 0.1, 0.5, 500.0, " m");
                ui.end_row();
                ui.label("Depth");
                ch |= drag(ui, &mut r.depth, 0.1, 0.0, 200.0, " m");
                ui.end_row();
                ui.label("Bank Falloff");
                ch |= drag(ui, &mut r.bank, 0.1, 0.0, 500.0, " m");
                ui.end_row();
                ui.label("Water");
                ch |= ui.checkbox(&mut r.water, "Place a water strip").changed();
                ui.end_row();
                ui.label("Points");
                ui.horizontal(|ui| {
                    ui.label(r.points.len().to_string());
                    if ui.small_button("+ Add").clicked() {
                        let n = r.points.len();
                        let next = match n {
                            0 => [0.0, 0.0],
                            1 => [r.points[0][0] + 20.0, r.points[0][1]],
                            _ => {
                                let (a, b) = (r.points[n - 2], r.points[n - 1]);
                                [2.0 * b[0] - a[0], 2.0 * b[1] - a[1]]
                            }
                        };
                        r.points.push(next);
                        ch = true;
                    }
                    if ui
                        .add_enabled(r.points.len() > 2, egui::Button::new("− Remove").small())
                        .clicked()
                    {
                        r.points.pop();
                        ch = true;
                    }
                });
                ui.end_row();
            }
            StampKind::Water(w) => {
                crate::tutorials::targets::label(ui, "Mode");
                ui.horizontal(|ui| {
                    ch |= ui
                        .selectable_value(&mut w.mode, WaterMode::Lake, "Lake")
                        .changed();
                    ch |= ui
                        .selectable_value(&mut w.mode, WaterMode::Sea, "Sea Level")
                        .changed();
                });
                ui.end_row();
                ui.label("Water Level");
                moved |= drag(ui, &mut height, 0.1, -1000.0, 5000.0, " m");
                ui.end_row();
            }
            StampKind::RockArea(r) => {
                ui.label("Strength");
                ch |= ui
                    .add(egui::Slider::new(&mut r.strength, 0.0..=1.0))
                    .changed();
                ui.end_row();
            }
            StampKind::BiomeZone(z) => {
                crate::tutorials::targets::label(ui, "Biome");
                let name = |p: &str| super::biome_ui::display_name(p);
                egui::ComboBox::from_id_salt(("zone_biome", e))
                    .selected_text(if z.biome.is_empty() {
                        "None".to_owned()
                    } else {
                        name(&z.biome)
                    })
                    .show_ui(ui, |ui| {
                        for p in &biomes {
                            ch |= ui
                                .selectable_value(&mut z.biome, p.clone(), name(p))
                                .changed();
                        }
                    });
                ui.end_row();
                ui.label("Shape");
                ui.horizontal(|ui| {
                    if z.polygon.is_empty() {
                        ui.weak("Circle");
                        if ui.small_button("Make Polygon").clicked() {
                            let r = s.radius;
                            z.polygon = (0..6)
                                .map(|i| {
                                    let a = i as f32 / 6.0 * std::f32::consts::TAU;
                                    [a.cos() * r, a.sin() * r]
                                })
                                .collect();
                            ch = true;
                        }
                    } else {
                        ui.weak(format!("Polygon ({} points)", z.polygon.len()));
                        if ui.small_button("Circle").clicked() {
                            z.polygon.clear();
                            ch = true;
                        }
                    }
                });
                ui.end_row();
            }
        }
    });
    ui.weak(hint(&s.kind));
    if moved {
        let mut p = state.scene.world_position(e);
        p.y = height;
        state.scene.set_world_position(e, p);
        state.mark_edited("Move World Stamp");
    }
    if ch {
        let _ = state.scene.world.insert_one(e, s);
        state.mark_edited("Change World Stamp");
    }
    ch || moved
}

fn hint(kind: &StampKind) -> &'static str {
    match kind {
        StampKind::Mountain(_) => {
            "Move, rotate and scale it with the usual tools; drag the dots for radius and falloff."
        }
        StampKind::Flatten(_) => "Flattens to the object's height (or the ground under it).",
        StampKind::River(_) => {
            "Drag the points in the Editor Scene. The river flows from the first point to the last."
        }
        StampKind::Water(_) => {
            "Lake: fills the basin under the object up to its height. Sea: everything below it."
        }
        StampKind::RockArea(_) => "Rock and cliff textures and rock scatter take over inside.",
        StampKind::BiomeZone(_) => "Assigns a biome inside; edges blend with noise.",
    }
}

fn drag(ui: &mut Ui, v: &mut f32, speed: f64, min: f32, max: f32, suffix: &str) -> bool {
    ui.add(
        egui::DragValue::new(v)
            .speed(speed)
            .range(min..=max)
            .suffix(suffix)
            .max_decimals(2),
    )
    .changed()
}

pub fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    v: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*v))
        .show_ui(ui, |ui| {
            for x in all {
                ch |= ui.selectable_value(v, *x, label(*x)).changed();
            }
        });
    ch
}
