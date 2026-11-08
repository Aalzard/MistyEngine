//! Node Settings: a floating card for the one selected Custom Function or Sub Graph node (like
//! the reference editor's Graph Inspector). A Custom Function edits its name, its source (inline
//! body or a `.wgsl` file) and its inputs and outputs; a Sub Graph node opens or refreshes its
//! asset.

use egui::{FontId, Rect, RichText, Sense, Ui, UiBuilder};
use misty_engine::shader_graph::{
    FileResolver, FunctionSource, NodeKind, ShaderGraph, Slot, ValueType,
};

use super::{ShaderGraphWindow, card, interacting, slot_type_combo};
use crate::state::EditorState;
use crate::theme;

/// The node Node Settings shows: exactly one selected Custom Function or Sub Graph.
pub fn selected(w: &ShaderGraphWindow) -> Option<u32> {
    if w.selected.len() != 1 {
        return None;
    }
    let id = *w.selected.iter().next()?;
    matches!(
        w.graph.node(id)?.kind,
        NodeKind::CustomFunction { .. } | NodeKind::SubGraph { .. }
    )
    .then_some(id)
}

pub fn height(g: &ShaderGraph, id: u32) -> f32 {
    match g.node(id).map(|n| &n.kind) {
        Some(NodeKind::CustomFunction {
            source,
            inputs,
            outputs,
            ..
        }) => {
            let code = if *source == FunctionSource::Inline {
                170.0
            } else {
                30.0
            };
            150.0 + code + (inputs.len() + outputs.len()) as f32 * 26.0
        }
        _ => 120.0,
    }
}

/// `.wgsl` files under the project's Assets folder (project paths).
fn wgsl_files(root: &std::path::Path) -> Vec<String> {
    let mut out = Vec::new();
    let mut stack = vec![root.join("Assets")];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "wgsl")
                && let Ok(rel) = p.strip_prefix(root)
            {
                out.push(rel.to_string_lossy().replace('\\', "/"));
            }
        }
    }
    out.sort();
    out
}

