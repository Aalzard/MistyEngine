//! Inspector of the Player Input component (actions asset, default map and control scheme,
//! behaviour, per-action event calls, and the live player while playing) and of Input Actions
//! assets (a summary with Edit Asset).

use egui::{Grid, RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::input::actions::{
    InputActionAsset, PlayerInput, PlayerInputBehavior, PlayerInputCall, player_input::message_name,
};

use crate::panels::input_actions::ICON;
use crate::state::EditorState;
use crate::theme;

/// Names of the components handled here.
pub const NAMES: [&str; 3] = ["Player Input", "On-Screen Stick", "On-Screen Button"];

pub fn icon(name: &str) -> Option<&'static str> {
    NAMES.contains(&name).then_some(ICON)
}

fn label(ui: &mut Ui, text: &str, tip: &str) {
    ui.add_sized([130.0, 18.0], egui::Label::new(text).truncate())
        .on_hover_text(tip);
}

fn load(state: &EditorState, rel: &str) -> Option<InputActionAsset> {
    InputActionAsset::load(&state.assets.absolute(rel)).ok()
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, name: &str) -> bool {
    match name {
        "On-Screen Stick" => return on_screen_stick(ui, state, e),
        "On-Screen Button" => return on_screen_button(ui, state, e),
        _ => {}
    }
    let Ok(mut c) = state
        .scene
        .world
        .get::<&PlayerInput>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = c.clone();
    let assets: Vec<String> = state.assets.input_actions.iter().cloned().collect();
    let asset = c.actions.as_deref().and_then(|p| load(state, p));
    ui.horizontal(|ui| {
        label(ui, "Actions", "The Input Actions asset this player uses.");
        let shown = c.actions.as_deref().map_or(
            "None (Input Actions)".to_owned(),
            misty_engine::scene::clip_name,
        );
        egui::ComboBox::from_id_salt(("pi_actions", e))
            .selected_text(shown)
            .width(180.0)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut c.actions, None, "None (Input Actions)");
                for a in &assets {
                    ui.selectable_value(
                        &mut c.actions,
                        Some(a.clone()),
                        misty_engine::scene::clip_name(a),
                    );
                }
            });
        if let Some(p) = c.actions.clone()
            && ui
                .small_button("Edit")
                .on_hover_text("Open in the Input Actions window")
                .clicked()
        {
            crate::panels::input_actions::open(state, &p);
        }
    });
    let Some(asset) = asset else {
        if c.actions.is_some() {
            ui.colored_label(crate::theme::error(), "The Actions asset can't be read.");
        } else {
            ui.weak("Assign an Input Actions asset (Assets ▸ Create ▸ Input Actions).");
        }
        return store(state, e, c, &before);
    };
    ui.horizontal(|ui| {
        label(
            ui,
            "Default Scheme",
            "Control scheme to start with; <Any> picks the first whose devices are there.",
        );
        let shown = if c.default_control_scheme.is_empty() {
            "<Any>".to_owned()
        } else {
            c.default_control_scheme.clone()
        };
        egui::ComboBox::from_id_salt(("pi_scheme", e))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut c.default_control_scheme, String::new(), "<Any>");
                for s in &asset.control_schemes {
                    ui.selectable_value(&mut c.default_control_scheme, s.name.clone(), &s.name);
                }
            });
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Auto-Switch",
            "Switch control scheme when another device is used (single player).",
        );
        ui.checkbox(&mut c.auto_switch, "");
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Default Map",
            "Action map enabled when the player starts.",
        );
        let shown = if c.default_map.is_empty() {
            asset
                .maps
                .first()
                .map_or("<None>".to_owned(), |m| m.name.clone())
        } else {
            c.default_map.clone()
        };
        egui::ComboBox::from_id_salt(("pi_map", e))
            .selected_text(shown)
            .show_ui(ui, |ui| {
                for m in &asset.maps {
                    ui.selectable_value(&mut c.default_map, m.name.clone(), &m.name);
                }
            });
    });
    ui.horizontal(|ui| {
        label(ui, "Behavior", "How actions reach the scripts.");
        egui::ComboBox::from_id_salt(("pi_behavior", e))
            .selected_text(c.behavior.label())
            .show_ui(ui, |ui| {
                for b in PlayerInputBehavior::ALL {
                    ui.selectable_value(&mut c.behavior, b, b.label());
                }
            });
    });
    behavior_help(ui, &c, &asset);
    if c.behavior == PlayerInputBehavior::InvokeEvents {
        events(ui, state, e, &mut c, &asset);
    }
    live(ui, state, e);
    store(state, e, c, &before)
}

fn store(state: &mut EditorState, e: Entity, c: PlayerInput, before: &PlayerInput) -> bool {
    if c == *before {
        return false;
    }
    let _ = state.scene.world.insert_one(e, c);
    true
}

