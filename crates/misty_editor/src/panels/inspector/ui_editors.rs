//! Inspector editors for the UI components: Rect Transform (with anchor presets), Canvas,
//! Canvas Scaler, Image, Text and Button.

use egui::{Grid, RichText, Ui};
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    AspectRatioFitter, ButtonCall, Canvas, CanvasGroup, CanvasRenderMode, CanvasScaler,
    RectTransform, SceneComponent, TextAnchor, Transform, UiButton, UiImage, UiText,
};
use misty_engine::ui::layout::{AnchorAxis, apply_anchor_preset};

use crate::state::EditorState;
use crate::theme;

#[path = "ui_widget_editors.rs"]
mod widgets;

#[path = "ui_canvas_editors.rs"]
mod canvas_eds;

/// Names of the UI components handled here (widgets and layout in `ui_widget_editors.rs`).
pub const NAMES: &[&str] = &[
    "Rect Transform",
    "Canvas",
    "Canvas Scaler",
    "Image",
    "Text",
    "Button",
    "Raw Image",
    "Toggle",
    "Toggle Group",
    "Slider",
    "Scrollbar",
    "Input Field",
    "Dropdown",
    "Scroll Rect",
    "Mask",
    "Rect Mask 2D",
    "Horizontal Layout Group",
    "Vertical Layout Group",
    "Grid Layout Group",
    "Content Size Fitter",
    "Layout Element",
    "Canvas Group",
    "Aspect Ratio Fitter",
    "Event System",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rect Transform" => "⬚",
        "Canvas" => "▭",
        "Canvas Scaler" => "⤢",
        "Image" => "🖼",
        "Text" => "T",
        "Button" => "▣",
        "Canvas Group" => "◫",
        "Aspect Ratio Fitter" => "⊡",
        other => return widgets::icon(other),
    })
}

/// Header checkbox value of a UI component (`None` = not a UI component with one).
pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Canvas" => w.get::<&Canvas>(e).ok().map(|c| c.enabled),
        "Canvas Scaler" => w.get::<&CanvasScaler>(e).ok().map(|c| c.enabled),
        "Image" => w.get::<&UiImage>(e).ok().map(|c| c.enabled),
        "Text" => w.get::<&UiText>(e).ok().map(|c| c.enabled),
        "Canvas Group" => w.get::<&CanvasGroup>(e).ok().map(|c| c.enabled),
        "Aspect Ratio Fitter" => w.get::<&AspectRatioFitter>(e).ok().map(|c| c.enabled),
        other => widgets::enabled_flag(state, e, other),
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Canvas" => w.get::<&mut Canvas>(e).map(|mut c| c.enabled = on),
        "Canvas Scaler" => w.get::<&mut CanvasScaler>(e).map(|mut c| c.enabled = on),
        "Image" => w.get::<&mut UiImage>(e).map(|mut c| c.enabled = on),
        "Text" => w.get::<&mut UiText>(e).map(|mut c| c.enabled = on),
        "Canvas Group" => w.get::<&mut CanvasGroup>(e).map(|mut c| c.enabled = on),
        "Aspect Ratio Fitter" => w
            .get::<&mut AspectRatioFitter>(e)
            .map(|mut c| c.enabled = on),
        other => return widgets::set_enabled(state, e, other, on),
    };
}

/// Draw the editor for UI component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Rect Transform" => rect_transform(ui, state, e),
        "Canvas" => canvas(ui, state, e),
        "Canvas Scaler" => {
            let world = state
                .scene
                .world
                .get::<&Canvas>(e)
                .is_ok_and(|c| c.render_mode == CanvasRenderMode::WorldSpace);
            edit_copy::<CanvasScaler>(state, e, |c| canvas_eds::canvas_scaler(ui, c, world))
        }
        "Image" => {
            let textures = state.assets.textures.clone();
            edit_copy::<UiImage>(state, e, |c| image(ui, c, &textures))
        }
        "Canvas Group" => edit_copy::<CanvasGroup>(state, e, |c| canvas_eds::canvas_group(ui, c)),
        "Aspect Ratio Fitter" => {
            edit_copy::<AspectRatioFitter>(state, e, |c| canvas_eds::aspect_ratio_fitter(ui, c))
        }
        "Text" => {
            let textures = state.assets.textures.clone();
            let fonts = state.assets.fonts.clone();
            edit_copy::<UiText>(state, e, |c| text(ui, c, &textures, &fonts))
        }
        "Button" => {
            let objects: canvas_eds::Objects = state
                .scene
                .walk()
                .into_iter()
                .map(|(o, _)| (o, state.scene.name(o)))
                .collect();
            edit_copy::<UiButton>(state, e, |c| button(ui, c, &objects))
        }
        other => widgets::edit(ui, state, e, other),
    }
}

