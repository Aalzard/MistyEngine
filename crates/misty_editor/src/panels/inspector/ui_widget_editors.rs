//! Inspector editors for the UI widgets (Toggle, Toggle Group, Slider, Scrollbar, Input Field,
//! Dropdown, Scroll Rect, Mask, Rect Mask 2D, Raw Image), the layout components and the Event
//! System.

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    ButtonCall, ContentSizeFitter, EventSystem, FitMode, GridAxis, GridConstraint, GridCorner,
    GridLayoutGroup, HorizontalLayoutGroup, InputContentType, LayoutElement, LinearLayout,
    MovementType, Padding, Selectable, SliderDirection, UiDropdown, UiInputField, UiMask,
    UiRawImage, UiRectMask2D, UiScrollRect, UiScrollbar, UiSlider, UiToggle, UiToggleGroup,
    VerticalLayoutGroup,
};

use super::{alignment, color, edit_copy, grid, vec2};
use crate::state::EditorState;
use crate::theme;

/// Scene objects for reference pickers: (entity, name).
type Objects = Vec<(Entity, String)>;

pub fn icon(name: &str) -> Option<&'static str> {
    Some(match name {
        "Raw Image" => "🖼",
        "Toggle" => "☑",
        "Toggle Group" => "☷",
        "Slider" => "⊶",
        "Scrollbar" => "⇕",
        "Input Field" => "⌨",
        "Dropdown" => "⏷",
        "Scroll Rect" => "↕",
        "Mask" | "Rect Mask 2D" => "◩",
        "Horizontal Layout Group" => "⫼",
        "Vertical Layout Group" => "☰",
        "Grid Layout Group" => "▦",
        "Content Size Fitter" => "⤡",
        "Layout Element" => "▢",
        "Event System" => "⚡",
        _ => return None,
    })
}

macro_rules! flags {
    ($($name:literal => $t:ty),* $(,)?) => {
        pub fn enabled_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
            let w = &state.scene.world;
            match name {
                $($name => w.get::<&$t>(e).ok().map(|c| c.enabled),)*
                _ => None,
            }
        }

        pub fn set_enabled(state: &mut EditorState, e: Entity, name: &str, on: bool) {
            let w = &mut state.scene.world;
            let _ = match name {
                $($name => w.get::<&mut $t>(e).map(|mut c| c.enabled = on),)*
                _ => return,
            };
        }
    };
}

flags! {
    "Raw Image" => UiRawImage,
    "Scroll Rect" => UiScrollRect,
    "Mask" => UiMask,
    "Rect Mask 2D" => UiRectMask2D,
    "Grid Layout Group" => GridLayoutGroup,
    "Content Size Fitter" => ContentSizeFitter,
    "Layout Element" => LayoutElement,
    "Event System" => EventSystem,
}

/// Linear groups keep `enabled` one level down.
fn linear_flag(state: &EditorState, e: Entity, name: &str) -> Option<bool> {
    let w = &state.scene.world;
    match name {
        "Horizontal Layout Group" => w.get::<&HorizontalLayoutGroup>(e).ok().map(|c| c.0.enabled),
        "Vertical Layout Group" => w.get::<&VerticalLayoutGroup>(e).ok().map(|c| c.0.enabled),
        _ => None,
    }
}

/// What drives `e`'s Rect Transform (a parent layout group or a widget), if anything.
pub fn driven_by(state: &EditorState, e: Entity) -> Option<String> {
    let scene = &state.scene;
    let ignored = scene
        .world
        .get::<&LayoutElement>(e)
        .is_ok_and(|le| le.enabled && le.ignore_layout);
    if let Some(p) = scene.parent(e)
        && !ignored
        && misty_engine::ui::auto_layout::has_group(scene, p)
    {
        return Some(scene.name(p));
    }
    let anchors = misty_engine::ui::driven::driven_anchors(scene);
    if anchors.contains_key(&e) {
        return Some("a Slider or Scrollbar".into());
    }
    None
}

