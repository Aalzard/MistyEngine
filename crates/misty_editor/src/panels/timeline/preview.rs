//! Edit-mode preview, run before the UI pass: while previewing, the timeline is evaluated onto
//! the scene at the playhead every frame (Activation and Animation tracks). Every value it
//! overrides is backed up the first time and written back when the preview ends, so previewing
//! never changes the saved scene. Audio and signals only run in Play mode.

use misty_engine::timeline::{TrackKind, evaluate};

use super::{current, director, sync_target};
use crate::state::EditorState;

/// Before the UI: advance window playback and pose the scene. In Play mode the playhead
/// follows the running director instead.
pub fn pre_frame(s: &mut EditorState, dt: f32) {
    if s.play.in_play_mode() {
        stop_preview(s);
        follow_game(s);
        return;
    }
    sync_target(s);
    let Some((Some(d), path)) = current(s) else {
        stop_preview(s);
        return;
    };
    if s.timeline.preview_director.is_some_and(|p| p != d) {
        stop_preview(s);
    }
    if !s.timeline.preview {
        return;
    }
    let Ok(asset) = s.anim_clips.timelines.get(&path).cloned() else {
        return;
    };
    // Load the clips the animation tracks play (the clip store reads them on first use).
    for tr in asset
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Animation)
    {
        for c in &tr.clips {
            if let Some(a) = &c.asset
                && let Err(e) = s.anim_clips.get(a)
                && s.timeline.load_error.as_deref() != Some(e.as_str())
            {
                s.console.warn(e.clone());
                s.timeline.load_error = Some(e);
            }
        }
    }
    let Some(comp) = director(s, d) else { return };
    let w = &mut s.timeline;
    w.preview_director = Some(d);
    let duration = asset.duration();
    if w.playing {
        w.time += dt;
        if w.time > duration {
            w.time = if duration > 0.0 { 0.0 } else { duration };
        }
    }
    let bind = |id: &str| comp.binding(id);
    evaluate::apply(
        &mut s.scene,
        &asset,
        &bind,
        &s.anim_clips,
        w.time,
        &mut w.base,
    );
}

/// Play mode: the playhead shows the target director's live time.
fn follow_game(s: &mut EditorState) {
    sync_target(s);
    let (Some(d), Some(rt)) = (s.timeline.director, s.runtime.as_ref()) else {
        return;
    };
    let (t, _) = rt.director_status(d);
    s.timeline.time = t as f32;
}

/// End the preview: the scene goes back to what it was before it started.
pub fn stop_preview(s: &mut EditorState) {
    let w = &mut s.timeline;
    let base = std::mem::take(&mut w.base);
    base.restore(&mut s.scene);
    w.preview = false;
    w.playing = false;
    w.preview_director = None;
}
