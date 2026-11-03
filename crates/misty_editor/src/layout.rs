//! Dockable panel tabs and the built-in layouts.

use egui_dock::{DockState, NodeIndex, NodePath};
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Tab {
    Scene,
    Game,
    Hierarchy,
    Inspector,
    Project,
    Console,
    Animation,
    /// Window ▸ Rendering ▸ Lighting.
    Lighting,
    /// Window ▸ Animation ▸ Animator.
    Animator,
    /// Window ▸ Audio ▸ Audio Mixer.
    AudioMixer,
    /// Window ▸ AI ▸ Navigation.
    Navigation,
}

impl Tab {
    /// Tabs every built-in layout contains.
    pub const ALL: [Tab; 7] = [
        Tab::Scene,
        Tab::Game,
        Tab::Inspector,
        Tab::Hierarchy,
        Tab::Project,
        Tab::Console,
        Tab::Animation,
    ];

    /// Tabs no built-in layout contains; opened from the Window menu.
    pub const ON_DEMAND: [Tab; 4] = [Tab::Lighting, Tab::Animator, Tab::AudioMixer, Tab::Navigation];

    /// Built-in layout tabs, then on-demand ones.
    pub fn every() -> impl Iterator<Item = Tab> {
        Self::ALL.into_iter().chain(Self::ON_DEMAND)
    }

    /// Where an on-demand tab docks when first opened (Unity puts Lighting by the Inspector).
    fn opens_beside(self) -> Option<Tab> {
        match self {
            Tab::Lighting | Tab::Navigation => Some(Tab::Inspector),
            // The graph wants room: it docks with the Scene and Game views.
            Tab::Animator => Some(Tab::Scene),
            // Docks with the Project and Console like the reference editor.
            Tab::AudioMixer => Some(Tab::Project),
            _ => None,
        }
    }

    pub fn title(self) -> &'static str {
        match self {
            Tab::Scene => "Scene",
            Tab::Game => "Game",
            Tab::Hierarchy => "Hierarchy",
            Tab::Inspector => "Inspector",
            Tab::Project => "Project",
            Tab::Console => "Console",
            Tab::Animation => "Animation",
            Tab::Lighting => "Lighting",
            Tab::Animator => "Animator",
            Tab::AudioMixer => "Audio Mixer",
            Tab::Navigation => "Navigation",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Tab::Scene => "#",
            Tab::Game => "🎮",
            Tab::Hierarchy => "☰",
            Tab::Inspector => "ℹ",
            Tab::Project => "🗁",
            Tab::Console => "🗒",
            Tab::Animation => "⏵",
            Tab::Lighting => "☀",
            Tab::Animator => crate::panels::animator::CONTROLLER_ICON,
            Tab::AudioMixer => crate::panels::audio_mixer::MIXER_ICON,
            Tab::Navigation => "🚶",
        }
    }

    /// Window-menu shortcut label.
    pub fn shortcut(self) -> &'static str {
        match self {
            Tab::Scene => "Ctrl+1",
            Tab::Game => "Ctrl+2",
            Tab::Inspector => "Ctrl+3",
            Tab::Hierarchy => "Ctrl+4",
            Tab::Project => "Ctrl+5",
            Tab::Console => "Ctrl+Shift+C",
            Tab::Animation => "Ctrl+6",
            Tab::Lighting | Tab::Animator | Tab::AudioMixer | Tab::Navigation => "",
        }
    }
}

/// Layouts that ship with the editor (Window ▸ Layouts).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuiltinLayout {
    Default,
    TwoByThree,
    FourSplit,
    Tall,
    Wide,
}

impl BuiltinLayout {
    pub const ALL: [BuiltinLayout; 5] = [
        BuiltinLayout::TwoByThree,
        BuiltinLayout::FourSplit,
        BuiltinLayout::Default,
        BuiltinLayout::Tall,
        BuiltinLayout::Wide,
    ];

    pub fn name(self) -> &'static str {
        match self {
            BuiltinLayout::Default => "Default",
            BuiltinLayout::TwoByThree => "2 by 3",
            BuiltinLayout::FourSplit => "4 Split",
            BuiltinLayout::Tall => "Tall",
            BuiltinLayout::Wide => "Wide",
        }
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|b| b.name() == name)
    }

    pub fn build(self) -> DockState<Tab> {
        match self {
            BuiltinLayout::Default => default_layout(),
            BuiltinLayout::TwoByThree => two_by_three(),
            BuiltinLayout::FourSplit => four_split(),
            BuiltinLayout::Tall => tall(),
            BuiltinLayout::Wide => wide(),
        }
    }
}

