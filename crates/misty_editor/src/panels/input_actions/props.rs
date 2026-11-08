//! Properties column of the Input Actions window: the selected action (type, control type,
//! interactions, processors), binding (path with a picker and Listen, control schemes,
//! interactions, processors) or composite (type and parameters); and the control scheme
//! dialog.

use egui::{RichText, Ui};
use misty_engine::input::actions::asset::{self, ControlScheme, has_group, set_group};
use misty_engine::input::actions::path::{ControlPath, DeviceKind, controls, keyboard_path};
use misty_engine::input::actions::value::{NamedParams, list_to_text, parse_list};
use misty_engine::input::actions::{
    ActionType, ControlType, InputActionAsset, composites, interactions, processors,
};

use super::{Selection, title_case};
use crate::state::EditorState;
use crate::theme;

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.add_sized([110.0, 18.0], egui::Label::new(text).truncate())
        .on_hover_text(tip);
}

fn section(ui: &mut Ui, title: &str) {
    ui.add_space(6.0);
    ui.label(theme::bold(ui.ctx(), title));
    ui.separator();
}

pub fn column(ui: &mut Ui, s: &mut EditorState, a: &mut InputActionAsset) {
    let m = s.input_actions.map;
    let sel = s.input_actions.selection;
    egui::ScrollArea::vertical()
        .id_salt("ia_props")
        .auto_shrink(false)
        .show(ui, |ui| match sel {
            Selection::None => {
                ui.label(theme::bold(ui.ctx(), "Properties"));
                ui.separator();
                ui.weak("Select an action or a binding.");
            }
            Selection::Action(ai) => {
                if let Some(act) = a.maps.get_mut(m).and_then(|x| x.actions.get_mut(ai)) {
                    action_props(ui, act);
                }
            }
            Selection::Binding(ai, bi, part) => {
                let schemes = a.control_schemes.clone();
                let Some(act) = a.maps.get_mut(m).and_then(|x| x.actions.get_mut(ai)) else {
                    return;
                };
                let vector = act.control_type == ControlType::Vector2;
                let Some(b) = act.bindings.get_mut(bi) else {
                    return;
                };
                match part {
                    Some(pi) if pi < b.parts.len() => {
                        let p = &mut b.parts[pi];
                        section(ui, &format!("Composite Part: {}", title_case(&p.name)));
                        path_row(ui, s, &mut p.path, false);
                        schemes_rows(ui, &schemes, &mut p.groups);
                        params_list(
                            ui,
                            "Processors",
                            "ia_part_proc",
                            &mut p.processors,
                            Kinds::Processors,
                        );
                    }
                    Some(_) => {}
                    None if b.is_composite() => {
                        composite_props(ui, b);
                        params_list(
                            ui,
                            "Interactions",
                            "ia_comp_int",
                            &mut b.interactions,
                            Kinds::Interactions,
                        );
                        params_list(
                            ui,
                            "Processors",
                            "ia_comp_proc",
                            &mut b.processors,
                            Kinds::Processors,
                        );
                    }
                    None => {
                        section(ui, "Binding");
                        path_row(ui, s, &mut b.path, vector);
                        schemes_rows(ui, &schemes, &mut b.groups);
                        params_list(
                            ui,
                            "Interactions",
                            "ia_bind_int",
                            &mut b.interactions,
                            Kinds::Interactions,
                        );
                        params_list(
                            ui,
                            "Processors",
                            "ia_bind_proc",
                            &mut b.processors,
                            Kinds::Processors,
                        );
                    }
                }
            }
        });
}

fn action_props(ui: &mut Ui, act: &mut misty_engine::input::actions::Action) {
    section(ui, "Action");
    ui.horizontal(|ui| {
        label(
            ui,
            "Action Type",
            "Value: continuous, the most actuated control drives it. Button: pressed and \
             released. Pass Through: every change of every control.",
        );
        egui::ComboBox::from_id_salt("ia_action_type")
            .selected_text(act.kind.label())
            .show_ui(ui, |ui| {
                for t in ActionType::ALL {
                    if ui.selectable_value(&mut act.kind, t, t.label()).changed() {
                        if t == ActionType::Button {
                            act.control_type = ControlType::Button;
                        }
                        act.initial_state_check = t == ActionType::Value;
                    }
                }
            });
    });
    if act.kind != ActionType::Button {
        ui.horizontal(|ui| {
            label(ui, "Control Type", "What ReadValue returns.");
            egui::ComboBox::from_id_salt("ia_control_type")
                .selected_text(act.control_type.label())
                .show_ui(ui, |ui| {
                    for t in ControlType::ALL {
                        ui.selectable_value(&mut act.control_type, t, t.label());
                    }
                });
        });
        ui.horizontal(|ui| {
            label(
                ui,
                "Initial State Check",
                "Read controls that are already actuated when the action is enabled.",
            );
            ui.checkbox(&mut act.initial_state_check, "");
        });
    }
    params_list(
        ui,
        "Interactions",
        "ia_act_int",
        &mut act.interactions,
        Kinds::Interactions,
    );
    params_list(
        ui,
        "Processors",
        "ia_act_proc",
        &mut act.processors,
        Kinds::Processors,
    );
}

