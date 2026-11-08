//! Terrain Inspector sections for procedural painting: a layer's Auto Paint Rules, the Generate
//! row, Grass & Details, Cliffs & Blending and Color Harmony. All sections are collapsible and
//! live in the Paint Texture tab. Every change is one undo step (`terrain_tools::begin_edit`);
//! settings-only edits don't make the renderer re-upload the terrain.

use egui::{Grid, RichText, Ui};
use misty_engine::terrain::auto_paint::{self, AutoPaintRules};
use misty_engine::terrain::harmony::HarmonyTarget;
use misty_engine::terrain::{TerrainData, details, store};

use crate::state::EditorState;
use crate::terrain_tools;
use crate::tutorials::targets;

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

/// Apply a settings edit as one undo step.
fn edit_settings(
    state: &mut EditorState,
    path: &str,
    label: &str,
    f: impl FnOnce(&mut TerrainData),
) {
    terrain_tools::begin_edit(state, path, label);
    store::modify_settings(path, f);
}

fn srgb(ui: &mut Ui, c: &mut [f32; 3]) -> bool {
    let mut v = c.map(|x| (x.clamp(0.0, 1.0) * 255.0).round() as u8);
    let ch = ui.color_edit_button_srgb(&mut v).changed();
    if ch {
        *c = v.map(|x| x as f32 / 255.0);
    }
    ch
}

/// Two-handle slider for a `[min, max]` range.
pub fn range_slider(
    ui: &mut Ui,
    v: &mut [f32; 2],
    range: std::ops::RangeInclusive<f32>,
    suffix: &str,
) -> bool {
    let (lo, hi) = (*range.start(), *range.end());
    let mut ch = false;
    ui.horizontal(|ui| {
        let width = 120.0;
        let (rect, response) =
            ui.allocate_exact_size(egui::vec2(width, 18.0), egui::Sense::click_and_drag());
        let to_x = |f: f32| rect.left() + (f - lo) / (hi - lo) * rect.width();
        let to_v = |x: f32| (lo + (x - rect.left()) / rect.width() * (hi - lo)).clamp(lo, hi);
        // Which handle the current drag moves (the nearer one when it started).
        let id = response.id.with("handle");
        if let Some(p) = response.interact_pointer_pos() {
            let handle = if response.drag_started() || response.clicked() {
                let h = usize::from((p.x - to_x(v[0])).abs() > (p.x - to_x(v[1])).abs());
                ui.data_mut(|d| d.insert_temp(id, h));
                h
            } else {
                ui.data(|d| d.get_temp::<usize>(id)).unwrap_or(0)
            };
            let new = to_v(p.x);
            let new = if handle == 0 {
                new.min(v[1])
            } else {
                new.max(v[0])
            };
            if new != v[handle] {
                v[handle] = new;
                ch = true;
            }
        }
        let painter = ui.painter_at(rect.expand(4.0));
        let visuals = ui.visuals();
        let y = rect.center().y;
        painter.line_segment(
            [egui::pos2(rect.left(), y), egui::pos2(rect.right(), y)],
            egui::Stroke::new(4.0, visuals.widgets.inactive.bg_fill),
        );
        painter.line_segment(
            [egui::pos2(to_x(v[0]), y), egui::pos2(to_x(v[1]), y)],
            egui::Stroke::new(4.0, visuals.selection.bg_fill),
        );
        for x in [to_x(v[0]), to_x(v[1])] {
            painter.circle_filled(
                egui::pos2(x, y),
                6.0,
                visuals.widgets.active.fg_stroke.color,
            );
        }
        for x in v.iter_mut() {
            ch |= ui
                .add(
                    egui::DragValue::new(x)
                        .speed(0.25)
                        .range(lo..=hi)
                        // Showing a value past the range (a rule's default 1000 m top on a
                        // lower terrain) must not change it: that saved the asset on open.
                        .clamp_existing_to_range(false)
                        .max_decimals(1)
                        .suffix(suffix),
                )
                .changed();
        }
        if v[0] > v[1] {
            v.swap(0, 1);
        }
    });
    ch
}

