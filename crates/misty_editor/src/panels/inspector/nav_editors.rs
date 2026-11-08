//! Inspector editors for the navigation components: Nav Mesh Agent, Nav Mesh Obstacle, NavMesh
//! Surface (with Bake / Clear and the bake progress), NavMesh Modifier, NavMesh Modifier Volume
//! and NavMesh Link.

use egui::Ui;
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::navigation::{
    AREA_COUNT, CollectObjects, NavMeshAgent, NavMeshLink, NavMeshModifier, NavMeshModifierVolume,
    NavMeshObstacle, NavMeshSurface, NavigationSettings, ObstacleAvoidance, ObstacleShape,
};
use misty_engine::scene::SceneComponent;

use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 6] = [
    "Nav Mesh Agent",
    "Nav Mesh Obstacle",
    "NavMesh Surface",
    "NavMesh Modifier",
    "NavMesh Modifier Volume",
    "NavMesh Link",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Nav Mesh Agent" => "🚶",
        "Nav Mesh Obstacle" => "⬢",
        "NavMesh Surface" => "▦",
        "NavMesh Modifier" => "◩",
        "NavMesh Modifier Volume" => "⬚",
        "NavMesh Link" => "⤴",
        _ => return None,
    })
}

pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Nav Mesh Agent" => w.get::<&NavMeshAgent>(e).ok().map(|c| c.enabled),
        "Nav Mesh Obstacle" => w.get::<&NavMeshObstacle>(e).ok().map(|c| c.enabled),
        "NavMesh Surface" => w.get::<&NavMeshSurface>(e).ok().map(|c| c.enabled),
        "NavMesh Modifier" => w.get::<&NavMeshModifier>(e).ok().map(|c| c.enabled),
        "NavMesh Modifier Volume" => w.get::<&NavMeshModifierVolume>(e).ok().map(|c| c.enabled),
        "NavMesh Link" => w.get::<&NavMeshLink>(e).ok().map(|c| c.enabled),
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
        "NavMesh Surface" => {
            let _ = w.get::<&mut NavMeshSurface>(e).map(|mut c| c.enabled = on);
        }
        "NavMesh Modifier" => {
            let _ = w.get::<&mut NavMeshModifier>(e).map(|mut c| c.enabled = on);
        }
        "NavMesh Modifier Volume" => {
            let _ = w
                .get::<&mut NavMeshModifierVolume>(e)
                .map(|mut c| c.enabled = on);
        }
        "NavMesh Link" => {
            let _ = w.get::<&mut NavMeshLink>(e).map(|mut c| c.enabled = on);
        }
        _ => {}
    }
}