fn objects(state: &EditorState) -> Objects {
    state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| (o, state.scene.name(o)))
        .collect()
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    let objs = objects(state);
    let textures = state.assets.textures.clone();
    match name {
        "Raw Image" => edit_copy::<UiRawImage>(state, e, |c| raw_image(ui, c, &textures)),
        "Toggle" => edit_copy::<UiToggle>(state, e, |c| toggle(ui, c, &objs)),
        "Toggle Group" => edit_copy::<UiToggleGroup>(state, e, |c| {
            grid(ui, "ui_toggle_group", |ui| {
                ui.label("Allow Switch Off");
                let ch = ui.checkbox(&mut c.allow_switch_off, "").changed();
                ui.end_row();
                ch
            })
        }),
        "Slider" => edit_copy::<UiSlider>(state, e, |c| slider(ui, c, &objs)),
        "Scrollbar" => edit_copy::<UiScrollbar>(state, e, |c| scrollbar(ui, c, &objs)),
        "Input Field" => edit_copy::<UiInputField>(state, e, |c| input_field(ui, c, &objs)),
        "Dropdown" => edit_copy::<UiDropdown>(state, e, |c| dropdown(ui, c, &objs)),
        "Scroll Rect" => edit_copy::<UiScrollRect>(state, e, |c| scroll_rect(ui, c, &objs)),
        "Mask" => edit_copy::<UiMask>(state, e, |c| {
            grid(ui, "ui_mask", |ui| {
                ui.label("Show Mask Graphic");
                let ch = ui.checkbox(&mut c.show_mask_graphic, "").changed();
                ui.end_row();
                ch
            })
        }),
        "Rect Mask 2D" => edit_copy::<UiRectMask2D>(state, e, |c| {
            grid(ui, "ui_rect_mask", |ui| {
                let mut ch = false;
                for (i, l) in ["Left", "Bottom", "Right", "Top"].into_iter().enumerate() {
                    ui.label(format!("Padding {l}"));
                    ch |= drag(ui, &mut c.padding[i]);
                    ui.end_row();
                }
                ch
            })
        }),
        "Horizontal Layout Group" => {
            linear_header(ui, state, e, name);
            edit_copy::<HorizontalLayoutGroup>(state, e, |c| linear(ui, &mut c.0, "h"))
        }
        "Vertical Layout Group" => {
            linear_header(ui, state, e, name);
            edit_copy::<VerticalLayoutGroup>(state, e, |c| linear(ui, &mut c.0, "v"))
        }
        "Grid Layout Group" => edit_copy::<GridLayoutGroup>(state, e, |c| grid_layout(ui, c)),
        "Content Size Fitter" => edit_copy::<ContentSizeFitter>(state, e, |c| fitter(ui, c)),
        "Layout Element" => edit_copy::<LayoutElement>(state, e, |c| layout_element(ui, c)),
        "Event System" => edit_copy::<EventSystem>(state, e, |c| event_system(ui, c, &objs)),
        _ => false,
    }
}

/// Linear groups have no header checkbox; show an Enabled row instead.
fn linear_header(ui: &mut Ui, state: &EditorState, e: Entity, name: &str) {
    if linear_flag(state, e, name) == Some(false) {
        ui.label(RichText::new("Disabled: children keep their own rects.").color(theme::TEXT_DIM));
    }
}

fn drag(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(egui::DragValue::new(v).speed(0.5).max_decimals(3))
        .changed()
}

/// Scene object picker (`None (Object)` clears it).
fn object_field(ui: &mut Ui, id: &str, v: &mut Option<Entity>, objs: &Objects) -> bool {
    let name = v
        .and_then(|t| objs.iter().find(|(e, _)| *e == t))
        .map_or("None (Object)".to_owned(), |(_, n)| n.clone());
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(name)
        .show_ui(ui, |ui| {
            ch |= ui.selectable_value(v, None, "None (Object)").changed();
            for (e, n) in objs {
                ch |= ui.selectable_value(v, Some(*e), n).changed();
            }
        });
    ch
}

fn combo<T: Copy + PartialEq>(
    ui: &mut Ui,
    id: &str,
    v: &mut T,
    all: &[T],
    label: fn(T) -> &'static str,
) -> bool {
    let mut ch = false;
    egui::ComboBox::from_id_salt(id)
        .selected_text(label(*v))
        .show_ui(ui, |ui| {
            for a in all {
                ch |= ui.selectable_value(v, *a, label(*a)).changed();
            }
        });
    ch
}

