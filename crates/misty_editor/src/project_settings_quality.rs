//! Project Settings ▸ Quality: the list of levels (one is the default for new players) and the
//! selected level's shadows, rendering, textures and effects.

use egui::Ui;
use misty_engine::quality::{
    Anisotropic, AntiAliasing, QualityLevel, ShadowResolution, TextureQuality, Tier, UpscaleFilter,
};
use misty_engine::settings::ProjectSettings;
use misty_engine::sprites::import::{MAX_SIZES, max_size_label};

use crate::project_settings::section;

fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
    })
    .inner
}

fn combo<T: PartialEq + Copy>(
    ui: &mut Ui,
    id: &str,
    value: &mut T,
    all: &[T],
    label: impl Fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*value))
        .width(170.0)
        .show_ui(ui, |ui| {
            for v in all {
                ch |= ui.selectable_value(value, *v, label(*v)).changed();
            }
        });
    ch
}

/// Draw the page; `selected` is the level being edited. True when something changed.
pub fn page(ui: &mut Ui, s: &mut ProjectSettings, selected: &mut usize) -> bool {
    let q = &mut s.quality;
    let mut edited = false;
    *selected = (*selected).min(q.levels.len().saturating_sub(1));
    ui.weak("Players start on the Default level; scripts and your options menu can switch.");
    egui::Grid::new("quality_levels")
        .num_columns(3)
        .spacing([10.0, 4.0])
        .show(ui, |ui| {
            ui.strong("Level");
            ui.strong("Default");
            ui.label("");
            ui.end_row();
            for i in 0..q.levels.len() {
                let name = q.levels[i].name.clone();
                if ui.selectable_label(*selected == i, &name).clicked() {
                    *selected = i;
                }
                if ui
                    .radio(q.default_level == i, "")
                    .on_hover_text(format!("New players start on {name}"))
                    .clicked()
                {
                    q.default_level = i;
                    edited = true;
                }
                if misty_engine::quality::level() == i {
                    ui.weak("previewing");
                } else {
                    ui.label("");
                }
                ui.end_row();
            }
        });
    ui.horizontal(|ui| {
        if ui.button("Add Level").clicked() {
            let mut level = q.levels[*selected].clone();
            level.name = format!("{} Copy", level.name);
            q.levels.push(level);
            *selected = q.levels.len() - 1;
            edited = true;
        }
        let can_remove = q.levels.len() > 1;
        if ui
            .add_enabled(can_remove, egui::Button::new("Remove Level"))
            .clicked()
        {
            q.levels.remove(*selected);
            if q.default_level >= *selected && q.default_level > 0 {
                q.default_level -= 1;
            }
            *selected = (*selected).min(q.levels.len() - 1);
            edited = true;
        }
    });
    ui.add_space(6.0);
    let level = &mut q.levels[*selected];
    edited |= row(ui, "Name", "Shown in your game's options menu.", |ui| {
        ui.text_edit_singleline(&mut level.name).lost_focus()
    });
    section(ui, "Shadows", true, |ui| edited |= shadows(ui, level));
    section(ui, "Rendering", true, |ui| edited |= rendering(ui, level));
    section(ui, "Textures", true, |ui| edited |= textures(ui, level));
    section(ui, "Effects", false, |ui| edited |= effects(ui, level));
    edited
}

fn shadows(ui: &mut Ui, l: &mut QualityLevel) -> bool {
    let mut edited = row(
        ui,
        "Shadow Resolution",
        "Size of the sun's shadow map. Higher = sharper shadows, more GPU memory.",
        |ui| {
            combo(
                ui,
                "q_shadow_res",
                &mut l.shadow_resolution,
                &ShadowResolution::ALL,
                ShadowResolution::label,
            )
        },
    );
    edited |= row(
        ui,
        "Shadow Distance",
        "Shadows are drawn up to this far from the camera (metres).",
        |ui| {
            ui.add(
                egui::DragValue::new(&mut l.shadow_distance)
                    .range(1.0..=10000.0)
                    .suffix(" m"),
            )
            .changed()
        },
    );
    edited |= row(
        ui,
        "Shadow Cascades",
        "Splits the sun's shadow into 1, 2 or 4 bands so near shadows stay sharp.",
        |ui| {
            let mut ch = false;
            for n in [1, 2, 4] {
                ch |= ui
                    .selectable_value(&mut l.shadow_cascades, n, n.to_string())
                    .changed();
            }
            ch
        },
    );
    if l.shadow_cascades > 1 {
        edited |= row(
            ui,
            "Cascade Split",
            "0 = even bands, 1 = most detail close to the camera.",
            |ui| {
                ui.add(egui::Slider::new(&mut l.cascade_split, 0.0..=1.0))
                    .changed()
            },
        );
        edited |= row(
            ui,
            "Cascade Blend",
            "How much neighbouring cascades fade into each other (hides the seam).",
            |ui| {
                ui.add(egui::Slider::new(&mut l.cascade_blend, 0.0..=0.5))
                    .changed()
            },
        );
    }
    edited |= row(
        ui,
        "Soft Shadows",
        "How smooth shadow edges are (Off = hard edges, fastest).",
        |ui| combo(ui, "q_soft", &mut l.soft_shadows, &Tier::ALL, Tier::label),
    );
    if l.soft_shadows != Tier::Off {
        edited |= row(
            ui,
            "Temporal Shadows",
            "Soft shadows build up over a few frames from a pattern that moves every frame: \
             many more samples, no visible grain. Costs one screen-space pass.",
            |ui| ui.checkbox(&mut l.shadow_temporal, "").changed(),
        );
        edited |= row(
            ui,
            "Shadow Denoise",
            "Edge-aware blur of soft shadows, as wide as half their penumbra (contact \
             shadows stay crisp).",
            |ui| ui.checkbox(&mut l.shadow_denoise, "").changed(),
        );
    }
    edited
}