fn composite_props(ui: &mut Ui, b: &mut misty_engine::input::actions::Binding) {
    section(ui, "Composite");
    ui.horizontal(|ui| {
        label(ui, "Composite Type", "How the parts combine.");
        let current = composites::KINDS
            .iter()
            .find(|k| k.0 == b.composite)
            .map_or(b.composite.clone(), |k| k.1.to_owned());
        egui::ComboBox::from_id_salt("ia_comp_type")
            .selected_text(current)
            .width(150.0)
            .show_ui(ui, |ui| {
                for (kind, lbl, parts, params) in composites::KINDS {
                    if ui.selectable_label(b.composite == *kind, *lbl).clicked()
                        && b.composite != *kind
                    {
                        // Keep the paths of parts with the same names.
                        let old = std::mem::take(&mut b.parts);
                        b.parts = parts
                            .iter()
                            .map(|p| {
                                old.iter()
                                    .find(|o| o.name == *p)
                                    .cloned()
                                    .unwrap_or_else(|| asset::CompositePart {
                                        name: (*p).into(),
                                        id: asset::new_id(),
                                        groups: old
                                            .first()
                                            .map(|o| o.groups.clone())
                                            .unwrap_or_default(),
                                        ..Default::default()
                                    })
                            })
                            .collect();
                        b.composite = (*kind).into();
                        b.parameters = (*params).into();
                    }
                }
            });
    });
    let mut params = NamedParams {
        name: String::new(),
        params: misty_engine::input::actions::value::parse_params(&b.parameters),
    };
    let mut changed = false;
    match b.composite.as_str() {
        "2DVector" => {
            ui.horizontal(|ui| {
                label(ui, "Mode", "Digital Normalized: diagonals have length 1. Digital: (1, 1). Analog: part values as they are.");
                let cur = composites::VectorMode::parse(params.get("mode").unwrap_or(""));
                egui::ComboBox::from_id_salt("ia_comp_mode")
                    .selected_text(cur.name())
                    .show_ui(ui, |ui| {
                        for m in composites::VectorMode::ALL {
                            if ui.selectable_label(cur == m, m.name()).clicked() {
                                params.set("mode", m.name().into());
                                changed = true;
                            }
                        }
                    });
            });
        }
        "1DAxis" => {
            ui.horizontal(|ui| {
                label(ui, "Which Side Wins", "When both parts are held.");
                let cur =
                    composites::WhichSideWins::parse(params.get("whichSideWins").unwrap_or(""));
                egui::ComboBox::from_id_salt("ia_comp_side")
                    .selected_text(cur.name())
                    .show_ui(ui, |ui| {
                        for w in composites::WhichSideWins::ALL {
                            if ui.selectable_label(cur == w, w.name()).clicked() {
                                params.set("whichSideWins", w.name().into());
                                changed = true;
                            }
                        }
                    });
            });
            for (key, default) in [("minValue", -1.0), ("maxValue", 1.0)] {
                ui.horizontal(|ui| {
                    label(ui, &title_case(key), "Value range of the axis.");
                    let mut v = params.num(key, default);
                    if ui.add(egui::DragValue::new(&mut v).speed(0.05)).changed() {
                        params.set(key, trim_num(v));
                        changed = true;
                    }
                });
            }
        }
        _ => {}
    }
    if changed {
        b.parameters = params
            .params
            .iter()
            .map(|(k, v)| format!("{k}={v}"))
            .collect::<Vec<_>>()
            .join(",");
    }
}

/// `0.5`, `2` (no trailing zeros).
fn trim_num(v: f32) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_owned()
}