/// Interactable, Colour Tint colours and Target Graphic rows.
fn selectable(ui: &mut Ui, id: &str, s: &mut Selectable, objs: &Objects) -> bool {
    let mut c = false;
    ui.label("Interactable");
    c |= ui.checkbox(&mut s.interactable, "").changed();
    ui.end_row();
    ui.label("Transition");
    ui.label("Color Tint");
    ui.end_row();
    ui.label("Target Graphic");
    c |= object_field(ui, &format!("{id}_target"), &mut s.target_graphic, objs);
    ui.end_row();
    let b = &mut s.colors;
    for (label, col) in [
        ("Normal Color", &mut b.normal),
        ("Highlighted Color", &mut b.highlighted),
        ("Pressed Color", &mut b.pressed),
        ("Selected Color", &mut b.selected),
        ("Disabled Color", &mut b.disabled),
    ] {
        ui.label(label);
        c |= color(ui, col);
        ui.end_row();
    }
    ui.label("Color Multiplier");
    c |= ui
        .add(egui::Slider::new(&mut b.multiplier, 1.0..=5.0))
        .changed();
    ui.end_row();
    c
}

/// A persistent call list ("On Value Changed (Single)" …).
fn calls(ui: &mut Ui, id: &str, title: &str, list: &mut Vec<ButtonCall>, objs: &Objects) -> bool {
    let mut ch = false;
    ui.add_space(4.0);
    ui.label(title);
    egui::Frame::group(ui.style()).show(ui, |ui| {
        if list.is_empty() {
            ui.label(RichText::new("List is Empty").color(theme::TEXT_DIM));
        }
        let mut remove = None;
        for (i, call) in list.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ch |= object_field(ui, &format!("{id}_{i}"), &mut call.target, objs);
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
            list.remove(i);
            ch = true;
        }
        if ui.small_button("+").on_hover_text("Add a call").clicked() {
            list.push(ButtonCall::default());
            ch = true;
        }
    });
    ch
}

fn raw_image(ui: &mut Ui, img: &mut UiRawImage, textures: &[String]) -> bool {
    grid(ui, "ui_raw_image", |ui| {
        let mut ch = false;
        ui.label("Texture");
        ch |= super::super::slots::texture_slot(ui, "ui_raw_tex", &mut img.texture, textures);
        ui.end_row();
        ui.label("Color");
        ch |= color(ui, &mut img.color);
        ui.end_row();
        ui.label("Raycast Target");
        ch |= ui.checkbox(&mut img.raycast_target, "").changed();
        ui.end_row();
        ui.label("UV Rect");
        ui.horizontal(|ui| {
            for (i, l) in ["X", "Y", "W", "H"].into_iter().enumerate() {
                ui.label(l);
                ch |= ui
                    .add(egui::DragValue::new(&mut img.uv_rect[i]).speed(0.01))
                    .changed();
            }
        });
        ui.end_row();
        ch
    })
}

fn toggle(ui: &mut Ui, t: &mut UiToggle, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ui_toggle", |ui| {
        let mut c = selectable(ui, "toggle", &mut t.selectable, objs);
        ui.label("Is On");
        c |= ui.checkbox(&mut t.is_on, "").changed();
        ui.end_row();
        ui.label("Graphic");
        c |= object_field(ui, "toggle_graphic", &mut t.graphic, objs);
        ui.end_row();
        ui.label("Group");
        c |= object_field(ui, "toggle_group", &mut t.group, objs);
        ui.end_row();
        c
    });
    ch |= calls(
        ui,
        "toggle_calls",
        "On Value Changed (Boolean)",
        &mut t.on_value_changed,
        objs,
    );
    ch
}

fn direction(ui: &mut Ui, id: &str, d: &mut SliderDirection) -> bool {
    combo(ui, id, d, &SliderDirection::ALL, SliderDirection::label)
}

fn slider(ui: &mut Ui, s: &mut UiSlider, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ui_slider", |ui| {
        let mut c = selectable(ui, "slider", &mut s.selectable, objs);
        ui.label("Fill Rect");
        c |= object_field(ui, "slider_fill", &mut s.fill_rect, objs);
        ui.end_row();
        ui.label("Handle Rect");
        c |= object_field(ui, "slider_handle", &mut s.handle_rect, objs);
        ui.end_row();
        ui.label("Direction");
        c |= direction(ui, "slider_dir", &mut s.direction);
        ui.end_row();
        ui.label("Min Value");
        c |= drag(ui, &mut s.min_value);
        ui.end_row();
        ui.label("Max Value");
        c |= drag(ui, &mut s.max_value);
        ui.end_row();
        ui.label("Whole Numbers");
        c |= ui.checkbox(&mut s.whole_numbers, "").changed();
        ui.end_row();
        ui.label("Value");
        let (lo, hi) = (s.min_value.min(s.max_value), s.min_value.max(s.max_value));
        let mut slider = egui::Slider::new(&mut s.value, lo..=hi);
        if s.whole_numbers {
            slider = slider.integer();
        }
        c |= ui.add(slider).changed();
        ui.end_row();
        c
    });
    ch |= calls(
        ui,
        "slider_calls",
        "On Value Changed (Single)",
        &mut s.on_value_changed,
        objs,
    );
    ch
}

