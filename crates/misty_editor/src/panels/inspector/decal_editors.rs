//! Inspector of the Decal Projector (Unity URP / HDRP-like): box size and pivot, the texture
//! set, what it affects, its surface values, fades and draw order.

use egui::{DragValue, Slider, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::DecalProjector;

use super::editors::{color_rgb, grid};
use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Names of the components handled here.
pub const NAMES: [&str; 1] = ["Decal Projector"];

pub fn icon(name: &str) -> Option<&'static str> {
    (name == "Decal Projector").then_some("▣")
}

pub fn enabled_flag(state: &EditorState, e: Entity) -> Option<bool> {
    let w = &state.scene.world;
    w.get::<&DecalProjector>(e).ok().map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, on: bool) {
    let w = &state.scene.world;
    let _ = w.get::<&mut DecalProjector>(e).map(|mut c| c.enabled = on);
}

fn row(ui: &mut Ui, label: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::tutorials::targets::label(ui, label);
    let ch = widget(ui);
    ui.end_row();
    ch
}

fn drag(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>, speed: f64) -> bool {
    ui.add(DragValue::new(v).speed(speed).range(range))
        .changed()
}

fn pair(ui: &mut Ui, v: &mut [f32; 2], speed: f64) -> bool {
    ui.horizontal(|ui| {
        let a = ui.add(DragValue::new(&mut v[0]).speed(speed)).changed();
        let b = ui.add(DragValue::new(&mut v[1]).speed(speed)).changed();
        a || b
    })
    .inner
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let Ok(mut d) = state
        .scene
        .world
        .get::<&DecalProjector>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    // Materials that can drive a decal: those using a Shader Graph (Decal target).
    let mut graph_materials: Vec<String> = state
        .assets
        .materials
        .iter()
        .filter(|(_, m)| misty_engine::shader_graph::is_graph_path(&m.shader))
        .map(|(p, _)| p.clone())
        .collect();
    graph_materials.sort();
    let textures = state.assets.textures.clone();
    let mut ch = vec3_row(ui, "Size", &mut d.size, 0.02);
    ch |= vec3_row(ui, "Pivot", &mut d.pivot, 0.02);
    ch |= grid(ui, "decal_grid", |ui| {
        let mut ch = row(ui, "Material", |ui| {
            let current = d.material.as_deref().map_or(
                "None (texture maps)".to_owned(),
                misty_engine::scene::clip_name,
            );
            let mut changed = false;
            egui::ComboBox::from_id_salt("decal_material")
                .selected_text(current)
                .show_ui(ui, |ui| {
                    changed |= ui
                        .selectable_value(&mut d.material, None, "None (texture maps)")
                        .changed();
                    for m in &graph_materials {
                        changed |= ui
                            .selectable_value(
                                &mut d.material,
                                Some(m.clone()),
                                misty_engine::scene::clip_name(m),
                            )
                            .changed();
                    }
                })
                .response
                .on_hover_text("A material using a Decal Shader Graph replaces the maps below");
            changed
        });
        let slot = |ui: &mut Ui, id: &str, v: &mut Option<String>| {
            super::slots::texture_slot(ui, id, v, &textures)
        };
        ch |= row(ui, "Base Map", |ui| slot(ui, "decal_base", &mut d.base_map));
        ch |= row(ui, "Normal Map", |ui| {
            slot(ui, "decal_normal", &mut d.normal_map)
        });
        ch |= row(ui, "Mask Map", |ui| slot(ui, "decal_mask", &mut d.mask_map));
        ch |= row(ui, "Color", |ui| {
            let mut srgba = d.color.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
            let changed = ui
                .color_edit_button_srgba_unmultiplied(&mut srgba)
                .changed();
            if changed {
                d.color = srgba.map(|v| v as f32 / 255.0);
            }
            changed
        });
        ch |= row(ui, "Tiling", |ui| pair(ui, &mut d.tiling, 0.01));
        ch |= row(ui, "Offset", |ui| pair(ui, &mut d.offset, 0.01));
        ch |= row(ui, "Opacity", |ui| {
            ui.add(Slider::new(&mut d.opacity, 0.0..=1.0)).changed()
        });
        ch |= row(ui, "Affects Base Color", |ui| {
            ui.checkbox(&mut d.affects_albedo, "").changed()
        });
        ch |= row(ui, "Affects Normal", |ui| {
            ui.checkbox(&mut d.affects_normal, "").changed()
        });
        ch |= row(ui, "Affects Mask", |ui| {
            ui.checkbox(&mut d.affects_mask, "")
                .on_hover_text("Metallic, smoothness and occlusion")
                .changed()
        });
        ch |= row(ui, "Normal Blend", |ui| {
            ui.add(Slider::new(&mut d.normal_strength, 0.0..=1.0))
                .changed()
        });
        ch |= row(ui, "Metallic", |ui| {
            ui.add(Slider::new(&mut d.metallic, 0.0..=1.0)).changed()
        });
        ch |= row(ui, "Smoothness", |ui| {
            ui.add(Slider::new(&mut d.smoothness, 0.0..=1.0)).changed()
        });
        ch |= row(ui, "Emission", |ui| color_rgb(ui, &mut d.emission));
        ch |= row(ui, "Emission Intensity", |ui| {
            drag(ui, &mut d.emission_intensity, 0.0..=100.0, 0.05)
        });
        ch |= row(ui, "Draw Distance", |ui| {
            drag(ui, &mut d.draw_distance, 0.0..=1e5, 0.5)
        });
        ch |= row(ui, "Start Fade", |ui| {
            ui.add(Slider::new(&mut d.start_fade, 0.0..=1.0)).changed()
        });
        ch |= row(ui, "Angle Fade", |ui| {
            let a = ui.add(
                DragValue::new(&mut d.angle_fade[0])
                    .speed(0.5)
                    .range(0.0..=180.0)
                    .suffix("°"),
            );
            let b = ui.add(
                DragValue::new(&mut d.angle_fade[1])
                    .speed(0.5)
                    .range(0.0..=180.0)
                    .suffix("°"),
            );
            a.changed() || b.changed()
        });
        ch |= row(ui, "Priority", |ui| {
            ui.add(DragValue::new(&mut d.priority).speed(0.1))
                .on_hover_text("Higher draws on top")
                .changed()
        });
        ch
    });
    ui.weak("Projects along its blue (Z) axis onto opaque meshes and terrain inside the box.");
    if ch {
        d.size = d.size.map(|s| s.max(0.0));
        let _ = state.scene.world.insert_one(e, d);
    }
    ch
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;
    use misty_engine::scene::{DecalProjector, PostProcessVolume};

    use crate::project::ProjectBrowser;
    use crate::state::EditorState;

    fn state(name: &str) -> EditorState {
        EditorState::new(ProjectBrowser::open(std::env::temp_dir().join(name)))
    }

    #[test]
    fn decal_projector_inspector_and_gizmo() {
        let mut s = state("misty_decal_insp_test");
        let e = s.scene.spawn("Decal", None);
        s.scene.add_by_name(e, "Decal Projector");
        assert!(s.scene.world.get::<&DecalProjector>(e).is_ok());
        s.selection.set(e);
        let lines = crate::gizmos::scene_lines(&s.scene, &s.selection).0.len();
        assert!(lines >= 24, "box, arrow and up chevron: {lines}");
        let mut h = Harness::builder()
            .with_size([420.0, 1600.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Base Map",
            "Normal Map",
            "Mask Map",
            "Opacity",
            "Affects Normal",
            "Angle Fade",
            "Draw Distance",
            "Priority",
        ] {
            h.get_by_label(label);
        }
    }

    #[test]
    fn rendering_menu_makes_a_projector_pointing_down() {
        let mut s = state("misty_decal_create_test");
        let what = crate::actions::Create::DecalProjector;
        crate::commands::apply(
            &mut s,
            crate::actions::Action::Create { what, parent: None },
        );
        let e = s.scene.find_by_name("Decal Projector").expect("created");
        assert!(s.scene.world.get::<&DecalProjector>(e).is_ok());
        // Its projection axis (+Z) points down.
        let forward = s.scene.world_matrix(e).z_axis.truncate().normalize();
        assert!(forward.y < -0.99, "{forward}");
    }

    #[test]
    fn volume_shows_the_camera_effect_overrides() {
        let mut s = state("misty_postfx_insp_test");
        let e = s.scene.spawn("Global Volume", None);
        let mut v = PostProcessVolume::default();
        v.depth_of_field.active = true;
        v.depth_of_field.mode = misty_engine::scene::DofMode::Bokeh;
        v.auto_exposure.active = true;
        s.scene.add(e, v);
        s.selection.set(e);
        let mut h = Harness::builder()
            .with_size([420.0, 4000.0])
            .build_ui_state(|ui, s: &mut EditorState| super::super::show(ui, s), s);
        h.run();
        for label in [
            "Depth of Field",
            "Motion Blur",
            "Auto Exposure",
            "Focus Distance",
            "Aperture (f-stop)",
            "Blade Count",
            "Metering Mode",
            "Speed Dark to Light",
        ] {
            h.get_by_label(label);
        }
    }
}
