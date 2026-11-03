//! Inspector editors for the Character Controller and the Fixed, Hinge and Spring Joints.

use egui::{Grid, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    CharacterController, FixedJoint, HingeJoint, Rigidbody, SceneComponent, SpringJoint,
};

use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 4] = [
    "Character Controller",
    "Fixed Joint",
    "Hinge Joint",
    "Spring Joint",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Character Controller" => "🚶",
        "Fixed Joint" | "Hinge Joint" | "Spring Joint" => "🔗",
        _ => return None,
    })
}

/// Draw the editor for component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    // Objects that can be a Connected Body: other objects with a Rigidbody.
    let bodies: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| o)
        .filter(|o| *o != e && state.scene.world.get::<&Rigidbody>(*o).is_ok())
        .map(|o| (o, state.scene.name(o)))
        .collect();
    match name {
        "Character Controller" => edit_copy::<CharacterController>(state, e, |c| controller(ui, c)),
        "Fixed Joint" => edit_copy::<FixedJoint>(state, e, |j| fixed(ui, j, &bodies)),
        "Hinge Joint" => edit_copy::<HingeJoint>(state, e, |j| hinge(ui, j, &bodies)),
        "Spring Joint" => edit_copy::<SpringJoint>(state, e, |j| spring(ui, j, &bodies)),
        _ => false,
    }
}

/// Edit a copy of `T` with `f` and store it if anything changed.
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
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

/// Label + number row inside a grid.
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

fn vec3(ui: &mut Ui, label: &str, v: &mut Vec3) -> bool {
    let mut a = v.to_array();
    let ch = vec3_row(ui, label, &mut a, 0.01);
    if ch {
        *v = Vec3::from(a);
    }
    ch
}

