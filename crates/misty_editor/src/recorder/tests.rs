//! The Recorder package in the editor: not installed by default, Install / Remove bring and
//! take its window, menu item, Timeline track and settings; recordings run in capture time and
//! write exactly the frames of their range.

use std::path::PathBuf;

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::recorder::{OutputFormat, RangeMode, RecorderSettings};

use super::*;
use crate::layout::Tab;
use crate::packages::manifest::{Lock, Manifest};
use crate::project::ProjectBrowser;

/// Recorder requests from scripts go through one queue for the whole process: the tests that
/// run the Recorder's frame take turns.
static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn serial() -> std::sync::MutexGuard<'static, ()> {
    SERIAL.lock().unwrap_or_else(|e| e.into_inner())
}

fn project(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_recorder_{tag}_{n}"));
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    root
}

fn state(root: &std::path::Path) -> EditorState {
    EditorState::new(ProjectBrowser::open(root.to_path_buf()))
}

fn install(s: &mut EditorState) {
    let root = s.project.root.clone();
    s.packages.ensure_loaded(&root);
    s.packages.install_optional(&root, optional::RECORDER);
    s.optional_packages = optional::Installed::load(&root);
    package_changed(s);
}

fn remove(s: &mut EditorState) {
    let root = s.project.root.clone();
    s.packages.ensure_loaded(&root);
    s.packages.remove(&root, optional::RECORDER);
    s.optional_packages = optional::Installed::load(&root);
    package_changed(s);
}

