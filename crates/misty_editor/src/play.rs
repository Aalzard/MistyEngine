//! Play mode: snapshot on Play, run the game loop, restore everything on Stop.

use misty_engine::audio::Audio;
use misty_engine::build::SceneRef;
use misty_engine::runtime::{Runtime, Time};
use misty_engine::scripting::{LogKind, Request};

use crate::console::LogLevel;
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

impl PlaySession {
    /// The Edit-mode scene and its undo history, restored on Stop (asset renames rewrite them).
    pub fn scenes_mut(&mut self) -> Vec<&mut Scene> {
        let mut out = vec![&mut self.scene];
        out.extend(self.undo.scenes_mut());
        out
    }
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
    s.time = Time::new(&s.project_settings.time);
    // The editor's clip preview stops when the game starts.
    if let Some(p) = s.clip_preview.as_mut() {
        p.stop_all();
    }
    let mut audio = Audio::with_device(s.project.root.clone());
    audio.set_muted(s.game_view.mute);
    let mut runtime = Runtime::start_with_audio(
        &mut s.scene,
        s.project.root.clone(),
        &s.project_settings,
        audio,
    );
    // Addressables load per the Asset Groups window's Play Mode Script.
    runtime.set_assets(misty_engine::asset_groups::AssetSystem::for_editor(
        &s.project.root,
        s.build_settings.target.slug(),
    ));
    s.runtime = Some(runtime);
    flush_logs(s);
    s.game_view.focused = s.game_view.play_mode_target != 2;
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
    s.runtime = None;
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
    let (muted, paused) = (s.game_view.mute, s.play == PlayState::Paused);
    if let Some(runtime) = s.runtime.as_mut() {
        runtime.set_audio_state(muted, paused);
    }
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
    let input = std::mem::take(&mut s.game_input);
    if let Some(runtime) = s.runtime.as_mut() {
        runtime.update(&mut s.scene, &s.time, input);
    }
    flush_logs(s);
    handle_requests(s);
}

/// `SceneManager.LoadScene` / `UnloadSceneAsync` / `Application.Quit` from scripts.
fn handle_requests(s: &mut EditorState) {
    let Some(runtime) = s.runtime.as_mut() else {
        return;
    };
    // The last single LoadScene of a frame wins; additive loads and unloads run in order.
    let mut load = None;
    for r in runtime.take_requests() {
        match r {
            Request::LoadScene(scene) => load = Some(scene),
            Request::Quit => s.console.info("Application.Quit() ignored in the editor"),
            other => {
                let root = s.project.root.clone();
                let Some(runtime) = s.runtime.as_mut() else {
                    return;
                };
                if let Err(e) =
                    runtime.apply_scene_request(&mut s.scene, &s.build_settings, &root, &other)
                {
                    s.console.log(LogLevel::Error, e);
                }
                s.selection.retain_existing(&s.scene);
            }
        }
    }
    flush_logs(s);
    if let Some(scene) = load {
        load_scene(s, &scene);
    }
}

/// Swap in a build scene while playing (the edit-mode scene comes back on Stop).
fn load_scene(s: &mut EditorState, which: &SceneRef) {
    let loaded = s.build_settings.resolve(which).and_then(|(rel, _)| {
        misty_engine::scene::Scene::load(&s.project.root.join(&rel)).map(|r| (rel, r))
    });
    match loaded {
        Ok((rel, (mut scene, report))) => {
            for w in report.warnings {
                s.console.warn(format!("{rel}: {w}"));
            }
            s.runtime = None;
            let mut audio = Audio::with_device(s.project.root.clone());
            audio.set_muted(s.game_view.mute);
            s.runtime = Some(Runtime::start_with_audio(
                &mut scene,
                s.project.root.clone(),
                &s.project_settings,
                audio,
            ));
            scene.set_scene_path(0, &rel);
            s.scene = scene;
            s.selection.clear();
            s.inspector_targets.clear();
            flush_logs(s);
        }
        Err(e) => s.console.log(LogLevel::Error, e),
    }
}

