//! The Node panel: the selected node's name and settings (or the selected comment or group).

use std::collections::BTreeMap;
use std::path::Path;

use egui::{Color32, Rect, RichText, Ui, UiBuilder};
use misty_engine::behaviour_tree::BtNode;
use misty_engine::behaviour_tree::asset::{AbortMode, LogLevel, NodeKind, ParallelPolicy};
use misty_engine::behaviour_tree::blackboard::{BbValue, CompareOp, KeyType};

use super::interacting;
use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;

pub fn height(s: &EditorState) -> f32 {
    let w = &s.behaviour_tree;
    if w.canvas.comment.is_some() || w.canvas.group.is_some() {
        return 190.0;
    }
    match w.selected.len() {
        1 => 380.0,
        _ => 90.0,
    }
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 6.0, Color32::from_rgba_unmultiplied(40, 41, 45, 240));
    let busy = interacting(ui.ctx());
    let root = s.project.root.clone();
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
        egui::ScrollArea::vertical()
            .id_salt("bt_inspector")
            .auto_shrink([false, false])
            .show(ui, |ui| {
                if let Some(ci) = s.behaviour_tree.canvas.comment {
                    comment(ui, s, ci, busy);
                    return;
                }
                if let Some(gi) = s.behaviour_tree.canvas.group {
                    group(ui, s, gi, busy);
                    return;
                }
                let w = &mut s.behaviour_tree;
                if w.selected.len() != 1 {
                    ui.label(RichText::new(tr!("Node")).strong());
                    let text = if w.selected.is_empty() {
                        tr!("Select a node to edit its settings.")
                    } else {
                        tr!("{count} nodes selected", count = w.selected.len())
                    };
                    ui.weak(text);
                    return;
                }
                let id = *w.selected.iter().next().expect("one");
                let Some(node) = w.tree.node(id).cloned() else {
                    return;
                };
                let keys: Vec<(String, KeyType)> =
                    w.tree.keys.iter().map(|k| (k.name.clone(), k.ty)).collect();
                let mut copy = node.clone();
                ui.label(RichText::new(tr(node.kind.label())).strong());
                ui.weak(tr(node.kind.help()));
                ui.add_space(4.0);
                egui::Grid::new("bt_node_grid")
                    .num_columns(2)
                    .spacing([8.0, 4.0])
                    .show(ui, |ui| {
                        ui.label(tr!("Name"));
                        ui.add(
                            egui::TextEdit::singleline(&mut copy.name)
                                .hint_text(tr(node.kind.label()))
                                .desired_width(150.0),
                        );
                        ui.end_row();
                        settings(ui, &mut copy, &keys, &root);
                    });
                if let Some(e) = w.error_of(id) {
                    ui.add_space(4.0);
                    ui.label(RichText::new(format!("⚠ {e}")).color(crate::panels::node_canvas::ERROR));
                }
                if let Some((now, last, _)) = w.live_of(id) {
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(tr!(
                            "Now: {now} · Last: {last}",
                            now = tr(now.label()),
                            last = tr(last.label())
                        ))
                        .color(theme::TEXT_DIM),
                    );
                }
                if copy != node {
                    w.edit("Edit Node", busy, |t| {
                        if let Some(n) = t.node_mut(id) {
                            *n = copy;
                        }
                    });
                }
            });
    });
}

fn comment(ui: &mut Ui, s: &mut EditorState, ci: usize, busy: bool) {
    let w = &mut s.behaviour_tree;
    let Some(c) = w.tree.comments.get(ci).cloned() else {
        w.canvas.comment = None;
        return;
    };
    ui.label(RichText::new(tr!("Comment")).strong());
    let mut copy = c.clone();
    ui.add(
        egui::TextEdit::multiline(&mut copy.text)
            .desired_rows(3)
            .desired_width(f32::INFINITY),
    );
    ui.horizontal(|ui| {
        ui.label(tr!("Size"));
        ui.add(egui::DragValue::new(&mut copy.size[0]).range(60.0..=2000.0));
        ui.add(egui::DragValue::new(&mut copy.size[1]).range(30.0..=2000.0));
    });
    if copy != c {
        w.edit("Edit Comment", busy, |t| {
            if let Some(x) = t.comments.get_mut(ci) {
                *x = copy;
            }
        });
    }
    if ui.button(tr!("Delete Comment")).clicked() {
        w.edit("Delete Comment", false, |t| {
            if ci < t.comments.len() {
                t.comments.remove(ci);
            }
        });
        w.canvas.comment = None;
    }
}

