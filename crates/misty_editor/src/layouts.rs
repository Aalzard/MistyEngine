//! Saved window layouts: the current dock arrangement and named user layouts,
//! kept per user in `<Project>/Library/Layouts.json`, plus the Save Layout prompt.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{Duration, Instant};

use egui::Rect;
use egui_dock::{DockState, Node};
use serde::{Deserialize, Serialize};

use crate::actions::Action;
use crate::layout::{BuiltinLayout, Tab};
use crate::state::EditorState;

const LAYOUT_FILE: &str = "Library/Layouts.json";
/// Layout changes are written at most this often.
const SAVE_INTERVAL: Duration = Duration::from_secs(1);

/// Which layout to apply.
#[derive(Clone, Debug, PartialEq)]
pub enum LayoutRef {
    Builtin(BuiltinLayout),
    User(String),
}

/// What is stored on disk.
#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
struct LayoutFile {
    current_name: String,
    current: Option<DockState<Tab>>,
    user: BTreeMap<String, DockState<Tab>>,
}

/// Named user layouts, listed by the Window menu and the toolbar.
pub struct Layouts {
    pub user: BTreeMap<String, DockState<Tab>>,
    /// Layout last applied or saved (shown on the toolbar dropdown).
    pub current_name: String,
    /// Save Layout prompt is open, holding the typed name.
    pub save_prompt: Option<String>,
}

impl Default for Layouts {
    fn default() -> Self {
        Self {
            user: BTreeMap::new(),
            current_name: BuiltinLayout::Default.name().to_owned(),
            save_prompt: None,
        }
    }
}

impl Layouts {
    /// Look up a layout by reference.
    pub fn get(&self, layout: &LayoutRef) -> Option<DockState<Tab>> {
        match layout {
            LayoutRef::Builtin(b) => Some(b.build()),
            LayoutRef::User(name) => self.user.get(name).cloned(),
        }
    }
}

/// Load the saved layouts and the last dock arrangement (default layout if missing or corrupt).
pub fn load(root: &Path) -> (Layouts, DockState<Tab>) {
    let file: LayoutFile = std::fs::read_to_string(root.join(LAYOUT_FILE))
        .ok()
        .and_then(|t| serde_json::from_str(&t).ok())
        .unwrap_or_default();
    let mut layouts = Layouts {
        user: file.user,
        ..Default::default()
    };
    if !file.current_name.is_empty() {
        layouts.current_name = file.current_name;
    }
    let dock = file
        .current
        .filter(|d| d.iter_all_tabs().next().is_some())
        .unwrap_or_else(crate::layout::default_layout);
    (layouts, dock)
}

/// Screen rects are recomputed every frame; zero them so files stay stable and
/// never contain infinities (which JSON can't hold).
pub(crate) fn normalized(dock: &DockState<Tab>) -> DockState<Tab> {
    let mut dock = dock.clone();
    for (_, node) in dock.iter_all_nodes_mut() {
        match node {
            Node::Leaf(leaf) => {
                leaf.rect = Rect::ZERO;
                leaf.viewport = Rect::ZERO;
            }
            Node::Vertical(split) | Node::Horizontal(split) => split.rect = Rect::ZERO,
            Node::Empty => {}
        }
    }
    dock
}

/// Serialize the current dock and user layouts.
fn to_json(layouts: &Layouts, dock: &DockState<Tab>) -> String {
    let file = LayoutFile {
        current_name: layouts.current_name.clone(),
        current: Some(normalized(dock)),
        user: layouts
            .user
            .iter()
            .map(|(k, v)| (k.clone(), normalized(v)))
            .collect(),
    };
    serde_json::to_string_pretty(&file).unwrap_or_default()
}

/// Writes the layout file when it changed, at most once per [`SAVE_INTERVAL`].
#[derive(Default)]
pub struct Autosave {
    last_check: Option<Instant>,
    written: String,
}

impl Autosave {
    /// Remember what is on disk so an unchanged layout isn't rewritten.
    pub fn new(layouts: &Layouts, dock: &DockState<Tab>) -> Self {
        Self {
            last_check: Some(Instant::now()),
            written: to_json(layouts, dock),
        }
    }

    /// Call every frame; `force` writes now (exit, user layout changes).
    pub fn tick(&mut self, root: &Path, layouts: &Layouts, dock: &DockState<Tab>, force: bool) {
        let due = self.last_check.is_none_or(|t| t.elapsed() >= SAVE_INTERVAL);
        if !force && !due {
            return;
        }
        self.last_check = Some(Instant::now());
        let text = to_json(layouts, dock);
        if text == self.written {
            return;
        }
        let path = root.join(LAYOUT_FILE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if std::fs::write(path, &text).is_ok() {
            self.written = text;
        }
    }
}

/// Why a typed layout name can't be used, if it can't.
pub fn name_problem(name: &str) -> Option<&'static str> {
    let name = name.trim();
    if name.is_empty() {
        Some("Enter a name.")
    } else if BuiltinLayout::from_name(name).is_some() {
        Some("That name is used by a built-in layout.")
    } else {
        None
    }
}

