//! Play mode: snapshot on Play, run the game loop, restore everything on Stop.

use misty_engine::runtime::{self, Time};
use misty_engine::scene::Scene;

use crate::selection::Selection;
use crate::state::{EditorState, PlayState};
use crate::undo::UndoStack;

/// Edit-mode state saved when Play starts.
pub struct PlaySession {
    scene: Scene,
    selection: Selection,
    undo: UndoStack,
    dirty: bool,
}

/// Enter Play mode (optionally starting paused, like pressing Pause before Play).
pub fn start(s: &mut EditorState) {
    if s.play.in_play_mode() {
        return;
    }
    if s.console.clear_on_play {
        s.console.clear();
    }
    s.play_session = Some(PlaySession {
        scene: s.scene.clone(),
        selection: s.selection.clone(),
        undo: std::mem::take(&mut s.undo),
        dirty: s.scene_dirty,
    });
    s.time = Time::default();
    s.play = if s.pause_on_play {
        PlayState::Paused
    } else {
        PlayState::Playing
    };
    s.console.info("Entered Play mode.");
}

/// Leave Play mode: every change made while playing is reverted.
pub fn stop(s: &mut EditorState) {
    let Some(session) = s.play_session.take() else {
        return;
    };
    s.scene = session.scene;
    s.selection = session.selection;
    s.selection.retain_existing(&s.scene);
    s.undo = session.undo;
    s.scene_dirty = session.dirty;
    s.play = PlayState::Edit;
    s.console.info("Exited Play mode.");
}

/// Run one frame of the game if playing (or a single step while paused).
pub fn tick(s: &mut EditorState, real_dt: f32) {
    let run = match s.play {
        PlayState::Playing => true,
        PlayState::Paused => std::mem::take(&mut s.step_requested),
        PlayState::Edit => false,
    };
    if !run {
        return;
    }
    // A single step advances one nominal frame.
    let dt = if s.play == PlayState::Paused {
        1.0 / 60.0
    } else {
        real_dt
    };
    s.time.advance(dt);
    runtime::update(&mut s.scene, &s.time);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_play_test"),
        ))
    }

    #[test]
    fn changes_during_play_are_reverted_on_stop() {
        let mut s = state();
        let cam = s.scene.roots()[0];
        s.record("Before Play");
        start(&mut s);
        assert!(s.play.in_play_mode());
        s.scene.game_object_mut(cam).name = "Changed".into();
        let extra = s.scene.spawn("Spawned in play", None);
        s.selection.set(extra);
        s.record("During Play");
        stop(&mut s);
        assert_eq!(s.scene.name(cam), "Main Camera");
        assert_eq!(s.scene.len(), 2);
        assert!(
            s.selection.is_empty(),
            "selection of play-only objects is dropped"
        );
        assert_eq!(
            s.undo.undo_label(),
            Some("Before Play"),
            "undo history is the edit-mode one"
        );
    }

    #[test]
    fn pause_freezes_time_and_step_advances_one_frame() {
        let mut s = state();
        start(&mut s);
        tick(&mut s, 0.1);
        assert_eq!(s.time.frame, 1);
        s.play = PlayState::Paused;
        tick(&mut s, 0.1);
        assert_eq!(s.time.frame, 1, "paused");
        s.step_requested = true;
        tick(&mut s, 0.1);
        assert_eq!(s.time.frame, 2);
        assert!((s.time.delta - 1.0 / 60.0).abs() < 1e-6);
    }

    #[test]
    fn pause_before_play_starts_paused() {
        let mut s = state();
        s.pause_on_play = true;
        start(&mut s);
        assert_eq!(s.play, PlayState::Paused);
    }
}
