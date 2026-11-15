//! "Save changes?" for the Script Graph, Effect Graph and Behaviour Tree windows.
//!
//! Each of these windows holds one asset. Opening another asset in it, or quitting the editor,
//! used to drop its unsaved changes without a word (and a clean exit then deleted their
//! autosave backups too). Now the window asks first: Save, Don't Save or Cancel, like the
//! Shader Graph window and the scene. (The Shader Graph window keeps its own prompt.)

use crate::i18n::tr;
use crate::layout::Tab;
use crate::state::EditorState;

/// The windows this prompt covers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    ScriptGraph,
    EffectGraph,
    BehaviourTree,
}

impl Kind {
    pub const ALL: [Kind; 3] = [Kind::ScriptGraph, Kind::EffectGraph, Kind::BehaviourTree];

    fn path(self, s: &EditorState) -> Option<String> {
        match self {
            Kind::ScriptGraph => s.script_graph.path.clone(),
            Kind::EffectGraph => s.effect_graph.path.clone(),
            Kind::BehaviourTree => s.behaviour_tree.path.clone(),
        }
    }

    pub fn dirty(self, s: &EditorState) -> bool {
        match self {
            Kind::ScriptGraph => s.script_graph.dirty(),
            Kind::EffectGraph => s.effect_graph.dirty(),
            Kind::BehaviourTree => s.behaviour_tree.dirty(),
        }
    }

    fn tab(self) -> Tab {
        match self {
            Kind::ScriptGraph => Tab::ScriptGraph,
            Kind::EffectGraph => Tab::EffectGraph,
            Kind::BehaviourTree => Tab::BehaviourTree,
        }
    }

    /// Write the asset; false when it failed (the Console says why).
    fn save(self, s: &mut EditorState) -> bool {
        match self {
            Kind::ScriptGraph => crate::panels::script_graph::save(s),
            Kind::EffectGraph => crate::panels::effect_graph::save(s),
            Kind::BehaviourTree => crate::panels::behaviour_tree::save(s),
        }
    }

    /// Drop the unsaved changes: the asset as its file holds it.
    fn revert(self, s: &mut EditorState) {
        let Some(rel) = self.path(s) else {
            return;
        };
        let file = s.assets.absolute(&rel);
        match self {
            Kind::ScriptGraph => s.script_graph.open(&file, &rel),
            Kind::EffectGraph => s.effect_graph.open(&file, &rel),
            Kind::BehaviourTree => s.behaviour_tree.open(&file, &rel),
        }
    }

    /// Open `rel` in the window (no question asked).
    fn open(self, s: &mut EditorState, rel: &str) {
        match self {
            Kind::ScriptGraph => crate::panels::script_graph::open(s, rel),
            Kind::EffectGraph => crate::panels::effect_graph::open(s, rel),
            Kind::BehaviourTree => crate::panels::behaviour_tree::open(s, rel),
        }
    }
}

/// What happens once the question is answered with Save or Don't Save.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    /// Open this asset in the window.
    Open(String),
    /// Go on closing the editor.
    Quit,
}

/// The question on screen.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Prompt {
    pub kind: Kind,
    pub after: After,
}

/// Opening `rel` in `kind`'s window would drop its unsaved changes: ask first. True when the
/// question is now up (the caller doesn't open).
pub fn ask_before_open(s: &mut EditorState, kind: Kind, rel: &str) -> bool {
    if kind.path(s).as_deref() == Some(rel) || !kind.dirty(s) {
        return false;
    }
    s.graph_prompt = Some(Prompt {
        kind,
        after: After::Open(rel.to_owned()),
    });
    s.push(crate::actions::Action::FocusTab(kind.tab()));
    true
}

/// The editor is closing: true (and the question is up) while one of the windows has unsaved
/// changes.
pub fn cancel_close(s: &mut EditorState) -> bool {
    if let Some(prompt) = s.graph_prompt.as_mut() {
        // "Save before opening the other asset?" is up: it becomes the quit's question (the
        // other asset would only open to close again), so answering it goes on closing.
        prompt.after = After::Quit;
        return true;
    }
    let Some(kind) = Kind::ALL.into_iter().find(|k| k.dirty(s)) else {
        return false;
    };
    s.graph_prompt = Some(Prompt {
        kind,
        after: After::Quit,
    });
    s.push(crate::actions::Action::FocusTab(kind.tab()));
    true
}