/// Draw the editor for component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    let nav = state.scene.navigation.clone();
    match name {
        "Nav Mesh Agent" => edit_copy::<NavMeshAgent>(state, e, |a| agent(ui, a, &nav)),
        "Nav Mesh Obstacle" => edit_copy::<NavMeshObstacle>(state, e, |o| obstacle(ui, o)),
        "NavMesh Surface" => surface_editor(ui, state, e, &nav),
        "NavMesh Modifier" => edit_copy::<NavMeshModifier>(state, e, |m| modifier(ui, m, &nav)),
        "NavMesh Modifier Volume" => {
            edit_copy::<NavMeshModifierVolume>(state, e, |v| volume(ui, v, &nav))
        }
        "NavMesh Link" => edit_copy::<NavMeshLink>(state, e, |l| link(ui, l, &nav)),
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
    super::card::grid(ui, id, body)
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

fn check(ui: &mut Ui, label: &str, v: &mut bool) -> bool {
    ui.label(label);
    let ch = ui.checkbox(v, "").changed();
    ui.end_row();
    ch
}

/// Agent type picker (a grid row).
pub fn agent_type_row(ui: &mut Ui, id: &str, v: &mut u32, nav: &NavigationSettings) -> bool {
    ui.label("Agent Type");
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(nav.agent_type_name(*v))
        .show_ui(ui, |ui| {
            for t in nav.all_agent_types() {
                ch |= ui.selectable_value(v, t.id, t.name).changed();
            }
        });
    ui.end_row();
    ch
}

/// Area picker (a grid row), with each area's colour.
pub fn area_row(ui: &mut Ui, id: &str, label: &str, v: &mut u8, nav: &NavigationSettings) -> bool {
    ui.label(label);
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(nav.area_label(*v as usize))
        .show_ui(ui, |ui| {
            for i in 0..AREA_COUNT {
                let a = nav.area(i);
                if a.name.is_empty() && i != *v as usize {
                    continue;
                }
                let text = egui::RichText::new(format!("■ {}", nav.area_label(i)))
                    .color(area_color32(i as u8));
                ch |= ui.selectable_value(v, i as u8, text).changed();
            }
        });
    ui.end_row();
    ch
}

pub fn area_color32(i: u8) -> egui::Color32 {
    let [r, g, b] = misty_engine::navigation::area_color(i);
    egui::Color32::from_rgb((r * 255.0) as u8, (g * 255.0) as u8, (b * 255.0) as u8)
}

/// Area mask picker (a grid row): ticks per named area.
fn area_mask_row(ui: &mut Ui, id: &str, mask: &mut u32, nav: &NavigationSettings) -> bool {
    ui.label("Area Mask");
    let named: Vec<usize> = (0..AREA_COUNT)
        .filter(|&i| !nav.area(i).name.is_empty() && i != 1)
        .collect();
    let on = named.iter().filter(|&&i| *mask & (1 << i) != 0).count();
    let text = if named.iter().all(|&i| *mask & (1 << i) != 0) {
        "Everything".to_owned()
    } else if on == 0 {
        "Nothing".to_owned()
    } else if on == 1 {
        nav.area_label(*named.iter().find(|&&i| *mask & (1 << i) != 0).expect("one"))
    } else {
        "Mixed…".to_owned()
    };
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .show_ui(ui, |ui| {
            if ui.button("Everything").clicked() {
                *mask = u32::MAX;
                ch = true;
            }
            if ui.button("Nothing").clicked() {
                *mask = 0;
                ch = true;
            }
            for &i in &named {
                let mut bit = *mask & (1 << i) != 0;
                let label = egui::RichText::new(nav.area_label(i)).color(area_color32(i as u8));
                if ui.checkbox(&mut bit, label).changed() {
                    if bit {
                        *mask |= 1 << i;
                    } else {
                        *mask &= !(1 << i);
                    }
                    ch = true;
                }
            }
        });
    ui.end_row();
    ch
}

/// Affected agent types (-1 = all), as a dropdown with ticks.
fn affected_row(ui: &mut Ui, id: &str, list: &mut Vec<i32>, nav: &NavigationSettings) -> bool {
    ui.label("Affected Agents");
    let all = list.contains(&-1);
    let text = if all {
        "All".to_owned()
    } else if list.is_empty() {
        "None".to_owned()
    } else {
        list.iter()
            .map(|&i| nav.agent_type_name(i.max(0) as u32))
            .collect::<Vec<_>>()
            .join(", ")
    };
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(text)
        .show_ui(ui, |ui| {
            let mut a = all;
            if ui.checkbox(&mut a, "All").changed() {
                *list = if a { vec![-1] } else { Vec::new() };
                ch = true;
            }
            for t in nav.all_agent_types() {
                let mut on = all || list.contains(&(t.id as i32));
                if ui.checkbox(&mut on, &t.name).changed() {
                    list.retain(|&x| x != -1 && x != t.id as i32);
                    if on {
                        list.push(t.id as i32);
                    }
                    ch = true;
                }
            }
        });
    ui.end_row();
    ch
}

fn agent(ui: &mut Ui, a: &mut NavMeshAgent, nav: &NavigationSettings) -> bool {
    let mut ch = grid(ui, "nav_agent_top", |ui| {
        let mut ch = agent_type_row(ui, "nav_agent_type", &mut a.agent_type, nav);
        ch |= number(ui, "Base Offset", &mut a.base_offset, 0.01, -100.0, 100.0);
        ch
    });
    super::card::sub_header(ui, "Steering");
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
        ch |= check(ui, "Auto Braking", &mut a.auto_braking);
        ch
    });
    super::card::sub_header(ui, "Obstacle Avoidance");
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
    super::card::sub_header(ui, "Path Finding");
    ch |= grid(ui, "nav_agent_paths", |ui| {
        let mut ch = check(
            ui,
            "Auto Traverse Off Mesh Link",
            &mut a.auto_traverse_off_mesh_link,
        );
        ch |= check(ui, "Auto Repath", &mut a.auto_repath);
        ch |= area_mask_row(ui, "nav_agent_mask", &mut a.area_mask, nav);
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
    ch |= grid(ui, "nav_obstacle_carve", |ui| {
        let mut ch = check(ui, "Carve", &mut o.carve);
        if o.carve {
            ch |= number(
                ui,
                "Move Threshold",
                &mut o.move_threshold,
                0.01,
                0.0,
                100.0,
            );
            ch |= number(
                ui,
                "Time To Stationary",
                &mut o.time_to_stationary,
                0.01,
                0.0,
                100.0,
            );
            ch |= check(ui, "Carve Only Stationary", &mut o.carve_only_stationary);
        }
        ch
    });
    ch
}

/// What the Surface editor's buttons asked for.
#[derive(Default)]
struct SurfaceButtons {
    bake: bool,
    clear: bool,
}

fn surface_editor(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    nav: &NavigationSettings,
) -> bool {
    let mut buttons = SurfaceButtons::default();
    let baking = crate::panels::navigation::surface_progress(state, e);
    let busy = baking.is_some() || state.play.in_play_mode();
    let has_data = state
        .scene
        .world
        .get::<&NavMeshSurface>(e)
        .is_ok_and(|s| s.nav_mesh_data.is_some());
    let changed = edit_copy::<NavMeshSurface>(state, e, |s| {
        surface(ui, s, nav, busy, has_data, &mut buttons)
    });
    if let Some((f, stage)) = baking {
        ui.add(egui::ProgressBar::new(f).text(format!("Baking: {stage}…")));
        ui.ctx().request_repaint();
    } else if let Some(msg) = crate::panels::navigation::surface_status(state, e) {
        ui.weak(msg);
    }
    if buttons.bake {
        crate::panels::navigation::start_surface_bake(state, e);
    }
    if buttons.clear {
        crate::panels::navigation::clear_surface(state, e);
    }
    changed
}

fn surface(
    ui: &mut Ui,
    s: &mut NavMeshSurface,
    nav: &NavigationSettings,
    busy: bool,
    has_data: bool,
    buttons: &mut SurfaceButtons,
) -> bool {
    let mut ch = grid(ui, "nav_surface", |ui| {
        let mut ch = agent_type_row(ui, "nav_surface_agent", &mut s.agent_type, nav);
        ch |= area_row(
            ui,
            "nav_surface_area",
            "Default Area",
            &mut s.default_area,
            nav,
        );
        ch
    });
    super::card::sub_header(ui, "Object Collection");
    ch |= grid(ui, "nav_surface_collect", |ui| {
        ui.label("Collect Objects");
        let mut ch = false;
        egui::ComboBox::from_id_salt("nav_surface_collect_mode")
            .selected_text(s.collect.label())
            .show_ui(ui, |ui| {
                for c in CollectObjects::ALL {
                    ch |= ui.selectable_value(&mut s.collect, c, c.label()).changed();
                }
            });
        ui.end_row();
        ui.label("Include Layers");
        let text = if s.layer_mask == u32::MAX {
            "Everything".to_owned()
        } else if s.layer_mask == 0 {
            "Nothing".to_owned()
        } else {
            format!("{} layers", s.layer_mask.count_ones())
        };
        egui::ComboBox::from_id_salt("nav_surface_layers")
            .selected_text(text)
            .show_ui(ui, |ui| {
                if ui.button("Everything").clicked() {
                    s.layer_mask = u32::MAX;
                    ch = true;
                }
                for l in 0..32u32 {
                    let mut on = s.layer_mask & (1 << l) != 0;
                    if ui.checkbox(&mut on, format!("Layer {l}")).changed() {
                        s.layer_mask ^= 1 << l;
                        ch = true;
                    }
                }
            });
        ui.end_row();
        ch
    });
    if s.collect == CollectObjects::Volume {
        let mut v = s.center.to_array();
        if vec3_row(ui, "Center", &mut v, 0.01) {
            s.center = Vec3::from(v);
            ch = true;
        }
        let mut v = s.size.to_array();
        if vec3_row(ui, "Size", &mut v, 0.01) {
            s.size = Vec3::from(v).max(Vec3::splat(0.01));
            ch = true;
        }
    }
    super::card::sub_header(ui, "Advanced");
    ch |= grid(ui, "nav_surface_advanced", |ui| {
        let mut ch = check(ui, "Override Voxel Size", &mut s.override_voxel_size);
        if s.override_voxel_size {
            ch |= number(ui, "Voxel Size", &mut s.voxel_size, 0.005, 0.01, 5.0);
        } else {
            let r = nav.agent_settings(s.agent_type).unwrap_or(nav.agent).radius;
            ui.label("Voxel Size");
            ui.weak(format!("{:.4} (radius / 3)", r / 3.0));
            ui.end_row();
        }
        ch |= check(ui, "Override Tile Size", &mut s.override_tile_size);
        if s.override_tile_size {
            ui.label("Tile Size");
            ch |= ui
                .add(egui::DragValue::new(&mut s.tile_size).range(16..=1024))
                .changed();
            ui.end_row();
        }
        ch |= number(
            ui,
            "Minimum Region Area",
            &mut s.min_region_area,
            0.1,
            0.0,
            1000.0,
        );
        ch
    });
    ui.horizontal(|ui| {
        if ui
            .add_enabled(!busy && has_data, egui::Button::new("Clear"))
            .clicked()
        {
            buttons.clear = true;
        }
        if ui.add_enabled(!busy, egui::Button::new("Bake")).clicked() {
            buttons.bake = true;
        }
    });
    if let Some(p) = &s.nav_mesh_data {
        ui.weak(format!("NavMesh Data: {p}"));
    }
    ch
}

fn modifier(ui: &mut Ui, m: &mut NavMeshModifier, nav: &NavigationSettings) -> bool {
    grid(ui, "nav_modifier", |ui| {
        let mut ch = check(ui, "Ignore From Build", &mut m.ignore_from_build);
        if !m.ignore_from_build {
            ch |= check(ui, "Override Area", &mut m.override_area);
            if m.override_area {
                ch |= area_row(ui, "nav_modifier_area", "Area Type", &mut m.area, nav);
            }
        }
        ch |= check(ui, "Apply To Children", &mut m.apply_to_children);
        ch |= affected_row(ui, "nav_modifier_agents", &mut m.affected_agents, nav);
        ch
    })
}

fn volume(ui: &mut Ui, v: &mut NavMeshModifierVolume, nav: &NavigationSettings) -> bool {
    let mut ch = false;
    let mut c = v.size.to_array();
    if vec3_row(ui, "Size", &mut c, 0.01) {
        v.size = Vec3::from(c).max(Vec3::ZERO);
        ch = true;
    }
    let mut c = v.center.to_array();
    if vec3_row(ui, "Center", &mut c, 0.01) {
        v.center = Vec3::from(c);
        ch = true;
    }
    ch |= grid(ui, "nav_volume", |ui| {
        let mut ch = area_row(ui, "nav_volume_area", "Area Type", &mut v.area, nav);
        ch |= affected_row(ui, "nav_volume_agents", &mut v.affected_agents, nav);
        ch
    });
    ch
}

fn link(ui: &mut Ui, l: &mut NavMeshLink, nav: &NavigationSettings) -> bool {
    let mut ch = grid(ui, "nav_link_top", |ui| {
        agent_type_row(ui, "nav_link_agent", &mut l.agent_type, nav)
    });
    let mut c = l.start.to_array();
    if vec3_row(ui, "Start Point", &mut c, 0.01) {
        l.start = Vec3::from(c);
        ch = true;
    }
    let mut c = l.end.to_array();
    if vec3_row(ui, "End Point", &mut c, 0.01) {
        l.end = Vec3::from(c);
        ch = true;
    }
    if ui.button("Swap").clicked() {
        std::mem::swap(&mut l.start, &mut l.end);
        ch = true;
    }
    ch |= grid(ui, "nav_link", |ui| {
        let mut ch = number(ui, "Width", &mut l.width, 0.01, 0.0, 100.0);
        ui.label("Cost Modifier");
        let mut override_cost = l.cost_modifier >= 0.0;
        ui.horizontal(|ui| {
            if ui.checkbox(&mut override_cost, "").changed() {
                l.cost_modifier = if override_cost { 1.0 } else { -1.0 };
                ch = true;
            }
            if override_cost {
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut l.cost_modifier)
                            .speed(0.05)
                            .range(0.0..=1000.0),
                    )
                    .changed();
            }
        });
        ui.end_row();
        ch |= check(ui, "Bidirectional", &mut l.bidirectional);
        ch |= area_row(ui, "nav_link_area", "Area Type", &mut l.area, nav);
        ch
    });
    ch
}
