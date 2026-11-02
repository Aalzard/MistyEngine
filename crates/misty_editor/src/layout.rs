//! Dockable panel tabs and the default layout.

use egui_dock::{DockState, NodeIndex, NodePath};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Tab {
    Scene,
    Game,
    Hierarchy,
    Inspector,
    Project,
    Console,
}

impl Tab {
    pub const ALL: [Tab; 6] = [
        Tab::Scene,
        Tab::Game,
        Tab::Inspector,
        Tab::Hierarchy,
        Tab::Project,
        Tab::Console,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Tab::Scene => "Scene",
            Tab::Game => "Game",
            Tab::Hierarchy => "Hierarchy",
            Tab::Inspector => "Inspector",
            Tab::Project => "Project",
            Tab::Console => "Console",
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
        }
    }
}

/// Default layout: Hierarchy | Scene+Game over Project+Console, Inspector on the right.
pub fn default_layout() -> DockState<Tab> {
    let mut dock = DockState::new(vec![Tab::Scene, Tab::Game]);
    let tree = dock.main_surface_mut();
    let [left, _inspector] = tree.split_right(NodeIndex::root(), 0.78, vec![Tab::Inspector]);
    let [top, _bottom] = tree.split_below(left, 0.66, vec![Tab::Project, Tab::Console]);
    let [_scene, _hierarchy] = tree.split_left(top, 0.24, vec![Tab::Hierarchy]);
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
