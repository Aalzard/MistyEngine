//! Wheel Collider Inspector: wheel shape, then the Suspension Spring, Forward Friction and
//! Sideways Friction groups, the wheel model and drive inputs, and live values while playing.

use egui::{RichText, Ui};
use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{WheelCollider, WheelFrictionCurve};

use super::card::{grid, sub_header};
use crate::state::EditorState;
use crate::theme;
use crate::widgets::vec3_row;

/// Draw the Wheel Collider editor; true when a value changed.
pub fn wheel_collider(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut c) = state
        .scene
        .world
        .get::<&WheelCollider>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    // Any other object can be the wheel model.
    let objects: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| o)
        .filter(|o| *o != e)
        .map(|o| (o, state.scene.name(o)))
        .collect();
    let changed = edit(ui, &mut c, &objects);
    if changed {
        let _ = state.scene.world.insert_one(e, c);
    }
    changed
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

fn edit(ui: &mut Ui, c: &mut WheelCollider, objects: &[(Entity, String)]) -> bool {
    let mut ch = grid(ui, "wheel_shape", |ui| {
        let mut ch = number(ui, "Mass", &mut c.mass, 0.1, 1e-4, 1e6);
        ch |= number(ui, "Radius", &mut c.radius, 0.01, 0.0, 1e4);
        ch |= number(
            ui,
            "Wheel Damping Rate",
            &mut c.wheel_damping_rate,
            0.01,
            0.0,
            1e6,
        );
        ch |= number(
            ui,
            "Suspension Distance",
            &mut c.suspension_distance,
            0.01,
            0.0,
            1e4,
        );
        ch |= number(
            ui,
            "Force App Point Distance",
            &mut c.force_app_point_distance,
            0.01,
            -1e4,
            1e4,
        );
        ch
    });
    let mut center = c.center.to_array();
    if vec3_row(ui, "Center", &mut center, 0.01) {
        c.center = Vec3::from(center);
        ch = true;
    }

    sub_header(ui, "Suspension Spring");
    ch |= grid(ui, "wheel_spring", |ui| {
        let s = &mut c.suspension_spring;
        let mut ch = number(ui, "Spring", &mut s.spring, 10.0, 0.0, 1e9);
        ch |= number(ui, "Damper", &mut s.damper, 10.0, 0.0, 1e9);
        ch |= number(
            ui,
            "Target Position",
            &mut s.target_position,
            0.01,
            0.0,
            1.0,
        );
        ch
    });
    sub_header(ui, "Forward Friction");
    ch |= curve(ui, "wheel_forward", &mut c.forward_friction);
    sub_header(ui, "Sideways Friction");
    ch |= curve(ui, "wheel_sideways", &mut c.sideways_friction);

    sub_header(ui, "Drive");
    ch |= grid(ui, "wheel_drive", |ui| {
        let mut ch = number(ui, "Motor Torque", &mut c.motor_torque, 1.0, -1e9, 1e9);
        ch |= number(ui, "Brake Torque", &mut c.brake_torque, 1.0, 0.0, 1e9);
        ch |= number(ui, "Steer Angle", &mut c.steer_angle, 0.5, -90.0, 90.0);
        ui.label("Wheel Model");
        ch |= model_field(ui, &mut c.wheel_model, objects);
        ui.end_row();
        ch
    });

    if c.state.simulated {
        sub_header(ui, "Live");
        let s = c.state;
        grid(ui, "wheel_live", |ui| {
            let row = |ui: &mut Ui, label: &str, value: String| {
                ui.label(label);
                ui.label(RichText::new(value).color(theme::TEXT_DIM));
                ui.end_row();
            };
            row(
                ui,
                "Grounded",
                if s.is_grounded { "Yes" } else { "No" }.into(),
            );
            row(ui, "RPM", format!("{:.0}", s.rpm));
            let (dist, _) = c.travel();
            let compression = if dist > 0.0 {
                1.0 - s.extension / dist
            } else {
                0.0
            };
            row(ui, "Compression", format!("{:.0} %", compression * 100.0));
            if let Some(h) = s.hit {
                row(ui, "Load", format!("{:.0} N", h.force));
                row(ui, "Forward Slip", format!("{:.2}", h.forward_slip));
                row(ui, "Sideways Slip", format!("{:.2}", h.sideways_slip));
            }
        });
    }
    ch
}