/// Default layout: Hierarchy | Scene+Game over Project+Console+Animation, Inspector on the right.
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene, Tab::Game]);
    let tree = dock.main_surface_mut();
    let [left, _inspector] = tree.split_right(NodeIndex::root(), 0.78, vec![Tab::Inspector]);
    let [top, _bottom] =
        tree.split_below(left, 0.66, vec![Tab::Project, Tab::Console, Tab::Animation]);
    let [_scene, _hierarchy] = tree.split_left(top, 0.24, vec![Tab::Hierarchy]);
    dock
}

/// Scene over Game on the left, then Hierarchy, Project+Console+Animation and Inspector columns.
fn two_by_three() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene]);
    let tree = dock.main_surface_mut();
    let [views, rest] = tree.split_right(NodeIndex::root(), 0.5, vec![Tab::Hierarchy]);
    let [_scene, _game] = tree.split_below(views, 0.5, vec![Tab::Game]);
    let [_hierarchy, rest] =
        tree.split_right(rest, 0.33, vec![Tab::Project, Tab::Console, Tab::Animation]);
    let [_project, _inspector] = tree.split_right(rest, 0.5, vec![Tab::Inspector]);
    dock
}

/// A 2×2 grid (Scene, Game / Hierarchy, Project+Console+Animation) with the Inspector on the right.
fn four_split() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene]);
    let tree = dock.main_surface_mut();
    let [grid, _inspector] = tree.split_right(NodeIndex::root(), 0.78, vec![Tab::Inspector]);
    let [top, bottom] = tree.split_below(grid, 0.5, vec![Tab::Hierarchy]);
    let [_scene, _game] = tree.split_right(top, 0.5, vec![Tab::Game]);
    let [_hierarchy, _project] = tree.split_right(
        bottom,
        0.5,
        vec![Tab::Project, Tab::Console, Tab::Animation],
    );
    dock
}

/// Full-height Scene+Game, then Hierarchy over Project+Console+Animation, Inspector on the right.
fn tall() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene, Tab::Game]);
    let tree = dock.main_surface_mut();
    let [_views, rest] = tree.split_right(NodeIndex::root(), 0.55, vec![Tab::Hierarchy]);
    let [column, _inspector] = tree.split_right(rest, 0.5, vec![Tab::Inspector]);
    let [_hierarchy, _project] = tree.split_below(
        column,
        0.5,
        vec![Tab::Project, Tab::Console, Tab::Animation],
    );
    dock
}

/// Wide Scene+Game on top, Hierarchy | Project+Console+Animation below, full-height Inspector.
fn wide() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene, Tab::Game]);
    let tree = dock.main_surface_mut();
    let [left, _inspector] = tree.split_right(NodeIndex::root(), 0.8, vec![Tab::Inspector]);
    let [_views, bottom] = tree.split_below(left, 0.62, vec![Tab::Hierarchy]);
    let [_hierarchy, _project] = tree.split_right(
        bottom,
        0.3,
        vec![Tab::Project, Tab::Console, Tab::Animation],
    );
    dock
}

/// Focus a tab if it is open anywhere, otherwise add it to the focused area
/// (on-demand tabs go next to their usual neighbour when it is open).
pub fn focus_or_open(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(NodePath {
            surface: path.surface,
            node: path.node,
        });
    } else {
        if let Some(beside) = tab.opens_beside().and_then(|b| dock.find_tab(&b)) {
            dock.set_focused_node_and_surface(NodePath {
                surface: beside.surface,
                node: beside.node,
            });
        }
        dock.push_to_focused_leaf(tab);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_builtin_layout_has_each_tab_once() {
        for layout in BuiltinLayout::ALL {
            let dock = layout.build();
            let mut tabs: Vec<Tab> = dock.iter_all_tabs().map(|(_, t)| *t).collect();
            assert_eq!(tabs.len(), Tab::ALL.len(), "{}", layout.name());
            for tab in Tab::ALL {
                tabs.retain(|t| *t != tab);
            }
            assert!(tabs.is_empty(), "{}", layout.name());
            assert_eq!(BuiltinLayout::from_name(layout.name()), Some(layout));
            // On-demand windows (Lighting) open from the Window menu instead.
            for tab in Tab::ON_DEMAND {
                assert!(dock.find_tab(&tab).is_none(), "{}", layout.name());
            }
        }
    }

    #[test]
    fn lighting_opens_next_to_the_inspector() {
        let mut dock = default_layout();
        focus_or_open(&mut dock, Tab::Lighting);
        let lighting = dock.find_tab(&Tab::Lighting).expect("opened");
        let inspector = dock.find_tab(&Tab::Inspector).unwrap();
        assert_eq!(lighting.node, inspector.node);
        // A second request only focuses it.
        focus_or_open(&mut dock, Tab::Lighting);
        assert_eq!(dock.iter_all_tabs().count(), Tab::ALL.len() + 1);
    }
}
