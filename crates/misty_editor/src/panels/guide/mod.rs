//! The User Guide window (Help ▸ User Guide, a dockable tab): the guide's chapters rendered in
//! the editor's language, with search and "Open example" buttons that open each guide's scene
//! and Script Graph (copying them into the project first when it doesn't have them).
//!
//! The chapters and pictures are built in ([`content`]), from `docs/guide/`.

pub mod content;
pub mod markdown;
#[cfg(test)]
mod tests;
pub mod view;

use egui::Ui;

use crate::state::EditorState;
pub use view::{GuideAction, GuideView};

pub const ICON: &str = "📖";

pub fn show(ui: &mut Ui, s: &mut EditorState) {
    if let Some(GuideAction::OpenExample(id)) = view::show(ui, &mut s.guide, true) {
        open_example(s, id);
    }
}

/// Help ▸ User Guide: bring the guide forward.
pub fn open(s: &mut EditorState) {
    s.push(crate::actions::Action::FocusTab(crate::layout::Tab::Guide));
}

/// Open chapter `id`'s example: its files are copied into the project when missing, then its
/// scene opens (asking to save changes first) and its graph opens in the Script Graph window.
pub fn open_example(s: &mut EditorState, id: &str) {
    let Some(ex) = content::example(id) else {
        return;
    };
    let mut copied = 0;
    for (rel, bytes) in ex.files {
        let path = s.project.root.join(rel);
        if path.exists() {
            continue;
        }
        let written = path
            .parent()
            .map_or(Ok(()), std::fs::create_dir_all)
            .and_then(|_| std::fs::write(&path, bytes));
        match written {
            Ok(()) => copied += 1,
            Err(e) => {
                s.console.warn(format!("Could not copy {rel}: {e}"));
                return;
            }
        }
    }
    if copied > 0 {
        s.refresh_assets();
        s.console.info(format!(
            "Copied the guide example into {}",
            parent(ex.scene)
        ));
    }
    crate::panels::script_graph::open(s, ex.graph);
    s.push(crate::actions::Action::OpenScenePath(ex.scene.to_owned()));
}

fn parent(path: &str) -> &str {
    path.rsplit_once('/').map_or(path, |(dir, _)| dir)
}