/// Edit a copy of `T` with `f` and store it if anything changed.
pub(super) fn edit_copy<T: SceneComponent>(
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

pub(super) fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    crate::panels::inspector::card::PropertyGrid::new(id)
        .show(ui, body)
        .inner
}

fn drag(ui: &mut Ui, v: &mut f32, speed: f64) -> bool {
    ui.add(egui::DragValue::new(v).speed(speed).max_decimals(3))
        .changed()
}

pub(super) fn vec2(ui: &mut Ui, v: &mut Vec2, speed: f64) -> bool {
    ui.horizontal(|ui| {
        let mut ch = false;
        ui.label("X");
        ch |= drag(ui, &mut v.x, speed);
        ui.label("Y");
        ch |= drag(ui, &mut v.y, speed);
        ch
    })
    .inner
}

/// sRGB + alpha colour picker.
pub(super) fn color(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut rgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = crate::panels::inspector::card::wide(ui, |ui| {
        ui.color_edit_button_srgba_unmultiplied(&mut rgba)
    })
    .changed();
    if changed {
        *c = rgba.map(|v| v as f32 / 255.0);
    }
    changed
}

// ---- Rect Transform --------------------------------------------------------------------------

/// Parent rect size (canvas units) of `e` at the Game view's current size.
fn parent_size(state: &EditorState, e: Entity) -> Vec2 {
    let layout = misty_engine::ui::layout(&state.scene, state.game_view.screen_px);
    state
        .scene
        .parent(e)
        .and_then(|p| layout.node(p))
        .map_or(Vec2::new(800.0, 600.0), |n| n.rect.size())
}

fn rect_transform(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    // A root screen-space canvas' rect is the screen: the Canvas drives it (a World Space
    // canvas sizes itself with its Rect Transform).
    let driven = state
        .scene
        .world
        .get::<&Canvas>(e)
        .is_ok_and(|c| c.render_mode != CanvasRenderMode::WorldSpace)
        && state
            .scene
            .parent(e)
            .is_none_or(|p| misty_engine::ui::canvas_of(&state.scene, p).is_none());
    if driven {
        ui.label(RichText::new("Some values driven by Canvas.").color(theme::TEXT_DIM));
        return false;
    }
    if let Some(by) = widgets::driven_by(state, e) {
        ui.label(RichText::new(format!("Some values driven by {by}.")).color(theme::TEXT_DIM));
    }
    let parent = parent_size(state, e);
    let id = ui.make_persistent_id(("anchor_presets", e));
    edit_copy::<RectTransform>(state, e, |rt| {
        let mut ch = false;
        ui.horizontal(|ui| {
            ch |= presets_button(ui, id, rt, parent);
            ui.vertical(|ui| ch |= position_fields(ui, rt));
        });
        egui::CollapsingHeader::new(crate::theme::bold(ui.ctx(), "Anchors"))
            .id_salt(("rt_anchors", e))
            .default_open(true)
            .show(ui, |ui| {
                ch |= grid(ui, "rt_anchor_grid", |ui| {
                    let mut c = false;
                    ui.label("Min");
                    c |= vec2(ui, &mut rt.anchor_min, 0.01);
                    ui.end_row();
                    ui.label("Max");
                    c |= vec2(ui, &mut rt.anchor_max, 0.01);
                    ui.end_row();
                    c
                });
            });
        ch |= grid(ui, "rt_pivot_grid", |ui| {
            ui.label("Pivot");
            let c = vec2(ui, &mut rt.pivot, 0.01);
            ui.end_row();
            c
        });
        ch
    })
}

