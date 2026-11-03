//! Inspector editors for the Nav Mesh Agent and Nav Mesh Obstacle.

use egui::{Grid, RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::navigation::{NavMeshAgent, NavMeshObstacle, ObstacleAvoidance, ObstacleShape};
use misty_engine::scene::SceneComponent;

use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 2] = ["Nav Mesh Agent", "Nav Mesh Obstacle"];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Nav Mesh Agent" => "🚶",
        "Nav Mesh Obstacle" => "⬢",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Nav Mesh Agent" => w.get::<&NavMeshAgent>(e).ok().map(|c| c.enabled),
        "Nav Mesh Obstacle" => w.get::<&NavMeshObstacle>(e).ok().map(|c| c.enabled),
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    match name {
        "Nav Mesh Agent" => {
            let _ = w.get::<&mut NavMeshAgent>(e).map(|mut c| c.enabled = on);
        }
        "Nav Mesh Obstacle" => {
            let _ = w.get::<&mut NavMeshObstacle>(e).map(|mut c| c.enabled = on);
        }
        _ => {}
    }
}

/// Draw the editor for component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Nav Mesh Agent" => edit_copy::<NavMeshAgent>(state, e, |a| agent(ui, a)),
        "Nav Mesh Obstacle" => edit_copy::<NavMeshObstacle>(state, e, |o| obstacle(ui, o)),
        _ => false,
    }
}

fn edit_copy<T: SceneComponent>(
    state: &mut EditorState,
    e: Entity,
    f: impl FnOnce(&mut T) -> bool,
) -> bool {
    let Ok(mut copy) = state.scene.world.get::<&T>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let changed = f(&mut copy);
    if changed {
        let _ = state.scene.world.insert_one(e, copy);
    }
    changed
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(130.0)
        .show(ui, body)
        .inner
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, speed: f64, min: f32, max: f32) -> bool {
    ui.label(label);
    let ch = ui
        .add(
            egui::DragValue::new(v)
                .speed(speed)
                .range(min..=max)
                .max_decimals(3),
        )
        .changed();
    ui.end_row();
    ch
}

fn agent(ui: &mut Ui, a: &mut NavMeshAgent) -> bool {
    let mut ch = grid(ui, "nav_agent_top", |ui| {
        ui.label("Agent Type");
        ui.weak("Humanoid");
        ui.end_row();
        number(ui, "Base Offset", &mut a.base_offset, 0.01, -100.0, 100.0)
    });
    ui.label(RichText::new("Steering").strong());
    ch |= grid(ui, "nav_agent_steering", |ui| {
        let mut ch = number(ui, "Speed", &mut a.speed, 0.05, 0.0, 1000.0);
        ch |= number(
            ui,
            "Angular Speed",
            &mut a.angular_speed,
            1.0,
            0.0,
            100000.0,
        );
        ch |= number(ui, "Acceleration", &mut a.acceleration, 0.05, 0.0, 1000.0);
        ch |= number(
            ui,
            "Stopping Distance",
            &mut a.stopping_distance,
            0.01,
            0.0,
            1000.0,
        );
        ui.label("Auto Braking");
        ch |= ui.checkbox(&mut a.auto_braking, "").changed();
        ui.end_row();
        ch
    });
    ui.label(RichText::new("Obstacle Avoidance").strong());
    ch |= grid(ui, "nav_agent_avoid", |ui| {
        let mut ch = number(ui, "Radius", &mut a.radius, 0.01, 0.01, 100.0);
        ch |= number(ui, "Height", &mut a.height, 0.01, 0.01, 100.0);
        ui.label("Quality");
        egui::ComboBox::from_id_salt("nav_agent_quality")
            .selected_text(a.obstacle_avoidance.label())
            .show_ui(ui, |ui| {
                for q in ObstacleAvoidance::ALL {
                    ch |= ui
                        .selectable_value(&mut a.obstacle_avoidance, q, q.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label("Priority");
        ch |= ui
            .add(egui::DragValue::new(&mut a.priority).range(0..=99))
            .changed();
        ui.end_row();
        ch
    });
    ch
}

fn obstacle(ui: &mut Ui, o: &mut NavMeshObstacle) -> bool {
    let mut ch = grid(ui, "nav_obstacle", |ui| {
        ui.label("Shape");
        let mut ch = false;
        egui::ComboBox::from_id_salt("nav_obstacle_shape")
            .selected_text(o.shape.label())
            .show_ui(ui, |ui| {
                for s in ObstacleShape::ALL {
                    ch |= ui.selectable_value(&mut o.shape, s, s.label()).changed();
                }
            });
        ui.end_row();
        ch
    });
    let mut v = o.center.to_array();
    if vec3_row(ui, "Center", &mut v, 0.01) {
        o.center = Vec3::from(v);
        ch = true;
    }
    match o.shape {
        ObstacleShape::Box => {
            let mut v = o.size.to_array();
            if vec3_row(ui, "Size", &mut v, 0.01) {
                o.size = Vec3::from(v).max(Vec3::ZERO);
                ch = true;
            }
        }
        ObstacleShape::Capsule => {
            ch |= grid(ui, "nav_obstacle_capsule", |ui| {
                let ch = number(ui, "Radius", &mut o.radius, 0.01, 0.0, 1000.0);
                ch | number(ui, "Height", &mut o.height, 0.01, 0.0, 1000.0)
            });
        }
    }
    ui.add_enabled(false, egui::Checkbox::new(&mut false, "Carve"))
        .on_disabled_hover_text("Carving isn't supported: agents steer around obstacles.");
    ch
}
