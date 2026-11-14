//! The Method field of a persistent UI call (On Click, On Value Changed, On End Edit): a text
//! box plus a menu of what the target can run, the functions of its Lua scripts and the Custom
//! Event names of its Script Graphs (both hear the call at play time).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use egui::{RichText, Ui};
use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, Scripts};
use misty_engine::visual_script::ScriptGraph;

use crate::i18n::tr;
use crate::state::EditorState;
use crate::theme;

/// The script files of every object that has scripts (no file is read until a menu opens).
pub struct CallTargets {
    root: PathBuf,
    scripts: HashMap<Entity, Vec<String>>,
}

impl CallTargets {
    pub fn new(state: &EditorState) -> Self {
        Self::from_scene(&state.project.root, &state.scene)
    }

    pub fn from_scene(root: &Path, scene: &Scene) -> Self {
        let scripts = scene
            .world
            .query::<(Entity, &Scripts)>()
            .iter()
            .map(|(e, s)| (e, s.list.iter().map(|r| r.path.clone()).collect()))
            .collect();
        Self {
            root: root.to_path_buf(),
            scripts,
        }
    }

    /// (name, is a graph Custom Event) for `target`, sorted, without repeats.
    pub fn methods(&self, target: Entity) -> Vec<(String, bool)> {
        let mut out: Vec<(String, bool)> = Vec::new();
        for path in self.scripts.get(&target).into_iter().flatten() {
            let file = self.root.join(path);
            let lower = path.to_ascii_lowercase();
            let (names, graph) = if lower.ends_with(".vgraph") {
                let names = ScriptGraph::load(&file)
                    .map(|g| graph_events(&g))
                    .unwrap_or_default();
                (names, true)
            } else if lower.ends_with(".lua") {
                let src = std::fs::read_to_string(&file).unwrap_or_default();
                (lua_functions(&src), false)
            } else {
                continue;
            };
            for n in names {
                if !out.iter().any(|(m, _)| *m == n) {
                    out.push((n, graph));
                }
            }
        }
        out.sort();
        out
    }
}

/// Engine callbacks a UI call wouldn't name.
const CALLBACKS: &[&str] = &[
    "Awake",
    "Start",
    "Update",
    "FixedUpdate",
    "LateUpdate",
    "OnEnable",
    "OnDisable",
    "OnDestroy",
];

/// Names of `function T:Name(` / `function T.Name(` in a Lua script.
pub fn lua_functions(src: &str) -> Vec<String> {
    let mut out = Vec::new();
    for line in src.lines() {
        let Some(rest) = line.trim_start().strip_prefix("function ") else {
            continue;
        };
        let Some(head) = rest.split('(').next() else {
            continue;
        };
        let Some((_, name)) = head.trim().rsplit_once([':', '.']) else {
            continue;
        };
        let ok = !name.is_empty()
            && name.chars().all(|c| c.is_alphanumeric() || c == '_')
            && !CALLBACKS.contains(&name);
        if ok && !out.iter().any(|n| n == name) {
            out.push(name.to_owned());
        }
    }
    out
}

/// Names of a graph's Custom Event nodes.
pub fn graph_events(g: &ScriptGraph) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for n in g.nodes.iter().filter(|n| n.kind == "OnCustomEvent") {
        let name = n.param("name").trim();
        if !name.is_empty() && !out.iter().any(|m| m == name) {
            out.push(name.to_owned());
        }
    }
    out
}

/// The Method text box and its ▾ menu of `target`'s methods.
pub fn method_field(
    ui: &mut Ui,
    method: &mut String,
    target: Option<Entity>,
    targets: &CallTargets,
) -> bool {
    let mut ch = ui
        .add(
            egui::TextEdit::singleline(method)
                .hint_text(tr!("Method"))
                .desired_width(100.0),
        )
        .changed();
    let _ = ui
        .menu_button("▾", |ui| {
            let list = target.map(|t| targets.methods(t)).unwrap_or_default();
            if list.is_empty() {
                ui.label(
                    RichText::new(tr!(
                        "No Lua functions or Script Graph Custom Events on the target"
                    ))
                    .color(theme::text_dim()),
                );
            }
            for (name, graph) in list {
                let kind = if graph {
                    tr!("Custom Event")
                } else {
                    tr!("Lua")
                };
                let label = format!("{name}   ({kind})");
                if ui.selectable_label(*method == name, label).clicked() {
                    *method = name;
                    ch = true;
                    ui.close();
                }
            }
        })
        .response
        .on_hover_text(tr!(
            "Lua functions and Script Graph Custom Events of the target"
        ));
    ch
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lua_functions_skip_callbacks_and_repeats() {
        let src = "local P = {}\nfunction P:Start() end\nfunction P:Play()\nend\n  function P.Reset(x) end\nlocal function helper() end\nfunction P:Play() end\nreturn P\n";
        assert_eq!(lua_functions(src), ["Play", "Reset"]);
    }

    #[test]
    fn methods_list_graph_custom_events_and_lua_functions() {
        let n = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let root = std::env::temp_dir().join(format!("misty_call_methods_{n}"));
        std::fs::create_dir_all(root.join("Assets")).unwrap();
        let mut g = ScriptGraph::default();
        g.add_node_with("OnCustomEvent", &[("name", "Play")], [0.0, 0.0]);
        g.add_node_with("OnCustomEvent", &[("name", "Quit")], [0.0, 50.0]);
        g.add_node("OnStart", [0.0, 100.0]);
        g.save(&root.join("Assets/Menu.vgraph")).unwrap();
        std::fs::write(
            root.join("Assets/Menu.lua"),
            "local M = {}\nfunction M:Options() end\nreturn M\n",
        )
        .unwrap();
        let mut scene = Scene::new("t");
        let e = scene.spawn("Menu", None);
        scene.add(
            e,
            Scripts {
                list: ["Assets/Menu.vgraph", "Assets/Menu.lua"]
                    .into_iter()
                    .map(|p| misty_engine::scene::ScriptRef {
                        path: p.into(),
                        ..Default::default()
                    })
                    .collect(),
            },
        );
        let t = CallTargets::from_scene(&root, &scene);
        assert_eq!(
            t.methods(e),
            [
                ("Options".to_owned(), false),
                ("Play".to_owned(), true),
                ("Quit".to_owned(), true)
            ]
        );
        let _ = std::fs::remove_dir_all(root);
    }
}