/// Carry out an answer: `Some(true)` Save, `Some(false)` Don't Save, `None` Cancel. Returns
/// true when the editor should go on closing.
pub fn answer(s: &mut EditorState, save: Option<bool>) -> bool {
    let Some(prompt) = s.graph_prompt.take() else {
        return false;
    };
    match save {
        None => return false,
        Some(true) => {
            if !prompt.kind.save(s) {
                return false;
            }
        }
        Some(false) => prompt.kind.revert(s),
    }
    match prompt.after {
        After::Open(rel) => {
            prompt.kind.open(s, &rel);
            false
        }
        After::Quit => true,
    }
}

/// The question (drawn even while the window's tab is hidden).
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let Some(prompt) = s.graph_prompt.clone() else {
        return;
    };
    let name = prompt
        .kind
        .path(s)
        .as_deref()
        .map(misty_engine::scene::clip_name)
        .unwrap_or_default();
    let mut choice = None;
    let mut cancel = false;
    egui::Modal::new(egui::Id::new("graph_save_prompt")).show(ctx, |ui| {
        ui.set_width(380.0);
        ui.heading(tr!("Save changes?"));
        ui.add_space(6.0);
        ui.label(match prompt.after {
            After::Open(_) => tr!(
                "{name} has unsaved changes. Save them before opening the other asset?",
                name = name
            ),
            After::Quit => tr!(
                "{name} has unsaved changes. Save them before quitting?",
                name = name
            ),
        });
        ui.add_space(10.0);
        ui.horizontal(|ui| {
            if ui.button(tr!("Save")).clicked() {
                choice = Some(true);
            }
            if ui.button(tr!("Don't Save")).clicked() {
                choice = Some(false);
            }
            if ui.button(tr!("Cancel")).clicked() {
                cancel = true;
            }
        });
    });
    if cancel {
        answer(s, None);
    } else if choice.is_some() && answer(s, choice) {
        // The next window with changes (if any) asks on the next close request.
        ctx.send_viewport_cmd(egui::ViewportCommand::Close);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use misty_engine::behaviour_tree::BehaviourTree;
    use misty_engine::behaviour_tree::asset::NodeKind;
    use misty_engine::visual_script::{Literal, PinType, ScriptGraph, Variable};

    fn project(tag: &str) -> (EditorState, std::path::PathBuf) {
        let root =
            std::env::temp_dir().join(format!("misty_graph_prompt_{tag}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/AI")).unwrap();
        for name in ["A", "B"] {
            ScriptGraph::default()
                .save(&root.join(format!("Assets/AI/{name}.vgraph")))
                .unwrap();
            BehaviourTree::new_default()
                .save(&root.join(format!("Assets/AI/{name}.btree")))
                .unwrap();
        }
        (EditorState::new(ProjectBrowser::open(root.clone())), root)
    }

    /// Round 70: opening another graph in the Script Graph window dropped the unsaved changes
    /// of the one shown. It asks now; Save writes them, then the other graph opens.
    #[test]
    fn opening_another_graph_asks_to_save_first() {
        let (mut s, root) = project("open");
        crate::panels::script_graph::open(&mut s, "Assets/AI/A.vgraph");
        s.script_graph.graph.variables.push(Variable::new(
            "Unsaved",
            PinType::Bool,
            Literal::Bool(true),
        ));
        assert!(s.script_graph.dirty());
        crate::panels::script_graph::open(&mut s, "Assets/AI/B.vgraph");
        assert_eq!(
            s.script_graph.path.as_deref(),
            Some("Assets/AI/A.vgraph"),
            "the changed graph stays until the question is answered"
        );
        assert_eq!(
            s.graph_prompt,
            Some(Prompt {
                kind: Kind::ScriptGraph,
                after: After::Open("Assets/AI/B.vgraph".into()),
            })
        );
        assert!(!answer(&mut s, Some(true)));
        assert_eq!(s.script_graph.path.as_deref(), Some("Assets/AI/B.vgraph"));
        let on_disk = ScriptGraph::load(&root.join("Assets/AI/A.vgraph")).unwrap();
        assert_eq!(on_disk.variables.len(), 1, "saved before the switch");
        // Cancel keeps the graph and its changes.
        s.script_graph.graph.variables.push(Variable::new(
            "Again",
            PinType::Bool,
            Literal::Bool(true),
        ));
        crate::panels::script_graph::open(&mut s, "Assets/AI/A.vgraph");
        answer(&mut s, None);
        assert_eq!(s.script_graph.path.as_deref(), Some("Assets/AI/B.vgraph"));
        assert!(s.script_graph.dirty());
        assert!(s.graph_prompt.is_none());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Round 70: quitting with an unsaved Behaviour Tree (or Script / Effect Graph) closed the
    /// editor without asking, and the clean exit deleted the autosave backup too.
    #[test]
    fn quitting_with_an_unsaved_tree_asks_first() {
        let (mut s, root) = project("quit");
        crate::panels::behaviour_tree::open(&mut s, "Assets/AI/A.btree");
        assert!(!cancel_close(&mut s), "nothing unsaved: closes");
        s.behaviour_tree.add_node(
            NodeKind::Wait {
                seconds: 1.0,
                random: 0.0,
            },
            [300.0, 80.0],
            Some(2),
        );
        assert!(s.behaviour_tree.dirty());
        assert!(cancel_close(&mut s), "the close waits for the answer");
        assert_eq!(
            s.graph_prompt.as_ref().map(|p| (p.kind, p.after.clone())),
            Some((Kind::BehaviourTree, After::Quit))
        );
        // Don't Save: the tree goes back to its file and the editor closes.
        assert!(answer(&mut s, Some(false)));
        assert!(!s.behaviour_tree.dirty());
        assert!(!cancel_close(&mut s));
        let on_disk = BehaviourTree::load(&root.join("Assets/AI/A.btree")).unwrap();
        assert_eq!(on_disk, BehaviourTree::new_default());
        let _ = std::fs::remove_dir_all(root);
    }

    /// Round 83: closing the editor while the "open another graph?" question was up lost the
    /// quit: the close waited for the answer, which then opened the other graph and stayed.
    /// The question becomes the quit's question; answering it goes on closing.
    #[test]
    fn quitting_while_the_open_question_is_up_folds_into_the_quit() {
        let (mut s, root) = project("open_quit");
        crate::panels::script_graph::open(&mut s, "Assets/AI/A.vgraph");
        s.script_graph.graph.variables.push(Variable::new(
            "Unsaved",
            PinType::Bool,
            Literal::Bool(true),
        ));
        crate::panels::script_graph::open(&mut s, "Assets/AI/B.vgraph");
        assert!(cancel_close(&mut s), "the close waits for the answer");
        assert_eq!(
            s.graph_prompt.as_ref().map(|p| (p.kind, p.after.clone())),
            Some((Kind::ScriptGraph, After::Quit))
        );
        assert!(answer(&mut s, Some(true)), "Save goes on closing");
        assert_eq!(s.script_graph.path.as_deref(), Some("Assets/AI/A.vgraph"));
        let on_disk = ScriptGraph::load(&root.join("Assets/AI/A.vgraph")).unwrap();
        assert_eq!(on_disk.variables.len(), 1, "saved before quitting");
        assert!(!cancel_close(&mut s), "nothing left to ask: closes");
        // Cancel still keeps the editor open with the changes.
        s.script_graph.graph.variables.push(Variable::new(
            "Again",
            PinType::Bool,
            Literal::Bool(true),
        ));
        crate::panels::script_graph::open(&mut s, "Assets/AI/B.vgraph");
        assert!(cancel_close(&mut s));
        assert!(!answer(&mut s, None));
        assert!(s.script_graph.dirty());
        let _ = std::fs::remove_dir_all(root);
    }
}