/// Pos/Width per axis when the anchors meet, Left/Right (Top/Bottom) when they are apart.
fn position_fields(ui: &mut Ui, rt: &mut RectTransform) -> bool {
    let stretch_x = (rt.anchor_min.x - rt.anchor_max.x).abs() > 1e-4;
    let stretch_y = (rt.anchor_min.y - rt.anchor_max.y).abs() > 1e-4;
    let (mut omin, mut omax) = (rt.offset_min(), rt.offset_max());
    let mut offsets_changed = false;
    let mut ch = false;
    Grid::new("rt_pos_grid")
        .num_columns(2)
        .spacing([8.0, 2.0])
        .show(ui, |ui| {
            let label =
                |ui: &mut Ui, s: &str| ui.label(RichText::new(s).small().color(theme::TEXT_DIM));
            label(ui, if stretch_x { "Left" } else { "Pos X" });
            label(ui, if stretch_y { "Top" } else { "Pos Y" });
            ui.end_row();
            if stretch_x {
                offsets_changed |= drag(ui, &mut omin.x, 1.0);
            } else {
                ch |= drag(ui, &mut rt.anchored_position.x, 1.0);
            }
            if stretch_y {
                let mut top = -omax.y;
                if drag(ui, &mut top, 1.0) {
                    omax.y = -top;
                    offsets_changed = true;
                }
            } else {
                ch |= drag(ui, &mut rt.anchored_position.y, 1.0);
            }
            ui.end_row();
            label(ui, if stretch_x { "Right" } else { "Width" });
            label(ui, if stretch_y { "Bottom" } else { "Height" });
            ui.end_row();
            if stretch_x {
                let mut right = -omax.x;
                if drag(ui, &mut right, 1.0) {
                    omax.x = -right;
                    offsets_changed = true;
                }
            } else {
                ch |= drag(ui, &mut rt.size_delta.x, 1.0);
            }
            if stretch_y {
                offsets_changed |= drag(ui, &mut omin.y, 1.0);
            } else {
                ch |= drag(ui, &mut rt.size_delta.y, 1.0);
            }
            ui.end_row();
        });
    if offsets_changed {
        // Keep the non-stretched axis exactly as it was.
        let (pos, size) = (rt.anchored_position, rt.size_delta);
        rt.set_offsets(omin, omax);
        if !stretch_x {
            rt.anchored_position.x = pos.x;
            rt.size_delta.x = size.x;
        }
        if !stretch_y {
            rt.anchored_position.y = pos.y;
            rt.size_delta.y = size.y;
        }
    }
    ch || offsets_changed
}

fn axis_name(a: Option<AnchorAxis>, vertical: bool) -> &'static str {
    match (a, vertical) {
        (Some(AnchorAxis::Min), false) => "left",
        (Some(AnchorAxis::Center), false) => "center",
        (Some(AnchorAxis::Max), false) => "right",
        (Some(AnchorAxis::Min), true) => "bottom",
        (Some(AnchorAxis::Center), true) => "middle",
        (Some(AnchorAxis::Max), true) => "top",
        (Some(AnchorAxis::Stretch), _) => "stretch",
        (None, _) => "custom",
    }
}