/// Slope, height, Spread and Breakup rows shared by layers and details.
fn rule_rows(ui: &mut Ui, r: &mut AutoPaintRules, max_height: f32, slope_label: &str) -> bool {
    let mut ch = false;
    targets::label(ui, slope_label);
    ch |= range_slider(ui, &mut r.slope, 0.0..=90.0, "°");
    ui.end_row();
    ui.label("Height Range");
    ui.horizontal(|ui| {
        ch |= ui.checkbox(&mut r.use_height, "").changed();
        ui.add_enabled_ui(r.use_height, |ui| {
            ch |= range_slider(ui, &mut r.height, 0.0..=max_height.max(1.0), " m");
        });
    });
    ui.end_row();
    ui.label("Spread");
    ch |= ui
        .add(egui::Slider::new(&mut r.spread, 0.0..=1.0))
        .on_hover_text("How far the coverage fades out past the rule edges")
        .changed();
    ui.end_row();
    ui.label("Breakup");
    ui.horizontal(|ui| {
        ch |= ui
            .add(egui::Slider::new(&mut r.breakup, 0.0..=1.0))
            .on_hover_text("Noise that makes the edges natural and irregular")
            .changed();
        ch |= ui
            .add(
                egui::DragValue::new(&mut r.breakup_scale)
                    .speed(0.1)
                    .range(0.5..=500.0)
                    .suffix(" m"),
            )
            .on_hover_text("Size of the breakup noise")
            .changed();
    });
    ui.end_row();
    ch
}

/// The selected layer's Auto Paint Rules (collapsible).
pub fn layer_rules(
    ui: &mut Ui,
    state: &mut EditorState,
    path: &str,
    d: &TerrainData,
    layer: usize,
) {
    let Some(mut rules) = d.layers.get(layer).map(|l| l.rules.clone()) else {
        return;
    };
    let mut ch = false;
    let section = egui::CollapsingHeader::new(RichText::new("Auto Paint Rules").strong())
        .id_salt(("terrain_rules", layer))
        .default_open(rules.enabled)
        .show(ui, |ui| {
            let _scope = targets::scope("Auto Paint Rules");
            ch |= ui
                .checkbox(&mut rules.enabled, "Rule-driven")
                .on_hover_text("Generate repaints this layer from its rules. Off = hand-painted only (Generate keeps its strokes).")
                .changed();
            ui.add_enabled_ui(rules.enabled, |ui| {
                ch |= grid(ui, "terrain_rule_grid", |ui| {
                    let mut ch = rule_rows(ui, &mut rules, d.size.y, "Slope");
                    ui.label("Priority");
                    ch |= ui
                        .add(egui::DragValue::new(&mut rules.priority).range(-10..=10))
                        .on_hover_text("Higher priorities paint over lower ones; the lowest fills the rest")
                        .changed();
                    ui.end_row();
                    ch
                });
            });
        });
    targets::row(ui, "Auto Paint Rules", section.header_response.rect);
    if ch {
        edit_settings(state, path, "Change Auto Paint Rules", |d| {
            d.layers[layer].rules = rules
        });
    }
}

/// Generate button (whole terrain) and the rules seed.
pub fn generate_row(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let rule_layers = d.layers.iter().filter(|l| l.rules.enabled).count();
    ui.horizontal(|ui| {
        let can = rule_layers > 0 || d.details.enabled;
        let b = ui
            .add_enabled(can, egui::Button::new("⟳ Generate"))
            .on_hover_text("Paint rule-driven layers and scatter grass over the whole terrain")
            .on_disabled_hover_text(
                "Make a layer rule-driven (Auto Paint Rules) or enable Grass & Details",
            );
        targets::header(ui, "Generate", b.rect);
        if b.clicked() {
            generate(state, path);
        }
        ui.label("Seed");
        let mut seed = d.rule_seed;
        if ui.add(egui::DragValue::new(&mut seed)).changed() {
            terrain_tools::begin_edit(state, path, "Generate Terrain");
            store::modify(path, |d| {
                d.rule_seed = seed;
                auto_paint::generate(d);
                details::generate(d);
            });
        }
    });
    ui.checkbox(
        &mut state.terrain_tools.generate_in_brush,
        "Generate in brush",
    )
    .on_hover_text("The brush paints the rule result (layers and grass) instead of one layer");
}

/// Regenerate splat and detail maps (one undo step).
pub fn generate(state: &mut EditorState, path: &str) {
    terrain_tools::begin_edit(state, path, "Generate Terrain");
    let started = std::time::Instant::now();
    store::modify(path, |d| {
        auto_paint::generate(d);
        details::generate(d);
    });
    state.console.info(format!(
        "Generated terrain in {:.0} ms.",
        started.elapsed().as_secs_f32() * 1000.0
    ));
}

