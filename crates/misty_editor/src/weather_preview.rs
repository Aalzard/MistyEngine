//! Edit-mode weather: while a Weather volume has Preview in Edit Mode on, the scene's weather
//! runs (timelines, wetness, snow cover, lightning) around the Scene camera, so the Scene and
//! Game views show it without entering Play. Off again: the weather stops and resets.

use crate::state::EditorState;

/// Advance the preview by `dt`. Returns true when the views need repainting.
pub fn tick(s: &mut EditorState, dt: f32) -> bool {
    if s.play.in_play_mode() {
        return false;
    }
    let previewing = misty_engine::weather::volumes(&s.scene)
        .iter()
        .any(|(_, w, _)| w.preview_in_edit_mode);
    if !previewing {
        if s.scene.weather.live {
            s.scene.weather.reset();
            return true;
        }
        return false;
    }
    let eye = s.scene_view.camera.position();
    // Long gaps (the window was idle) count as one short step.
    misty_engine::weather::update_scene(&mut s.scene, dt.clamp(0.0, 0.1), eye);
    true
}