/// The anchor presets button and its 4×4 popup (with "also set pivot / position" toggles).
fn presets_button(ui: &mut Ui, id: egui::Id, rt: &mut RectTransform, parent: Vec2) -> bool {
    let h = AnchorAxis::of(rt.anchor_min.x, rt.anchor_max.x);
    let v = AnchorAxis::of(rt.anchor_min.y, rt.anchor_max.y);
    let label = format!("{}\n{}", axis_name(h, false), axis_name(v, true));
    let mut changed = false;
    ui.menu_button(RichText::new(label).small(), |ui| {
        let (mut set_pivot, mut set_position) =
            ui.data(|d| d.get_temp::<(bool, bool)>(id).unwrap_or_default());
        ui.label(crate::theme::bold(ui.ctx(), "Anchor Presets"));
        ui.checkbox(&mut set_pivot, "Also set pivot (Shift)");
        ui.checkbox(&mut set_position, "Also set position (Alt)");
        ui.data_mut(|d| d.insert_temp(id, (set_pivot, set_position)));
        // Held modifiers work like the toggles.
        let m = ui.input(|i| i.modifiers);
        let (pivot, position) = (set_pivot || m.shift, set_position || m.alt);
        // Rows: top, middle, bottom, stretch; columns: left, center, right, stretch.
        let rows = [
            AnchorAxis::Max,
            AnchorAxis::Center,
            AnchorAxis::Min,
            AnchorAxis::Stretch,
        ];
        let cols = [
            AnchorAxis::Min,
            AnchorAxis::Center,
            AnchorAxis::Max,
            AnchorAxis::Stretch,
        ];
        Grid::new("anchor_preset_grid").show(ui, |ui| {
            ui.label("");
            for c in cols {
                ui.label(RichText::new(axis_name(Some(c), false)).small());
            }
            ui.end_row();
            for r in rows {
                ui.label(RichText::new(axis_name(Some(r), true)).small());
                for c in cols {
                    let current = h == Some(c) && v == Some(r);
                    let text = if current { "▣" } else { "□" };
                    let tip = format!("{} {}", axis_name(Some(c), false), axis_name(Some(r), true));
                    if ui.button(text).on_hover_text(tip).clicked() {
                        apply_anchor_preset(rt, parent, c, r, pivot, position);
                        changed = true;
                        ui.close();
                    }
                }
                ui.end_row();
            }
        });
    });
    changed
}

// ---- Canvas ----------------------------------------------------------------------------------

/// The Canvas: switching to World Space gives the canvas the size it had on screen and a
/// Transform scale of 0.01 (a canvas unit becomes a centimetre), so it doesn't fill the world.
fn canvas(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    let cameras: canvas_eds::Objects = state
        .scene
        .walk()
        .into_iter()
        .filter(|(o, _)| {
            state
                .scene
                .world
                .get::<&misty_engine::scene::Camera>(*o)
                .is_ok()
        })
        .map(|(o, _)| (o, state.scene.name(o)))
        .collect();
    let before = state
        .scene
        .world
        .get::<&Canvas>(e)
        .map(|c| c.render_mode)
        .ok();
    let changed = edit_copy::<Canvas>(state, e, |c| canvas_eds::canvas(ui, c, &cameras));
    let after = state
        .scene
        .world
        .get::<&Canvas>(e)
        .map(|c| c.render_mode)
        .ok();
    if changed
        && after == Some(CanvasRenderMode::WorldSpace)
        && before != after
        && let Some(n) = misty_engine::ui::layout(&state.scene, state.game_view.screen_px)
            .node(e)
            .copied()
    {
        if let Ok(mut rt) = state.scene.world.get::<&mut RectTransform>(e) {
            *rt = RectTransform::sized(n.rect.size().x, n.rect.size().y);
        }
        let t = state.scene.transform(e);
        if t.scale == glam::Vec3::ONE {
            state.scene.set_transform(
                e,
                Transform {
                    scale: glam::Vec3::splat(0.01),
                    ..t
                },
            );
        }
    }
    changed
}

// ---- Image / Text / Button ---------------------------------------------------------------------

