//! Dock tab viewer: routes each tab to its panel.

pub mod animation;
pub mod animator;
pub mod asset_groups;
pub mod audio_mixer;
pub mod code_editor;
mod console;
pub(crate) mod folder_colors;
mod game;
mod hierarchy;
mod inspector;
pub use inspector::preset_window;
pub(crate) use inspector::script_fields;
#[cfg(test)]
pub(crate) use inspector::show as inspector_show;
pub(crate) use inspector::slots::texture_slot;
pub mod effect_graph;
pub mod frame_debugger;
pub mod input_actions;
mod lighting;
mod lighting_hdri;
mod prefab_rows;
pub(crate) use inspector::prefab_overrides::demo as prefab_demo;
pub mod memory_profiler;
pub mod navigation;
pub mod occlusion;
pub mod packages;
pub mod profiler;
pub mod project;
pub(crate) mod project_menus;
mod project_tree;
pub(crate) mod scene;
pub mod shader_graph;
pub mod test_runner;
pub mod tile_palette;
pub mod timeline;
pub mod undo_history;
pub mod world_builder;

#[cfg(test)]
pub use game::stats_lines as game_stats_lines;

use egui::{Id, Ui, WidgetText};
use egui_dock::TabViewer;

use crate::layout::Tab;
use crate::state::EditorState;
use crate::viewport::Gpu;

/// Drag-and-drop payload: a project-relative asset path ("Assets/…").
#[derive(Clone, Debug)]
pub struct AssetPayload(pub String);

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
        if let Tab::Tool(id) = tab {
            let title = crate::editor_scripts::windows::title(self.state, *id);
            return format!("{} {title}", tab.icon()).into();
        }
        format!("{} {}", tab.icon(), tab.title()).into()
    }

    fn ui(&mut self, ui: &mut Ui, tab: &mut Tab) {
        if crate::tutorials::targets::enabled() {
            let id = format!("panel.{}", tab.title());
            crate::tutorials::targets::mark(ui, &id, ui.max_rect());
        }
        match tab {
            Tab::Scene => scene::show(ui, self.state, self.gpu.as_deref_mut()),
            Tab::Game => game::show(ui, self.state, self.gpu.as_deref_mut()),
            Tab::Hierarchy => hierarchy::show(ui, self.state),
            Tab::Inspector => inspector::show(ui, self.state),
            Tab::Project => project::show(ui, self.state),
            Tab::Console => console::show(ui, self.state),
            Tab::Animation => animation::show(ui, self.state),
            Tab::Lighting => lighting::show(ui, self.state),
            Tab::Animator => animator::show(ui, self.state),
            Tab::AudioMixer => audio_mixer::show(ui, self.state),
            Tab::Navigation => navigation::show(ui, self.state),
            Tab::CodeEditor => code_editor::show(ui, self.state),
            Tab::Profiler => profiler::show(ui, self.state),
            Tab::WorldBuilder => world_builder::show(ui, self.state),
            Tab::UndoHistory => undo_history::show(ui, self.state),
            Tab::Timeline => timeline::show(ui, self.state),
            Tab::TilePalette => tile_palette::show(ui, self.state),
            Tab::ShaderGraph => shader_graph::show(ui, self.state),
            Tab::EffectGraph => effect_graph::show(ui, self.state),
            Tab::OcclusionCulling => occlusion::show(ui, self.state),
            Tab::FrameDebugger => frame_debugger::show(ui, self.state),
            Tab::MemoryProfiler => memory_profiler::show(ui, self.state, self.gpu.as_deref()),
            Tab::TestRunner => test_runner::show(ui, self.state),
            Tab::AssetGroups => asset_groups::show(ui, self.state),
            Tab::PackageManager => packages::show(ui, self.state),
            Tab::InputActions => input_actions::show(ui, self.state),
            Tab::Tool(id) => crate::editor_scripts::windows::show(ui, self.state, *id),
        }
    }

    // Scene/Game views draw edge to edge; the other panels manage their own scrolling.
    fn scroll_bars(&self, _tab: &Tab) -> [bool; 2] {
        [false, false]
    }
}
