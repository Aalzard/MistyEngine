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
}

impl Tab {
    pub const ALL: [Tab; 7] = [
        Tab::Scene,
        Tab::Game,
        Tab::Inspector,
        Tab::Hierarchy,
        Tab::Project,
        Tab::Console,
        Tab::Animation,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Scene => "Scene",
            Tab::Game => "Game",
            Tab::Hierarchy => "Hierarchy",
            Tab::Inspector => "Inspector",
            Tab::Project => "Project",
            Tab::Console => "Console",
            Tab::Animation => "Animation",
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

/// Focus a tab if it is open anywhere, otherwise add it to the focused area.
pub fn focus_or_open(dock: &mut DockState<Tab>, tab: Tab) {
    if let Some(path) = dock.find_tab(&tab) {
        let _ = dock.set_active_tab(path);
        dock.set_focused_node_and_surface(NodePath {
            surface: path.surface,
            node: path.node,
        });
    } else {
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
        }
    }
}
