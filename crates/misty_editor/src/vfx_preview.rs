//! Visual Effects in Edit mode: every active effect simulates in the Scene view (the effect
//! being edited in the Effect Graph window shows unsaved changes live), like the reference
//! engine's GPU effects. The view repaints continuously only while an effect is selected or the
//! Effect Graph window has one open, so an idle editor stays idle.

use misty_engine::hecs::Entity;
use misty_engine::vfx::{self, EffectInstance, VisualEffect};

use crate::state::EditorState;

/// Step every active effect by `dt` (Edit mode). True when the view should keep repainting.
pub fn tick(s: &mut EditorState, dt: f32) -> bool {
    if s.play.in_play_mode() {
        return false;
    }
    let any = s
        .scene
        .world
        .query::<&VisualEffect>()
        .iter()
        .next()
        .is_some();
    if !any {
        return false;
    }
    // Long gaps (the window was idle) count as one short step.
    vfx::update(&mut s.scene, dt.min(0.1));
    let editing = crate::panels::effect_graph::preview_targets(s);
    let watched = |e: Entity| {
        editing.contains(&e)
            || s.selection
                .list()
                .iter()
                .any(|sel| s.scene.is_ancestor(*sel, e))
    };
    let list: Vec<Entity> = s
        .scene
        .world
        .query::<(Entity, &EffectInstance)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    list.into_iter().any(watched)
}