/// Grass & Details: placement rules, density and look.
pub fn details_section(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let mut s = d.details.clone();
    let mut ch = false;
    let mut regenerate = false;
    egui::CollapsingHeader::new(RichText::new("Grass & Details").strong())
        .id_salt("terrain_details")
        .show(ui, |ui| {
            if ui.checkbox(&mut s.enabled, "Enabled").changed() {
                regenerate = true;
            }
            ui.add_enabled_ui(s.enabled, |ui| {
                ch |= grid(ui, "terrain_details_grid", |ui| {
                    let mut ch = rule_rows(ui, &mut s.rules, d.size.y, "Slope (grows on)");
                    ui.label("Grow On");
                    let name = |l: Option<usize>| match l.and_then(|i| d.layers.get(i)) {
                        Some(layer) => layer.name.clone(),
                        None => "Any layer".into(),
                    };
                    egui::ComboBox::from_id_salt("terrain_details_layer")
                        .selected_text(name(s.on_layer))
                        .show_ui(ui, |ui| {
                            ch |= ui
                                .selectable_value(&mut s.on_layer, None, "Any layer")
                                .changed();
                            for i in 0..d.layers.len() {
                                ch |= ui
                                    .selectable_value(&mut s.on_layer, Some(i), name(Some(i)))
                                    .changed();
                            }
                        });
                    ui.end_row();
                    ui.label("Density");
                    ch |= ui
                        .add(
                            egui::Slider::new(&mut s.density, 0.0..=details::MAX_DENSITY)
                                .logarithmic(true)
                                .suffix(" /m²"),
                        )
                        .changed();
                    ui.end_row();
                    ui.label("Draw Distance");
                    ch |= ui
                        .add(egui::Slider::new(&mut s.draw_distance, 10.0..=400.0).suffix(" m"))
                        .changed();
                    ui.end_row();
                    ui.label("Blade Height");
                    ch |= range_slider(ui, &mut s.height, 0.05..=2.5, " m");
                    ui.end_row();
                    ui.label("Blade Width");
                    ch |= ui
                        .add(egui::Slider::new(&mut s.width, 0.01..=0.3).suffix(" m"))
                        .changed();
                    ui.end_row();
                    ui.label("Healthy / Dry");
                    ui.horizontal(|ui| {
                        ch |= srgb(ui, &mut s.healthy_color);
                        ch |= srgb(ui, &mut s.dry_color);
                    });
                    ui.end_row();
                    ui.label("Wind");
                    ui.horizontal(|ui| {
                        ch |= ui
                            .add(egui::Slider::new(&mut s.wind_strength, 0.0..=1.5))
                            .changed();
                        ch |= ui
                            .add(
                                egui::DragValue::new(&mut s.wind_speed)
                                    .speed(0.02)
                                    .range(0.0..=6.0),
                            )
                            .on_hover_text("Speed")
                            .changed();
                    });
                    ui.end_row();
                    let slider = |ui: &mut Ui, label: &str, tip: &str, v: &mut f32, range| {
                        ui.label(label).on_hover_text(tip);
                        let c = ui
                            .add(egui::Slider::new(v, range))
                            .on_hover_text(tip)
                            .changed();
                        ui.end_row();
                        c
                    };
                    ch |= slider(
                        ui,
                        "Wind Direction",
                        "Compass direction the wind blows towards (a Wind Zone in the scene overrides it).",
                        &mut s.wind_direction,
                        0.0..=360.0,
                    );
                    ch |= slider(
                        ui,
                        "Gusts",
                        "Strength of the gust waves rolling across the field.",
                        &mut s.gust_strength,
                        0.0..=1.5,
                    );
                    ch |= slider(
                        ui,
                        "Gust Size",
                        "Metres between gust waves.",
                        &mut s.gust_scale,
                        4.0..=120.0,
                    );
                    ch |= slider(
                        ui,
                        "Flutter",
                        "Fast flutter of single blades.",
                        &mut s.turbulence,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Patch Size",
                        "Size of the soft lush / dry and taller / shorter patches.",
                        &mut s.patch_scale,
                        2.0..=80.0,
                    );
                    ui.label("Lush Colour");
                    ch |= srgb(ui, &mut s.lush_color);
                    ui.end_row();
                    ch |= slider(
                        ui,
                        "Lush Patches",
                        "How strongly lush patches darken and green the grass.",
                        &mut s.lush_amount,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Dry Patches",
                        "How strongly dry patches turn the grass towards the dry colour.",
                        &mut s.dry_amount,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Colour Jitter",
                        "Hue and brightness differences between tufts and blades.",
                        &mut s.color_variation,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Height Variation",
                        "Taller, fuller grass in lush patches, shorter elsewhere.",
                        &mut s.height_variation,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Density Variation",
                        "How much the blade count follows the patches.",
                        &mut s.density_variation,
                        0.0..=1.0,
                    );
                    ch |= slider(
                        ui,
                        "Ground Tint",
                        "The ground under grass takes the grass's shaded colour, so gaps read as deep grass.",
                        &mut s.ground_tint,
                        0.0..=1.0,
                    );
                    ui.label("Cast Shadows").on_hover_text(
                        "Blades near the camera cast shadows (Quality ▸ Grass Shadows sets how far).",
                    );
                    ch |= ui.checkbox(&mut s.cast_shadows, "").changed();
                    ui.end_row();
                    ch
                });
                ui.weak("Rules apply on Generate; grass never grows past the slope limit.");
            });
        });
    if regenerate {
        terrain_tools::begin_edit(state, path, "Grass & Details");
        store::modify(path, |d| {
            d.details = s;
            details::generate(d);
        });
    } else if ch {
        edit_settings(state, path, "Grass & Details", |d| d.details = s);
    }
}

