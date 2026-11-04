//! Renderer input for the editor's views; the scene → draws/lights code lives in the engine
//! (`misty_engine::render_scene`, shared with the player). This adds the selection outline.

use misty_engine::render::{MaterialParams, MeshDraw};
use misty_engine::render_scene::MaterialSource;

pub use misty_engine::render_scene::{environment, headlight, lights, post_settings};

use crate::assets::AssetDatabase;

impl MaterialSource for AssetDatabase {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        AssetDatabase::params(self, rel)
    }
}

/// The Scene view's draws through its persistent cache (only changed objects are rebuilt);
/// selected objects and their children get the selection outline.
pub fn cached_draws(state: &mut crate::state::EditorState) -> std::sync::Arc<Vec<MeshDraw>> {
    let selection = &state.selection;
    state
        .scene_draws
        .update(&state.scene, &state.assets, u32::MAX, |e| {
            selection.list().contains(&e)
        })
}