/// What the scripts receive, like the reference editor's help box.
fn behavior_help(ui: &mut Ui, c: &PlayerInput, asset: &InputActionAsset) {
    let text = match c.behavior {
        PlayerInputBehavior::SendMessages | PlayerInputBehavior::BroadcastMessages => {
            let map = asset
                .find_map(&c.default_map)
                .or((!asset.maps.is_empty()).then_some(0));
            let names: Vec<String> = map
                .map(|m| {
                    asset.maps[m]
                        .actions
                        .iter()
                        .map(|a| format!("{}(value)", message_name(&a.name)))
                        .collect()
                })
                .unwrap_or_default();
            let target = if c.behavior == PlayerInputBehavior::SendMessages {
                "this object's scripts"
            } else {
                "the scripts of this object and its children"
            };
            format!(
                "Calls on {target}: {}, OnDeviceLost, OnDeviceRegained, OnControlsChanged.",
                names.join(", ")
            )
        }
        PlayerInputBehavior::InvokeEvents => {
            "Calls the methods below with a callback context (started, performed and canceled), \
             and onActionTriggered listeners."
                .to_owned()
        }
    };
    egui::Frame::group(ui.style()).show(ui, |ui| {
        ui.label(RichText::new(text).color(theme::text_dim()));
    });
}

/// Per-action event calls, grouped by map.
fn events(
    ui: &mut Ui,
    state: &EditorState,
    e: Entity,
    c: &mut PlayerInput,
    asset: &InputActionAsset,
) {
    let objs: Vec<(Entity, String)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| (o, state.scene.name(o)))
        .collect();
    egui::CollapsingHeader::new("Events")
        .id_salt(("pi_events", e))
        .default_open(true)
        .show(ui, |ui| {
            for (mi, map) in asset.maps.iter().enumerate() {
                egui::CollapsingHeader::new(&map.name)
                    .id_salt(("pi_map_events", e, &map.name))
                    .default_open(mi == 0)
                    .show(ui, |ui| {
                        for a in &map.actions {
                            let full = format!("{}/{}", map.name, a.name);
                            ui.label(format!("{} (CallbackContext)", a.name));
                            egui::Frame::group(ui.style()).show(ui, |ui| {
                                let mut remove = None;
                                let mut any = false;
                                for (i, call) in c.events.iter_mut().enumerate() {
                                    if call.action != full {
                                        continue;
                                    }
                                    any = true;
                                    ui.horizontal(|ui| {
                                        let shown = call
                                            .target
                                            .and_then(|t| objs.iter().find(|(o, _)| *o == t))
                                            .map_or("None (Object)".to_owned(), |(_, n)| n.clone());
                                        egui::ComboBox::from_id_salt(("pi_call", e, i))
                                            .selected_text(shown)
                                            .width(110.0)
                                            .show_ui(ui, |ui| {
                                                ui.selectable_value(
                                                    &mut call.target,
                                                    None,
                                                    "None (Object)",
                                                );
                                                for (o, n) in &objs {
                                                    ui.selectable_value(
                                                        &mut call.target,
                                                        Some(*o),
                                                        n,
                                                    );
                                                }
                                            });
                                        ui.add(
                                            egui::TextEdit::singleline(&mut call.method)
                                                .hint_text("Method")
                                                .desired_width(100.0),
                                        );
                                        if ui.small_button("−").on_hover_text("Remove").clicked()
                                        {
                                            remove = Some(i);
                                        }
                                    });
                                }
                                if !any {
                                    ui.label(
                                        RichText::new("List is Empty").color(theme::text_dim()),
                                    );
                                }
                                if let Some(i) = remove {
                                    c.events.remove(i);
                                }
                                if ui.small_button("+").on_hover_text("Add a call").clicked() {
                                    c.events.push(PlayerInputCall {
                                        action: full.clone(),
                                        target: Some(e),
                                        method: message_name(&a.name),
                                    });
                                }
                            });
                        }
                    });
            }
        });
}

/// While playing: the player's index, control scheme, devices and map.
fn live(ui: &mut Ui, state: &EditorState, e: Entity) {
    let Some(rt) = state.runtime.as_ref() else {
        return;
    };
    let Some(info) = rt.player_input_info(e) else {
        return;
    };
    ui.add_space(4.0);
    egui::CollapsingHeader::new("Debug")
        .id_salt(("pi_debug", e))
        .default_open(true)
        .show(ui, |ui| {
            Grid::new(("pi_debug_grid", e))
                .num_columns(2)
                .show(ui, |ui| {
                    for (k, v) in info {
                        ui.label(k);
                        ui.label(v);
                        ui.end_row();
                    }
                });
        });
}