fn rendering(ui: &mut Ui, l: &mut QualityLevel) -> bool {
    let mut edited = row(
        ui,
        "Anti-Aliasing",
        "Smooths jagged edges. The renderer currently always uses 4x; this choice is saved \
         for when it can switch.",
        |ui| {
            combo(
                ui,
                "q_msaa",
                &mut l.anti_aliasing,
                &AntiAliasing::ALL,
                AntiAliasing::label,
            )
        },
    );
    edited |= row(
        ui,
        "FXAA",
        "Post-process edge smoothing on the final image; also catches edges MSAA misses          (alpha cut-outs, shading).",
        |ui| ui.checkbox(&mut l.fxaa, "").changed(),
    );
    edited |= row(
        ui,
        "Temporal Anti-aliasing",
        "Jitters the camera each frame and blends the frames over time: smooth edges, and thin          grass, foliage and shading that stop crawling. A little softer while moving.",
        |ui| ui.checkbox(&mut l.temporal_aa, "").changed(),
    );
    edited |= row(
        ui,
        "Render Scale",
        "Draws the game at this fraction of the window size and stretches it. Below 1 = \
         faster on weak GPUs, blurrier.",
        |ui| {
            ui.add(egui::Slider::new(&mut l.render_scale, 0.25..=2.0).max_decimals(2))
                .changed()
        },
    );
    edited |= row(
        ui,
        "Upscale Filter",
        "How the Render Scale image is stretched: Bilinear is smooth, Nearest keeps hard          pixels (pixel-art look), Sharpen adds contrast-adaptive sharpening.",
        |ui| {
            combo(
                ui,
                "q_upscale",
                &mut l.upscale_filter,
                &UpscaleFilter::ALL,
                UpscaleFilter::label,
            )
        },
    );
    edited |= row(
        ui,
        "LOD Bias",
        "Above 1 keeps LOD Groups on their detailed levels further away.",
        |ui| {
            ui.add(
                egui::DragValue::new(&mut l.lod_bias)
                    .speed(0.05)
                    .range(0.01..=10.0),
            )
            .changed()
        },
    );
    edited
}

fn textures(ui: &mut Ui, l: &mut QualityLevel) -> bool {
    let mut edited = row(
        ui,
        "Texture Quality",
        "Global mipmap limit: Half skips the largest mip level of every texture (¼ the \
         memory), Quarter skips two, and so on.",
        |ui| {
            combo(
                ui,
                "q_tex_quality",
                &mut l.texture_quality,
                &TextureQuality::ALL,
                TextureQuality::label,
            )
        },
    );
    edited |= row(
        ui,
        "Anisotropic Textures",
        "Sharper textures seen at a slant (floors, roads).\nPer Texture: each texture's Aniso \
         Level. Forced On: at least 8x for trilinear textures.",
        |ui| {
            combo(
                ui,
                "q_aniso",
                &mut l.anisotropic_textures,
                &Anisotropic::ALL,
                Anisotropic::label,
            )
        },
    );
    edited |= row(
        ui,
        "Max Texture Size",
        "No texture is uploaded larger than this, whatever its own Max Size.",
        |ui| {
            let mut ch = false;
            egui::ComboBox::from_id_salt("q_max_tex")
                .selected_text(max_size_label(l.max_texture_size))
                .width(170.0)
                .show_ui(ui, |ui| {
                    for s in MAX_SIZES {
                        ch |= ui
                            .selectable_value(&mut l.max_texture_size, s, max_size_label(s))
                            .changed();
                    }
                });
            ch
        },
    );
    edited
}

fn effects(ui: &mut Ui, l: &mut QualityLevel) -> bool {
    let mut edited = row(
        ui,
        "Ambient Occlusion",
        "Soft contact shadows in corners and creases.",
        |ui| {
            combo(
                ui,
                "q_ao",
                &mut l.ambient_occlusion,
                &Tier::ALL,
                Tier::label,
            )
        },
    );
    edited |= row(
        ui,
        "Volumetric Fog",
        "Light shafts and fog that lights shine through.",
        |ui| combo(ui, "q_fog", &mut l.volumetric_fog, &Tier::ALL, Tier::label),
    );
    edited |= row(
        ui,
        "Sharp Sun Shafts",
        "Per-pixel sun light shafts in volumetric fog (Off = softer froxel shafts).",
        |ui| {
            combo(
                ui,
                "q_sun_shafts",
                &mut l.sun_shafts,
                &Tier::ALL,
                Tier::label,
            )
        },
    );
    edited |= row(
        ui,
        "Volumetric Clouds",
        "Resolution and detail of the raymarched cloud layer.",
        |ui| {
            combo(
                ui,
                "q_clouds",
                &mut l.volumetric_clouds,
                &Tier::ALL,
                Tier::label,
            )
        },
    );
    edited |= row(
        ui,
        "Reflection Probes",
        "Resolution of reflections from Reflection Probes (Off = sky only).",
        |ui| {
            combo(
                ui,
                "q_probes",
                &mut l.reflection_probes,
                &Tier::ALL,
                Tier::label,
            )
        },
    );
    edited |= row(
        ui,
        "Lens Flares",
        "Lens Flare components draw (light halos always do).",
        |ui| ui.checkbox(&mut l.lens_flares, "").changed(),
    );
    edited
}
