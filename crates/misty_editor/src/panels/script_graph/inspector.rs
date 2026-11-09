//! The Graph Inspector: settings of the selected node (variable, event name and arguments,
//! input action, pin counts, Lua script and function, subgraph pins…), its problems, and with
//! nothing selected the graph's problems and the shortcuts.

use egui::{Color32, Rect, RichText, Ui, UiBuilder};
use misty_engine::visual_script::{self, Node, nodes};

use crate::panels::node_canvas::ERROR;
use crate::state::EditorState;
use crate::theme;

pub fn height(s: &EditorState) -> f32 {
    match single(s) {
        Some(_) => 330.0,
        None => 120.0 + 18.0 * s.script_graph.errors.len().min(10) as f32,
    }
}

fn single(s: &EditorState) -> Option<Node> {
    let w = &s.script_graph;
    if w.selected.len() != 1 {
        return None;
    }
    let id = *w.selected.iter().next()?;
    w.view().node(id).cloned()
}

/// A text setting; returns the new text when edited.
fn text_param(ui: &mut Ui, node: &Node, key: &str, label: &str, hint: &str) -> Option<String> {
    let mut v = node.param(key).to_owned();
    let mut out = None;
    ui.horizontal(|ui| {
        ui.label(label);
        if ui
            .add(
                egui::TextEdit::singleline(&mut v)
                    .hint_text(hint)
                    .desired_width(f32::INFINITY),
            )
            .changed()
        {
            out = Some(v.clone());
        }
    });
    out
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
    ui.interact(rect, ui.id().with("vs_gi_block"), egui::Sense::click_and_drag());
    let busy = super::interacting(ui.ctx());
    ui.scope_builder(UiBuilder::new().max_rect(rect.shrink(8.0)), |ui| {
        ui.label(RichText::new("Graph Inspector").strong().color(theme::TEXT));
        ui.separator();
        match single(s) {
            Some(node) => {
                egui::ScrollArea::vertical().show(ui, |ui| node_settings(ui, s, &node, busy));
            }
            None => {
                egui::ScrollArea::vertical().show(ui, |ui| graph_summary(ui, s));
            }
        }
    });
}

fn graph_summary(ui: &mut Ui, s: &mut EditorState) {
    let w = &s.script_graph;
    let g = w.view();
    ui.label(
        RichText::new(format!(
            "{} nodes, {} variables, {} macros",
            g.nodes.len(),
            g.variables.len(),
            g.macros.len()
        ))
        .color(theme::TEXT_DIM),
    );
    if w.errors.is_empty() {
        ui.label(RichText::new("✔ No problems").color(theme::TEXT_DIM));
    }
    let errors = w.errors.clone();
    for e in errors.iter().take(10) {
        let text = match e.node.and_then(|n| s.script_graph.view().node(n)) {
            Some(n) => format!("⚠ {}: {}", nodes::title(n, s.script_graph.view()), e.message),
            None => format!("⚠ {}", e.message),
        };
        if ui
            .add(egui::Label::new(RichText::new(text).color(ERROR)).sense(egui::Sense::click()))
            .on_hover_text("Click to select the node")
            .clicked()
            && let Some(n) = e.node
        {
            s.script_graph.selected = [n].into();
        }
    }
    ui.add_space(6.0);
    ui.label(
        RichText::new(
            "Right-click or Space: add a node\nDrag a pin to empty space: matching nodes\n\
             F9: breakpoint · F10: step · F: frame all",
        )
        .small()
        .color(theme::TEXT_DIM),
    );
}

