//! Inspector editors for the UI components: Rect Transform (with anchor presets), Canvas,
//! Canvas Scaler, Image, Text and Button.

use egui::{Grid, RichText, Ui};
use glam::Vec2;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    ButtonCall, Canvas, CanvasRenderMode, CanvasScaler, RectTransform, SceneComponent, TextAnchor,
    UiButton, UiImage, UiScaleMode, UiText,
};
use misty_engine::ui::layout::{AnchorAxis, apply_anchor_preset};

use crate::state::EditorState;
use crate::theme;

/// Names of the UI components handled here.
pub const NAMES: [&str; 6] = [
    "Rect Transform",
    "Canvas",
    "Canvas Scaler",
    "Image",
    "Text",
    "Button",
];

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Rect Transform" => "⬚",
        "Canvas" => "▭",
        "Canvas Scaler" => "⤢",
        "Image" => "🖼",
        "Text" => "T",
        "Button" => "▣",
        _ => return None,
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
        _ => None,
    }
}

pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
    let w = &mut state.scene.world;
    let _ = match name {
        "Canvas" => w.get::<&mut Canvas>(e).map(|mut c| c.enabled = on),
        "Canvas Scaler" => w.get::<&mut CanvasScaler>(e).map(|mut c| c.enabled = on),
        "Image" => w.get::<&mut UiImage>(e).map(|mut c| c.enabled = on),
        "Text" => w.get::<&mut UiText>(e).map(|mut c| c.enabled = on),
        _ => return,
    };
}

/// Draw the editor for UI component `name`; true when a value changed.
pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "Rect Transform" => rect_transform(ui, state, e),
        "Canvas" => with::<Canvas>(ui, state, e, canvas),
        "Canvas Scaler" => with::<CanvasScaler>(ui, state, e, canvas_scaler),
        "Image" => {
            let textures = state.assets.textures.clone();
            edit_copy::<UiImage>(state, e, |c| image(ui, c, &textures))
        }
        "Text" => with::<UiText>(ui, state, e, text),
        "Button" => {
            let objects: Vec<(Entity, String)> = state
                .scene
                .walk()
                .into_iter()
                .map(|(o, _)| (o, state.scene.name(o)))
                .collect();
            edit_copy::<UiButton>(state, e, |c| button(ui, c, &objects))
        }
        _ => false,
    }
}