/// Connected Body picker: "None (Rigidbody)" connects to the world.
fn connected_body(
    ui: &mut Ui,
    id: &str,
    v: &mut Option<Entity>,
    bodies: &[(Entity, String)],
) -> bool {
    let none = "None (Rigidbody)";
    let name = v
        .and_then(|t| bodies.iter().find(|(e, _)| *e == t))
        .map_or(none.to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    ui.label("Connected Body");
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .width(150.0)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, none).changed();
            for (e, n) in bodies {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ui.end_row();
    ch
}

fn controller(ui: &mut Ui, c: &mut CharacterController) -> bool {
    let mut ch = grid(ui, "cc_grid", |ui| {
        let mut ch = number(ui, "Slope Limit", &mut c.slope_limit, 0.5, 0.0, 180.0);
        ch |= number(ui, "Step Offset", &mut c.step_offset, 0.01, 0.0, 1e4);
        ch |= number(ui, "Skin Width", &mut c.skin_width, 0.001, 1e-4, 1e4);
        ch |= number(
            ui,
            "Min Move Distance",
            &mut c.min_move_distance,
            0.001,
            0.0,
            1e4,
        );
        ch
    });
    ch |= vec3(ui, "Center", &mut c.center);
    ch |= grid(ui, "cc_dims", |ui| {
        let mut ch = number(ui, "Radius", &mut c.radius, 0.01, 0.0, 1e5);
        ch |= number(ui, "Height", &mut c.height, 0.01, 0.0, 1e5);
        ch
    });
    ch
}

fn fixed(ui: &mut Ui, j: &mut FixedJoint, bodies: &[(Entity, String)]) -> bool {
    grid(ui, "fixed_joint", |ui| {
        let mut ch = connected_body(ui, "fixed_cb", &mut j.connected_body, bodies);
        ch |= check(ui, "Enable Collision", &mut j.enable_collision);
        ch
    })
}

/// Anchor, Auto Configure Connected Anchor, Connected Anchor rows.
fn anchor_rows(
    ui: &mut Ui,
    id: &str,
    anchor: &mut Vec3,
    auto: &mut bool,
    connected: &mut Vec3,
) -> bool {
    let mut ch = vec3(ui, "Anchor", anchor);
    ch |= grid(ui, id, |ui| {
        check(ui, "Auto Configure Connected Anchor", auto)
    });
    ui.add_enabled_ui(!*auto, |ui| ch |= vec3(ui, "Connected Anchor", connected));
    ch
}

fn hinge(ui: &mut Ui, j: &mut HingeJoint, bodies: &[(Entity, String)]) -> bool {
    let mut ch = grid(ui, "hinge_cb", |ui| {
        connected_body(ui, "hinge_cb_combo", &mut j.connected_body, bodies)
    });
    ch |= anchor_rows(
        ui,
        "hinge_auto",
        &mut j.anchor,
        &mut j.auto_configure_connected_anchor,
        &mut j.connected_anchor,
    );
    ch |= vec3(ui, "Axis", &mut j.axis);
    ch |= grid(ui, "hinge_flags", |ui| {
        let mut ch = check(ui, "Use Spring", &mut j.use_spring);
        ch |= check(ui, "Use Motor", &mut j.use_motor);
        ch |= check(ui, "Use Limits", &mut j.use_limits);
        ch
    });
    egui::CollapsingHeader::new("Spring")
        .id_salt("hinge_spring")
        .show(ui, |ui| {
            ch |= grid(ui, "hinge_spring_grid", |ui| {
                let s = &mut j.spring;
                let mut ch = number(ui, "Spring", &mut s.spring, 0.1, 0.0, 1e9);
                ch |= number(ui, "Damper", &mut s.damper, 0.1, 0.0, 1e9);
                ch |= number(
                    ui,
                    "Target Position",
                    &mut s.target_position,
                    0.5,
                    -180.0,
                    180.0,
                );
                ch
            });
        });
    egui::CollapsingHeader::new("Motor")
        .id_salt("hinge_motor")
        .show(ui, |ui| {
            ch |= grid(ui, "hinge_motor_grid", |ui| {
                let m = &mut j.motor;
                let mut ch = number(
                    ui,
                    "Target Velocity",
                    &mut m.target_velocity,
                    1.0,
                    -1e6,
                    1e6,
                );
                ch |= number(ui, "Force", &mut m.force, 0.1, 0.0, 1e9);
                ch
            });
        });
    egui::CollapsingHeader::new("Limits")
        .id_salt("hinge_limits")
        .show(ui, |ui| {
            ch |= grid(ui, "hinge_limits_grid", |ui| {
                let l = &mut j.limits;
                let mut ch = number(ui, "Min", &mut l.min, 0.5, -180.0, 180.0);
                ch |= number(ui, "Max", &mut l.max, 0.5, -180.0, 180.0);
                ch
            });
        });
    ch |= grid(ui, "hinge_collision", |ui| {
        check(ui, "Enable Collision", &mut j.enable_collision)
    });
    ch
}

fn spring(ui: &mut Ui, j: &mut SpringJoint, bodies: &[(Entity, String)]) -> bool {
    let mut ch = grid(ui, "spring_cb", |ui| {
        connected_body(ui, "spring_cb_combo", &mut j.connected_body, bodies)
    });
    ch |= anchor_rows(
        ui,
        "spring_auto",
        &mut j.anchor,
        &mut j.auto_configure_connected_anchor,
        &mut j.connected_anchor,
    );
    ch |= grid(ui, "spring_grid", |ui| {
        let mut ch = number(ui, "Spring", &mut j.spring, 0.1, 0.0, 1e9);
        ch |= number(ui, "Damper", &mut j.damper, 0.01, 0.0, 1e9);
        ch |= number(ui, "Min Distance", &mut j.min_distance, 0.01, 0.0, 1e6);
        ch |= number(ui, "Max Distance", &mut j.max_distance, 0.01, 0.0, 1e6);
        ch |= check(ui, "Enable Collision", &mut j.enable_collision);
        ch
    });
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::HingeJoint;

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    #[test]
    fn joint_and_controller_editors_show_their_fields() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_physics_insp_test"),
        ));
        let frame = s.scene.spawn("Frame", None);
        s.scene.add_by_name(frame, "Rigidbody");
        let door = s.scene.spawn("Door", None);
        s.scene.add_by_name(door, "Hinge Joint");
        s.scene.add_by_name(door, "Character Controller");
        s.scene
            .world
            .get::<&mut HingeJoint>(door)
            .unwrap()
            .connected_body = Some(frame);
        s.selection.set(door);
        let mut h = Harness::builder()
            .with_size([420.0, 1600.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Connected Body",
            "Use Motor",
            "Use Limits",
            "Auto Configure Connected Anchor",
            "Slope Limit",
            "Step Offset",
            "Skin Width",
        ] {
            h.get_by_label(label);
        }
    }

    #[test]
    fn selected_joints_and_controllers_draw_gizmos() {
        let mut scene = misty_engine::scene::Scene::new("t");
        let e = scene.spawn("Door", None);
        let mut sel = crate::selection::Selection::default();
        sel.set(e);
        let empty = crate::gizmos::scene_lines(&scene, &sel).0.len();
        scene.add_by_name(e, "Hinge Joint");
        let hinge = crate::gizmos::scene_lines(&scene, &sel).0.len();
        assert!(hinge > empty, "hinge anchor and axis");
        scene.add_by_name(e, "Character Controller");
        assert!(
            crate::gizmos::scene_lines(&scene, &sel).0.len() > hinge,
            "capsule"
        );
    }
}