/// Script output and runtime problems go to the Console.
fn flush_logs(s: &mut EditorState) {
    let Some(runtime) = s.runtime.as_mut() else {
        return;
    };
    for (kind, text) in runtime.take_logs() {
        let level = match kind {
            LogKind::Info => LogLevel::Info,
            LogKind::Warning => LogLevel::Warning,
            LogKind::Error => LogLevel::Error,
        };
        s.console.log(level, text);
    }
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
    fn scripts_load_build_scenes_until_stop_and_quit_is_ignored() {
        use misty_engine::build::BuildScene;
        use misty_engine::scene::{ScriptRef, Scripts};
        let root = std::env::temp_dir().join(format!("misty_play_load_{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
        std::fs::write(
            root.join("Assets/Scripts/Next.lua"),
            "local N = {}
function N:Update() SceneManager.LoadScene(1) Application.Quit() end
return N
",
        )
        .unwrap();
        let mut level = Scene::new("Level2");
        level.spawn("Only In Level2", None);
        level
            .save(&root.join("Assets/Scenes/Level2.scene"))
            .unwrap();

        let mut s = EditorState::new(ProjectBrowser::open(root));
        s.scene.set_scene_path(0, "Assets/Scenes/Menu.scene");
        let e = s.scene.spawn("Loader", None);
        s.scene.add(
            e,
            Scripts {
                list: vec![ScriptRef {
                    path: "Assets/Scripts/Next.lua".into(),
                    enabled: true,
                    fields: Default::default(),
                }],
            },
        );
        s.build_settings.scenes = vec![
            BuildScene::new("Assets/Scenes/Menu.scene"),
            BuildScene::new("Assets/Scenes/Level2.scene"),
        ];
        start(&mut s);
        tick(&mut s, 1.0 / 60.0);
        assert_eq!(s.scene.name, "Level2");
        assert!(s.scene.find_by_name("Only In Level2").is_some());
        assert_eq!(
            s.scene.active_path().as_deref(),
            Some("Assets/Scenes/Level2.scene")
        );
        assert!(
            s.console
                .entries
                .iter()
                .any(|l| l.message == "Application.Quit() ignored in the editor")
        );
        stop(&mut s);
        assert!(
            s.scene.find_by_name("Loader").is_some(),
            "edit scene restored"
        );
        assert_eq!(
            s.scene.active_path().as_deref(),
            Some("Assets/Scenes/Menu.scene")
        );

        // Scenes outside the build list fail with a Console error.
        s.build_settings.scenes.truncate(1);
        start(&mut s);
        tick(&mut s, 1.0 / 60.0);
        assert!(s.scene.find_by_name("Loader").is_some());
        assert!(s.console.count(LogLevel::Error) > 0);
    }

    #[test]
    fn pause_before_play_starts_paused() {
        let mut s = state();
        s.pause_on_play = true;
        start(&mut s);
        assert_eq!(s.play, PlayState::Paused);
    }

    #[test]
    fn rigidbody_falls_in_play_and_returns_on_stop() {
        use crate::actions::{Action, Create};
        use misty_engine::scene::PrimitiveMesh;
        let mut s = state();
        for what in [
            Create::Primitive(PrimitiveMesh::Plane),
            Create::Primitive(PrimitiveMesh::Cube),
        ] {
            crate::commands::apply(&mut s, Action::Create { what, parent: None });
        }
        let cube = s.scene.find_by_name("Cube").unwrap();
        assert_eq!(
            s.scene.component_names(cube),
            ["Transform", "Mesh Filter", "Mesh Renderer", "Box Collider"]
        );
        let mut t = s.scene.transform(cube);
        t.position.y = 4.0;
        s.scene.set_transform(cube, t);
        s.scene.add_by_name(cube, "Rigidbody");

        start(&mut s);
        for _ in 0..240 {
            tick(&mut s, 1.0 / 60.0);
        }
        let y = s.scene.world_position(cube).y;
        assert!((y - 0.5).abs() < 0.05, "landed on the plane: {y}");
        stop(&mut s);
        assert_eq!(s.scene.world_position(cube).y, 4.0);
    }

    #[test]
    fn audio_source_menu_item_and_play_without_clip() {
        use crate::actions::{Action, Create};
        let mut s = state();
        let what = Create::AudioSource;
        crate::commands::apply(&mut s, Action::Create { what, parent: None });
        let e = s.scene.find_by_name("Audio Source").unwrap();
        assert_eq!(s.scene.component_names(e), ["Transform", "Audio Source"]);
        s.game_view.mute = true;
        start(&mut s);
        tick(&mut s, 1.0 / 60.0);
        let stats = s.runtime.as_ref().unwrap().stats();
        assert_eq!(
            stats.voices, 0,
            "no clip, nothing plays (and no device is opened)"
        );
        stop(&mut s);
    }
}