fn group(ui: &mut Ui, s: &mut EditorState, gi: usize, busy: bool) {
    let w = &mut s.behaviour_tree;
    let Some(g) = w.tree.groups.get(gi).cloned() else {
        w.canvas.group = None;
        return;
    };
    ui.label(RichText::new(tr!("Group")).strong());
    let mut title = g.title.clone();
    ui.horizontal(|ui| {
        ui.label(tr!("Title"));
        ui.text_edit_singleline(&mut title);
    });
    if title != g.title {
        w.edit("Rename Group", busy, |t| {
            if let Some(x) = t.groups.get_mut(gi) {
                x.title = title;
            }
        });
    }
    ui.weak(tr!("{count} nodes", count = g.nodes.len()));
    if ui.button(tr!("Ungroup")).clicked() {
        w.edit("Ungroup", false, |t| {
            if gi < t.groups.len() {
                t.groups.remove(gi);
            }
        });
        w.canvas.group = None;
    }
}

/// A key picker limited to `types`.
fn key_field(ui: &mut Ui, label: &str, key: &mut String, keys: &[(String, KeyType)], types: &[KeyType], optional: bool) {
    ui.label(label);
    let shown = if key.is_empty() {
        tr!("(none)")
    } else {
        key.clone()
    };
    egui::ComboBox::from_id_salt(("bt_key", label))
        .selected_text(shown)
        .width(150.0)
        .show_ui(ui, |ui| {
            if optional {
                ui.selectable_value(key, String::new(), tr!("(none)"));
            }
            for (name, ty) in keys.iter().filter(|(_, t)| types.contains(t)) {
                ui.selectable_value(key, name.clone(), format!("{name} ({})", tr(ty.label())));
            }
        });
    ui.end_row();
}

fn number(ui: &mut Ui, label: &str, v: &mut f32, min: f32, max: f32, suffix: &str) {
    ui.label(label);
    ui.add(
        egui::DragValue::new(v)
            .speed(0.05)
            .range(min..=max)
            .suffix(suffix),
    );
    ui.end_row();
}

fn text(ui: &mut Ui, label: &str, v: &mut String) {
    ui.label(label);
    ui.add(egui::TextEdit::singleline(v).desired_width(150.0));
    ui.end_row();
}

/// A value of `ty` (a literal for a key of that type).
pub fn value_field(ui: &mut Ui, v: &mut BbValue, ty: KeyType) {
    if v.key_type() != ty {
        *v = v.convert(ty).unwrap_or_else(|| ty.zero());
    }
    match v {
        BbValue::Bool(b) => {
            ui.checkbox(b, "");
        }
        BbValue::Int(i) => {
            ui.add(egui::DragValue::new(i));
        }
        BbValue::Float(f) => {
            ui.add(egui::DragValue::new(f).speed(0.05));
        }
        BbValue::Vector(p) => {
            ui.horizontal(|ui| {
                ui.add(egui::DragValue::new(&mut p.x).speed(0.1).prefix("x "));
                ui.add(egui::DragValue::new(&mut p.y).speed(0.1).prefix("y "));
                ui.add(egui::DragValue::new(&mut p.z).speed(0.1).prefix("z "));
            });
        }
        BbValue::Object(_) => {
            ui.weak(tr!("set by overrides or scripts"));
        }
        BbValue::String(s) => {
            ui.add(egui::TextEdit::singleline(s).desired_width(140.0));
        }
    }
}

/// Files under Assets with extension `ext` (project-relative).
pub fn assets_with(root: &Path, ext: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("Assets")];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p
                .extension()
                .is_some_and(|x| x.to_string_lossy().eq_ignore_ascii_case(ext))
            {
                out.push(
                    p.strip_prefix(root)
                        .unwrap_or(&p)
                        .to_string_lossy()
                        .replace('\\', "/"),
                );
            }
        }
    }
    out.sort();
    out
}