fn scrollbar(ui: &mut Ui, s: &mut UiScrollbar, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ui_scrollbar", |ui| {
        let mut c = selectable(ui, "scrollbar", &mut s.selectable, objs);
        ui.label("Handle Rect");
        c |= object_field(ui, "scrollbar_handle", &mut s.handle_rect, objs);
        ui.end_row();
        ui.label("Direction");
        c |= direction(ui, "scrollbar_dir", &mut s.direction);
        ui.end_row();
        ui.label("Value");
        c |= ui.add(egui::Slider::new(&mut s.value, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Size");
        c |= ui.add(egui::Slider::new(&mut s.size, 0.0..=1.0)).changed();
        ui.end_row();
        ui.label("Number Of Steps");
        c |= ui
            .add(egui::DragValue::new(&mut s.number_of_steps).range(0..=11))
            .changed();
        ui.end_row();
        c
    });
    ch |= calls(
        ui,
        "scrollbar_calls",
        "On Value Changed (Single)",
        &mut s.on_value_changed,
        objs,
    );
    ch
}

fn input_field(ui: &mut Ui, f: &mut UiInputField, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ui_input_field", |ui| {
        let mut c = selectable(ui, "input", &mut f.selectable, objs);
        ui.label("Text Component");
        c |= object_field(ui, "input_text", &mut f.text_component, objs);
        ui.end_row();
        ui.label("Text");
        if ui.text_edit_singleline(&mut f.text).changed() {
            f.text = misty_engine::ui::input_field::sanitize(f, &f.text);
            c = true;
        }
        ui.end_row();
        ui.label("Character Limit");
        c |= ui
            .add(egui::DragValue::new(&mut f.character_limit))
            .changed();
        ui.end_row();
        ui.label("Content Type");
        c |= combo(
            ui,
            "input_type",
            &mut f.content_type,
            &InputContentType::ALL,
            InputContentType::label,
        );
        ui.end_row();
        ui.label("Line Type");
        ui.label(RichText::new("Single Line").color(theme::TEXT_DIM));
        ui.end_row();
        ui.label("Placeholder");
        c |= object_field(ui, "input_placeholder", &mut f.placeholder, objs);
        ui.end_row();
        ui.label("Caret Blink Rate");
        c |= ui
            .add(
                egui::DragValue::new(&mut f.caret_blink_rate)
                    .speed(0.05)
                    .range(0.0..=4.0),
            )
            .changed();
        ui.end_row();
        ui.label("Caret Width");
        c |= ui
            .add(
                egui::DragValue::new(&mut f.caret_width)
                    .speed(0.1)
                    .range(1.0..=5.0),
            )
            .changed();
        ui.end_row();
        ui.label("Selection Color");
        c |= color(ui, &mut f.selection_color);
        ui.end_row();
        ui.label("Read Only");
        c |= ui.checkbox(&mut f.read_only, "").changed();
        ui.end_row();
        c
    });
    ch |= calls(
        ui,
        "input_changed",
        "On Value Changed (String)",
        &mut f.on_value_changed,
        objs,
    );
    ch |= calls(
        ui,
        "input_end",
        "On End Edit (String)",
        &mut f.on_end_edit,
        objs,
    );
    ch
}

fn dropdown(ui: &mut Ui, d: &mut UiDropdown, objs: &Objects) -> bool {
    let mut ch = grid(ui, "ui_dropdown", |ui| {
        let mut c = selectable(ui, "dropdown", &mut d.selectable, objs);
        ui.label("Template");
        c |= object_field(ui, "dd_template", &mut d.template, objs);
        ui.end_row();
        ui.label("Caption Text");
        c |= object_field(ui, "dd_caption", &mut d.caption_text, objs);
        ui.end_row();
        ui.label("Item Text");
        c |= object_field(ui, "dd_item", &mut d.item_text, objs);
        ui.end_row();
        ui.label("Value");
        let max = (d.options.len() as i32 - 1).max(0);
        c |= ui
            .add(egui::DragValue::new(&mut d.value).range(0..=max))
            .changed();
        ui.end_row();
        c
    });
    ui.add_space(4.0);
    ui.label("Options");
    egui::Frame::group(ui.style()).show(ui, |ui| {
        let mut remove = None;
        for (i, o) in d.options.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ch |= ui.text_edit_singleline(o).changed();
                if ui.small_button("−").on_hover_text("Remove").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            d.options.remove(i);
            ch = true;
        }
        if ui
            .small_button("+")
            .on_hover_text("Add an option")
            .clicked()
        {
            d.options.push(format!("Option {}", d.options.len() + 1));
            ch = true;
        }
    });
    ch |= calls(
        ui,
        "dd_calls",
        "On Value Changed (Int32)",
        &mut d.on_value_changed,
        objs,
    );
    ch
}