/// Cliffs & Blending: triplanar projection and height blending.
pub fn cliffs_section(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let mut c = d.cliffs.clone();
    let mut ch = false;
    egui::CollapsingHeader::new(RichText::new("Cliffs & Blending").strong())
        .id_salt("terrain_cliffs")
        .show(ui, |ui| {
            ch |= grid(ui, "terrain_cliffs_grid", |ui| {
                let mut ch = false;
                ui.label("Triplanar Cliffs");
                ch |= ui
                    .checkbox(&mut c.triplanar, "")
                    .on_hover_text("Project textures from the side on steep slopes (no stretching)")
                    .changed();
                ui.end_row();
                ui.add_enabled_ui(c.triplanar, |ui| ui.label("Above Angle"));
                ch |= ui
                    .add_enabled(c.triplanar, egui::Slider::new(&mut c.triplanar_angle, 10.0..=80.0).suffix("°"))
                    .changed();
                ui.end_row();
                ui.add_enabled_ui(c.triplanar, |ui| ui.label("Blend Sharpness"));
                ch |= ui
                    .add_enabled(c.triplanar, egui::Slider::new(&mut c.sharpness, 1.0..=16.0))
                    .changed();
                ui.end_row();
                ui.label("Height Blend");
                ch |= ui
                    .add(egui::Slider::new(&mut c.height_blend, 0.0..=1.0))
                    .on_hover_text("Contrast of height-based blending: rock shows through grass cracks. Uses each layer's Height Map, or its albedo brightness.")
                    .changed();
                ui.end_row();
                ch
            });
        });
    if ch {
        edit_settings(state, path, "Cliffs & Blending", |d| d.cliffs = c);
    }
}

/// Color Harmony: pull the layers' colours towards one tone (non-destructive).
pub fn harmony_section(ui: &mut Ui, state: &mut EditorState, path: &str, d: &TerrainData) {
    let mut h = d.harmony.clone();
    let mut ch = false;
    egui::CollapsingHeader::new(RichText::new("Color Harmony").strong())
        .id_salt("terrain_harmony")
        .show(ui, |ui| {
            ch |= ui
                .checkbox(&mut h.enabled, "Enabled")
                .on_hover_text("Shifts each layer's colour towards a shared tone in the shader; textures are untouched")
                .changed();
            ui.add_enabled_ui(h.enabled, |ui| {
                ch |= grid(ui, "terrain_harmony_grid", |ui| {
                    let mut ch = false;
                    ui.label("Target");
                    ui.horizontal(|ui| {
                        for (t, label) in [
                            (HarmonyTarget::Average, "Average"),
                            (HarmonyTarget::Tint, "Tint"),
                            (HarmonyTarget::Lighting, "Lighting"),
                        ] {
                            ch |= ui.selectable_value(&mut h.target, t, label).changed();
                        }
                        if h.target == HarmonyTarget::Tint {
                            ch |= srgb(ui, &mut h.tint);
                        }
                    });
                    ui.end_row();
                    ui.label("Strength");
                    ch |= ui.add(egui::Slider::new(&mut h.strength, 0.0..=1.0)).changed();
                    ui.end_row();
                    for (label, v) in [
                        ("Hue", &mut h.hue),
                        ("Saturation", &mut h.saturation),
                        ("Brightness", &mut h.brightness),
                    ] {
                        ui.label(label);
                        ch |= ui.add(egui::Slider::new(v, 0.0..=1.0)).changed();
                        ui.end_row();
                    }
                    ui.label("Weight by Coverage");
                    ch |= ui.checkbox(&mut h.weight_by_coverage, "").changed();
                    ui.end_row();
                    ch
                });
                ui.horizontal(|ui| {
                    let label = if h.preview_original { "Showing: Before" } else { "Showing: After" };
                    if ui
                        .toggle_value(&mut h.preview_original, label)
                        .on_hover_text("Preview the original colours (not saved)")
                        .changed()
                    {
                        // Preview only: no undo step.
                        let on = h.preview_original;
                        store::modify_settings(path, |d| d.harmony.preview_original = on);
                    }
                });
            });
        });
    if ch {
        edit_settings(state, path, "Color Harmony", |d| d.harmony = h);
    }
}