/// Path text field, the control picker and the Listen button.
fn path_row(ui: &mut Ui, s: &mut EditorState, path: &mut String, vector: bool) {
    ui.horizontal(|ui| {
        label(
            ui,
            "Path",
            "Control path, e.g. <Keyboard>/space or <Gamepad>/leftStick.",
        );
        let ok = path.trim().is_empty() || ControlPath::parse(path).is_ok();
        let edit = egui::TextEdit::singleline(path)
            .desired_width(ui.available_width() - 30.0)
            .text_color(if ok {
                theme::TEXT
            } else {
                egui::Color32::LIGHT_RED
            });
        ui.add(edit);
        ui.menu_button("▾", |ui| {
            for d in DeviceKind::ALL {
                ui.menu_button(d.name(), |ui| {
                    egui::ScrollArea::vertical()
                        .max_height(320.0)
                        .show(ui, |ui| {
                            for (p, name) in controls(d) {
                                if ui.selectable_label(*path == p, name).clicked() {
                                    *path = p;
                                    ui.close();
                                }
                            }
                        });
                });
            }
        });
    });
    if let Ok(c) = ControlPath::parse(path) {
        ui.horizontal(|ui| {
            label(ui, "", "");
            ui.weak(c.display());
        });
    }
    ui.horizontal(|ui| {
        label(ui, "", "");
        let w = &mut s.input_actions;
        let text = if w.listening {
            "Listening… (Esc cancels)"
        } else {
            "Listen"
        };
        let button = egui::Button::new(text).selected(w.listening);
        if ui
            .add(button)
            .on_hover_text("Press a key or a gamepad button (or move a stick) to bind it.")
            .clicked()
        {
            w.listening = !w.listening;
            w.listen_modifiers = ui.input(|i| i.modifiers);
        }
    });
    if s.input_actions.listening {
        if let Some(found) = listen(ui, s, vector) {
            if found != "<Keyboard>/escape" {
                *path = found;
            }
            s.input_actions.listening = false;
        }
        ui.ctx().request_repaint();
    }
}

/// The first key (or gamepad control) used this frame, as a path.
fn listen(ui: &mut Ui, s: &mut EditorState, vector: bool) -> Option<String> {
    let mut input = misty_engine::scripting::Input::default();
    let (keys, mods) = ui.input(|i| {
        let keys: Vec<String> = i
            .events
            .iter()
            .filter_map(|e| match e {
                egui::Event::Key {
                    key,
                    pressed: true,
                    repeat: false,
                    ..
                } => Some(crate::game_input::key_name(*key)),
                _ => None,
            })
            .collect();
        (keys, i.modifiers)
    });
    if keys.iter().any(|k| k == "escape") {
        return Some("<Keyboard>/escape".into());
    }
    // Modifiers are flags in egui: a newly held one counts as its key.
    let was = s.input_actions.listen_modifiers;
    s.input_actions.listen_modifiers = mods;
    for (now, before, key) in [
        (mods.shift, was.shift, "left shift"),
        (mods.ctrl, was.ctrl, "left ctrl"),
        (mods.alt, was.alt, "left alt"),
    ] {
        if now && !before && keys.is_empty() && !vector {
            return keyboard_path(key);
        }
    }
    input.pressed.extend(keys);
    #[cfg(not(test))]
    {
        input.joysticks = crate::game_input::gamepads();
    }
    misty_engine::input::actions::system::candidates(&input, vector)
        .into_iter()
        .next()
}

