//! Lighting window (Window ▸ Rendering ▸ Lighting): Scene and Environment tabs. The Scene tab
//! holds the lightmapping settings and Generate Lighting (`lightmap_tools`); the Environment
//! tab edits the open scene's skybox, ambient light, reflections and fog
//! (`Scene::render_settings`); edits are undoable and mark the scene dirty.

use egui::{Grid, RichText, Ui};
use misty_engine::render_scene;
use misty_engine::scene::LightmapSettings;
use misty_engine::scene::lightmapping::MixedLightingMode;
use misty_engine::scene::render_settings::{
    AmbientSource, FogMode, RenderSettings, SkyboxKind, SunDisk,
};

use super::{lighting_hdri, lighting_moon};
use crate::state::EditorState;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum Page {
    #[default]
    Scene,
    Environment,
}

pub fn show(ui: &mut Ui, state: &mut EditorState) {
    let id = ui.id().with("lighting_page");
    // `MISTY_LIGHTING_PAGE=Environment` opens that page first (screenshots).
    let mut page: Page = ui.data(|d| d.get_temp(id)).unwrap_or_else(|| {
        match std::env::var("MISTY_LIGHTING_PAGE").as_deref() {
            Ok("Environment") => Page::Environment,
            _ => Page::Scene,
        }
    });
    ui.horizontal(|ui| {
        ui.selectable_value(&mut page, Page::Scene, "Scene");
        ui.selectable_value(&mut page, Page::Environment, "Environment");
    });
    ui.data_mut(|d| d.insert_temp(id, page));
    ui.separator();
    let mut area = egui::ScrollArea::vertical().auto_shrink([false, false]);
    if let Some(px) = crate::capture::lighting_scroll() {
        area = area.vertical_scroll_offset(px);
    }
    area.show(ui, |ui| match page {
        Page::Scene => scene_page(ui, state),
        Page::Environment => environment_page(ui, state),
    });
}

/// Lighting Settings of the scene: Mixed Lighting, Lightmapping Settings, Generate Lighting
/// (background bake with progress and Cancel), Clear Baked Data; then the probe tools.
fn scene_page(ui: &mut Ui, state: &mut EditorState) {
    ui.label(RichText::new("Lighting Settings").strong());
    ui.horizontal(|ui| {
        ui.label("Scene");
        ui.weak(&state.scene.name);
    });
    ui.add_space(6.0);
    ui.label(RichText::new("Realtime Lighting").strong());
    let mut gi = state.scene.render_settings.realtime_gi;
    if ui
        .checkbox(&mut gi, "Realtime Global Illumination")
        .on_hover_text(
            "Misty GI: bounce light, sky occlusion and glossy reflections that follow moving \
            lights and objects, no baking. Quality ▸ Global Illumination sets how good it is; \
            a Post-process Volume can override it.",
        )
        .changed()
    {
        state.scene.render_settings.realtime_gi = gi;
        state.mark_edited(if gi {
            "Enable Realtime Global Illumination"
        } else {
            "Disable Realtime Global Illumination"
        });
    }
    ui.add_space(6.0);
    let mut s = state.scene.render_settings.lightmapping.clone();
    let busy = state.lightmapper.baking();
    let changed = ui
        .add_enabled_ui(!busy, |ui| lightmap_settings(ui, &mut s))
        .inner;
    if changed {
        let label = crate::undo_names::struct_change(
            "Lightmapping ",
            &state.scene.render_settings.lightmapping,
            &s,
        );
        state.scene.render_settings.lightmapping = s;
        state.mark_edited(&label);
    }
    ui.add_space(4.0);
    let play = state.play.in_play_mode();
    ui.horizontal(|ui| {
        if busy {
            if ui.button("Cancel").clicked() {
                crate::lightmap_tools::cancel(state);
            }
        } else if ui
            .add_enabled(!play, egui::Button::new("Generate Lighting"))
            .on_hover_text("Bake lightmaps for the static objects (runs in the background)")
            .clicked()
        {
            crate::lightmap_tools::start(state);
        }
        if ui
            .add_enabled(!busy && !play, egui::Button::new("Clear Baked Data"))
            .clicked()
        {
            crate::lightmap_tools::clear(state);
        }
    });
    if let Some((f, label)) = state.lightmapper.progress() {
        ui.add(egui::ProgressBar::new(f).text(label).show_percentage());
        ui.ctx().request_repaint();
    } else if !state.lightmapper.status.is_empty() {
        ui.weak(&state.lightmapper.status);
    } else {
        let n = state.scene.render_settings.lightmapping.lightmaps.len();
        ui.weak(match n {
            0 => "No baked lightmaps.".to_string(),
            1 => "1 baked lightmap.".to_string(),
            n => format!("{n} baked lightmaps."),
        });
    }
    ui.add_space(8.0);
    crate::probe_tools::lighting_section(ui, state);
    crate::light_probe_tools::lighting_section(ui, state);
}

