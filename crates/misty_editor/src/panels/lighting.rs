//! Lighting window (Window ▸ Rendering ▸ Lighting): Scene and Environment tabs. The
//! Environment tab edits the open scene's skybox, ambient light, reflections and fog
//! (`Scene::render_settings`); edits are undoable and mark the scene dirty.

use egui::{Grid, RichText, Ui};
use misty_engine::render_scene;
use misty_engine::scene::render_settings::{
    AmbientSource, FogMode, RenderSettings, SkyboxKind, SunDisk,
};

use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Scene,
    Environment,
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let id = ui.id().with("lighting_page");
    let mut page: Page = ui.data(|d| d.get_temp(id)).unwrap_or_default();
    ui.horizontal(|ui| {
        ui.selectable_value(&mut page, Page::Scene, "Scene");
        ui.selectable_value(&mut page, Page::Environment, "Environment");
    });
    ui.data_mut(|d| d.insert_temp(id, page));
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match page {
            Page::Scene => scene_page(ui, state),
            Page::Environment => environment_page(ui, state),
        });
}

/// Baked lighting isn't supported: everything is realtime.
fn scene_page(ui: &mut Ui, state: &EditorState) {
    ui.label(RichText::new("Lighting Settings").strong());
    ui.horizontal(|ui| {
        ui.label("Scene");
        ui.weak(&state.scene.name);
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Realtime Lighting").strong());
    ui.add_enabled(
        false,
        egui::Checkbox::new(&mut false, "Realtime Global Illumination"),
    );
    ui.add_space(6.0);
    ui.label(RichText::new("Lightmapping").strong());
    ui.weak("All lights are realtime; lightmap baking is not available.");
    ui.add_enabled(false, egui::Button::new("Generate Lighting"));
}

fn environment_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.render_settings.clone();
    let sun = render_scene::lights(&state.scene)
        .iter()
        .any(|l| l.kind == misty_engine::scene::LightType::Directional);
    if edit_environment(ui, &mut s, sun) {
        state.scene.render_settings = s;
        state.mark_edited("Lighting");
    }
}

/// All Environment tab fields; true when something changed.
fn edit_environment(ui: &mut Ui, s: &mut RenderSettings, has_sun: bool) -> bool {
    let mut ch = false;
    ui.label(RichText::new("Environment").strong());
    ch |= grid(ui, "lighting_skybox", |ui| {
        let sky = &mut s.skybox;
        let mut ch = row(ui, "Skybox", |ui| {
            combo(
                ui,
                "sky_kind",
                &mut sky.kind,
                &SkyboxKind::ALL,
                SkyboxKind::label,
            )
        });
        match sky.kind {
            SkyboxKind::Procedural => {
                ch |= row(ui, "Sun", |ui| {
                    combo(
                        ui,
                        "sun_disk",
                        &mut sky.sun_disk,
                        &SunDisk::ALL,
                        SunDisk::label,
                    )
                });
                ch |= row(ui, "Sun Size", |ui| {
                    slider(ui, &mut sky.sun_size, 0.0..=1.0)
                });
                ch |= row(ui, "Atmosphere Thickness", |ui| {
                    slider(ui, &mut sky.atmosphere_thickness, 0.0..=5.0)
                });
                ch |= row(ui, "Sky Tint", |ui| color(ui, &mut sky.sky_tint));
                ch |= row(ui, "Ground", |ui| color(ui, &mut sky.ground));
                ch |= row(ui, "Exposure", |ui| {
                    slider(ui, &mut sky.exposure, 0.0..=8.0)
                });
            }
            SkyboxKind::SolidColor => {
                ch |= row(ui, "Color", |ui| color(ui, &mut sky.color));
            }
        }
        row(ui, "Sun Source", |ui| {
            let text = if has_sun {
                "Brightest Directional Light"
            } else {
                "None (no Directional Light)"
            };
            ui.weak(text);
            false
        });
        ch
    });

    ui.add_space(4.0);
    ui.label(RichText::new("Environment Lighting").strong());
    ch |= grid(ui, "lighting_ambient", |ui| {
        let env = &mut s.environment_lighting;
        let mut ch = row(ui, "Source", |ui| {
            combo(
                ui,
                "ambient_source",
                &mut env.source,
                &AmbientSource::ALL,
                AmbientSource::label,
            )
        });
        match env.source {
            AmbientSource::Skybox => {}
            AmbientSource::Gradient => {
                ch |= row(ui, "Sky Color", |ui| color(ui, &mut env.sky_color));
                ch |= row(ui, "Equator Color", |ui| color(ui, &mut env.equator_color));
                ch |= row(ui, "Ground Color", |ui| color(ui, &mut env.ground_color));
            }
            AmbientSource::Color => {
                ch |= row(ui, "Ambient Color", |ui| color(ui, &mut env.sky_color));
            }
        }
        ch |= row(ui, "Intensity Multiplier", |ui| {
            slider(ui, &mut env.intensity, 0.0..=8.0)
        });
        ch
    });

    ui.add_space(4.0);
    ui.label(RichText::new("Environment Reflections").strong());
    ch |= grid(ui, "lighting_reflections", |ui| {
        row(ui, "Source", |ui| {
            ui.weak("Skybox");
            false
        });
        row(ui, "Intensity Multiplier", |ui| {
            slider(ui, &mut s.reflection_intensity, 0.0..=1.0)
        })
    });

    ui.add_space(8.0);
    ui.label(RichText::new("Other Settings").strong());
    ch |= grid(ui, "lighting_fog", |ui| {
        let fog = &mut s.fog;
        let mut ch = ui.checkbox(&mut fog.enabled, "Fog").changed();
        ui.end_row();
        if fog.enabled {
            ch |= row(ui, "Color", |ui| color(ui, &mut fog.color));
            ch |= row(ui, "Mode", |ui| {
                combo(ui, "fog_mode", &mut fog.mode, &FogMode::ALL, FogMode::label)
            });
            if fog.mode == FogMode::Linear {
                ch |= row(ui, "Start", |ui| drag(ui, &mut fog.start));
                ch |= row(ui, "End", |ui| drag(ui, &mut fog.end));
            } else {
                ch |= row(ui, "Density", |ui| slider(ui, &mut fog.density, 0.0..=1.0));
            }
        }
        ch
    });
    ch
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.indent(id, |ui| {
        Grid::new(id)
            .num_columns(2)
            .spacing([10.0, 4.0])
            .min_col_width(150.0)
            .show(ui, body)
            .inner
    })
    .inner
}

/// Label + widget on one grid row.
fn row(ui: &mut Ui, label: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.label(label);
    let ch = widget(ui);
    ui.end_row();
    ch
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    options: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .width(160.0)
        .show_ui(ui, |ui| {
            for &o in options {
                changed |= ui.selectable_value(value, o, label(o)).changed();
            }
        });
    changed
}

fn slider(ui: &mut Ui, v: &mut f32, range: std::ops::RangeInclusive<f32>) -> bool {
    ui.add(egui::Slider::new(v, range)).changed()
}

fn drag(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(egui::DragValue::new(v).speed(0.5).range(0.0..=1e6))
        .changed()
}

/// sRGB float colour through egui's sRGB colour picker.
fn color(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut srgb = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = ui.color_edit_button_srgb(&mut srgb).changed();
    if changed {
        *c = srgb.map(|v| v as f32 / 255.0);
    }
    changed
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::actions::Action;
    use crate::layout::Tab;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> EditorState {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    #[test]
    fn window_menu_opens_the_lighting_window() {
        let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| crate::menu_bar::show(ui, s),
            state("misty_lighting_menu_test"),
        );
        h.run();
        h.get_by_label("Window").click();
        h.run();
        h.get_by_label("Rendering ⏵").hover();
        h.run();
        h.get_by_label("Lighting").click();
        h.run();
        let actions = std::mem::take(&mut h.state_mut().actions);
        assert!(
            matches!(actions.as_slice(), [Action::FocusTab(Tab::Lighting)]),
            "{actions:?}"
        );
    }

    #[test]
    fn editing_fog_marks_the_scene_dirty_and_undoes() {
        let mut h = Harness::builder().with_size([520.0, 640.0]).build_ui_state(
            |ui, s: &mut EditorState| show(ui, s),
            state("misty_lighting_fog_test"),
        );
        h.run();
        h.get_by_label("Realtime Global Illumination");
        h.get_by_label("Environment").click();
        h.run();
        h.get_by_label("Atmosphere Thickness");
        assert!(
            h.query_by_label("Density").is_none(),
            "fog fields hidden while off"
        );
        let s = h.state_mut();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        h.get_by_label("Fog").click();
        h.run();
        let s = h.state_mut();

        assert!(s.scene.render_settings.fog.enabled);
        assert!(s.scene_dirty);
        // The click was one interaction: commit it like the app does at the end of a frame.
        s.undo.end_frame(false);
        crate::commands::apply(s, Action::Undo);
        assert!(!s.scene.render_settings.fog.enabled);
        s.scene.render_settings.fog.enabled = true;
        h.run();
        h.get_by_label("Density");
        h.get_by_label("Mode");
    }
}
