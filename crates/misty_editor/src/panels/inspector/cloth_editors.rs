//! Inspector editors for the Cloth (settings, collider lists, max distance tools) and the
//! Ragdoll (start state, blend time, its bones, the wizard, switching it while playing).

use egui::Ui;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    CapsuleCollider, Cloth, ColorChannel, MeshFilter, MeshRenderer, Ragdoll, SkinnedMeshRenderer,
    SphereCollider,
};

use super::physics_editors::{check, grid, number, vec3};
use crate::state::EditorState;

/// Objects with a component `T` (collider list choices).
fn with<T: misty_engine::hecs::Component>(state: &EditorState) -> Vec<(Entity, String)> {
    state
        .scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| state.scene.world.get::<&T>(*e).is_ok())
        .map(|e| (e, state.scene.name(e)))
        .collect()
}

/// An editable list of object slots.
fn object_list(
    ui: &mut Ui,
    title: &str,
    id: &str,
    list: &mut Vec<Option<Entity>>,
    choices: &[(Entity, String)],
) -> bool {
    let mut ch = false;
    egui::CollapsingHeader::new(format!("{title} ({})", list.len()))
        .id_salt(id)
        .show(ui, |ui| {
            let mut remove = None;
            for (i, slot) in list.iter_mut().enumerate() {
                ui.horizontal(|ui| {
                    let name = slot
                        .and_then(|e| choices.iter().find(|(c, _)| *c == e))
                        .map_or("None".to_owned(), |(_, n)| n.clone());
                    egui::ComboBox::from_id_salt((id, i))
                        .selected_text(name)
                        .show_ui(ui, |ui| {
                            ch |= ui.selectable_value(slot, None, "None").changed();
                            for (e, n) in choices {
                                ch |= ui.selectable_value(slot, Some(*e), n).changed();
                            }
                        });
                    if ui.small_button("−").on_hover_text("Remove").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                list.remove(i);
                ch = true;
            }
            if ui.small_button("+ Add").clicked() {
                list.push(None);
                ch = true;
            }
        });
    ch
}

/// Cloth component editor.
pub(super) fn cloth(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut c) = state.scene.world.get::<&Cloth>(e).map(|c| (*c).clone()) else {
        return false;
    };
    let mut ch = false;
    let smr = state
        .scene
        .world
        .get::<&SkinnedMeshRenderer>(e)
        .ok()
        .map(|r| (*r).clone());
    let vertex_count = smr
        .as_ref()
        .and_then(|r| r.mesh.as_ref())
        .and_then(misty_engine::models::mesh)
        .map(|m| m.vertices.len());
    if smr.is_none() {
        ui.colored_label(
            crate::theme::AXIS_X,
            "Cloth simulates a Skinned Mesh Renderer's mesh.",
        );
        let plain = state.scene.world.get::<&MeshFilter>(e).is_ok()
            && state.scene.world.get::<&MeshRenderer>(e).is_ok();
        if plain && ui.button("Convert to Skinned Mesh Renderer").clicked() {
            convert_to_skinned(state, e);
            return true;
        }
    }
    ch |= grid(ui, "cloth_main", |ui| {
        let mut ch = check(ui, "Enabled", &mut c.enabled);
        ch |= number(
            ui,
            "Stretching Stiffness",
            &mut c.stretching_stiffness,
            0.01,
            0.0,
            1.0,
        );
        ch |= number(
            ui,
            "Bending Stiffness",
            &mut c.bending_stiffness,
            0.01,
            0.0,
            1.0,
        );
        ch |= check(ui, "Use Tethers", &mut c.use_tethers);
        ch |= check(ui, "Use Gravity", &mut c.use_gravity);
        ch |= number(ui, "Damping", &mut c.damping, 0.01, 0.0, 1.0);
        ch
    });
    ch |= vec3(ui, "External Acceleration", &mut c.external_acceleration);
    ch |= vec3(ui, "Random Acceleration", &mut c.random_acceleration);
    ch |= grid(ui, "cloth_more", |ui| {
        let mut ch = number(ui, "Wind Scale", &mut c.wind_scale, 0.01, 0.0, 10.0);
        ch |= number(
            ui,
            "World Velocity Scale",
            &mut c.world_velocity_scale,
            0.01,
            0.0,
            1.0,
        );
        ch |= number(ui, "Friction", &mut c.friction, 0.01, 0.0, 1.0);
        ch |= number(ui, "Thickness", &mut c.thickness, 0.001, 0.0, 1.0);
        ch |= number(
            ui,
            "Solver Frequency",
            &mut c.solver_frequency,
            1.0,
            30.0,
            2000.0,
        );
        ch |= check(ui, "Self Collision", &mut c.self_collision);
        ch |= number(
            ui,
            "Self Collision Distance",
            &mut c.self_collision_distance,
            0.001,
            0.001,
            1.0,
        );
        ch
    });
    let spheres = with::<SphereCollider>(state);
    let capsules = with::<CapsuleCollider>(state);
    ch |= object_list(
        ui,
        "Sphere Colliders",
        "cloth_spheres",
        &mut c.sphere_colliders,
        &spheres,
    );
    ch |= object_list(
        ui,
        "Capsule Colliders",
        "cloth_capsules",
        &mut c.capsule_colliders,
        &capsules,
    );

    super::card::sub_header(ui, "Constraints");
    let n = vertex_count.unwrap_or(0);
    let pinned = c.max_distances.iter().filter(|d| **d == 0.0).count();
    let limited = c.max_distances.iter().filter(|d| **d > 0.0).count();
    ui.weak(format!(
        "{n} vertices: {pinned} pinned, {limited} limited, {} free",
        n.saturating_sub(pinned + limited)
    ));
    ui.horizontal(|ui| {
        let editing = crate::cloth_tools::editing() == Some(e);
        let label = if editing {
            "Stop Editing Constraints"
        } else {
            "Edit Constraints"
        };
        if ui
            .selectable_label(editing, label)
            .on_hover_text("Paint max distances in the Scene view")
            .clicked()
        {
            crate::cloth_tools::set_editing((!editing).then_some(e));
        }
        if ui.button("Free All").clicked() {
            c.max_distances.clear();
            ch = true;
        }
    });
    if crate::cloth_tools::editing() == Some(e) {
        crate::cloth_tools::brush_ui(ui);
    }
    ch |= grid(ui, "cloth_colors", |ui| {
        ui.label("Vertex Colour Channel");
        let mut ch = false;
        egui::ComboBox::from_id_salt("cloth_channel")
            .selected_text(c.color_channel.name())
            .show_ui(ui, |ui| {
                for k in ColorChannel::ALL {
                    ch |= ui
                        .selectable_value(&mut c.color_channel, k, k.name())
                        .changed();
                }
            });
        ui.end_row();
        ch |= number(ui, "Colour Scale", &mut c.color_scale, 0.01, 0.0, 100.0);
        ch
    });
    if ui
        .button("Max Distance From Vertex Colours")
        .on_hover_text("0 in the channel pins a vertex; 1 lets it move Colour Scale metres")
        .clicked()
        && let Some(mesh) = smr.as_ref().and_then(|r| r.mesh.as_ref())
    {
        match misty_engine::physics::cloth::flag::max_distances_from_colors(
            mesh,
            c.color_channel,
            c.color_scale,
        ) {
            Ok(md) => {
                c.max_distances = md;
                ch = true;
            }
            Err(err) => state.console.warn(err),
        }
    }
    if state.play.in_play_mode()
        && let Some(rt) = state.runtime.as_ref()
    {
        let s = rt.with_physics(|p| p.cloth_stats());
        ui.weak(format!(
            "Playing: {} cloth(s), {} particles, {:.2} ms per solve (background), {:.3} ms main thread",
            s.cloths, s.particles, s.solve_ms, s.main_thread_ms
        ));
    }
    if ch {
        if let Some(n) = vertex_count
            && !c.max_distances.is_empty()
        {
            c.resize_max_distances(n);
        }
        let _ = state.scene.world.insert_one(e, c);
    }
    ch
}

/// Swap a Mesh Filter + Mesh Renderer for a Skinned Mesh Renderer drawing the same mesh.
pub(crate) fn convert_to_skinned(state: &mut EditorState, e: Entity) {
    state.record("Convert to Skinned Mesh Renderer");
    let mesh = state
        .scene
        .world
        .get::<&MeshFilter>(e)
        .ok()
        .and_then(|f| f.mesh.clone());
    let r = state
        .scene
        .world
        .get::<&MeshRenderer>(e)
        .ok()
        .map(|r| (*r).clone())
        .unwrap_or_default();
    state.scene.remove_by_name(e, "Mesh Renderer");
    state.scene.remove_by_name(e, "Mesh Filter");
    state.scene.add(
        e,
        SkinnedMeshRenderer {
            mesh,
            materials: vec![r.material.clone()],
            cast_shadows: r.cast_shadows,
            receive_shadows: r.receive_shadows,
            update_when_offscreen: true,
            ..Default::default()
        },
    );
}

/// Ragdoll component editor.
pub(super) fn ragdoll(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut r) = state.scene.world.get::<&Ragdoll>(e).map(|r| (*r).clone()) else {
        return false;
    };
    let ch = grid(ui, "ragdoll_grid", |ui| {
        let mut ch = check(ui, "Enabled On Start", &mut r.enabled_on_start);
        ch |= number(ui, "Blend Time", &mut r.blend_time, 0.01, 0.0, 10.0);
        ch
    });
    let names: Vec<String> = r
        .bones
        .iter()
        .map(|b| match b.filter(|b| state.scene.contains(*b)) {
            Some(b) => state.scene.name(b),
            None => "Missing".to_owned(),
        })
        .collect();
    egui::CollapsingHeader::new(format!("Bones ({})", names.len()))
        .id_salt("ragdoll_bones")
        .show(ui, |ui| {
            for n in &names {
                ui.label(n);
            }
        });
    ui.horizontal(|ui| {
        if ui.button("Ragdoll Wizard…").clicked() {
            crate::ragdoll_wizard::open(state, Some(e));
        }
        if state.play.in_play_mode() {
            let on = state
                .runtime
                .as_ref()
                .and_then(|rt| rt.with_physics(|p| p.ragdoll_enabled(e)))
                .unwrap_or(false);
            let label = if on {
                "Back to Animation"
            } else {
                "Go Ragdoll"
            };
            if ui.button(label).clicked()
                && let Some(rt) = state.runtime.as_mut()
            {
                rt.with_physics_scene(&mut state.scene, |p, s| {
                    p.set_ragdoll(s, e, !on, None);
                });
            }
        }
    });
    if ch {
        let _ = state.scene.world.insert_one(e, r);
    }
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::{
        Cloth, MeshFilter, MeshRenderer, PrimitiveMesh, Ragdoll, SkinnedMeshRenderer,
    };

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    #[test]
    fn cloth_editor_converts_a_plain_mesh_and_edits_constraints() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_cloth_insp_test"),
        ));
        let e = s.scene.spawn("Cape", None);
        s.scene.add(e, MeshFilter::primitive(PrimitiveMesh::Plane));
        s.scene.add(e, MeshRenderer::default());
        s.scene.add_by_name(e, "Cloth");
        s.selection.set(e);
        let mut h = Harness::builder()
            .with_size([460.0, 2000.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        h.get_by_label("Convert to Skinned Mesh Renderer").click();
        h.run();
        h.run();
        assert!(h.state().scene.world.get::<&SkinnedMeshRenderer>(e).is_ok());
        assert!(h.state().scene.world.get::<&MeshFilter>(e).is_err());
        for label in [
            "Stretching Stiffness",
            "Bending Stiffness",
            "Use Tethers",
            "Wind Scale",
            "Self Collision",
            "Vertex Colour Channel",
            "Max Distance From Vertex Colours",
            "Edit Constraints",
        ] {
            h.get_by_label(label);
        }
        h.get_by_label("Edit Constraints").click();
        h.run();
        assert_eq!(crate::cloth_tools::editing(), Some(e));
        h.get_by_label("Brush Size");
        h.get_by_label("Stop Editing Constraints").click();
        h.run();
        assert_eq!(crate::cloth_tools::editing(), None);
        assert!(h.state().scene.world.get::<&Cloth>(e).is_ok());
    }

    #[test]
    fn ragdoll_editor_lists_bones_and_opens_the_wizard() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_ragdoll_insp_test"),
        ));
        let e = s.scene.spawn("Hero", None);
        let bone = s.scene.spawn("Hips", Some(e));
        s.scene.add(
            e,
            Ragdoll {
                bones: vec![Some(bone)],
                ..Default::default()
            },
        );
        s.selection.set(e);
        let mut h = Harness::builder()
            .with_size([460.0, 900.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        h.get_by_label("Enabled On Start");
        h.get_by_label("Blend Time");
        h.get_by_label("Bones (1)");
        h.get_by_label("Ragdoll Wizard…").click();
        h.run();
        assert!(crate::ragdoll_wizard::is_open());
        crate::ragdoll_wizard::close();
    }
}