/// A path field with a menu of the project's files of `ext`.
fn asset_field(ui: &mut Ui, label: &str, v: &mut String, root: &Path, ext: &str) {
    ui.label(label);
    ui.horizontal(|ui| {
        ui.add(egui::TextEdit::singleline(v).desired_width(120.0));
        ui.menu_button("▾", |ui| {
            if ui.button(tr!("(none)")).clicked() {
                v.clear();
                ui.close();
            }
            for f in assets_with(root, ext) {
                if ui.button(&f).clicked() {
                    *v = f;
                    ui.close();
                }
            }
        });
    });
    ui.end_row();
}

const TARGET: &[KeyType] = &[KeyType::Vector, KeyType::Object];

fn settings(ui: &mut Ui, n: &mut BtNode, keys: &[(String, KeyType)], root: &Path) {
    let key_type = |k: &str| keys.iter().find(|(n, _)| n == k).map(|(_, t)| *t);
    match &mut n.kind {
        NodeKind::Parallel { policy } => {
            ui.label(tr!("Policy"));
            egui::ComboBox::from_id_salt("bt_policy")
                .selected_text(tr(policy.label()))
                .show_ui(ui, |ui| {
                    for p in ParallelPolicy::ALL {
                        ui.selectable_value(policy, p, tr(p.label()));
                    }
                });
            ui.end_row();
        }
        NodeKind::Repeat { count, until_fail } => {
            ui.label(tr!("Until it fails"));
            ui.checkbox(until_fail, "");
            ui.end_row();
            if !*until_fail {
                ui.label(tr!("Times (0 = forever)"));
                ui.add(egui::DragValue::new(count).range(0..=100_000));
                ui.end_row();
            }
        }
        NodeKind::Cooldown { seconds } | NodeKind::TimeLimit { seconds } => {
            number(ui, &tr!("Seconds"), seconds, 0.0, 3600.0, " s");
        }
        NodeKind::Conditional {
            key,
            op,
            value,
            abort,
        } => {
            key_field(ui, &tr!("Key"), key, keys, &KeyType::ALL, false);
            ui.label(tr!("Condition"));
            egui::ComboBox::from_id_salt("bt_op")
                .selected_text(tr(op.label()))
                .show_ui(ui, |ui| {
                    for o in CompareOp::ALL {
                        ui.selectable_value(op, o, tr(o.label()));
                    }
                });
            ui.end_row();
            if op.needs_value() {
                ui.label(tr!("Value"));
                let ty = key_type(key).unwrap_or(value.key_type());
                value_field(ui, value, ty);
                ui.end_row();
            }
            ui.label(tr!("Abort"));
            egui::ComboBox::from_id_salt("bt_abort")
                .selected_text(tr(abort.label()))
                .show_ui(ui, |ui| {
                    for a in AbortMode::ALL {
                        ui.selectable_value(abort, a, tr(a.label()));
                    }
                });
            ui.end_row();
        }
        NodeKind::MoveTo { target, acceptance } => {
            key_field(ui, &tr!("Target"), target, keys, TARGET, false);
            number(ui, &tr!("Stop within"), acceptance, 0.01, 100.0, " m");
        }
        NodeKind::Wait { seconds, random } => {
            number(ui, &tr!("Seconds"), seconds, 0.0, 3600.0, " s");
            number(ui, &tr!("Random ±"), random, 0.0, 3600.0, " s");
        }
        NodeKind::PlayAnimation { state, crossfade } => {
            text(ui, &tr!("State"), state);
            number(ui, &tr!("Cross-fade"), crossfade, 0.0, 10.0, " s");
        }
        NodeKind::SetAnimatorParameter {
            parameter,
            value,
            from_key,
        } => {
            text(ui, &tr!("Parameter"), parameter);
            key_field(ui, &tr!("From Key"), from_key, keys, &[KeyType::Bool, KeyType::Int, KeyType::Float], true);
            if from_key.is_empty() {
                ui.label(tr!("Value"));
                let ty = match value {
                    BbValue::Bool(_) => KeyType::Bool,
                    BbValue::Int(_) => KeyType::Int,
                    _ => KeyType::Float,
                };
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt("bt_param_ty")
                        .selected_text(tr(ty.label()))
                        .width(60.0)
                        .show_ui(ui, |ui| {
                            for t in [KeyType::Bool, KeyType::Int, KeyType::Float] {
                                if ui.selectable_label(ty == t, tr(t.label())).clicked() {
                                    *value = value.convert(t).unwrap_or_else(|| t.zero());
                                }
                            }
                        });
                    let ty = value.key_type();
                    value_field(ui, value, ty);
                });
                ui.end_row();
            }
        }
        NodeKind::LookAt { target, turn_speed } => {
            key_field(ui, &tr!("Target"), target, keys, TARGET, false);
            number(ui, &tr!("Turn speed (0 = at once)"), turn_speed, 0.0, 3600.0, " °/s");
        }
        NodeKind::SetBlackboard {
            key,
            value,
            from_key,
        } => {
            key_field(ui, &tr!("Key"), key, keys, &KeyType::ALL, false);
            key_field(ui, &tr!("From Key"), from_key, keys, &KeyType::ALL, true);
            if from_key.is_empty() {
                ui.label(tr!("Value"));
                let ty = key_type(key).unwrap_or(value.key_type());
                value_field(ui, value, ty);
                ui.end_row();
            }
        }
        NodeKind::IsInRange { target, range } => {
            key_field(ui, &tr!("Target"), target, keys, TARGET, false);
            number(ui, &tr!("Range"), range, 0.0, 10_000.0, " m");
        }
        NodeKind::CanSeeTarget {
            target,
            range,
            fov,
            eye_height,
            store,
        } => {
            key_field(ui, &tr!("Target"), target, keys, TARGET, false);
            number(ui, &tr!("Range"), range, 0.0, 10_000.0, " m");
            number(ui, &tr!("Field of view"), fov, 0.0, 360.0, "°");
            number(ui, &tr!("Eye height"), eye_height, -10.0, 100.0, " m");
            key_field(ui, &tr!("Store in"), store, keys, &[KeyType::Bool], true);
        }
        NodeKind::HasLineOfSight { target, eye_height } => {
            key_field(ui, &tr!("Target"), target, keys, TARGET, false);
            number(ui, &tr!("Eye height"), eye_height, -10.0, 100.0, " m");
        }
        NodeKind::RunScriptGraph { graph, event, wait } => {
            asset_field(ui, &tr!("Script Graph"), graph, root, "vgraph");
            text(ui, &tr!("Event"), event);
            ui.label(tr!("Wait for Finish"));
            ui.checkbox(wait, "").on_hover_text(tr!(
                "Keep running until the graph calls Behaviour Tree Finish"
            ));
            ui.end_row();
        }
        NodeKind::RunLua { function } => {
            text(ui, &tr!("Function"), function);
        }
        NodeKind::CustomLua { script, params } => {
            asset_field(ui, &tr!("Script"), script, root, "lua");
            params_field(ui, params);
        }
        NodeKind::Log { message, level } => {
            text(ui, &tr!("Message"), message);
            ui.label(tr!("Level"));
            egui::ComboBox::from_id_salt("bt_log")
                .selected_text(tr(level.label()))
                .show_ui(ui, |ui| {
                    for l in LogLevel::ALL {
                        ui.selectable_value(level, l, tr(l.label()));
                    }
                });
            ui.end_row();
        }
        NodeKind::Root
        | NodeKind::Sequence
        | NodeKind::Selector
        | NodeKind::RandomSelector
        | NodeKind::Inverter
        | NodeKind::ForceSuccess
        | NodeKind::ForceFailure => {}
    }
}

/// A custom leaf's settings (name → text).
fn params_field(ui: &mut Ui, params: &mut BTreeMap<String, String>) {
    let names: Vec<String> = params.keys().cloned().collect();
    let mut remove = None;
    for name in names {
        ui.label(&name);
        ui.horizontal(|ui| {
            if let Some(v) = params.get_mut(&name) {
                ui.add(egui::TextEdit::singleline(v).desired_width(110.0));
            }
            if ui.small_button("✖").clicked() {
                remove = Some(name.clone());
            }
        });
        ui.end_row();
    }
    if let Some(r) = remove {
        params.remove(&r);
    }
    ui.label("");
    if ui.small_button(tr!("Add Setting")).clicked() {
        let mut k = 1;
        while params.contains_key(&format!("setting{k}")) {
            k += 1;
        }
        params.insert(format!("setting{k}"), String::new());
    }
    ui.end_row();
}