fn scroll_rect(ui: &mut Ui, s: &mut UiScrollRect, objs: &Objects) -> bool {
    grid(ui, "ui_scroll_rect", |ui| {
        let mut c = false;
        ui.label("Content");
        c |= object_field(ui, "sr_content", &mut s.content, objs);
        ui.end_row();
        ui.label("Horizontal");
        c |= ui.checkbox(&mut s.horizontal, "").changed();
        ui.end_row();
        ui.label("Vertical");
        c |= ui.checkbox(&mut s.vertical, "").changed();
        ui.end_row();
        ui.label("Movement Type");
        c |= combo(
            ui,
            "sr_move",
            &mut s.movement_type,
            &MovementType::ALL,
            MovementType::label,
        );
        ui.end_row();
        if s.movement_type == MovementType::Elastic {
            ui.label("Elasticity");
            c |= ui
                .add(
                    egui::DragValue::new(&mut s.elasticity)
                        .speed(0.01)
                        .range(0.0..=10.0),
                )
                .changed();
            ui.end_row();
        }
        ui.label("Inertia");
        c |= ui.checkbox(&mut s.inertia, "").changed();
        ui.end_row();
        if s.inertia {
            ui.label("Deceleration Rate");
            c |= ui
                .add(
                    egui::DragValue::new(&mut s.deceleration_rate)
                        .speed(0.01)
                        .range(0.0..=1.0),
                )
                .changed();
            ui.end_row();
        }
        ui.label("Scroll Sensitivity");
        c |= ui
            .add(egui::DragValue::new(&mut s.scroll_sensitivity).speed(0.05))
            .changed();
        ui.end_row();
        ui.label("Viewport");
        c |= object_field(ui, "sr_viewport", &mut s.viewport, objs);
        ui.end_row();
        ui.label("Horizontal Scrollbar");
        c |= object_field(ui, "sr_hbar", &mut s.horizontal_scrollbar, objs);
        ui.end_row();
        ui.label("Vertical Scrollbar");
        c |= object_field(ui, "sr_vbar", &mut s.vertical_scrollbar, objs);
        ui.end_row();
        c
    })
}

fn padding(ui: &mut Ui, p: &mut Padding) -> bool {
    let mut c = false;
    for (label, v) in [
        ("Left", &mut p.left),
        ("Right", &mut p.right),
        ("Top", &mut p.top),
        ("Bottom", &mut p.bottom),
    ] {
        ui.label(format!("    {label}"));
        c |= drag(ui, v);
        ui.end_row();
    }
    c
}

fn linear(ui: &mut Ui, l: &mut LinearLayout, id: &str) -> bool {
    grid(ui, &format!("ui_linear_{id}"), |ui| {
        let mut c = false;
        ui.label("Enabled");
        c |= ui.checkbox(&mut l.enabled, "").changed();
        ui.end_row();
        ui.label("Padding");
        ui.label("");
        ui.end_row();
        c |= padding(ui, &mut l.padding);
        ui.label("Spacing");
        c |= drag(ui, &mut l.spacing);
        ui.end_row();
        ui.label("Child Alignment");
        c |= alignment(ui, &mut l.child_alignment);
        ui.end_row();
        ui.label("Reverse Arrangement");
        c |= ui.checkbox(&mut l.reverse_arrangement, "").changed();
        ui.end_row();
        ui.label("Control Child Size");
        ui.horizontal(|ui| {
            c |= ui.checkbox(&mut l.control_child_width, "Width").changed();
            c |= ui.checkbox(&mut l.control_child_height, "Height").changed();
        });
        ui.end_row();
        ui.label("Child Force Expand");
        ui.horizontal(|ui| {
            c |= ui
                .checkbox(&mut l.child_force_expand_width, "Width")
                .changed();
            c |= ui
                .checkbox(&mut l.child_force_expand_height, "Height")
                .changed();
        });
        ui.end_row();
        c
    })
}