/// "Use in control scheme" checkboxes.
fn schemes_rows(ui: &mut Ui, schemes: &[ControlScheme], groups: &mut String) {
    if schemes.is_empty() {
        return;
    }
    section(ui, "Use in Control Scheme");
    for c in schemes {
        let g = c.group().to_owned();
        let mut on = has_group(groups, &g);
        if ui.checkbox(&mut on, &c.name).changed() {
            set_group(groups, &g, on);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Kinds {
    Interactions,
    Processors,
}

/// An interactions or processors list: one foldout per entry with its parameters, "+" to add.
fn params_list(ui: &mut Ui, title: &str, id: &str, text: &mut String, kinds: Kinds) {
    let mut list = parse_list(text);
    let mut changed = false;
    ui.add_space(6.0);
    ui.horizontal(|ui| {
        ui.label(theme::bold(ui.ctx(), title));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.add_space(10.0);
            ui.menu_button("+", |ui| {
                let names: Vec<(&str, &str)> = match kinds {
                    Kinds::Interactions => interactions::KINDS.to_vec(),
                    Kinds::Processors => {
                        processors::KINDS.iter().map(|(n, p, _)| (*n, *p)).collect()
                    }
                };
                for (name, defaults) in names {
                    if ui.button(title_case(name)).clicked() {
                        list.push(NamedParams {
                            name: name.into(),
                            params: misty_engine::input::actions::value::parse_params(defaults),
                        });
                        changed = true;
                        ui.close();
                    }
                }
            });
        });
    });
    ui.separator();
    if list.is_empty() {
        ui.label(
            RichText::new(format!("No {} have been added.", title.to_lowercase()))
                .color(theme::TEXT_DIM),
        );
    }
    let mut remove = None;
    for (i, entry) in list.iter_mut().enumerate() {
        egui::CollapsingHeader::new(title_case(&entry.name))
            .id_salt((id, i))
            .default_open(true)
            .show(ui, |ui| {
                for (k, v) in entry.params.iter_mut() {
                    ui.horizontal(|ui| {
                        label(ui, &title_case(k), "");
                        changed |= param_field(ui, (id, i, k.as_str()), k, v);
                    });
                }
                if ui.small_button("Remove").clicked() {
                    remove = Some(i);
                }
            });
    }
    if let Some(i) = remove {
        list.remove(i);
        changed = true;
    }
    if changed {
        *text = list_to_text(&list);
    }
}

/// One parameter: a combo for known enums, a checkbox for booleans, else a number.
fn param_field(
    ui: &mut Ui,
    id: impl std::hash::Hash + std::fmt::Debug,
    key: &str,
    v: &mut String,
) -> bool {
    use misty_engine::input::actions::interactions::PressBehavior;
    let mut changed = false;
    if key.eq_ignore_ascii_case("behavior") {
        let cur = PressBehavior::parse(v);
        egui::ComboBox::from_id_salt(id)
            .selected_text(cur.name())
            .show_ui(ui, |ui| {
                for b in PressBehavior::ALL {
                    if ui.selectable_label(cur == b, b.name()).clicked() {
                        *v = b.name().into();
                        changed = true;
                    }
                }
            });
        return changed;
    }
    if matches!(v.as_str(), "true" | "false") {
        let mut on = v == "true";
        if ui.checkbox(&mut on, "").changed() {
            *v = on.to_string();
            changed = true;
        }
        return changed;
    }
    let mut n: f32 = v.parse().unwrap_or(0.0);
    if ui
        .add(egui::DragValue::new(&mut n).speed(0.01).max_decimals(3))
        .changed()
    {
        *v = trim_num(n);
        changed = true;
    }
    changed
}

/// Add / Edit Control Scheme dialog; returns the edit while it stays open.
pub fn scheme_dialog(
    ctx: &egui::Context,
    a: &mut InputActionAsset,
    (index, mut scheme): (Option<usize>, ControlScheme),
) -> Option<(Option<usize>, ControlScheme)> {
    let mut keep = true;
    let mut done = false;
    egui::Window::new(if index.is_some() {
        "Edit Control Scheme"
    } else {
        "Add Control Scheme"
    })
    .collapsible(false)
    .resizable(false)
    .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
    .show(ctx, |ui| {
        ui.horizontal(|ui| {
            label(ui, "Scheme Name", "");
            ui.text_edit_singleline(&mut scheme.name);
        });
        ui.label(theme::bold(ui.ctx(), "Devices"));
        let mut remove = None;
        for (i, d) in scheme.devices.iter_mut().enumerate() {
            ui.horizontal(|ui| {
                ui.label(&d.path);
                ui.checkbox(&mut d.optional, "Optional");
                if ui.small_button("−").clicked() {
                    remove = Some(i);
                }
            });
        }
        if let Some(i) = remove {
            scheme.devices.remove(i);
        }
        ui.menu_button("+ Add Device", |ui| {
            for d in [
                DeviceKind::Keyboard,
                DeviceKind::Mouse,
                DeviceKind::Gamepad,
                DeviceKind::Touchscreen,
            ] {
                if ui.button(d.name()).clicked() {
                    scheme.devices.push(asset::DeviceRequirement {
                        path: d.layout(),
                        optional: false,
                    });
                    ui.close();
                }
            }
        });
        ui.separator();
        ui.horizontal(|ui| {
            let name_ok = !scheme.name.trim().is_empty()
                && !a.control_schemes.iter().enumerate().any(|(i, c)| {
                    Some(i) != index && c.name.eq_ignore_ascii_case(scheme.name.trim())
                });
            if ui.add_enabled(name_ok, egui::Button::new("Save")).clicked() {
                done = true;
            }
            if ui.button("Cancel").clicked() {
                keep = false;
            }
        });
    });
    if done {
        scheme.name = scheme.name.trim().to_owned();
        match index {
            Some(i) => {
                let old = a.control_schemes[i].group().to_owned();
                let new = scheme.group().to_owned();
                if old != new {
                    rename_group(a, &old, &new);
                }
                a.control_schemes[i] = scheme;
            }
            None => a.control_schemes.push(scheme),
        }
        return None;
    }
    keep.then_some((index, scheme))
}

fn rename_group(a: &mut InputActionAsset, old: &str, new: &str) {
    let fix = |g: &mut String| {
        if has_group(g, old) {
            set_group(g, old, false);
            set_group(g, new, true);
        }
    };
    for m in &mut a.maps {
        for act in &mut m.actions {
            for b in &mut act.bindings {
                fix(&mut b.groups);
                for p in &mut b.parts {
                    fix(&mut p.groups);
                }
            }
        }
    }
}