#[test]
fn the_recorder_is_not_installed_by_default() {
    let root = project("default");
    let s = state(&root);
    assert!(!installed(&s));
    assert!(!optional::installed(&root, optional::RECORDER));
    assert!(!crate::menu_bar::general_extra_tabs(&s).contains(&Tab::Recorder));
    // Templates and new projects don't list it either.
    assert!(!root.join(SETTINGS_FILE).exists());
    let manifest = Manifest::load(&root).unwrap();
    assert!(!manifest.dependencies.contains_key(optional::RECORDER));
    // It is offered in the Package Manager's Built-in list.
    assert!(
        optional::all()
            .iter()
            .any(|p| p.name == optional::RECORDER && p.display_name == "Recorder")
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn install_adds_the_dependency_and_the_ui_and_remove_takes_them_away() {
    let root = project("install");
    let mut s = state(&root);
    install(&mut s);
    assert!(installed(&s));
    let manifest = Manifest::load(&root).unwrap();
    assert_eq!(
        manifest
            .dependencies
            .get(optional::RECORDER)
            .map(String::as_str),
        Some(optional::REQUIREMENT)
    );
    let lock = Lock::load(&root).unwrap();
    let entry = lock.dependencies.get(optional::RECORDER).expect("locked");
    assert_eq!(entry.source, "builtin");
    assert_eq!(entry.version, env!("CARGO_PKG_VERSION"));
    // Nothing is copied into Packages/.
    assert!(!root.join("Packages").join(optional::RECORDER).exists());
    assert!(crate::menu_bar::general_extra_tabs(&s).contains(&Tab::Recorder));
    // A fresh editor on the project sees it.
    assert!(installed(&state(&root)));
    // Its settings are saved in the project...
    s.recorder.list(&root).save(&root).unwrap();
    assert!(root.join(SETTINGS_FILE).is_file());

    remove(&mut s);
    assert!(!installed(&s));
    assert!(!crate::menu_bar::general_extra_tabs(&s).contains(&Tab::Recorder));
    let manifest = Manifest::load(&root).unwrap();
    assert!(!manifest.dependencies.contains_key(optional::RECORDER));
    let lock = Lock::load(&root).unwrap();
    assert!(!lock.dependencies.contains_key(optional::RECORDER));
    // ...and go with it; its tab is asked to close.
    assert!(!root.join(SETTINGS_FILE).exists());
    assert!(
        s.actions
            .iter()
            .any(|a| matches!(a, crate::actions::Action::CloseTab(Tab::Recorder)))
    );
    assert!(!installed(&state(&root)));
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_window_says_how_to_install_without_the_package() {
    let root = project("window");
    let s = state(&root);
    let mut h = Harness::builder()
        .with_size([700.0, 500.0])
        .build_ui_state(|ui, s: &mut EditorState| window::show(ui, s), s);
    h.run();
    h.get_by_label("The Recorder package is not installed.");
    assert!(h.query_by_label("⏺ Start Recording").is_none());
    let mut s = h.into_state();
    install(&mut s);
    let mut h = Harness::builder()
        .with_size([700.0, 500.0])
        .build_ui_state(|ui, s: &mut EditorState| window::show(ui, s), s);
    h.run();
    h.get_by_label("⏺ Start Recording");
    assert!(h.query_all_by_label("Movie Recorder").count() >= 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_timeline_recorder_track_needs_the_package() {
    let root = project("track");
    let mut s = state(&root);
    let clip = timeline::new_clip(2.0);
    assert_eq!(clip.duration, timeline::CLIP_LEN);
    assert_eq!(
        clip.recorder.as_ref().map(|r| r.range),
        Some(RangeMode::TimelineClip)
    );
    // The clip's settings survive a save.
    let text = serde_json::to_string(&clip).unwrap();
    let back: misty_engine::timeline::TimelineClip = serde_json::from_str(&text).unwrap();
    assert_eq!(back, clip);
    assert!(!s.optional_packages.has(optional::RECORDER));
    install(&mut s);
    assert!(s.optional_packages.has(optional::RECORDER));
    let _ = std::fs::remove_dir_all(&root);
}

/// Drive a recording the way the editor's frame loop does, with a slow "real" frame time.
fn run_frames(s: &mut EditorState, max: usize) -> usize {
    let mut frames = 0;
    for _ in 0..max {
        for a in std::mem::take(&mut s.actions) {
            if let crate::actions::Action::TogglePlay = a {
                if s.play.in_play_mode() {
                    crate::play::stop(s);
                } else {
                    crate::play::start(s);
                }
            }
        }
        // Each frame took half a second of wall-clock time.
        crate::play::tick(s, 0.5);
        gpu_frame(None, s);
        frames += 1;
        if !s.recorder.recording() && !s.play.in_play_mode() && frames > 1 {
            break;
        }
    }
    frames
}

#[test]
fn a_frame_range_records_exactly_its_frames_in_capture_time() {
    let _turn = serial();
    let root = project("range");
    let mut s = state(&root);
    install(&mut s);
    *s.recorder.list(&root) = RecorderList {
        recorders: vec![RecorderSettings {
            name: "Shots".into(),
            format: OutputFormat::PngSequence,
            range: RangeMode::FrameRange,
            start_frame: 5,
            end_frame: 14,
            fps: 30,
            match_game: false,
            width: 16,
            height: 8,
            folder: "Recordings".into(),
            pattern: "<Recorder>_<Take>_<Frame>".into(),
            ..Default::default()
        }],
    };
    request_start(&mut s, None);
    run_frames(&mut s, 200);
    wait_all(&mut s, None);
    assert!(!s.play.in_play_mode(), "Play mode ends with the recording");
    let mut files: Vec<String> = std::fs::read_dir(root.join("Recordings"))
        .unwrap()
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .collect();
    files.sort();
    // Ten pictures and the sound beside them (Record Audio is on by default).
    assert_eq!(files.len(), 11, "{files:?}");
    assert_eq!(files[0], "Shots_001.wav");
    assert_eq!(files[1], "Shots_001_0005.png");
    assert_eq!(files[10], "Shots_001_0014.png");
    // The take moved on, and the next recording doesn't overwrite this one.
    assert_eq!(s.recorder.list(&root).recorders[0].take, 2);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn recording_runs_the_game_at_the_capture_rate() {
    let _turn = serial();
    let root = project("rate");
    let mut s = state(&root);
    install(&mut s);
    *s.recorder.list(&root) = RecorderList {
        recorders: vec![RecorderSettings {
            format: OutputFormat::Gif,
            range: RangeMode::TimeRange,
            start_time: 0.0,
            end_time: 1.0,
            fps: 25,
            match_game: false,
            width: 8,
            height: 8,
            ..Default::default()
        }],
    };
    request_start(&mut s, None);
    // Start, then the first frames: the game sees 1/25 s a frame, not the half second each
    // frame took.
    run_frames(&mut s, 3);
    assert!(s.recorder.recording());
    assert!((s.time.delta - 1.0 / 25.0).abs() < 1e-6, "{}", s.time.delta);
    run_frames(&mut s, 200);
    wait_all(&mut s, None);
    let gif = root.join("Recordings/SampleScene_001.gif");
    use misty_engine::image::AnimationDecoder;
    let file = std::io::BufReader::new(std::fs::File::open(&gif).unwrap());
    let frames = misty_engine::image::codecs::gif::GifDecoder::new(file)
        .unwrap()
        .into_frames()
        .collect_frames()
        .unwrap();
    assert_eq!(frames.len(), 25);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn scripts_can_start_and_stop_but_only_with_the_package() {
    let _turn = serial();
    let root = project("lua");
    let mut s = state(&root);
    misty_engine::recorder::request(misty_engine::recorder::Request::Start(None));
    gpu_frame(None, &mut s);
    assert!(!s.recorder.recording());
    install(&mut s);
    s.recorder.list(&root).recorders[0].format = OutputFormat::JpgSequence;
    s.recorder.list(&root).recorders[0].source = misty_engine::recorder::Source::EditorScene;
    s.recorder.list(&root).recorders[0].match_game = false;
    s.recorder.list(&root).recorders[0].width = 16;
    s.recorder.list(&root).recorders[0].height = 16;
    misty_engine::recorder::request(misty_engine::recorder::Request::Start(None));
    gpu_frame(None, &mut s);
    assert!(s.recorder.recording());
    assert!(misty_engine::recorder::is_recording());
    // Editor Scene recordings run in Edit mode, a frame per editor frame.
    gpu_frame(None, &mut s);
    gpu_frame(None, &mut s);
    misty_engine::recorder::request(misty_engine::recorder::Request::Stop);
    gpu_frame(None, &mut s);
    assert!(!s.recorder.recording());
    wait_all(&mut s, None);
    let n = std::fs::read_dir(root.join("Recordings")).unwrap().count();
    assert_eq!(n, 3);
    let _ = std::fs::remove_dir_all(&root);
}

/// Removing the package mid-recording ends the recording for scripts too: the file is written
/// and `Recorder.isRecording` is false (it stayed true for the rest of the session).
#[test]
fn removing_the_package_mid_recording_ends_it_for_scripts_too() {
    let _turn = serial();
    let root = project("remove_mid");
    let mut s = state(&root);
    install(&mut s);
    {
        let r = &mut s.recorder.list(&root).recorders[0];
        r.format = OutputFormat::PngSequence;
        r.source = misty_engine::recorder::Source::EditorScene;
        r.match_game = false;
        r.width = 8;
        r.height = 8;
    }
    request_start(&mut s, None);
    gpu_frame(None, &mut s);
    gpu_frame(None, &mut s);
    assert!(misty_engine::recorder::is_recording());
    remove(&mut s);
    assert!(!s.recorder.recording());
    assert!(!misty_engine::recorder::is_recording());
    wait_all(&mut s, None);
    assert_eq!(
        std::fs::read_dir(root.join("Recordings")).unwrap().count(),
        2
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// While the editor's renderer is still being made (the first seconds of a project), a
/// recording waits and so does the game: the first frames of recordings started then came out
/// black (a solid frame stood in for each one captured without the renderer).
#[test]
fn recordings_and_the_game_wait_for_the_renderer() {
    let _turn = serial();
    let root = project("gpu_starting");
    let mut s = state(&root);
    install(&mut s);
    *s.recorder.list(&root) = RecorderList {
        recorders: vec![RecorderSettings {
            name: "Shots".into(),
            format: OutputFormat::PngSequence,
            range: RangeMode::FrameRange,
            start_frame: 0,
            end_frame: 4,
            match_game: false,
            width: 8,
            height: 8,
            ..Default::default()
        }],
    };
    s.recorder.gpu_starting = true;
    request_start(&mut s, None);
    run_frames(&mut s, 2);
    assert!(s.play.in_play_mode());
    let game_time = s.time.time;
    run_frames(&mut s, 10);
    assert!(s.recorder.recording(), "still waiting");
    assert_eq!(
        s.recorder.active[0].recording.elapsed(),
        0,
        "nothing captured"
    );
    assert_eq!(s.time.time, game_time, "the game waited too");
    s.recorder.gpu_starting = false;
    run_frames(&mut s, 50);
    wait_all(&mut s, None);
    assert_eq!(
        std::fs::read_dir(root.join("Recordings")).unwrap().count(),
        5
    );
    let _ = std::fs::remove_dir_all(&root);
}

/// Whether the game's mix is in capture time.
fn mix_captures(s: &EditorState) -> Option<bool> {
    s.runtime.as_ref().map(|r| r.with_audio(|a| a.capturing()))
}

#[test]
fn a_video_keeps_the_games_sound_in_capture_time() {
    let _turn = serial();
    let root = project("sound");
    let mut s = state(&root);
    install(&mut s);
    let settings = |audio: bool| RecorderSettings {
        name: if audio { "Loud" } else { "Mute" }.into(),
        format: OutputFormat::Mp4,
        range: RangeMode::FrameRange,
        start_frame: 0,
        end_frame: 44,
        fps: 30,
        match_game: false,
        width: 16,
        height: 16,
        record_audio: audio,
        pattern: "<Recorder>".into(),
        ..Default::default()
    };
    // Off: the mix is never touched.
    *s.recorder.list(&root) = RecorderList {
        recorders: vec![settings(false)],
    };
    request_start(&mut s, None);
    run_frames(&mut s, 3);
    assert_eq!(mix_captures(&s), Some(false), "no recording keeps sound");
    run_frames(&mut s, 200);
    wait_all(&mut s, None);
    let mute = misty_recorder::mp4::read_audio(&root.join("Recordings/Mute.mp4")).unwrap();
    assert!(mute.is_none());
    // On (the default): the mix runs in capture time while recording, real time after.
    *s.recorder.list(&root) = RecorderList {
        recorders: vec![settings(true)],
    };
    request_start(&mut s, None);
    run_frames(&mut s, 3);
    assert_eq!(mix_captures(&s), Some(true));
    run_frames(&mut s, 200);
    wait_all(&mut s, None);
    assert!(!s.play.in_play_mode());
    let path = root.join("Recordings/Loud.mp4");
    let video = misty_recorder::mp4::read_samples(&path).unwrap();
    assert_eq!(video.samples.len(), 45);
    let audio = misty_recorder::mp4::read_audio(&path)
        .unwrap()
        .expect("a sound track");
    assert_eq!(audio.codec, "Opus");
    // 45 frames at 30 fps: 1.5 s of sound, exactly.
    assert_eq!(audio.samples, 72_000);
    let _ = std::fs::remove_dir_all(&root);
}