/// Inspector of an Input Actions asset.
pub fn actions_asset(ui: &mut Ui, s: &mut EditorState, rel: &str) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(ICON).size(22.0));
        ui.label(theme::bold(
            ui.ctx(),
            format!("{} (Input Actions)", misty_engine::scene::clip_name(rel)),
        ));
    });
    crate::asset_guids::path_label(ui, rel);
    ui.separator();
    let Some(a) = load(s, rel) else {
        ui.colored_label(crate::theme::error(), "This asset can't be read.");
        return;
    };
    Grid::new("ia_summary").num_columns(2).show(ui, |ui| {
        ui.label("Action Maps");
        ui.label(
            a.maps
                .iter()
                .map(|m| format!("{} ({})", m.name, m.actions.len()))
                .collect::<Vec<_>>()
                .join(", "),
        );
        ui.end_row();
        ui.label("Control Schemes");
        ui.label(
            a.control_schemes
                .iter()
                .map(|c| c.name.clone())
                .collect::<Vec<_>>()
                .join(", "),
        );
        ui.end_row();
    });
    ui.add_space(6.0);
    if ui.button("Edit Asset").clicked() {
        crate::panels::input_actions::open(s, rel);
    }
}

/// Control paths offered by the on-screen control editors (any other path can be typed).
const STICK_PATHS: [&str; 3] = [
    "<Gamepad>/leftStick",
    "<Gamepad>/rightStick",
    "<Gamepad>/dpad",
];
const BUTTON_PATHS: [&str; 12] = [
    "<Gamepad>/buttonSouth",
    "<Gamepad>/buttonEast",
    "<Gamepad>/buttonWest",
    "<Gamepad>/buttonNorth",
    "<Gamepad>/leftShoulder",
    "<Gamepad>/rightShoulder",
    "<Gamepad>/leftTrigger",
    "<Gamepad>/rightTrigger",
    "<Gamepad>/start",
    "<Gamepad>/select",
    "<Keyboard>/space",
    "<Keyboard>/escape",
];

/// A control path field with a menu of common paths, and the path's problem when it has one.
fn control_path(ui: &mut Ui, e: Entity, path: &mut String, common: &[&str]) {
    ui.horizontal(|ui| {
        label(
            ui,
            "Control Path",
            "The gamepad or keyboard control this drives, e.g. <Gamepad>/leftStick or \
             <Keyboard>/space. Scripts and Input Actions read it like a real gamepad named \
             \"On-Screen Controls\".",
        );
        ui.add(egui::TextEdit::singleline(path).desired_width(150.0));
        egui::ComboBox::from_id_salt(("osc_path", e))
            .selected_text("")
            .width(20.0)
            .show_ui(ui, |ui| {
                for p in common {
                    if ui.selectable_label(path == p, *p).clicked() {
                        *path = (*p).to_owned();
                    }
                }
            });
    });
    if let Err(err) = misty_engine::input::on_screen::target(path) {
        ui.colored_label(theme::shade(theme::AXIS_X), err);
    }
}

fn on_screen_stick(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    use misty_engine::input::OnScreenStick;
    let Ok(mut c) = state
        .scene
        .world
        .get::<&OnScreenStick>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = c.clone();
    ui.horizontal(|ui| {
        label(
            ui,
            "Enabled",
            "Off: touches pass through and the control rests.",
        );
        ui.checkbox(&mut c.enabled, "");
    });
    control_path(ui, e, &mut c.control_path, &STICK_PATHS);
    ui.horizontal(|ui| {
        label(
            ui,
            "Movement Range",
            "How far (canvas units) the stick moves from where it rests; full tilt there.",
        );
        ui.add(
            egui::DragValue::new(&mut c.movement_range)
                .range(1.0..=2000.0)
                .speed(1.0),
        );
    });
    ui.horizontal(|ui| {
        label(
            ui,
            "Dynamic Origin",
            "The stick first jumps to where the finger lands inside this object, so a large \
             (even invisible) area can hold a floating stick.",
        );
        ui.checkbox(&mut c.dynamic_origin, "");
    });
    ui.weak("Needs a graphic (Image) to catch touches. The left mouse button works too.");
    if c == before {
        return false;
    }
    let _ = state.scene.world.insert_one(e, c);
    true
}

fn on_screen_button(ui: &mut Ui, state: &mut EditorState, e: Entity) -> bool {
    use misty_engine::input::OnScreenButton;
    let Ok(mut c) = state
        .scene
        .world
        .get::<&OnScreenButton>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = c.clone();
    ui.horizontal(|ui| {
        label(
            ui,
            "Enabled",
            "Off: touches pass through and the control rests.",
        );
        ui.checkbox(&mut c.enabled, "");
    });
    control_path(ui, e, &mut c.control_path, &BUTTON_PATHS);
    ui.weak("Needs a graphic (Image) to catch touches. The left mouse button works too.");
    if c == before {
        return false;
    }
    let _ = state.scene.world.insert_one(e, c);
    true
}