fn grid_layout(ui: &mut Ui, g: &mut GridLayoutGroup) -> bool {
    grid(ui, "ui_grid_layout", |ui| {
        let mut c = false;
        ui.label("Padding");
        ui.label("");
        ui.end_row();
        c |= padding(ui, &mut g.padding);
        ui.label("Cell Size");
        c |= vec2(ui, &mut g.cell_size, 0.5);
        ui.end_row();
        ui.label("Spacing");
        c |= vec2(ui, &mut g.spacing, 0.5);
        ui.end_row();
        ui.label("Start Corner");
        c |= combo(
            ui,
            "grid_corner",
            &mut g.start_corner,
            &GridCorner::ALL,
            GridCorner::label,
        );
        ui.end_row();
        ui.label("Start Axis");
        let axis_label = |a: GridAxis| match a {
            GridAxis::Horizontal => "Horizontal",
            GridAxis::Vertical => "Vertical",
        };
        c |= combo(
            ui,
            "grid_axis",
            &mut g.start_axis,
            &[GridAxis::Horizontal, GridAxis::Vertical],
            axis_label,
        );
        ui.end_row();
        ui.label("Child Alignment");
        c |= alignment(ui, &mut g.child_alignment);
        ui.end_row();
        ui.label("Constraint");
        c |= combo(
            ui,
            "grid_constraint",
            &mut g.constraint,
            &GridConstraint::ALL,
            GridConstraint::label,
        );
        ui.end_row();
        if g.constraint != GridConstraint::Flexible {
            ui.label("Constraint Count");
            c |= ui
                .add(egui::DragValue::new(&mut g.constraint_count).range(1..=100))
                .changed();
            ui.end_row();
        }
        c
    })
}

fn fitter(ui: &mut Ui, f: &mut ContentSizeFitter) -> bool {
    grid(ui, "ui_fitter", |ui| {
        let mut c = false;
        ui.label("Horizontal Fit");
        c |= combo(
            ui,
            "fit_h",
            &mut f.horizontal_fit,
            &FitMode::ALL,
            FitMode::label,
        );
        ui.end_row();
        ui.label("Vertical Fit");
        c |= combo(
            ui,
            "fit_v",
            &mut f.vertical_fit,
            &FitMode::ALL,
            FitMode::label,
        );
        ui.end_row();
        c
    })
}

/// A checkbox + value pair for an optional layout size.
fn optional(ui: &mut Ui, label: &str, v: &mut Option<f32>, default: f32) -> bool {
    ui.label(label);
    let mut c = false;
    ui.horizontal(|ui| {
        let mut on = v.is_some();
        if ui.checkbox(&mut on, "").changed() {
            *v = on.then_some(default);
            c = true;
        }
        if let Some(x) = v.as_mut() {
            c |= drag(ui, x);
        }
    });
    ui.end_row();
    c
}

fn layout_element(ui: &mut Ui, le: &mut LayoutElement) -> bool {
    grid(ui, "ui_layout_element", |ui| {
        let mut c = false;
        ui.label("Ignore Layout");
        c |= ui.checkbox(&mut le.ignore_layout, "").changed();
        ui.end_row();
        if !le.ignore_layout {
            c |= optional(ui, "Min Width", &mut le.min_width, 0.0);
            c |= optional(ui, "Min Height", &mut le.min_height, 0.0);
            c |= optional(ui, "Preferred Width", &mut le.preferred_width, 100.0);
            c |= optional(ui, "Preferred Height", &mut le.preferred_height, 30.0);
            c |= optional(ui, "Flexible Width", &mut le.flexible_width, 1.0);
            c |= optional(ui, "Flexible Height", &mut le.flexible_height, 1.0);
        }
        c
    })
}

fn event_system(ui: &mut Ui, es: &mut EventSystem, objs: &Objects) -> bool {
    grid(ui, "ui_event_system", |ui| {
        let mut c = false;
        ui.label("First Selected");
        c |= object_field(ui, "es_first", &mut es.first_selected, objs);
        ui.end_row();
        ui.label("Send Navigation Events");
        c |= ui.checkbox(&mut es.send_navigation_events, "").changed();
        ui.end_row();
        ui.label("Drag Threshold");
        c |= ui
            .add(egui::DragValue::new(&mut es.drag_threshold).range(0.0..=100.0))
            .changed();
        ui.end_row();
        ui.label("Input");
        ui.label(RichText::new("Mouse + keyboard (built in)").color(theme::TEXT_DIM));
        ui.end_row();
        c
    })
}
