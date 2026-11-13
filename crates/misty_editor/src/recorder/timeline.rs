//! Timeline Recorder track: each clip records while it plays in Play mode, at the clip's
//! settings. The clip's range is the recording's: it starts on the first frame the director's
//! time is inside the clip and stops when it leaves it, so a timeline records its own shots.

use misty_engine::hecs::Entity;
use misty_engine::recorder::{RangeMode, RecorderSettings};
use misty_engine::timeline::{TimelineClip, TrackKind};

use super::{Origin, installed, start_one};
use crate::i18n::tr;
use crate::state::EditorState;
use crate::viewport::Gpu;

/// Seconds a new Recorder clip lasts.
pub const CLIP_LEN: f32 = 5.0;

/// A new Recorder clip at `time`.
pub fn new_clip(time: f32) -> TimelineClip {
    TimelineClip {
        name: "Recorder Clip".into(),
        start: time.max(0.0),
        duration: CLIP_LEN,
        recorder: Some(RecorderSettings {
            name: "Recorder Clip".into(),
            range: RangeMode::TimelineClip,
            ..Default::default()
        }),
        ..Default::default()
    }
}

/// The Recorder section of a Recorder clip's Properties.
pub fn clip_inspector(ui: &mut egui::Ui, s: &mut EditorState, clip: &mut TimelineClip) {
    ui.add_space(6.0);
    ui.label(egui::RichText::new(tr!("Recorder")).strong());
    if !installed(s) {
        ui.weak(tr!(
            "Install the Recorder package (Window ▸ Package Manager) to record this clip."
        ));
        return;
    }
    let cameras = super::camera_names(&s.scene);
    let settings = clip.recorder.get_or_insert_with(|| RecorderSettings {
        range: RangeMode::TimelineClip,
        ..Default::default()
    });
    super::window::settings_ui(ui, &cameras, settings, "tl_rec_clip", true);
    ui.weak(tr!(
        "Records while the clip plays in Play mode, {frames} frames at this length.",
        frames = (clip.duration * settings.frame_rate() as f32).round() as u64
    ));
}

/// Recorder clips the playing directors are inside now: (director, track, clip, settings).
fn live_clips(s: &mut EditorState) -> Vec<(Entity, String, usize, RecorderSettings)> {
    let Some(runtime) = s.runtime.as_ref() else {
        return Vec::new();
    };
    let directors: Vec<(Entity, String, f64)> = s
        .scene
        .active_with::<misty_engine::scene::PlayableDirector>()
        .into_iter()
        .filter_map(|e| {
            let d = s
                .scene
                .world
                .get::<&misty_engine::scene::PlayableDirector>(e)
                .ok()?;
            let path = d.playable.clone().filter(|_| d.enabled)?;
            let (t, state) = runtime.director_status(e);
            (state == misty_engine::timeline::PlayState::Playing).then_some((e, path, t))
        })
        .collect();
    let mut out = Vec::new();
    for (e, path, t) in directors {
        let Ok(asset) = s.anim_clips.timelines.get(&path) else {
            continue;
        };
        for track in asset
            .tracks
            .iter()
            .filter(|tr| tr.kind == TrackKind::Recorder && !asset.is_muted(tr))
        {
            for (i, c) in track.clips.iter().enumerate() {
                let inside = t >= c.start as f64 && t < c.end() as f64;
                if inside {
                    let mut settings = c.recorder.clone().unwrap_or_default();
                    settings.range = RangeMode::Manual;
                    if settings.name.trim().is_empty() {
                        settings.name = c.name.clone();
                    }
                    out.push((e, track.id.clone(), i, settings));
                }
            }
        }
    }
    out
}

/// Start the clips the directors entered, stop the ones they left (Play mode only).
pub fn frame(s: &mut EditorState, gpu: Option<&Gpu>) {
    let live = if s.play.in_play_mode() {
        live_clips(s)
    } else {
        Vec::new()
    };
    // Clips left behind.
    let mut keep = Vec::new();
    for a in std::mem::take(&mut s.recorder.active) {
        let still = match &a.origin {
            Origin::Clip(e, track, i) => live
                .iter()
                .any(|(le, lt, li, _)| le == e && lt == track && li == i),
            Origin::Manual(_) => true,
        };
        if still {
            keep.push(a);
        } else {
            super::finish(&mut s.recorder, a, gpu);
        }
    }
    s.recorder.active = keep;
    // Clips entered.
    for (e, track, i, settings) in live {
        let origin = Origin::Clip(e, track, i);
        if s.recorder.active.iter().any(|a| a.origin == origin) {
            continue;
        }
        let name = settings.name.clone();
        match start_one(s, gpu, settings, origin) {
            Ok((path, _)) => s.console.info(format!(
                "Timeline: recording {name} to {}",
                path.to_string_lossy().replace('\\', "/")
            )),
            Err(e) => s.console.warn(format!("Timeline recorder {name}: {e}")),
        }
    }
    if s.recorder.active.is_empty() {
        misty_engine::recorder::set_recording(false);
    }
}