fn image(ui: &mut Ui, img: &mut UiImage, textures: &[String]) -> bool {
    let has_border = img
        .sprite
        .as_deref()
        .and_then(misty_engine::sprites::resolve)
        .or_else(|| {
            img.texture
                .as_deref()
                .and_then(misty_engine::sprites::resolve)
        })
        .is_some_and(|s| !misty_engine::sprites::import::is_no_border(&s.border));
    grid(ui, "ui_image_grid", |ui| {
        let mut ch = false;
        ui.label("Source Image");
        ch |= super::slots::texture_slot(ui, "ui_image_tex", &mut img.texture, textures);
        ui.end_row();
        // A sprite replaces the texture and draws only its rect.
        ui.label("Sprite");
        let sprites = super::sprite_editors::sprite_choices_from(textures);
        ch |= super::sprite_editors::sprite_slot(ui, "ui_image_sprite", &mut img.sprite, &sprites);
        ui.end_row();
        ui.label("Color");
        ch |= color(ui, &mut img.color);
        ui.end_row();
        ui.label("Raycast Target");
        ch |= ui.checkbox(&mut img.raycast_target, "").changed();
        ui.end_row();
        ch |= canvas_eds::image_type_rows(ui, img, has_border);
        ch
    })
}

fn text(ui: &mut Ui, t: &mut UiText, textures: &[String], fonts: &[String]) -> bool {
    let mut ch = ui
        .add(
            egui::TextEdit::multiline(&mut t.text)
                .desired_rows(3)
                .desired_width(f32::INFINITY),
        )
        .changed();
    ch |= grid(ui, "ui_text_grid", |ui| {
        let mut c = false;
        ui.label("Font");
        c |= font_slot(ui, &mut t.font, fonts);
        ui.end_row();
        ui.label("Rich Text")
            .on_hover_text("<b> <i> <u> <s> <color> <size> <alpha> <mark> <sup> <sub> <br> <sprite> <link> <align> <noparse>");
        c |= ui.checkbox(&mut t.rich_text, "").changed();
        ui.end_row();
        ui.label("Font Size");
        c |= ui
            .add(
                egui::DragValue::new(&mut t.font_size)
                    .speed(0.5)
                    .range(1.0..=300.0),
            )
            .changed();
        ui.end_row();
        ui.label("Line Spacing");
        c |= ui
            .add(
                egui::DragValue::new(&mut t.line_spacing)
                    .speed(0.01)
                    .range(0.1..=10.0),
            )
            .changed();
        ui.end_row();
        ui.label("Alignment");
        c |= alignment(ui, &mut t.alignment);
        ui.end_row();
        ui.label(crate::i18n::tr!("Mirror for RTL"))
            .on_hover_text(crate::i18n::tr!(
                "Text that starts in a right-to-left script (Arabic, Hebrew) swaps Left and Right \
             alignment, so one Text suits every language of a Localized Text"
            ));
        c |= ui.checkbox(&mut t.mirror_rtl, "").changed();
        ui.end_row();
        ui.label("Horizontal Overflow");
        egui::ComboBox::from_id_salt("ui_text_wrap")
            .selected_text(if t.wrap { "Wrap" } else { "Overflow" })
            .show_ui(ui, |ui| {
                c |= ui.selectable_value(&mut t.wrap, true, "Wrap").changed();
                c |= ui
                    .selectable_value(&mut t.wrap, false, "Overflow")
                    .changed();
            });
        ui.end_row();
        ui.label("Color");
        c |= color(ui, &mut t.color);
        ui.end_row();
        ui.label("Raycast Target");
        c |= ui.checkbox(&mut t.raycast_target, "").changed();
        ui.end_row();
        c
    });
    ch |= rich_text_settings(ui, t, textures);
    ch
}

/// Font asset picker (None = the built-in font).
fn font_slot(ui: &mut Ui, font: &mut Option<String>, fonts: &[String]) -> bool {
    let mut ch = false;
    let builtin = "Ubuntu Light (built-in)";
    let name = font
        .as_deref()
        .map(misty_engine::scene::clip_name)
        .unwrap_or_else(|| builtin.into());
    let r = egui::ComboBox::from_id_salt("ui_text_font")
        .selected_text(name)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(font, None, builtin).changed();
            for f in fonts {
                ch |= ui
                    .selectable_value(font, Some(f.clone()), misty_engine::scene::clip_name(f))
                    .changed();
            }
        })
        .response;
    if let Some(p) = r.dnd_release_payload::<crate::panels::AssetPayload>()
        && misty_engine::ui::rich::fonts::is_font(&p.0)
    {
        *font = Some(p.0.clone());
        ch = true;
    }
    ch
}