/// The modal "Save Layout" prompt. Returns the save action once confirmed.
pub fn save_prompt_ui(ctx: &egui::Context, s: &mut EditorState) -> Option<Action> {
    let mut name = s.layouts.save_prompt.take()?;
    let mut result = None;
    let mut close = false;
    egui::Modal::new(egui::Id::new("save_layout_prompt")).show(ctx, |ui| {
        ui.set_width(300.0);
        ui.heading("Save Window Layout");
        ui.add_space(6.0);
        let edit = ui.add(egui::TextEdit::singleline(&mut name).desired_width(f32::INFINITY));
        edit.request_focus();
        let problem = name_problem(&name);
        let exists = s.layouts.user.contains_key(name.trim());
        if let Some(p) = problem {
            ui.label(egui::RichText::new(p).weak());
        } else if exists {
            ui.label(egui::RichText::new("Replaces the saved layout with this name.").weak());
        }
        ui.add_space(8.0);
        let enter = edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
        ui.horizontal(|ui| {
            let save = ui.add_enabled(problem.is_none(), egui::Button::new("Save"));
            if problem.is_none() && (save.clicked() || enter) {
                result = Some(Action::SaveLayout(name.trim().to_owned()));
                close = true;
            }
            if ui.button("Cancel").clicked() || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
                close = true;
            }
        });
    });
    if !close {
        s.layouts.save_prompt = Some(name);
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_dock::NodeIndex;

    fn temp_root(name: &str) -> std::path::PathBuf {
        let root =
            std::env::temp_dir().join(format!("misty_layouts_{name}_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        root
    }

    fn tab_paths(dock: &DockState<Tab>) -> Vec<String> {
        dock.iter_all_tabs()
            .map(|(p, t)| format!("{p:?} {t:?}"))
            .collect()
    }

    #[test]
    fn modified_layout_round_trips() {
        let mut dock = crate::layout::default_layout();
        // Close the Console and split a Game view off to the left.
        let console = dock.find_tab(&Tab::Console).unwrap();
        dock.remove_tab(console);
        dock.main_surface_mut()
            .split_left(NodeIndex::root(), 0.3, vec![Tab::Console]);

        let layouts = Layouts {
            current_name: "Mine".into(),
            ..Default::default()
        };
        let text = to_json(&layouts, &dock);
        let file: LayoutFile = serde_json::from_str(&text).unwrap();
        let back = file.current.unwrap();
        assert_eq!(tab_paths(&back), tab_paths(&dock));
        assert_eq!(file.current_name, "Mine");
        // Stable: serializing the loaded copy gives the same text.
        assert_eq!(to_json(&layouts, &back), text);
    }

    #[test]
    fn save_and_load_from_library_folder() {
        let root = temp_root("save");
        let mut layouts = Layouts::default();
        layouts
            .user
            .insert("Coding".into(), BuiltinLayout::Tall.build());
        layouts.current_name = "Coding".into();
        let dock = BuiltinLayout::Wide.build();
        Autosave::default().tick(&root, &layouts, &dock, true);

        let (loaded, loaded_dock) = load(&root);
        assert_eq!(loaded.current_name, "Coding");
        assert_eq!(loaded.user.keys().collect::<Vec<_>>(), ["Coding"]);
        assert_eq!(tab_paths(&loaded_dock), tab_paths(&dock));
    }

    #[test]
    fn missing_or_corrupt_file_gives_default_layout() {
        let root = temp_root("corrupt");
        let default = tab_paths(&crate::layout::default_layout());
        assert_eq!(tab_paths(&load(&root).1), default);
        std::fs::create_dir_all(root.join("Library")).unwrap();
        std::fs::write(root.join(LAYOUT_FILE), "{ not json").unwrap();
        let (layouts, dock) = load(&root);
        assert_eq!(tab_paths(&dock), default);
        assert_eq!(layouts.current_name, "Default");
    }

    #[test]
    fn save_prompt_queues_a_save_action() {
        use egui_kittest::Harness;
        use egui_kittest::kittest::Queryable;

        let root = temp_root("prompt");
        let mut state = EditorState::new(crate::project::ProjectBrowser::open(root));
        state.layouts.save_prompt = Some(String::new());
        let mut h = Harness::builder().with_size([800.0, 600.0]).build_ui_state(
            |ui, s: &mut EditorState| {
                if let Some(a) = save_prompt_ui(ui.ctx(), s) {
                    s.push(a);
                }
            },
            state,
        );
        h.run();
        h.get_by_role(egui::accesskit::Role::TextInput)
            .type_text("Coding");
        h.run();
        h.get_by_label("Save").click();
        h.run();
        assert!(h.state().layouts.save_prompt.is_none());
        assert!(matches!(
            h.state().actions.as_slice(),
            [Action::SaveLayout(name)] if name == "Coding"
        ));
    }

    #[test]
    fn layout_names_are_checked() {
        assert!(name_problem("  ").is_some());
        assert!(name_problem("Tall").is_some());
        assert!(name_problem("My Layout").is_none());
    }
}