/// Slot rows (name, type, remove) and an add button; true when changed.
fn slots_ui(ui: &mut Ui, label: &str, id: u32, slots: &mut Vec<Slot>, textures: bool) -> bool {
    let before = slots.clone();
    ui.label(RichText::new(label).color(theme::TEXT_DIM));
    let mut remove = None;
    for (i, s) in slots.iter_mut().enumerate() {
        ui.horizontal(|ui| {
            ui.add(egui::TextEdit::singleline(&mut s.name).desired_width(70.0));
            slot_type_combo(ui, ("sg_slot_ty", id, label, i), &mut s.ty, textures);
            if ui.small_button("✕").on_hover_text("Remove").clicked() {
                remove = Some(i);
            }
        });
    }
    if let Some(i) = remove {
        slots.remove(i);
    }
    if ui
        .button(format!("+ {}", label.trim_end_matches('s')))
        .clicked()
    {
        let base = if textures { "In" } else { "Out" };
        let mut n = slots.len() + 1;
        while slots.iter().any(|s| s.name == format!("{base}{n}")) {
            n += 1;
        }
        slots.push(Slot::new(&format!("{base}{n}"), ValueType::Float));
    }
    *slots != before
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect, id: u32) {
    ui.interact(
        rect,
        ui.id().with("sg_node_settings_bg"),
        Sense::click_and_drag(),
    );
    let Some(node) = s.shader_graph.graph.node(id).cloned() else {
        return;
    };
    let inner = card(ui, rect, "Node Settings");
    let busy = interacting(ui.ctx());
    match node.kind.clone() {
        NodeKind::CustomFunction {
            mut function,
            mut source,
            mut body,
            mut file,
            mut inputs,
            mut outputs,
        } => {
            let files = wgsl_files(&s.project.root);
            let mut changed = false;
            ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                egui::ScrollArea::vertical()
                    .id_salt(("sg_cf_scroll", id))
                    .show(ui, |ui| {
                        egui::Grid::new(("sg_cf", id))
                            .num_columns(2)
                            .spacing([6.0, 5.0])
                            .show(ui, |ui| {
                                ui.label("Name");
                                changed |= ui
                                    .add(egui::TextEdit::singleline(&mut function).desired_width(130.0))
                                    .changed();
                                ui.end_row();
                                ui.label("Type");
                                egui::ComboBox::from_id_salt(("sg_cf_src", id))
                                    .selected_text(match source {
                                        FunctionSource::Inline => "String",
                                        FunctionSource::File => "File",
                                    })
                                    .show_ui(ui, |ui| {
                                        changed |= ui
                                            .selectable_value(&mut source, FunctionSource::Inline, "String")
                                            .changed();
                                        changed |= ui
                                            .selectable_value(&mut source, FunctionSource::File, "File")
                                            .changed();
                                    });
                                ui.end_row();
                                if source == FunctionSource::File {
                                    ui.label("Source");
                                    let current = if file.is_empty() {
                                        "None".to_owned()
                                    } else {
                                        misty_engine::scene::clip_name(&file)
                                    };
                                    egui::ComboBox::from_id_salt(("sg_cf_file", id))
                                        .selected_text(current)
                                        .show_ui(ui, |ui| {
                                            for f in &files {
                                                changed |= ui
                                                    .selectable_value(&mut file, f.clone(), f.as_str())
                                                    .changed();
                                            }
                                            if files.is_empty() {
                                                ui.label("No .wgsl files in Assets");
                                            }
                                        });
                                    ui.end_row();
                                }
                            });
                        changed |= slots_ui(ui, "Inputs", id, &mut inputs, true);
                        changed |= slots_ui(ui, "Outputs", id, &mut outputs, false);
                        if source == FunctionSource::Inline {
                            ui.label(
                                RichText::new("Body: read the inputs, assign the outputs")
                                    .color(theme::TEXT_DIM),
                            );
                            changed |= ui
                                .add(
                                    egui::TextEdit::multiline(&mut body)
                                        .font(FontId::monospace(12.0))
                                        .desired_rows(7)
                                        .desired_width(f32::INFINITY)
                                        .code_editor(),
                                )
                                .changed();
                        } else {
                            ui.label(
                                RichText::new(format!(
                                    "The file defines fn {function}(inputs…) returning the output (or a struct of the outputs)"
                                ))
                                .small()
                                .color(theme::TEXT_DIM),
                            );
                        }
                    });
            });
            if changed {
                let kind = NodeKind::CustomFunction {
                    function,
                    source,
                    body,
                    file,
                    inputs,
                    outputs,
                };
                s.shader_graph.edit("Edit Custom Function", busy, |g| {
                    if let Some(n) = g.node_mut(id) {
                        n.kind = kind;
                    }
                    prune(g, id);
                });
            }
        }
        NodeKind::SubGraph { path, .. } => {
            let mut open = false;
            let mut refresh = false;
            ui.scope_builder(UiBuilder::new().max_rect(inner), |ui| {
                ui.label(RichText::new(&path).monospace().small());
                ui.add_space(4.0);
                ui.horizontal(|ui| {
                    open = ui.button("Open Sub Graph").clicked();
                    refresh = ui
                        .button("Refresh")
                        .on_hover_text("Read its inputs and outputs again")
                        .clicked();
                });
            });
            if refresh {
                s.shader_graph.edit("Refresh Sub Graph", false, |g| {
                    g.refresh_sub_graphs(&FileResolver);
                });
            }
            if open {
                super::open(s, &path);
            }
        }
        _ => {}
    }
}

/// Drop connections of `id` to ports it no longer has.
fn prune(g: &mut ShaderGraph, id: u32) {
    let Some(n) = g.node(id) else { return };
    let ins: Vec<&str> = g.inputs(n).iter().map(|p| p.name).collect();
    let outs: Vec<&str> = g.outputs(n).iter().map(|p| p.name).collect();
    g.connections.retain(|c| {
        (c.to != id || ins.contains(&c.input.as_str()))
            && (c.from != id || outs.contains(&c.output.as_str()))
    });
}
