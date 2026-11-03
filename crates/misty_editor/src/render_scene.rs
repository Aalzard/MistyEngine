//! Renderer input for the editor's views; the scene → draws/lights code lives in the engine
//! (`misty_engine::render_scene`, shared with the player). This adds the selection outline.

use misty_engine::render::{MaterialParams, MeshDraw};
use misty_engine::render_scene::{self, MaterialSource};
use misty_engine::scene::Scene;

pub use misty_engine::render_scene::{headlight, lights};

use crate::assets::AssetDatabase;
use crate::selection::Selection;

impl MaterialSource for AssetDatabase {
    fn params(&self, rel: Option<&str>) -> MaterialParams {
        AssetDatabase::params(self, rel)
    }
}

/// Visible meshes. Selected objects (and their children) get the selection outline.
pub fn mesh_draws(
    scene: &Scene,
    assets: &AssetDatabase,
    selection: Option<&Selection>,
) -> Vec<MeshDraw> {
    render_scene::mesh_draws(scene, assets, |e| {
        selection.is_some_and(|s| s.list().iter().any(|sel| scene.is_ancestor(*sel, e)))
    })
}