fn curve(ui: &mut Ui, id: &str, f: &mut WheelFrictionCurve) -> bool {
    grid(ui, id, |ui| {
        let mut ch = number(ui, "Extremum Slip", &mut f.extremum_slip, 0.01, 0.0, 1e4);
        ch |= number(ui, "Extremum Value", &mut f.extremum_value, 0.01, 0.0, 1e4);
        ch |= number(ui, "Asymptote Slip", &mut f.asymptote_slip, 0.01, 0.0, 1e4);
        ch |= number(
            ui,
            "Asymptote Value",
            &mut f.asymptote_value,
            0.01,
            0.0,
            1e4,
        );
        ch |= number(ui, "Stiffness", &mut f.stiffness, 0.01, 0.0, 1e4);
        ch
    })
}

/// Scene object picker; `None (Transform)` leaves the wheel without a visible model.
fn model_field(ui: &mut Ui, v: &mut Option<Entity>, objects: &[(Entity, String)]) -> bool {
    let none = "None (Transform)";
    let name = v
        .and_then(|t| objects.iter().find(|(e, _)| *e == t))
        .map_or(none.to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    egui::ComboBox::from_id_salt("wheel_model")
        .selected_text(name)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, none).changed();
            for (e, n) in objects {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    #[test]
    fn wheel_collider_editor_shows_its_groups() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_wheel_insp_test"),
        ));
        let car = s.scene.spawn("Car", None);
        s.scene.add_by_name(car, "Rigidbody");
        let wheel = s.scene.spawn("Wheel", Some(car));
        s.scene.add_by_name(wheel, "Wheel Collider");
        s.selection.set(wheel);
        let mut h = Harness::builder()
            .with_size([420.0, 1600.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Wheel Collider",
            "Suspension Distance",
            "Force App Point Distance",
            "Suspension Spring",
            "Target Position",
            "Forward Friction",
            "Sideways Friction",
            "Wheel Model",
            "Motor Torque",
            "Steer Angle",
        ] {
            assert!(
                h.query_by_label(label).is_some(),
                "missing {label} in the Wheel Collider editor"
            );
        }
        // Both friction groups have their own rows.
        assert_eq!(h.query_all_by_label("Extremum Slip").count(), 2);
    }

    #[test]
    fn selected_wheels_draw_suspension_and_tyre_gizmos() {
        let mut scene = misty_engine::scene::Scene::new("t");
        let e = scene.spawn("Wheel", None);
        let mut sel = crate::selection::Selection::default();
        sel.set(e);
        let empty = crate::gizmos::scene_lines(&scene, &sel).0.len();
        scene.add_by_name(e, "Wheel Collider");
        let wheel = crate::gizmos::scene_lines(&scene, &sel).0.len();
        assert!(wheel > empty + 60, "suspension line and wheel circle");
        // While simulated, the contact point gets a marker too.
        scene
            .world
            .get::<&mut misty_engine::scene::WheelCollider>(e)
            .unwrap()
            .state = misty_engine::scene::WheelState {
            simulated: true,
            is_grounded: true,
            extension: 0.1,
            hit: Some(misty_engine::scene::WheelHit {
                collider: e,
                point: glam::Vec3::new(0.0, -0.6, 0.0),
                normal: glam::Vec3::Y,
                forward_dir: glam::Vec3::Z,
                sideways_dir: glam::Vec3::X,
                force: 1000.0,
                forward_slip: 0.0,
                sideways_slip: 0.0,
            }),
            ..Default::default()
        };
        assert!(crate::gizmos::scene_lines(&scene, &sel).0.len() > wheel);
        // Unselected: nothing.
        sel.clear();
        assert_eq!(crate::gizmos::scene_lines(&scene, &sel).0.len(), empty);
    }
}
