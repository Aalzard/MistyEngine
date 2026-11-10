//! What Enter (default) and Alt+Enter (secondary actions) do with a result.

use crate::actions::Action;
use crate::layout::Tab;
use crate::state::EditorState;

use super::{Hit, Target};

/// One thing that can be done with a result.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Command {
    /// Objects: select and frame in the Scene view (default).
    SelectAndFrame,
    Select,
    CopyName,
    /// Assets: show in the Project window and Inspector (default).
    Ping,
    /// Open the asset as a double-click would (scenes load, scripts open in the Code Editor).
    Open,
    /// Scenes: open next to the open ones.
    OpenAdditive,
    /// Component presets: apply to the selected objects (one undo step).
    ApplyPreset,
    ShowInExplorer,
    CopyPath,
    /// Files: load again (Project ▸ Reimport).
    Reimport,
    /// Files: select the loaded scenes' objects that use the asset.
    FindReferences,
    /// Menu commands.
    Run,
    /// Settings pages and guides.
    OpenPage,
    /// Calculator.
    CopyResult,
    /// Recent searches: put the text back in the box.
    UseText,
}

impl Command {
    pub fn label(self) -> &'static str {
        match self {
            Command::SelectAndFrame => "Select and frame",
            Command::Select => "Select",
            Command::CopyName => "Copy name",
            Command::Ping => "Show in Asset Files",
            Command::Open => "Open",
            Command::OpenAdditive => "Open Scene Additive",
            Command::ApplyPreset => "Apply to selection",
            Command::ShowInExplorer => crate::panels::project_menus::explorer_label(),
            Command::CopyPath => "Copy path",
            Command::Reimport => "Reimport",
            Command::FindReferences => "Find References In Scene",
            Command::Run => "Run",
            Command::OpenPage => "Open",
            Command::CopyResult => "Copy result",
            Command::UseText => "Search again",
        }
    }
}

/// Actions for a result, default first.
pub fn commands(hit: &Hit) -> Vec<Command> {
    use Command::*;
    match &hit.target {
        Target::Object(_) => vec![SelectAndFrame, Select, CopyName],
        Target::Asset { rel, is_dir: false } if rel.ends_with(".scene") => {
            vec![Ping, Open, OpenAdditive, ShowInExplorer, CopyPath, Reimport]
        }
        Target::Asset { rel, is_dir: false } if rel.ends_with(".preset") => {
            vec![Ping, ApplyPreset, ShowInExplorer, CopyPath, Reimport]
        }
        Target::Asset { is_dir: false, .. } => {
            vec![
                Ping,
                Open,
                ShowInExplorer,
                CopyPath,
                Reimport,
                FindReferences,
            ]
        }
        Target::Asset { .. } => vec![Ping, Open, ShowInExplorer, CopyPath],
        Target::Menu(_) => vec![Run],
        Target::Settings(_) | Target::Doc(_) => vec![OpenPage],
        Target::Calc(_) => vec![CopyResult],
        Target::Recent(_) => vec![UseText],
    }
}

/// What the palette does after a command.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum After {
    Close,
    /// Stay open with this text.
    Search(String),
}

/// Carry out `cmd` on `hit`.
pub fn run(s: &mut EditorState, hit: &Hit, cmd: Command) -> After {
    match (&hit.target, cmd) {
        (Target::Recent(text), _) => return After::Search(text.clone()),
        (Target::Object(e), _) if !s.scene.world.contains(*e) => {
            s.console.warn(format!("'{}' no longer exists.", hit.label));
        }
        (Target::Object(e), Command::SelectAndFrame) => {
            s.select(*e);
            s.push(Action::FrameSelected);
        }
        (Target::Object(e), Command::Select) => s.select(*e),
        (Target::Object(_), _) => s.os_clipboard_out = Some(hit.label.clone()),
        (Target::Asset { rel, is_dir }, Command::Open) => {
            let abs = s.assets.absolute(rel);
            if *is_dir {
                s.project.search.clear();
                s.project.open_folder(abs);
                s.push(Action::FocusTab(Tab::Project));
            } else {
                crate::panels::project::open_file(s, Some(abs));
            }
        }
        (Target::Asset { rel, .. }, Command::OpenAdditive) => {
            s.push(Action::OpenSceneAdditive(rel.clone()));
        }
        (Target::Asset { rel, .. }, Command::ApplyPreset) => {
            let entities = s.selection.list().to_vec();
            if entities.is_empty() {
                s.console.warn("Select the objects to apply the preset to.");
            } else {
                s.push(Action::ApplyPreset {
                    entities,
                    preset: rel.clone(),
                });
            }
        }
        (Target::Asset { rel, .. }, Command::Reimport) => {
            let abs = s.assets.absolute(rel);
            s.push(Action::ReimportAsset(abs));
        }
        (Target::Asset { rel, .. }, Command::FindReferences) => {
            let abs = s.assets.absolute(rel);
            s.push(Action::FindReferencesInScene(abs));
        }
        (Target::Asset { rel, .. }, Command::ShowInExplorer) => {
            let abs = s.assets.absolute(rel);
            s.push(Action::ShowInExplorer(abs));
        }
        (Target::Asset { rel, .. }, Command::CopyPath) => s.os_clipboard_out = Some(rel.clone()),
        (Target::Asset { rel, .. }, _) => {
            crate::prefabs::select_asset(s, rel);
            s.push(Action::FocusTab(Tab::Project));
        }
        (Target::Menu(r), _) => super::menu_items::run(s, r),
        (Target::Settings(page), _) => page.open(s),
        (Target::Doc(file), _) => super::pages::open_doc(s, file),
        (Target::Calc(v), _) => s.os_clipboard_out = Some(v.clone()),
    }
    After::Close
}
