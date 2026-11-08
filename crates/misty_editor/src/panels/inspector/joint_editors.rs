//! Inspector editors for the Configurable and Character Joints, and the Break Force / Break
//! Torque rows every joint shares.

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    CharacterJoint, ConfigurableJoint, JointDrive, JointMotion, JointProjectionMode,
    RotationDriveMode, SoftJointLimit, SoftJointLimitSpring,
};

use super::physics_editors::{anchor_rows, check, connected_body, grid, number, vec3};

/// A value where infinity is allowed and shown as "Infinity" (type "inf" to set it).
pub(super) fn infinite_number(ui: &mut Ui, label: &str, v: &mut f32) -> bool {
    ui.label(label);
    let mut x = f64::from(*v);
    let ch = ui
        .add(
            egui::DragValue::new(&mut x)
                .speed(1.0)
                .range(0.0..=f64::INFINITY)
                .custom_formatter(|n, _| {
                    if n.is_infinite() || n > 1e30 {
                        "Infinity".to_owned()
                    } else {
                        format!("{n:.3}")
                            .trim_end_matches('0')
                            .trim_end_matches('.')
                            .to_owned()
                    }
                })
                .custom_parser(|t| {
                    let t = t.trim().to_lowercase();
                    if t.starts_with("inf") {
                        Some(f64::INFINITY)
                    } else {
                        t.parse().ok()
                    }
                }),
        )
        .changed();
    ui.end_row();
    if ch {
        *v = x as f32;
    }
    ch
}

/// Break Force and Break Torque rows (inside a grid).
pub(super) fn break_rows(ui: &mut Ui, force: &mut f32, torque: &mut f32) -> bool {
    let mut ch = infinite_number(ui, "Break Force", force);
    ch |= infinite_number(ui, "Break Torque", torque);
    ch
}

fn motion(ui: &mut Ui, label: &str, id: &str, m: &mut JointMotion) -> bool {
    ui.label(label);
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(m.name())
        .show_ui(ui, |ui| {
            for v in JointMotion::ALL {
                ch |= ui.selectable_value(m, v, v.name()).changed();
            }
        });
    ui.end_row();
    ch
}

fn limit_spring(ui: &mut Ui, title: &str, id: &str, s: &mut SoftJointLimitSpring) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(title)
        .id_salt(id)
        .show(ui, |ui| {
            ch |= grid(ui, &format!("{id}_g"), |ui| {
                let mut ch = number(ui, "Spring", &mut s.spring, 1.0, 0.0, 1e9);
                ch |= number(ui, "Damper", &mut s.damper, 0.1, 0.0, 1e9);
                ch
            });
        });
    ch
}

fn soft_limit(ui: &mut Ui, title: &str, id: &str, l: &mut SoftJointLimit, deg: bool) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(title)
        .id_salt(id)
        .show(ui, |ui| {
            ch |= grid(ui, &format!("{id}_g"), |ui| {
                let (lo, hi, speed) = if deg {
                    (-180.0, 180.0, 0.5)
                } else {
                    (0.0, 1e5, 0.01)
                };
                let mut ch = number(ui, "Limit", &mut l.limit, speed, lo, hi);
                ch |= number(ui, "Bounciness", &mut l.bounciness, 0.01, 0.0, 1.0);
                ch |= number(
                    ui,
                    "Contact Distance",
                    &mut l.contact_distance,
                    0.01,
                    0.0,
                    1e5,
                );
                ch
            });
        });
    ch
}

fn drive(ui: &mut Ui, title: &str, id: &str, d: &mut JointDrive) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(title)
        .id_salt(id)
        .show(ui, |ui| {
            ch |= grid(ui, &format!("{id}_g"), |ui| {
                let mut ch = number(ui, "Position Spring", &mut d.position_spring, 1.0, 0.0, 1e9);
                ch |= number(ui, "Position Damper", &mut d.position_damper, 0.1, 0.0, 1e9);
                ch |= infinite_number(ui, "Maximum Force", &mut d.maximum_force);
                ch
            });
        });
    ch
}