fn node_settings(ui: &mut Ui, s: &mut EditorState, node: &Node, busy: bool) {
    let g = s.script_graph.view().clone();
    ui.label(RichText::new(nodes::title(node, &g)).strong().color(theme::TEXT));
    ui.label(
        RichText::new(format!("{} · node {}", nodes::category(node), node.id))
            .small()
            .color(theme::TEXT_DIM),
    );
    ui.add_space(4.0);
    let mut set: Vec<(String, String)> = Vec::new();
    let mut param = |k: &str, v: Option<String>| {
        if let Some(v) = v {
            set.push((k.to_owned(), v));
        }
    };
    match node.kind.as_str() {
        "GetVariable" | "SetVariable" => {
            let mut name = node.param("name").to_owned();
            egui::ComboBox::from_label("Variable")
                .selected_text(&name)
                .show_ui(ui, |ui| {
                    for v in &g.variables {
                        ui.selectable_value(&mut name, v.name.clone(), &v.name);
                    }
                });
            if name != node.param("name") {
                param("name", Some(name));
            }
        }
        "OnInputAction" => {
            param(
                "action",
                text_param(ui, node, "action", "Action", "Jump or Player/Jump"),
            );
            let mut mode = node.param("mode").to_owned();
            if mode.is_empty() {
                mode = "performed".into();
            }
            egui::ComboBox::from_label("When")
                .selected_text(&mode)
                .show_ui(ui, |ui| {
                    for m in ["performed", "held", "released"] {
                        ui.selectable_value(&mut mode, m.to_owned(), m);
                    }
                });
            if mode != node.param("mode") {
                param("mode", Some(mode));
            }
            ui.label(
                RichText::new("Actions of the scene's Player Inputs (Input System).")
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        "OnCustomEvent" | "SendCustomEvent" | "BroadcastCustomEvent" => {
            param("name", text_param(ui, node, "name", "Event", "MyEvent"));
            param(
                "args",
                text_param(ui, node, "args", "Arguments", "comma separated names"),
            );
            ui.label(
                RichText::new(
                    "Custom events also call the Lua function of the same name on the target's scripts; \
                     Lua sends them with gameObject:SendCustomEvent(name, ...).",
                )
                .small()
                .color(theme::TEXT_DIM),
            );
        }
        "Sequence" | "MakeList" => {
            let default = if node.kind == "Sequence" { 2 } else { 3 };
            let mut n: usize = node.param("count").parse().unwrap_or(default);
            ui.horizontal(|ui| {
                ui.label(if node.kind == "Sequence" { "Outputs" } else { "Items" });
                ui.add(egui::DragValue::new(&mut n).range(1..=32));
            });
            if n != node.param("count").parse().unwrap_or(default) {
                param("count", Some(n.to_string()));
            }
        }
        "DoOnce" | "Gate" => {
            let mut closed = node.param("start_closed") == "true";
            if ui.checkbox(&mut closed, "Start Closed").changed() {
                param("start_closed", Some(closed.to_string()));
            }
        }
        "CallLua" => {
            param("script", text_param(ui, node, "script", "Script", "Lua script name"));
            param(
                "function",
                text_param(ui, node, "function", "Function", "function name"),
            );
            param(
                "args",
                text_param(ui, node, "args", "Arguments", "comma separated names"),
            );
        }
        "GraphInput" | "GraphOutput" => {
            param(
                "pins",
                text_param(ui, node, "pins", "Pins", "In:exec, Speed:float"),
            );
            ui.label(
                RichText::new("Name:type, … (exec, bool, int, float, string, vector3, quaternion, gameobject, list, any)")
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        "Subgraph" => {
            let m = node.param("macro");
            if m.is_empty() {
                ui.label(format!("Asset: {}", node.param("path")));
            } else {
                ui.label(format!("Macro: {m}"));
                if ui.button("Open Macro").clicked() {
                    let m = m.to_owned();
                    s.script_graph.enter_macro(&m);
                    return;
                }
            }
        }
        "Api" => {
            let a = visual_script::api::api_ref(node);
            let native = visual_script::api::native(a.owner, a.member, a.mode).is_some();
            ui.label(format!(
                "Lua: {}{}{}",
                match a.owner {
                    "" => String::new(),
                    o if o.starts_with('#') => format!("{}:", o.trim_start_matches('#').to_lowercase()),
                    o => format!("{o}."),
                },
                a.member,
                match a.mode {
                    visual_script::api::Mode::Call => "()",
                    _ => "",
                }
            ));
            ui.label(
                RichText::new(if native {
                    "Runs natively."
                } else {
                    "Calls the Lua binding (pins take any value)."
                })
                .small()
                .color(theme::TEXT_DIM),
            );
            if !native && a.mode == visual_script::api::Mode::Call {
                param(
                    "args",
                    text_param(ui, node, "args", "Arguments", "comma separated names"),
                );
            }
        }
        "Delay" | "OnTimer" => {
            ui.label(
                RichText::new("Seconds of game time.").small().color(theme::TEXT_DIM),
            );
        }
        "While" => {
            ui.label(
                RichText::new("Stops with an error after Max Iterations passes.")
                    .small()
                    .color(theme::TEXT_DIM),
            );
        }
        _ => {}
    }
    if !set.is_empty() {
        let id = node.id;
        s.script_graph.edit("Change Node Setting", busy, |g| {
            if let Some(n) = g.node_mut(id) {
                for (k, v) in set {
                    n.params.insert(k, v);
                }
            }
        });
    }
    if let Some(e) = s.script_graph.error_of(node.id) {
        ui.add_space(6.0);
        ui.label(RichText::new(format!("⚠ {e}")).color(ERROR));
    }
    if s.script_graph.at_root() {
        let on = s.script_graph.breakpoints.contains(&node.id);
        ui.add_space(6.0);
        if ui
            .button(if on { "Remove Breakpoint" } else { "Add Breakpoint" })
            .clicked()
        {
            s.script_graph.toggle_breakpoint(node.id);
        }
    }
}
