//! Project Settings ▸ Quality: the list of levels (one is the default for new players) and the
//! selected level's shadows, rendering, textures and effects.

use egui::Ui;
use misty_engine::quality::{
    Anisotropic, AntiAliasing, QualityLevel, ShadowMode, ShadowPool, ShadowResolution,
    TextureQuality, Tier, UPSCALING_MIN, UpscaleFilter, UpscalerBackend, Upscaling,
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
    // The page edits the level the views are showing, so every change is visible at once.
    *selected = misty_engine::quality::level().min(q.levels.len().saturating_sub(1));
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
                if ui
                    .selectable_label(*selected == i, &name)
                    .on_hover_text("Edit and preview this level")
                    .clicked()
                {
                    *selected = i;
                    misty_engine::quality::set_level(i);
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
            misty_engine::quality::install(q, *selected);
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
            misty_engine::quality::install(q, *selected);
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
        "Shadows",
        "How the sun's shadows are made. Cascaded: up to four maps fitted around the view          every frame. Virtual: a virtual shadow map of 128 × 128 pages around the camera, only          the pages visible pixels need, kept between frames until something over them moves          (sharper close up, cheap while the view and the scene stay still).",
        |ui| {
            combo(
                ui,
                "q_shadow_mode",
                &mut l.shadow_mode,
                &ShadowMode::ALL,
                ShadowMode::label,
            )
        },
    );
    let virtual_sun = l.shadow_mode == ShadowMode::Virtual;
    if virtual_sun {
        edited |= row(
            ui,
            "Shadow Page Pool",
            "Physical pages the virtual shadow map keeps (GPU memory). Too few for the view:              distant shadows fall back to coarser pages.",
            |ui| {
                combo(
                    ui,
                    "q_shadow_pool",
                    &mut l.shadow_pool,
                    &ShadowPool::ALL,
                    ShadowPool::label,
                )
            },
        );
    }
    let resolution_tip = if virtual_sun {
        "Texel size of the virtual shadow map against a screen pixel: Very High ½, High 1,          Medium 2, Low 4 pixels per texel."
    } else {
        "Size of the sun's shadow map. Higher = sharper shadows, more GPU memory."
    };
    edited |= row(ui, "Shadow Resolution", resolution_tip, |ui| {
        combo(
            ui,
            "q_shadow_res",
            &mut l.shadow_resolution,
            &ShadowResolution::ALL,
            ShadowResolution::label,
        )
    });
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
    if !virtual_sun {
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
        "Multisampling of the 3D view: smooths jagged edges (4x smoother, 2x cheaper on dense \
         grass). Disabled renders 2x for now. Changing it rebuilds the renderer (a short hitch).",
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
        "Jitters the camera each frame and blends the frames over time, following moving \
         objects and the wind through motion vectors: smooth edges, and thin grass, foliage \
         and shading that stop crawling.",
        |ui| ui.checkbox(&mut l.temporal_aa, "").changed(),
    );
    edited |= row(
        ui,
        "Upscaling",
        "Temporal upscaling: renders the 3D view at a share of its resolution (67, 58 or 50 \
         percent per side, or Custom) and rebuilds the full resolution from the jittered \
         frames of the temporal resolve. Much faster; post effects and the UI stay at full \
         resolution. Turns temporal anti-aliasing on.",
        |ui| {
            combo(
                ui,
                "q_upscaling",
                &mut l.upscaling,
                &Upscaling::ALL,
                Upscaling::label,
            )
        },
    );
    if l.upscaling == Upscaling::Custom {
        edited |= row(
            ui,
            "Upscaling Scale",
            "Share of the output resolution the 3D view renders at per side (Custom).",
            |ui| {
                ui.add(
                    egui::Slider::new(&mut l.upscaling_scale, UPSCALING_MIN..=1.0)
                        .custom_formatter(|v, _| format!("{:.0}%", v * 100.0))
                        .custom_parser(|t| {
                            t.trim()
                                .trim_end_matches('%')
                                .parse::<f64>()
                                .ok()
                                .map(|v| v / 100.0)
                        }),
                )
                .changed()
            },
        );
    }
    if l.upscaling != Upscaling::Off {
        edited |= row(
            ui,
            "Upscaler",
            "Which upscaler rebuilds the resolution. Misty Temporal is built in; vendor \
             upscalers can be added as further choices.",
            |ui| {
                combo(
                    ui,
                    "q_upscaler",
                    &mut l.upscaler,
                    &UpscalerBackend::ALL,
                    UpscalerBackend::label,
                )
            },
        );
    }
    if l.temporal() {
        edited |= row(
            ui,
            "TAA Blend",
            "Share of the newest frame in the blend: lower is smoother and slower to settle, \
             higher is crisper and noisier.",
            |ui| {
                ui.add(egui::Slider::new(&mut l.taa_blend, 0.02..=0.5).max_decimals(2))
                    .changed()
            },
        );
        edited |= row(
            ui,
            "TAA Sharpness",
            "Sharpens the accumulated history against the softening of blending, so a still \
             image stays as crisp as a single frame.",
            |ui| {
                ui.add(egui::Slider::new(&mut l.taa_sharpness, 0.0..=1.0).max_decimals(2))
                    .changed()
            },
        );
    }
    edited |= row(
        ui,
        "Render Scale",
        "Draws the game at this fraction of the window size and stretches it. Below 1 = \
         faster on weak GPUs, blurrier (Upscaling rebuilds a sharper image instead).",
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
    edited |= row(
        ui,
        "Virtual Geometry",
        "Prototype: models imported with Virtual Geometry draw as clusters with per-cluster          level of detail, culled on the GPU. Off = they draw as ordinary meshes.",
        |ui| ui.checkbox(&mut l.virtual_geometry, "").changed(),
    );
    if l.virtual_geometry {
        edited |= row(
            ui,
            "Virtual Geometry Error",
            "Largest simplification error on screen, in pixels: lower draws more triangles.",
            |ui| {
                ui.add(
                    egui::DragValue::new(&mut l.virtual_geometry_error)
                        .speed(0.05)
                        .range(0.1..=16.0)
                        .suffix(" px"),
                )
                .changed()
            },
        );
    }
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
    edited |= row(
        ui,
        "Grass Shadows",
        "Terrain grass near the camera casts shadows: Low 12 m, Medium 20 m, High 32 m (Off = none cast; grass still receives shadows).",
        |ui| {
            combo(
                ui,
                "q_grass_shadows",
                &mut l.grass_shadows,
                &Tier::ALL,
                Tier::label,
            )
        },
    );
    edited |= row(
        ui,
        "Grass Ground Cover",
        "Paints the middle distance's undergrowth on the ground instead of drawing the flat \
         fans of the blades the grass LOD shrinks away: faster, and a little different.",
        |ui| ui.checkbox(&mut l.grass_ground_cover, "").changed(),
    );
    edited
}