pub(super) fn configurable(
    ui: &mut Ui,
    j: &mut ConfigurableJoint,
    bodies: &[(Entity, String)],
) -> bool {
    let mut ch = grid(ui, "cj_cb", |ui| {
        connected_body(ui, "cj_cb_combo", &mut j.connected_body, bodies)
    });
    ch |= anchor_rows(
        ui,
        "cj_auto",
        &mut j.anchor,
        &mut j.auto_configure_connected_anchor,
        &mut j.connected_anchor,
    );
    ch |= vec3(ui, "Axis", &mut j.axis);
    ch |= vec3(ui, "Secondary Axis", &mut j.secondary_axis);
    ch |= grid(ui, "cj_motion", |ui| {
        let mut ch = motion(ui, "X Motion", "cj_xm", &mut j.x_motion);
        ch |= motion(ui, "Y Motion", "cj_ym", &mut j.y_motion);
        ch |= motion(ui, "Z Motion", "cj_zm", &mut j.z_motion);
        ch |= motion(ui, "Angular X Motion", "cj_axm", &mut j.angular_x_motion);
        ch |= motion(ui, "Angular Y Motion", "cj_aym", &mut j.angular_y_motion);
        ch |= motion(ui, "Angular Z Motion", "cj_azm", &mut j.angular_z_motion);
        ch
    });
    ch |= limit_spring(
        ui,
        "Linear Limit Spring",
        "cj_lls",
        &mut j.linear_limit_spring,
    );
    ch |= soft_limit(ui, "Linear Limit", "cj_ll", &mut j.linear_limit, false);
    ch |= limit_spring(
        ui,
        "Angular X Limit Spring",
        "cj_axls",
        &mut j.angular_x_limit_spring,
    );
    ch |= soft_limit(
        ui,
        "Low Angular X Limit",
        "cj_laxl",
        &mut j.low_angular_x_limit,
        true,
    );
    ch |= soft_limit(
        ui,
        "High Angular X Limit",
        "cj_haxl",
        &mut j.high_angular_x_limit,
        true,
    );
    ch |= limit_spring(
        ui,
        "Angular YZ Limit Spring",
        "cj_ayzls",
        &mut j.angular_yz_limit_spring,
    );
    ch |= soft_limit(
        ui,
        "Angular Y Limit",
        "cj_ayl",
        &mut j.angular_y_limit,
        true,
    );
    ch |= soft_limit(
        ui,
        "Angular Z Limit",
        "cj_azl",
        &mut j.angular_z_limit,
        true,
    );
    ch |= vec3(ui, "Target Position", &mut j.target_position);
    ch |= vec3(ui, "Target Velocity", &mut j.target_velocity);
    ch |= drive(ui, "X Drive", "cj_xd", &mut j.x_drive);
    ch |= drive(ui, "Y Drive", "cj_yd", &mut j.y_drive);
    ch |= drive(ui, "Z Drive", "cj_zd", &mut j.z_drive);
    ch |= vec3(ui, "Target Rotation", &mut j.target_rotation);
    ch |= vec3(
        ui,
        "Target Angular Velocity",
        &mut j.target_angular_velocity,
    );
    ch |= grid(ui, "cj_rdm", |ui| {
        ui.label("Rotation Drive Mode");
        let mut ch = false;
        let name = |m: RotationDriveMode| match m {
            RotationDriveMode::XAndYz => "X and YZ",
            RotationDriveMode::Slerp => "Slerp",
        };
        egui::ComboBox::from_id_salt("cj_rdm_combo")
            .selected_text(name(j.rotation_drive_mode))
            .show_ui(ui, |ui| {
                for m in [RotationDriveMode::XAndYz, RotationDriveMode::Slerp] {
                    ch |= ui
                        .selectable_value(&mut j.rotation_drive_mode, m, name(m))
                        .changed();
                }
            });
        ui.end_row();
        ch
    });
    match j.rotation_drive_mode {
        RotationDriveMode::XAndYz => {
            ch |= drive(ui, "Angular X Drive", "cj_axd", &mut j.angular_x_drive);
            ch |= drive(ui, "Angular YZ Drive", "cj_ayzd", &mut j.angular_yz_drive);
        }
        RotationDriveMode::Slerp => {
            ch |= drive(ui, "Slerp Drive", "cj_sd", &mut j.slerp_drive);
        }
    }
    ch |= grid(ui, "cj_tail", |ui| {
        ui.label("Projection Mode");
        let mut ch = false;
        let on = j.projection_mode == JointProjectionMode::PositionAndRotation;
        egui::ComboBox::from_id_salt("cj_proj")
            .selected_text(if on { "Position and Rotation" } else { "None" })
            .show_ui(ui, |ui| {
                ch |= ui
                    .selectable_value(&mut j.projection_mode, JointProjectionMode::None, "None")
                    .changed();
                ch |= ui
                    .selectable_value(
                        &mut j.projection_mode,
                        JointProjectionMode::PositionAndRotation,
                        "Position and Rotation",
                    )
                    .changed();
            });
        ui.end_row();
        ch |= number(
            ui,
            "Projection Distance",
            &mut j.projection_distance,
            0.01,
            0.0,
            1e5,
        );
        ch |= number(
            ui,
            "Projection Angle",
            &mut j.projection_angle,
            0.5,
            0.0,
            180.0,
        );
        ch |= check(ui, "Enable Collision", &mut j.enable_collision);
        ch |= break_rows(ui, &mut j.break_force, &mut j.break_torque);
        ch
    });
    ch
}

