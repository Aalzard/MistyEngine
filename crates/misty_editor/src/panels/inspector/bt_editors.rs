//! Properties editor for the Behaviour Tree Agent component (the optional Behaviour Trees
//! package): the tree asset, tick rate, start and restart, share group, seed and blackboard
//! overrides (an Object key picks an object of the scene).

use egui::{RichText, Ui};
use misty_engine::behaviour_tree::blackboard::{BbValue, KeyType};
use misty_engine::behaviour_tree::component::BlackboardOverride;
use misty_engine::behaviour_tree::{BehaviourTree, BehaviourTreeAgent};
use misty_engine::hecs::Entity;

use crate::i18n::tr;
use crate::state::EditorState;

pub const NAMES: [&str; 1] = ["Behaviour Tree Agent"];

/// Whether Add Component offers `name` in this project (optional packages' components appear
/// once the package is installed).
pub fn offered(name: &str) -> bool {
    !NAMES.contains(&name)
        || crate::panels::behaviour_tree::package_installed()
}

pub fn icon(name: &str) -> Option<&'static str> {
    NAMES
        .contains(&name)
        .then_some(crate::panels::behaviour_tree::ICON)
}

pub fn enabled_flag(state: &EditorState, e: Entity, _name: &str) -> Option<bool> {
    state
        .scene
        .world
        .get::<&BehaviourTreeAgent>(e)
        .ok()
        .map(|c| c.enabled)
}

pub fn set_enabled(state: &mut EditorState, e: Entity, _name: &str, on: bool) {
    let _ = state
        .scene
        .world
        .get::<&mut BehaviourTreeAgent>(e)
        .map(|mut c| c.enabled = on);
}

pub fn edit(ui: &mut Ui, state: &mut EditorState, e: Entity, _name: &str) -> bool {
    let Ok(mut c) = state
        .scene
        .world
        .get::<&BehaviourTreeAgent>(e)
        .map(|c| (*c).clone())
    else {
        return false;
    };
    let before = c.clone();
    let root = state.project.root.clone();
    let tree = c
        .tree
        .as_deref()
        .and_then(|p| BehaviourTree::load(&root.join(p)).ok());
    let objects: Vec<(String, Entity)> = state
        .scene
        .walk()
        .into_iter()
        .map(|(o, _)| (state.scene.name(o), o))
        .collect();
    let mut open = false;
    egui::Grid::new(("bt_agent", e))
        .num_columns(2)
        .spacing([8.0, 4.0])
        .show(ui, |ui| {
            ui.label(tr!("Tree"));
            ui.horizontal(|ui| {
                let shown = c
                    .tree
                    .as_deref()
                    .map(misty_engine::scene::clip_name)
                    .unwrap_or_else(|| tr!("(none)"));
                ui.menu_button(format!("{shown} ▾"), |ui| {
                    if ui.button(tr!("(none)")).clicked() {
                        c.tree = None;
                        ui.close();
                    }
                    for f in crate::panels::behaviour_tree::assets_with(&root, "btree") {
                        if ui.button(&f).clicked() {
                            c.tree = Some(f);
                            ui.close();
                        }
                    }
                });
                if c.tree.is_some() && ui.small_button(tr!("Open")).clicked() {
                    open = true;
                }
            });
            ui.end_row();
            ui.label(tr!("Tick Rate"));
            ui.add(
                egui::DragValue::new(&mut c.tick_rate)
                    .range(0.0..=240.0)
                    .speed(0.5)
                    .suffix(tr!(" /s")),
            )
            .on_hover_text(tr!("Ticks per second (0 = every frame)"));
            ui.end_row();
            ui.label(tr!("Start On Play"));
            ui.checkbox(&mut c.start_on_play, "");
            ui.end_row();
            ui.label(tr!("Restart When Done"));
            ui.checkbox(&mut c.restart_when_done, "");
            ui.end_row();
            ui.label(tr!("Share Group"));
            ui.add(
                egui::TextEdit::singleline(&mut c.share_group)
                    .hint_text(tr!("the tree's path"))
                    .desired_width(150.0),
            )
            .on_hover_text(tr!("Agents of one group share the tree's shared keys"));
            ui.end_row();
            ui.label(tr!("Seed"));
            ui.add(egui::DragValue::new(&mut c.seed))
                .on_hover_text(tr!("Random choices repeat with the same seed (0 = from the name)"));
            ui.end_row();
        });
    // Overrides.
    ui.add_space(4.0);
    ui.label(RichText::new(tr!("Blackboard Overrides")).strong());
    match &tree {
        None => {
            ui.weak(tr!("Choose a tree to override its keys."));
        }
        Some(t) => {
            let mut remove = None;
            for (i, o) in c.overrides.iter_mut().enumerate() {
                let ty = t.key(&o.key).map(|k| k.ty);
                ui.horizontal(|ui| {
                    ui.label(&o.key);
                    match ty {
                        None => {
                            ui.weak(tr!("(not in the tree)"));
                        }
                        Some(KeyType::Object) => {
                            let shown = o
                                .object
                                .filter(|x| objects.iter().any(|(_, e)| e == x))
                                .map(|x| state_name(&objects, x))
                                .unwrap_or_else(|| tr!("(none)"));
                            egui::ComboBox::from_id_salt(("bt_ov_obj", e, i))
                                .selected_text(crate::i18n::display(&shown))
                                .width(130.0)
                                .show_ui(ui, |ui| {
                                    if ui.selectable_label(o.object.is_none(), tr!("(none)")).clicked() {
                                        o.object = None;
                                    }
                                    for (name, x) in &objects {
                                        if ui
                                            .selectable_label(o.object == Some(*x), crate::i18n::display(name))
                                            .clicked()
                                        {
                                            o.object = Some(*x);
                                        }
                                    }
                                });
                        }
                        Some(ty) => {
                            crate::panels::behaviour_tree::value_field(ui, &mut o.value, ty);
                        }
                    }
                    if ui.small_button("✖").clicked() {
                        remove = Some(i);
                    }
                });
            }
            if let Some(i) = remove {
                c.overrides.remove(i);
            }
            let free: Vec<_> = t
                .keys
                .iter()
                .filter(|k| !c.overrides.iter().any(|o| o.key == k.name))
                .collect();
            if !free.is_empty() {
                ui.menu_button(tr!("＋ Override"), |ui| {
                    for k in free {
                        if ui.button(format!("{} ({})", k.name, tr(k.ty.label()))).clicked() {
                            c.overrides.push(BlackboardOverride {
                                key: k.name.clone(),
                                value: if k.ty == KeyType::Object {
                                    BbValue::Bool(false)
                                } else {
                                    k.initial()
                                },
                                object: None,
                            });
                            ui.close();
                        }
                    }
                });
            }
        }
    }
    if open && let Some(p) = c.tree.clone() {
        crate::panels::behaviour_tree::open(state, &p);
    }
    let changed = c != before;
    if changed {
        let _ = state.scene.world.insert_one(e, c);
    }
    changed
}

fn state_name(objects: &[(String, Entity)], e: Entity) -> String {
    objects
        .iter()
        .find(|(_, x)| *x == e)
        .map(|(n, _)| n.clone())
        .unwrap_or_default()
}