/// Edit a copy of `T` with `f` and store it if anything changed.
fn edit_copy<T: SceneComponent>(
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

fn with<T: SceneComponent>(
    ui: &mut Ui,
    state: &mut EditorState,
    e: Entity,
    draw: fn(&mut Ui, &mut T) -> bool,
) -> bool {
    edit_copy(state, e, |c| draw(ui, c))
}

fn grid(ui: &mut Ui, id: &str, body: impl FnOnce(&mut Ui) -> bool) -> bool {
    Grid::new(id)
        .num_columns(2)
        .spacing([10.0, 4.0])
        .min_col_width(110.0)
        .show(ui, body)
        .inner
}

fn drag(ui: &mut Ui, v: &mut f32, speed: f64) -> bool {
    ui.add(egui::DragValue::new(v).speed(speed).max_decimals(3))
        .changed()
}

fn vec2(ui: &mut Ui, v: &mut Vec2, speed: f64) -> bool {
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
fn color(ui: &mut Ui, c: &mut [f32; 4]) -> bool {
    let mut rgba = c.map(|v| (v.clamp(0.0, 1.0) * 255.0).round() as u8);
    let changed = ui.color_edit_button_srgba_unmultiplied(&mut rgba).changed();
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
    // A root canvas' rect is the screen: the Canvas drives it.
    let driven = state.scene.world.get::<&Canvas>(e).is_ok()
        && state
            .scene
            .parent(e)
            .is_none_or(|p| misty_engine::ui::canvas_of(&state.scene, p).is_none());
    if driven {
        ui.label(RichText::new("Some values driven by Canvas.").color(theme::TEXT_DIM));
        return false;
    }
    let parent = parent_size(state, e);
    let id = ui.make_persistent_id(("anchor_presets", e));
    edit_copy::<RectTransform>(state, e, |rt| {
        let mut ch = false;
        ui.horizontal(|ui| {
            ch |= presets_button(ui, id, rt, parent);
            ui.vertical(|ui| ch |= position_fields(ui, rt));
        });
        egui::CollapsingHeader::new("Anchors")
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
        ui.label(RichText::new("Anchor Presets").strong());
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

fn canvas(ui: &mut Ui, c: &mut Canvas) -> bool {
    grid(ui, "canvas_grid", |ui| {
        let mut ch = false;
        ui.label("Render Mode");
        egui::ComboBox::from_id_salt("canvas_mode")
            .selected_text(c.render_mode.label())
            .width(170.0)
            .show_ui(ui, |ui| {
                for m in CanvasRenderMode::ALL {
                    ch |= ui
                        .selectable_value(&mut c.render_mode, m, m.label())
                        .changed();
                }
            });
        ui.end_row();
        ui.label("Sort Order");
        ch |= ui.add(egui::DragValue::new(&mut c.sort_order)).changed();
        ui.end_row();
        ch
    })
}

fn canvas_scaler(ui: &mut Ui, s: &mut CanvasScaler) -> bool {
    grid(ui, "canvas_scaler_grid", |ui| {
        let mut ch = false;
        ui.label("UI Scale Mode");
        egui::ComboBox::from_id_salt("scaler_mode")
            .selected_text(s.mode.label())
            .width(170.0)
            .show_ui(ui, |ui| {
                for m in UiScaleMode::ALL {
                    ch |= ui.selectable_value(&mut s.mode, m, m.label()).changed();
                }
            });
        ui.end_row();
        match s.mode {
            UiScaleMode::ConstantPixelSize => {
                ui.label("Scale Factor");
                ch |= ui
                    .add(
                        egui::DragValue::new(&mut s.scale_factor)
                            .speed(0.01)
                            .range(0.01..=100.0),
                    )
                    .changed();
                ui.end_row();
            }
            UiScaleMode::ScaleWithScreenSize => {
                ui.label("Reference Resolution");
                ch |= vec2(ui, &mut s.reference_resolution, 1.0);
                s.reference_resolution = s.reference_resolution.max(Vec2::ONE);
                ui.end_row();
                ui.label("Screen Match Mode");
                ui.label("Match Width Or Height");
                ui.end_row();
                ui.label("Match");
                ui.vertical(|ui| {
                    ch |= ui
                        .add(egui::Slider::new(&mut s.match_width_or_height, 0.0..=1.0))
                        .changed();
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Width").small().color(theme::TEXT_DIM));
                        ui.add_space(110.0);
                        ui.label(RichText::new("Height").small().color(theme::TEXT_DIM));
                    });
                });
                ui.end_row();
            }
        }
        ch
    })
}

// ---- Image / Text / Button ---------------------------------------------------------------------

fn image(ui: &mut Ui, img: &mut UiImage, textures: &[String]) -> bool {
    grid(ui, "ui_image_grid", |ui| {
        let mut ch = false;
        ui.label("Source Image");
        ch |= super::slots::texture_slot(ui, "ui_image_tex", &mut img.texture, textures);
        ui.end_row();
        ui.label("Color");
        ch |= color(ui, &mut img.color);
        ui.end_row();
        ui.label("Raycast Target");
        ch |= ui.checkbox(&mut img.raycast_target, "").changed();
        ui.end_row();
        ch
    })
}

fn text(ui: &mut Ui, t: &mut UiText) -> bool {
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
        ui.label(RichText::new("Ubuntu Light (built-in)").color(theme::TEXT_DIM));
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
    ch
}

/// Horizontal (left/center/right) and vertical (upper/middle/lower) toggle rows.
fn alignment(ui: &mut Ui, a: &mut TextAnchor) -> bool {
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
fn target_field(
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