pub(super) fn character(ui: &mut Ui, j: &mut CharacterJoint, bodies: &[(Entity, String)]) -> bool {
    let mut ch = grid(ui, "chj_cb", |ui| {
        connected_body(ui, "chj_cb_combo", &mut j.connected_body, bodies)
    });
    ch |= anchor_rows(
        ui,
        "chj_auto",
        &mut j.anchor,
        &mut j.auto_configure_connected_anchor,
        &mut j.connected_anchor,
    );
    ch |= vec3(ui, "Axis", &mut j.axis);
    ch |= vec3(ui, "Swing Axis", &mut j.swing_axis);
    ch |= limit_spring(
        ui,
        "Twist Limit Spring",
        "chj_tls",
        &mut j.twist_limit_spring,
    );
    ch |= soft_limit(
        ui,
        "Low Twist Limit",
        "chj_ltl",
        &mut j.low_twist_limit,
        true,
    );
    ch |= soft_limit(
        ui,
        "High Twist Limit",
        "chj_htl",
        &mut j.high_twist_limit,
        true,
    );
    ch |= limit_spring(
        ui,
        "Swing Limit Spring",
        "chj_sls",
        &mut j.swing_limit_spring,
    );
    ch |= soft_limit(ui, "Swing 1 Limit", "chj_s1", &mut j.swing1_limit, true);
    ch |= soft_limit(ui, "Swing 2 Limit", "chj_s2", &mut j.swing2_limit, true);
    ch |= grid(ui, "chj_tail", |ui| {
        let mut ch = check(ui, "Enable Projection", &mut j.enable_projection);
        ch |= number(
            ui,
            "Projection Distance",
            &mut j.projection_distance,
            0.01,
            0.0,
            1e5,
        );
        ch |= number(
            ui,
            "Projection Angle",
            &mut j.projection_angle,
            0.5,
            0.0,
            180.0,
        );
        ch |= check(ui, "Enable Collision", &mut j.enable_collision);
        ch |= break_rows(ui, &mut j.break_force, &mut j.break_torque);
        ch
    });
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::{CharacterJoint, ConfigurableJoint, JointMotion};

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    fn state(dir: &str) -> EditorState {
        EditorState::new(ProjectBrowser::open(std::env::temp_dir().join(dir)))
    }

    #[test]
    fn configurable_joint_editor_shows_motions_limits_drives_and_breaks() {
        let mut s = state("misty_cj_insp_test");
        let e = s.scene.spawn("Arm", None);
        s.scene.add_by_name(e, "Configurable Joint");
        s.selection.set(e);
        let mut h = Harness::builder()
            .with_size([460.0, 2400.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Connected Body",
            "Secondary Axis",
            "X Motion",
            "Angular Z Motion",
            "Linear Limit",
            "Low Angular X Limit",
            "Angular YZ Limit Spring",
            "Target Position",
            "Y Drive",
            "Rotation Drive Mode",
            "Projection Mode",
            "Break Force",
            "Break Torque",
        ] {
            h.get_by_label(label);
        }
        // Drawing the editor changes nothing (unbreakable stays infinite).
        let j = (*h.state().scene.world.get::<&ConfigurableJoint>(e).unwrap()).clone();
        assert_eq!(j, ConfigurableJoint::default());
        assert!(j.break_force.is_infinite());
    }

    #[test]
    fn character_joint_editor_shows_twist_and_swing() {
        let mut s = state("misty_chj_insp_test");
        let e = s.scene.spawn("Knee", None);
        s.scene.add_by_name(e, "Character Joint");
        s.selection.set(e);
        let mut h = Harness::builder()
            .with_size([460.0, 1600.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Swing Axis",
            "Twist Limit Spring",
            "Low Twist Limit",
            "High Twist Limit",
            "Swing 1 Limit",
            "Swing 2 Limit",
            "Enable Projection",
            "Break Force",
        ] {
            h.get_by_label(label);
        }
        h.get_by_label("Enable Projection");
        assert!(
            !h.state()
                .scene
                .world
                .get::<&CharacterJoint>(e)
                .unwrap()
                .enable_projection
        );
    }

    #[test]
    fn selected_configurable_and_character_joints_draw_axes_and_limits() {
        let mut scene = misty_engine::scene::Scene::new("t");
        let e = scene.spawn("Arm", None);
        let mut sel = crate::selection::Selection::default();
        sel.set(e);
        let empty = crate::gizmos::scene_lines(&scene, &sel).0.len();
        scene.add(
            e,
            ConfigurableJoint {
                angular_x_motion: JointMotion::Limited,
                y_motion: JointMotion::Limited,
                linear_limit: misty_engine::scene::SoftJointLimit::new(0.3),
                ..Default::default()
            },
        );
        let config = crate::gizmos::scene_lines(&scene, &sel).0.len();
        assert!(config > empty + 60, "axes, the X limit arc, the Y range");
        scene.add(e, CharacterJoint::default());
        let both = crate::gizmos::scene_lines(&scene, &sel).0.len();
        assert!(both > config + 70, "twist and two swing arcs");
    }
}