/// Mixed Lighting and Lightmapping Settings rows; true when something changed.
fn lightmap_settings(ui: &mut Ui, s: &mut LightmapSettings) -> bool {
    let mut ch = false;
    ui.label(RichText::new("Mixed Lighting").strong());
    ch |= grid(ui, "lighting_mixed", |ui| {
        let mut ch = row(ui, "Baked Global Illumination", |ui| {
            ui.checkbox(&mut s.baked_gi, "").changed()
        });
        ch |= row(ui, "Lighting Mode", |ui| {
            combo(
                ui,
                "mixed_mode",
                &mut s.mixed_mode,
                &MixedLightingMode::ALL,
                MixedLightingMode::label,
            )
        });
        ch
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Lightmapping Settings").strong());
    ch |= grid(ui, "lighting_lightmapping", |ui| {
        let mut ch = row(ui, "Lightmapper", |ui| {
            ui.weak("CPU Path Tracer");
            false
        });
        ch |= row(ui, "Samples", |ui| {
            ui.add(egui::DragValue::new(&mut s.samples).range(1..=4096))
                .changed()
        });
        ch |= row(ui, "Bounces", |ui| {
            ui.add(egui::Slider::new(&mut s.bounces, 0..=4)).changed()
        });
        ch |= row(ui, "Lightmap Resolution", |ui| {
            ui.add(
                egui::DragValue::new(&mut s.resolution)
                    .speed(0.1)
                    .range(0.1..=200.0)
                    .suffix(" texels per unit"),
            )
            .changed()
        });
        ch |= row(ui, "Lightmap Padding", |ui| {
            ui.add(
                egui::DragValue::new(&mut s.padding)
                    .range(0..=16)
                    .suffix(" texels"),
            )
            .changed()
        });
        ch |= row(ui, "Max Lightmap Size", |ui| {
            let mut c = false;
            egui::ComboBox::from_id_salt("lightmap_max_size")
                .selected_text(s.max_size.to_string())
                .width(160.0)
                .show_ui(ui, |ui| {
                    for v in misty_engine::scene::lightmapping::MAX_SIZES {
                        c |= ui
                            .selectable_value(&mut s.max_size, v, v.to_string())
                            .changed();
                    }
                });
            c
        });
        ch |= row(ui, "Indirect Intensity", |ui| {
            slider(ui, &mut s.indirect_intensity, 0.0..=5.0)
        });
        ch
    });
    ch
}

fn environment_page(ui: &mut Ui, state: &mut EditorState) {
    let mut s = state.scene.render_settings.clone();
    let sun = render_scene::lights(&state.scene)
        .iter()
        .any(|l| l.kind == misty_engine::scene::LightType::Directional);
    let hdri = (s.skybox.kind == SkyboxKind::Hdri).then(|| lighting_hdri::context(ui, state, sun));
    let mut align_sun = false;
    let mut ch = edit_environment(ui, &mut s, sun, hdri.as_ref(), &mut align_sun);
    if s.skybox.kind != SkyboxKind::SolidColor {
        let cx = lighting_moon::MoonContext::new(state.assets.textures.clone(), &s.moon);
        ch |= lighting_moon::section(ui, &mut s.moon, &cx);
        if s.skybox.kind == SkyboxKind::Procedural {
            let date_time = s.moon.mode == misty_engine::moon::MoonMode::DateTime;
            ch |= lighting_moon::night_section(ui, &mut s.night_sky, date_time);
        }
    }
    if ch {
        let label = crate::undo_names::struct_change("", &state.scene.render_settings, &s);
        state.scene.render_settings = s;
        state.mark_edited(&label);
    }
    if align_sun {
        lighting_hdri::align_sun(state);
    }
}

/// All Environment tab fields; true when something changed. `hdri` is given while the skybox
/// is an HDRI; `align_sun` is set when its sun button is clicked.
fn edit_environment(
    ui: &mut Ui,
    s: &mut RenderSettings,
    has_sun: bool,
    hdri: Option<&lighting_hdri::HdriContext>,
    align_sun: &mut bool,
) -> bool {
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
            SkyboxKind::Hdri => {
                // The context is built for the kind stored at the start of the frame.
                if let Some(cx) = hdri {
                    let edit = lighting_hdri::rows(ui, &mut sky.hdri, cx);
                    ch |= edit.changed;
                    *align_sun |= edit.align_sun;
                }
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

pub(super) fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
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
pub(super) fn row(ui: &mut Ui, label: &str, widget: impl FnOnce(&mut Ui) -> bool) -> bool {
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
pub(super) fn color(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
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

    /// Dropping an HDRI on the Scene view sets the sky and undoes; the HDRI rows show, with
    /// the load error for a missing file.
    #[test]
    fn hdri_sky_rows_and_scene_drop() {
        use misty_engine::scene::render_settings::SkyboxKind;
        let mut s = state("misty_lighting_hdri_test");
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        assert!(!lighting_hdri::drop_on_scene(&mut s, "Assets/a.png"));
        assert!(lighting_hdri::drop_on_scene(
            &mut s,
            "Assets/HDRI/Missing.hdr"
        ));
        s.undo.end_frame(false);
        let sky = &s.scene.render_settings.skybox;
        assert_eq!(sky.kind, SkyboxKind::Hdri);
        assert_eq!(sky.hdri.texture.as_deref(), Some("Assets/HDRI/Missing.hdr"));
        crate::commands::apply(&mut s, Action::Undo);
        assert_eq!(s.scene.render_settings.skybox.kind, SkyboxKind::Procedural);
        crate::commands::apply(&mut s, Action::Redo);

        let mut h = Harness::builder()
            .with_size([520.0, 700.0])
            .build_ui_state(|ui, s: &mut EditorState| show(ui, s), s);
        h.run();
        h.get_by_label("Environment").click();
        h.run();
        h.get_by_label("Rotation");
        h.get_by_label("Exposure");
        h.get_by_label("Tint");
        // The error line names the missing file.
        assert!(h.query_all_by_label_contains("Missing.hdr").count() >= 1);
        assert!(h.query_by_label("Atmosphere Thickness").is_none());
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

    /// Lighting ▸ Realtime Global Illumination turns Misty GI on for the scene's cameras.
    #[test]
    fn realtime_gi_checkbox_turns_misty_gi_on_and_undoes() {
        let mut h = Harness::builder().with_size([520.0, 640.0]).build_ui_state(
            |ui, s: &mut EditorState| show(ui, s),
            state("misty_lighting_gi_test"),
        );
        h.run();
        assert!(
            !misty_engine::render_scene::post_settings(&h.state().scene)
                .misty_gi
                .active()
        );
        let s = h.state_mut();
        s.undo.begin_frame(true, &s.scene, &s.selection, || None);
        h.get_by_label("Realtime Global Illumination").click();
        h.run();
        let s = h.state_mut();
        assert!(s.scene.render_settings.realtime_gi);
        assert!(s.scene_dirty);
        assert!(
            misty_engine::render_scene::post_settings(&s.scene)
                .misty_gi
                .active()
        );
        s.undo.end_frame(false);
        crate::commands::apply(s, Action::Undo);
        assert!(!s.scene.render_settings.realtime_gi);
    }
}