/// Auto size, spacing, overflow, inline sprites and the outline / shadow / gradient effects.
fn rich_text_settings(ui: &mut Ui, t: &mut UiText, textures: &[String]) -> bool {
    use misty_engine::scene::TextOverflow;
    grid(ui, "ui_text_rich_grid", |ui| {
        let mut c = false;
        crate::panels::inspector::card::grid_sub_header(ui, "Layout");
        ui.label("Auto Size");
        c |= ui.checkbox(&mut t.auto_size, "").changed();
        ui.end_row();
        if t.auto_size {
            ui.label("Min / Max");
            ui.horizontal(|ui| {
                c |= ui
                    .add(
                        egui::DragValue::new(&mut t.font_size_min)
                            .speed(0.5)
                            .range(1.0..=300.0),
                    )
                    .changed();
                c |= ui
                    .add(
                        egui::DragValue::new(&mut t.font_size_max)
                            .speed(0.5)
                            .range(1.0..=300.0),
                    )
                    .changed();
            });
            ui.end_row();
        }
        ui.label("Character Spacing");
        c |= ui
            .add(
                egui::DragValue::new(&mut t.character_spacing)
                    .speed(0.5)
                    .range(-50.0..=200.0),
            )
            .changed();
        ui.end_row();
        ui.label("Word Spacing");
        c |= ui
            .add(
                egui::DragValue::new(&mut t.word_spacing)
                    .speed(0.5)
                    .range(-50.0..=200.0),
            )
            .changed();
        ui.end_row();
        ui.label("Vertical Overflow");
        egui::ComboBox::from_id_salt("ui_text_overflow")
            .selected_text(t.overflow.label())
            .show_ui(ui, |ui| {
                for o in TextOverflow::ALL {
                    c |= ui.selectable_value(&mut t.overflow, o, o.label()).changed();
                }
            });
        ui.end_row();
        if t.overflow == TextOverflow::Page {
            ui.label("Page");
            c |= ui
                .add(egui::DragValue::new(&mut t.page).range(1..=999))
                .changed();
            ui.end_row();
        }
        ui.label("Sprite Asset");
        let name = t
            .sprite_asset
            .as_deref()
            .map(misty_engine::scene::clip_name)
            .unwrap_or_else(|| "None".into());
        egui::ComboBox::from_id_salt("ui_text_sprite_asset")
            .selected_text(name)
            .show_ui(ui, |ui| {
                c |= ui
                    .selectable_value(&mut t.sprite_asset, None, "None")
                    .changed();
                for tex in textures {
                    c |= ui
                        .selectable_value(
                            &mut t.sprite_asset,
                            Some(tex.clone()),
                            misty_engine::scene::clip_name(tex),
                        )
                        .changed();
                }
            });
        ui.end_row();
        crate::panels::inspector::card::grid_sub_header(ui, "Effects");
        let fx = &mut t.effects;
        ui.label("Outline Width");
        c |= ui
            .add(
                egui::DragValue::new(&mut fx.outline_width)
                    .speed(0.05)
                    .range(0.0..=8.0),
            )
            .changed();
        ui.end_row();
        if fx.outline_width > 0.0 {
            ui.label("Outline Color");
            c |= color(ui, &mut fx.outline_color);
            ui.end_row();
        }
        ui.label("Shadow");
        c |= ui.checkbox(&mut fx.shadow, "").changed();
        ui.end_row();
        if fx.shadow {
            ui.label("Shadow Offset");
            ui.horizontal(|ui| {
                for v in &mut fx.shadow_offset {
                    c |= ui.add(egui::DragValue::new(v).speed(0.1)).changed();
                }
            });
            ui.end_row();
            ui.label("Shadow Color");
            c |= color(ui, &mut fx.shadow_color);
            ui.end_row();
            ui.label("Shadow Softness");
            c |= ui
                .add(egui::Slider::new(&mut fx.shadow_softness, 0.0..=1.0))
                .changed();
            ui.end_row();
        }
        ui.label("Gradient");
        c |= ui.checkbox(&mut fx.gradient, "").changed();
        ui.end_row();
        if fx.gradient {
            for (label, k) in [
                ("Top Left", 0),
                ("Top Right", 1),
                ("Bottom Left", 2),
                ("Bottom Right", 3),
            ] {
                ui.label(label);
                c |= color(ui, &mut fx.gradient_colors[k]);
                ui.end_row();
            }
        }
        c
    })
}

