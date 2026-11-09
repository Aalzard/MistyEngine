//! The Blackboard: the graph's variables (name, type, Graph or Object scope, starting value),
//! with Get / Set buttons that add a node, and their live values while playing.

use egui::{Color32, Rect, RichText, Ui, UiBuilder, pos2};
use misty_engine::visual_script::{Literal, PinType, VarScope, Variable};

use super::ScriptGraphWindow;
use crate::state::EditorState;
use crate::theme;

#[derive(Default)]
pub(crate) struct BlackboardState {
    /// Name being typed: (variable, text).
    renaming: Option<(usize, String)>,
}

const ROW: f32 = 64.0;

pub fn height(w: &ScriptGraphWindow) -> f32 {
    (52.0 + w.view().variables.len().max(1) as f32 * ROW).min(520.0)
}

fn unique_name(w: &ScriptGraphWindow, base: &str) -> String {
    let taken = |n: &str| w.view().variables.iter().any(|v| v.name == n);
    if !taken(base) {
        return base.to_owned();
    }
    (2..)
        .map(|i| format!("{base} {i}"))
        .find(|n| !taken(n))
        .unwrap_or_default()
}

/// Value field of a variable's starting value.
pub fn literal_ui(ui: &mut Ui, ty: PinType, lit: &mut Literal) -> bool {
    let mut v = lit.coerce(ty);
    let changed = match &mut v {
        Literal::Bool(b) => ui.checkbox(b, "").changed(),
        Literal::Int(i) => ui.add(egui::DragValue::new(i)).changed(),
        Literal::Float(f) => ui.add(egui::DragValue::new(f).speed(0.05)).changed(),
        Literal::String(t) => ui
            .add(egui::TextEdit::singleline(t).desired_width(120.0))
            .changed(),
        Literal::Vector3(a) | Literal::Quaternion(a) => {
            let mut ch = false;
            for c in a.iter_mut() {
                ch |= ui
                    .add(egui::DragValue::new(c).speed(0.05).max_decimals(2))
                    .changed();
            }
            ch
        }
        Literal::None => {
            ui.label(RichText::new("None").color(theme::TEXT_DIM));
            false
        }
    };
    if changed {
        *lit = v;
    }
    changed
}

pub fn show(ui: &mut Ui, s: &mut EditorState, rect: Rect) {
    ui.painter()
        .rect_filled(rect, 8.0, Color32::from_rgba_unmultiplied(44, 45, 49, 245));
    ui.painter().rect_stroke(
        rect,
        8.0,
        egui::Stroke::new(1.0, Color32::from_white_alpha(20)),
        egui::StrokeKind::Inside,
    );
    // Clicks on the panel don't reach the canvas.
    ui.interact(rect, ui.id().with("vs_bb_block"), egui::Sense::click_and_drag());
    let busy = super::interacting(ui.ctx());
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Blackboard").strong().color(theme::TEXT));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button("+", |ui| {
                    for ty in PinType::VARIABLE_TYPES {
                        if ui.button(ty.label()).clicked() {
                            let name = unique_name(&s.script_graph, &format!("New {}", ty.label()));
                            s.script_graph.edit("Add Variable", false, |g| {
                                g.variables
                                    .push(Variable::new(&name, ty, ty.default_literal()));
                            });
                            ui.close();
                        }
                    }
                })
                .response
                .on_hover_text("Add a variable");
            });
        });
        ui.separator();
        let live: Vec<(String, String)> = s
            .script_graph
            .live
            .as_ref()
            .filter(|_| s.script_graph.at_root())
            .map(|l| l.variables.clone())
            .unwrap_or_default();
        let vars = s.script_graph.view().variables.clone();
        if vars.is_empty() {
            ui.label(
                RichText::new("No variables. Use + to add one.").color(theme::TEXT_DIM),
            );
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            for (i, v) in vars.iter().enumerate() {
                ui.push_id(("vs_var", i), |ui| {
                    row(ui, s, i, v, &live, busy);
                });
                ui.separator();
            }
        });
    });
}

