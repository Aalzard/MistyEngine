//! Dock tab viewer: routes each tab to its panel.

mod console;
mod game;
mod hierarchy;
mod inspector;
mod project;
mod scene;

use egui::{Id, Ui, WidgetText};
use egui_dock::TabViewer;

use crate::layout::Tab;
use crate::state::EditorState;
use crate::viewport::Gpu;

pub struct PanelViewer<'a> {
    pub state: &'a mut EditorState,
    /// `None` when the renderer is not wgpu (views then show a placeholder).
    pub gpu: Option<&'a mut Gpu>,
}

impl TabViewer for PanelViewer<'_> {
    type Tab = Tab;

    fn id(&mut self, tab: &mut Tab) -> Id {
        Id::new(*tab)
    }

    fn title(&mut self, tab: &mut Tab) -> WidgetText {
        format!("{} {}", tab.icon(), tab.title()).into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        match tab {
            Tab::Scene => scene::show(ui, self.state, self.gpu.as_deref_mut()),
            Tab::Game => game::show(ui, self.state, self.gpu.as_deref_mut()),
            Tab::Hierarchy => hierarchy::show(ui, self.state),
            Tab::Inspector => inspector::show(ui, self.state),
            Tab::Project => project::show(ui, self.state),
            Tab::Console => console::show(ui, self.state),
        }
    }

    // Scene/Game views draw edge to edge; the other panels manage their own scrolling.
    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}
