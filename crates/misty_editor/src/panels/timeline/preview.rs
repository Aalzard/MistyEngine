//! Edit-mode preview, run before the UI pass: while previewing, the timeline is evaluated onto
//! the scene at the playhead every frame (Activation, Animation and Control tracks, with
//! sub-timelines), and Camera tracks pose the brain camera so the Scene and Game views show
//! the shot. Every value it overrides is backed up the first time and written back when the
//! preview ends, so previewing never changes the saved scene. Audio, particles, impulses and
//! signals only run in Play mode. Record mode (see [`super::record`]) keys hand-made changes
//! around the evaluation.

use misty_engine::scene::PlayableDirector;
use misty_engine::timeline::tracks::{camera, control};
use misty_engine::timeline::{TimelineAsset, TrackKind, evaluate};

use super::{current, director, record, sync_target};
use crate::state::EditorState;

/// Aspect the Camera track preview solves virtual cameras for (the Game view's usual 16:9).
const PREVIEW_ASPECT: f32 = 16.0 / 9.0;

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
    let Some(comp) = director(s, d) else { return };
    // Keys for what was changed by hand since the last frame (record mode).
    record::capture(s, &path);
    let Ok(asset) = s.anim_clips.timelines.get(&path).cloned() else {
        return;
    };
    load_sources(s, &asset, &comp, 0);
    let w = &mut s.timeline;
    w.preview_director = Some(d);
    let duration = asset.duration();
    if w.playing {
        w.time += dt;
        if w.time > duration {
            w.time = if duration > 0.0 { 0.0 } else { duration };
        }
    }
    let shots = evaluate::apply(
        &mut s.scene,
        &asset,
        &comp,
        &s.anim_clips,
        w.time,
        &mut w.base,
    );
    camera::preview(&mut s.scene, &shots, &mut w.base, PREVIEW_ASPECT);
    record::snapshot(s, &path);
}

/// Load the clips the animation tracks play and the sub-timelines Control clips play (and
/// theirs), so evaluation finds them in the stores.
fn load_sources(s: &mut EditorState, asset: &TimelineAsset, d: &PlayableDirector, depth: usize) {
    for tr in &asset.tracks {
        match tr.kind {
            TrackKind::Animation => {
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
            TrackKind::Control if depth + 1 < evaluate::MAX_DEPTH => {
                for c in tr.clips.iter().filter(|c| c.control.sub_timeline) {
                    let Some(obj) = control::object(c, d, &s.scene) else {
                        continue;
                    };
                    let Some((sub, path)) = control::sub_director(&s.scene, obj) else {
                        continue;
                    };
                    if let Ok(nested) = s.anim_clips.timelines.get(&path).cloned() {
                        load_sources(s, &nested, &sub, depth + 1);
                    }
                }
            }
            _ => {}
        }
    }
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

/// End the preview: the scene goes back to what it was before it started (recording stops).
pub fn stop_preview(s: &mut EditorState) {
    let w = &mut s.timeline;
    let base = std::mem::take(&mut w.base);
    base.restore(&mut s.scene);
    w.preview = false;
    w.playing = false;
    w.preview_director = None;
    w.recording = None;
    w.rec_snapshot.clear();
}