/// Horizontal (left/center/right) and vertical (upper/middle/lower) toggle rows.
pub(super) fn alignment(ui: &mut Ui, a: &mut TextAnchor) -> bool {
    let (col, row) = a.cell();
    let mut ch = false;
    ui.horizontal(|ui| {
        for (i, s) in ["⏴", "≡", "⏵"].into_iter().enumerate() {
            if ui.selectable_label(col == i, s).clicked() && col != i {
                *a = TextAnchor::from_cell(i, row);
                ch = true;
            }
        }
        ui.separator();
        for (i, s) in ["⏶", "⏺", "⏷"].into_iter().enumerate() {
            if ui.selectable_label(row == i, s).clicked() && row != i {
                *a = TextAnchor::from_cell(a.cell().0, i);
                ch = true;
            }
        }
    });
    ch
}

fn button(ui: &mut Ui, b: &mut UiButton, objects: &[(Entity, String)]) -> bool {
    let mut ch = grid(ui, "ui_button_grid", |ui| {
        let mut c = false;
        ui.label("Interactable");
        c |= ui.checkbox(&mut b.interactable, "").changed();
        ui.end_row();
        ui.label("Transition");
        ui.label("Color Tint");
        ui.end_row();
        ui.label("Target Graphic");
        ui.label(RichText::new("Image (this object)").color(theme::TEXT_DIM));
        ui.end_row();
        for (label, col) in [
            ("Normal Color", &mut b.normal_color),
            ("Highlighted Color", &mut b.highlighted_color),
            ("Pressed Color", &mut b.pressed_color),
            ("Disabled Color", &mut b.disabled_color),
        ] {
            ui.label(label);
            c |= color(ui, col);
            ui.end_row();
        }
        ui.label("Color Multiplier");
        c |= ui
            .add(egui::Slider::new(&mut b.color_multiplier, 1.0..=5.0))
            .changed();
        ui.end_row();
        c |= canvas_eds::navigation_rows(ui, "ui_button", &mut b.navigation, &objects.to_vec());
        c
    });
    ui.add_space(4.0);
    ui.label("On Click ()");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        if b.on_click.is_empty() {
            ui.label(RichText::new("List is Empty").color(theme::TEXT_DIM));
        }
        let mut remove = None;
        for (i, call) in b.on_click.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ch |= target_field(ui, i, call, objects);
                ch |= ui
                    .add(
                        egui::TextEdit::singleline(&mut call.method)
                            .hint_text("Method")
                            .desired_width(100.0),
                    )
                    .changed();
                if ui.small_button("−").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            b.on_click.remove(i);
            ch = true;
        }
        if ui.small_button("+").on_hover_text("Add a call").clicked() {
            b.on_click.push(ButtonCall::default());
            ch = true;
        }
    });
    ch
}

/// Scene object picker for an On Click target.
pub(super) fn target_field(
    ui: &mut Ui,
    i: usize,
    call: &mut ButtonCall,
    objects: &[(Entity, String)],
) -> bool {
    let name = call
        .target
        .and_then(|t| objects.iter().find(|(e, _)| *e == t))
        .map_or("None (Object)".to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    egui::ComboBox::from_id_salt(("on_click_target", i))
        .selected_text(name)
        .width(110.0)
        .show_ui(ui, |ui| {
            ch |= ui
                .selectable_value(&mut call.target, None, "None (Object)")
                .changed();
            for (e, n) in objects {
                ch |= ui.selectable_value(&mut call.target, Some(*e), n).changed();
            }
        });
    ch
}