fn row(
    ui: &mut Ui,
    s: &mut EditorState,
    i: usize,
    v: &Variable,
    live: &[(String, String)],
    busy: bool,
) {
    ui.horizontal(|ui| {
        let mut text = s
            .script_graph
            .blackboard
            .renaming
            .as_ref()
            .filter(|(k, _)| *k == i)
            .map_or_else(|| v.name.clone(), |(_, t)| t.clone());
        let edit = ui.add(egui::TextEdit::singleline(&mut text).desired_width(110.0));
        if edit.changed() {
            s.script_graph.blackboard.renaming = Some((i, text.clone()));
        }
        if edit.lost_focus()
            && let Some((_, new)) = s.script_graph.blackboard.renaming.take()
        {
            let new = new.trim().to_owned();
            let old = v.name.clone();
            let taken = s.script_graph.view().variables.iter().any(|x| x.name == new);
            if !new.is_empty() && new != old && !taken {
                s.script_graph.edit("Rename Variable", false, |g| {
                    if let Some(x) = g.variables.get_mut(i) {
                        x.name = new.clone();
                    }
                    for n in &mut g.nodes {
                        if matches!(n.kind.as_str(), "GetVariable" | "SetVariable")
                            && n.param("name") == old
                        {
                            n.params.insert("name".into(), new.clone());
                        }
                    }
                });
            }
        }
        let mut ty = v.ty;
        egui::ComboBox::from_id_salt("ty")
            .width(84.0)
            .selected_text(ty.label())
            .show_ui(ui, |ui| {
                for t in PinType::VARIABLE_TYPES {
                    ui.selectable_value(&mut ty, t, t.label());
                }
            });
        if ty != v.ty {
            s.script_graph.edit("Change Variable Type", false, |g| {
                if let Some(x) = g.variables.get_mut(i) {
                    x.ty = ty;
                    x.value = x.value.coerce(ty);
                }
            });
        }
        if ui
            .small_button("✕")
            .on_hover_text("Delete the variable")
            .clicked()
        {
            s.script_graph.edit("Delete Variable", false, |g| {
                if i < g.variables.len() {
                    g.variables.remove(i);
                }
            });
        }
    });
    ui.horizontal(|ui| {
        let mut scope = v.scope;
        egui::ComboBox::from_id_salt("scope")
            .width(70.0)
            .selected_text(match scope {
                VarScope::Graph => "Graph",
                VarScope::Object => "Object",
            })
            .show_ui(ui, |ui| {
                ui.selectable_value(&mut scope, VarScope::Graph, "Graph")
                    .on_hover_text("Private to this graph on each object");
                ui.selectable_value(&mut scope, VarScope::Object, "Object").on_hover_text(
                    "Stored on the GameObject: set per object in the Inspector, shared with its other graphs and Lua (GetVariable)",
                );
            });
        if scope != v.scope {
            s.script_graph.edit("Change Variable Scope", false, |g| {
                if let Some(x) = g.variables.get_mut(i) {
                    x.scope = scope;
                }
            });
        }
        let mut lit = v.value.clone();
        if literal_ui(ui, v.ty, &mut lit) {
            s.script_graph.edit("Change Variable Value", busy, |g| {
                if let Some(x) = g.variables.get_mut(i) {
                    x.value = lit.clone();
                }
            });
        }
    });
    ui.horizontal(|ui| {
        let centre = s
            .script_graph
            .canvas
            .view
            .map(|vw| vw.graph(vw.centre))
            .unwrap_or(pos2(0.0, 0.0));
        for (kind, label) in [("GetVariable", "Get"), ("SetVariable", "Set")] {
            if ui
                .small_button(label)
                .on_hover_text(format!("Add a {label} {} node", v.name))
                .clicked()
            {
                let at = [centre.x - 60.0, centre.y - 20.0];
                s.script_graph
                    .add_node(kind, &[("name".into(), v.name.clone())], at);
            }
        }
        if let Some((_, value)) = live.iter().find(|(n, _)| *n == v.name) {
            ui.label(RichText::new(format!("= {value}")).color(theme::WARN))
                .on_hover_text("Value in the playing game");
        }
    });
}
