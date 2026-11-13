//! The Recorder (optional package `com.misty.recorder`): records the Scene Viewer, a camera or
//! the Editor Scene to MP4, WebM, GIF or PNG / JPG sequences at a fixed frame rate.
//!
//! While a recording runs, the game runs in capture time: every Play-mode frame advances
//! exactly one frame of the recorder's rate however long it took to draw (see
//! [`capture_dt`]), and every such frame is rendered into the recorder's own target and copied
//! back through a staging-buffer ring ([`misty_recorder::readback`]); encoders run on worker
//! threads. A slow machine records slower than real time, but the file plays smoothly.
//!
//! - [`window`]: Window ▸ General ▸ Recorder (only with the package installed).
//! - [`timeline`]: the Timeline Recorder track (a clip records while it plays).
//! - Lua `Recorder.Start()` / `Recorder.Stop()` arrive through `misty_engine::recorder`.
//!
//! The recorder list is saved in `ProjectSettings/Recorder.json` (removed with the package).

pub mod timeline;
pub mod window;

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, channel};

use misty_engine::draw_cache::DrawCache;
use misty_engine::recorder::{PatternContext, RecorderSettings, Request, Source};
use misty_engine::render::RenderTarget;
use misty_recorder::readback::Readback;
use misty_recorder::schedule::Step;
use misty_recorder::session::Recording;
use misty_recorder::writer::Summary;
use misty_recorder::{Frame, Output};
use serde::{Deserialize, Serialize};

use crate::packages::optional;
use crate::state::EditorState;
use crate::viewport::Gpu;

/// Where the recorder list lives, relative to the project.
pub const SETTINGS_FILE: &str = "ProjectSettings/Recorder.json";

/// The project's recorders.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecorderList {
    pub recorders: Vec<RecorderSettings>,
}

impl Default for RecorderList {
    fn default() -> Self {
        Self {
            recorders: vec![RecorderSettings::default()],
        }
    }
}

impl RecorderList {
    pub fn load(project: &Path) -> Self {
        std::fs::read_to_string(project.join(SETTINGS_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, project: &Path) -> Result<(), String> {
        let path = project.join(SETTINGS_FILE);
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())? + "\n";
        std::fs::write(&path, text).map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// Who started a recording.
#[derive(Clone, Debug, PartialEq)]
pub enum Origin {
    /// The window's Start Recording (or a script); `usize` = the recorder's row.
    Manual(usize),
    /// A Timeline Recorder clip: (director, track id, clip index).
    Clip(misty_engine::hecs::Entity, String, usize),
}

/// One recording in progress.
pub struct Active {
    pub origin: Origin,
    pub recording: Recording,
    size: (u32, u32),
    target: Option<RenderTarget>,
    readback: Option<Readback>,
    /// The recorder's own draw lists (its camera's culling mask).
    draws: DrawCache,
    /// Capture-time seconds this recording has seen.
    time: f64,
}

impl Active {
    /// Capture-time seconds of this recording's next frame.
    fn next_time(&self) -> f64 {
        self.recording.elapsed() as f64 / self.recording.settings.frame_rate() as f64
    }
}

/// A recording whose encoder is finishing its files on a worker thread.
struct Finishing {
    name: String,
    rx: Receiver<Result<Summary, String>>,
}

/// Recorder window state and the recordings in progress.
#[derive(Default)]
pub struct RecorderState {
    list: Option<RecorderList>,
    pub selected: usize,
    pub active: Vec<Active>,
    finishing: Vec<Finishing>,
    /// The game advanced a frame of capture time this frame (Play mode tick).
    ticked: bool,
    /// Play mode was entered for the recording (left again when it ends).
    started_play: bool,
    /// Play mode ran during the recordings (leaving it ends them).
    play_seen: bool,
    /// Start / stop asked for this frame, handled where the GPU is at hand.
    pending: Vec<Request>,
    /// Last result to show in the window: (error, text, folder).
    pub status: Option<(bool, String, Option<PathBuf>)>,
    /// "Not installed" was said once for scripts.
    warned_missing: bool,
}

impl RecorderState {
    /// The recorder list (read from the project the first time).
    pub fn list(&mut self, project: &Path) -> &mut RecorderList {
        self.list.get_or_insert_with(|| RecorderList::load(project))
    }

    pub fn recording(&self) -> bool {
        !self.active.is_empty()
    }

    /// Recordings still being written to disk.
    pub fn finishing(&self) -> usize {
        self.finishing.len()
    }
}

pub fn installed(s: &EditorState) -> bool {
    s.optional_packages.has(optional::RECORDER)
}

/// Install / Remove in the Package Manager: without the package its tab closes and anything
/// recording stops.
pub fn package_changed(s: &mut EditorState) {
    if !installed(s) {
        for a in std::mem::take(&mut s.recorder.active) {
            finish(&mut s.recorder, a, None);
        }
        s.recorder.list = None;
        s.recorder.pending.clear();
        s.recorder.status = None;
        s.push(crate::actions::Action::CloseTab(crate::layout::Tab::Recorder));
    }
}

/// Seconds the next Play-mode frame advances: one frame of the fastest recording while any
/// runs (capture time), `None` otherwise (real time).
pub fn capture_dt(s: &EditorState) -> Option<f32> {
    let fps = s
        .recorder
        .active
        .iter()
        .map(|a| a.recording.settings.frame_rate())
        .max()?;
    Some(1.0 / fps as f32)
}

/// Play mode advanced one frame (called from `play::tick`).
pub fn on_tick(s: &mut EditorState) {
    if !s.recorder.active.is_empty() {
        s.recorder.ticked = true;
    }
}

/// Ask for every enabled recorder to start (the window's button, `Recorder.Start()`).
pub fn request_start(s: &mut EditorState, only: Option<String>) {
    s.recorder.pending.push(Request::Start(only));
}

pub fn request_stop(s: &mut EditorState) {
    s.recorder.pending.push(Request::Stop);
}

/// Whether a source draws the game (and so needs Play mode).
fn needs_play(source: Source) -> bool {
    source != Source::EditorScene
}

/// The camera object called `name` as a render camera.
pub fn named_camera(
    scene: &misty_engine::scene::Scene,
    name: &str,
) -> Option<(misty_engine::camera::Camera, misty_engine::scene::Camera)> {
    use misty_engine::camera::{Camera, Projection};
    let e = scene
        .active_with::<misty_engine::scene::Camera>()
        .into_iter()
        .find(|e| scene.name(*e) == name)?;
    let c = scene
        .world
        .get::<&misty_engine::scene::Camera>(e)
        .ok()?
        .clone();
    let (_, rotation, position) = scene.world_matrix(e).to_scale_rotation_translation();
    let projection = if c.orthographic {
        Projection::Orthographic { size: c.ortho_size }
    } else {
        Projection::Perspective { fov_y: c.fov }
    };
    Some((
        Camera {
            position,
            rotation,
            projection,
            near: c.near,
            far: c.far,
        },
        c,
    ))
}

/// Names of the scene's cameras (the Source ▸ Camera list).
pub fn camera_names(scene: &misty_engine::scene::Scene) -> Vec<String> {
    let mut names: Vec<String> = scene
        .active_with::<misty_engine::scene::Camera>()
        .into_iter()
        .map(|e| scene.name(e))
        .collect();
    names.sort();
    names.dedup();
    names
}

/// The size a recording of `settings` starts at.
fn start_size(s: &EditorState, gpu: Option<&Gpu>, settings: &RecorderSettings) -> (u32, u32) {
    let game = s.game_view.screen_px;
    let game = if game.x >= 2.0 && game.y >= 2.0 {
        (game.x as u32, game.y as u32)
    } else {
        (1920, 1080)
    };
    let view = match settings.source {
        Source::EditorScene => gpu
            .and_then(|g| g.scene.target())
            .map_or(game, |t| t.output_size()),
        _ => game,
    };
    settings.output_size(view)
}

/// The output of `settings` at take `take`.
fn output_for(
    s: &EditorState,
    settings: &RecorderSettings,
    take: u32,
    (w, h): (u32, u32),
) -> Output {
    let project = &s.project.root;
    let project_name = project
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ctx = PatternContext {
        scene: &s.scene.name,
        recorder: &settings.name,
        project: &project_name,
        take,
        width: w,
        height: h,
        source: settings.source.label(),
    };
    Output::new(settings, project, &ctx, w, h)
}

/// Start recording `settings` (opens the files and the encoder). With Auto-increment, takes
/// already on disk are skipped. Returns the file (or folder) and the take used.
pub fn start_one(
    s: &mut EditorState,
    gpu: Option<&Gpu>,
    mut settings: RecorderSettings,
    origin: Origin,
) -> Result<(PathBuf, u32), String> {
    let size = start_size(s, gpu, &settings);
    let mut output = output_for(s, &settings, settings.take, size);
    if settings.auto_increment {
        let mut take = settings.take;
        let first = |o: &Output| o.path(Some(0));
        while (output.path(None).exists() || first(&output).exists()) && take < 99_999 {
            take += 1;
            output = output_for(s, &settings, take, size);
        }
        settings.take = take;
    }
    let take = settings.take;
    let shown = if settings.format.is_sequence() {
        output.folder.clone()
    } else {
        output.path(None)
    };
    let recording = Recording::start(settings, output)?;
    s.recorder.active.push(Active {
        origin,
        recording,
        size,
        target: None,
        readback: None,
        draws: DrawCache::default(),
        time: 0.0,
    });
    misty_engine::recorder::set_recording(true);
    Ok((shown, take))
}

/// Start every enabled recorder of the window (or the one named `only`).
fn start_manual(s: &mut EditorState, gpu: Option<&Gpu>, only: Option<&str>) {
    if s.recorder.recording() {
        return;
    }
    let project = s.project.root.clone();
    let list = s.recorder.list(&project).clone();
    let picked: Vec<(usize, RecorderSettings)> = list
        .recorders
        .iter()
        .enumerate()
        .filter(|(_, r)| match only {
            Some(n) => r.name == n,
            None => r.enabled && r.range != misty_engine::recorder::RangeMode::TimelineClip,
        })
        .map(|(i, r)| (i, r.clone()))
        .collect();
    if picked.is_empty() {
        let msg = match only {
            Some(n) => crate::i18n::tr!("No recorder called {name}.", name = n),
            None => crate::i18n::tr!("No recorder is enabled."),
        };
        s.console.warn(msg.clone());
        s.recorder.status = Some((true, msg, None));
        return;
    }
    let mut started = Vec::new();
    for (i, settings) in picked {
        let name = settings.name.clone();
        let play = needs_play(settings.source);
        match start_one(s, gpu, settings, Origin::Manual(i)) {
            Ok((path, take)) => started.push((i, name, path, play, take)),
            Err(e) => {
                s.console.warn(format!("Recorder {name}: {e}"));
                s.recorder.status = Some((true, e, None));
            }
        }
    }
    // Take numbers move on for the next recording.
    let list = s.recorder.list(&project);
    for (i, _, _, _, take) in &started {
        if let Some(r) = list.recorders.get_mut(*i)
            && r.auto_increment
        {
            r.take = take.saturating_add(1);
        }
    }
    let _ = list.save(&project);
    if started.iter().any(|(_, _, _, play, _)| *play) && !s.play.in_play_mode() {
        s.recorder.started_play = true;
        s.push(crate::actions::Action::TogglePlay);
    }
    for (_, name, path, _, _) in &started {
        s.console.info(format!(
            "Recording {name} to {}",
            path.to_string_lossy().replace('\\', "/")
        ));
    }
    if let Some((_, _, path, _, _)) = started.first() {
        s.recorder.status = Some((false, crate::i18n::tr!("Recording…"), Some(path.clone())));
    }
}

/// Stop one recording: pick up its last frames and finish its files on a worker thread.
fn finish(r: &mut RecorderState, mut a: Active, gpu: Option<&Gpu>) {
    if let (Some(rb), Some(gpu)) = (a.readback.as_mut(), gpu) {
        let mut frames = Vec::new();
        rb.collect(&gpu.state.device, true, &mut frames);
        for f in frames {
            a.recording.push(f);
        }
    }
    let name = a.recording.settings.name.clone();
    let (tx, rx) = channel();
    let recording = a.recording;
    let _ = std::thread::Builder::new()
        .name("misty recorder finish".into())
        .spawn(move || {
            let _ = tx.send(recording.finish());
        });
    r.finishing.push(Finishing { name, rx });
}

/// Stop every recording.
pub fn stop_all(s: &mut EditorState, gpu: Option<&Gpu>) {
    for a in std::mem::take(&mut s.recorder.active) {
        finish(&mut s.recorder, a, gpu);
    }
    misty_engine::recorder::set_recording(false);
}

/// Requests from scripts and the window, then the Timeline clips.
fn handle_requests(s: &mut EditorState, gpu: Option<&Gpu>) {
    let scripts = misty_engine::recorder::take_requests();
    if !scripts.is_empty() && !installed(s) {
        if !std::mem::replace(&mut s.recorder.warned_missing, true) {
            s.console.warn(
                "Recorder.Start / Recorder.Stop: the Recorder package is not installed \
                 (Window ▸ Package Manager ▸ Built-in ▸ Recorder ▸ Install).",
            );
        }
        return;
    }
    let mut all = std::mem::take(&mut s.recorder.pending);
    all.extend(scripts);
    for r in all {
        match r {
            Request::Start(only) => start_manual(s, gpu, only.as_deref()),
            Request::Stop => stop_all(s, gpu),
        }
    }
}

/// Render what `a` looks through into its own target. `false` when there is nothing to draw
/// (no camera).
fn render_source(gpu: &mut Gpu, s: &mut EditorState, a: &mut Active) -> bool {
    let settings = &a.recording.settings;
    let Some(mut game) =
        misty_engine::render_scene::GameView::cached(&s.scene, &s.assets, &mut a.draws)
    else {
        return false;
    };
    if settings.source == Source::Camera {
        match named_camera(&s.scene, &settings.camera) {
            Some((camera, comp)) => {
                game.camera = camera;
                game.settings = comp;
            }
            None => return false,
        }
    }
    let (w, h) = a.size;
    let device = &gpu.state.device;
    let fits = a
        .target
        .as_ref()
        .is_some_and(|t| gpu.renderer.view_target_fits(t, w, h));
    if !fits {
        a.target = Some(gpu.renderer.create_view_target(device, w, h));
    }
    let target = a.target.as_ref().expect("made above");
    target.set_misty_gi_secondary(true);
    gpu.renderer.set_view_label("Recorder");
    let frame = game.frame();
    // Everything loads before the frame is drawn: no pop-in in recordings.
    gpu.renderer
        .render_with_effects(device, &gpu.state.queue, target, &frame, &game.effects);
    if settings.include_ui {
        gpu.ui
            .render(device, &gpu.state.queue, target, &s.scene, None);
    }
    true
}

/// Capture frame `index` of `a` (render, then copy back through the ring).
fn capture(gpu: &mut Gpu, s: &mut EditorState, a: &mut Active, index: u64) {
    let (w, h) = a.size;
    let mut done = Vec::new();
    match a.recording.settings.source {
        Source::EditorScene => {
            let Some(target) = gpu.scene.target() else {
                a.recording.push(Frame::solid(index, w, h, [0, 0, 0, 255]));
                return;
            };
            let rb = a
                .readback
                .get_or_insert_with(|| Readback::new(&gpu.state.device, w, h));
            if rb.fits(target) {
                rb.capture(&gpu.state.device, &gpu.state.queue, target, index, &mut done);
            } else {
                // The view was resized: wait for the older frames, then scale this one.
                rb.collect(&gpu.state.device, true, &mut done);
                let (tw, th) = target.output_size();
                let mut px = crate::project_thumbnail::read_pixels(
                    &gpu.state.device,
                    &gpu.state.queue,
                    target,
                );
                misty_recorder::readback::opaque(&mut px);
                done.push(Frame {
                    index,
                    width: w,
                    height: h,
                    rgba: misty_recorder::readback::fit(px, tw, th, w, h),
                });
            }
        }
        _ => {
            if !render_source(gpu, s, a) {
                a.recording.push(Frame::solid(index, w, h, [0, 0, 0, 255]));
                return;
            }
            let target = a.target.as_ref().expect("rendered");
            let rb = a
                .readback
                .get_or_insert_with(|| Readback::new(&gpu.state.device, w, h));
            rb.capture(&gpu.state.device, &gpu.state.queue, target, index, &mut done);
        }
    }
    for f in done {
        a.recording.push(f);
    }
}

/// Once a frame, after the panels drew: start and stop recordings, capture the frame the game
/// just advanced to, hand finished copies to the encoders. Returns true while busy.
pub fn gpu_frame(gpu: Option<&mut Gpu>, s: &mut EditorState) -> bool {
    let mut gpu = gpu;
    handle_requests(s, gpu.as_deref());
    if installed(s) {
        timeline::frame(s, gpu.as_deref());
    }
    poll_finishing(s);
    if s.recorder.active.is_empty() {
        s.recorder.ticked = false;
        s.recorder.play_seen = false;
        if std::mem::take(&mut s.recorder.started_play) && s.play.in_play_mode() {
            s.push(crate::actions::Action::TogglePlay);
        }
        return !s.recorder.finishing.is_empty();
    }
    let play_needed = s
        .recorder
        .active
        .iter()
        .any(|a| needs_play(a.recording.settings.source));
    // Play mode stopped under a game recording (Stop pressed): the recording ends too.
    if s.play.in_play_mode() {
        s.recorder.play_seen = true;
    } else if play_needed && (s.recorder.play_seen || !s.recorder.started_play) {
        s.recorder.started_play = false;
        stop_all(s, gpu.as_deref());
        return true;
    }
    let ticked = std::mem::take(&mut s.recorder.ticked)
        || (!play_needed && !s.play.in_play_mode());
    let mut active = std::mem::take(&mut s.recorder.active);
    if ticked {
        let dt = capture_dt_of(&active);
        for a in &mut active {
            // Due when the session's capture time reached this recording's next frame (its
            // rate may be lower than the fastest recording's).
            if a.next_time() > a.time + 1e-6 {
                a.time += dt;
                continue;
            }
            a.time += dt;
            match a.recording.step() {
                Step::Capture(i) => match gpu.as_deref_mut() {
                    Some(g) => capture(g, s, a, i),
                    None => {
                        let (w, h) = a.size;
                        a.recording.push(Frame::solid(i, w, h, [0, 0, 0, 255]));
                    }
                },
                Step::Skip | Step::Done => {}
            }
        }
    }
    // Copies the GPU finished since: to the encoders.
    if let Some(g) = gpu.as_deref() {
        for a in &mut active {
            if let Some(rb) = a.readback.as_mut() {
                let mut done = Vec::new();
                rb.collect(&g.state.device, false, &mut done);
                for f in done {
                    a.recording.push(f);
                }
            }
        }
    }
    // Ranges that are done, and encoders that failed.
    let mut keep = Vec::new();
    for a in active {
        if let Some(e) = a.recording.error() {
            s.console
                .warn(format!("Recorder {}: {e}", a.recording.settings.name));
        }
        if a.recording.complete() || a.recording.error().is_some() {
            finish(&mut s.recorder, a, gpu.as_deref());
        } else {
            keep.push(a);
        }
    }
    s.recorder.active = keep;
    if s.recorder.active.is_empty() {
        misty_engine::recorder::set_recording(false);
    }
    true
}

fn capture_dt_of(active: &[Active]) -> f64 {
    let fps = active
        .iter()
        .map(|a| a.recording.settings.frame_rate())
        .max()
        .unwrap_or(30);
    1.0 / fps as f64
}

/// Report recordings whose files are written.
fn poll_finishing(s: &mut EditorState) {
    let mut still = Vec::new();
    for f in std::mem::take(&mut s.recorder.finishing) {
        match f.rx.try_recv() {
            Ok(Ok(summary)) => {
                let where_ = summary.files.first().map(|p| {
                    if summary.files.len() > 1 {
                        p.parent().map(Path::to_path_buf).unwrap_or_default()
                    } else {
                        p.clone()
                    }
                });
                let shown = where_
                    .as_ref()
                    .map(|p| p.to_string_lossy().replace('\\', "/"))
                    .unwrap_or_default();
                let msg = crate::i18n::tr!(
                    "{name}: {frames} frames saved to {path}",
                    name = f.name,
                    frames = summary.frames,
                    path = shown
                );
                s.console.info(msg.clone());
                s.recorder.status = Some((false, msg, where_));
            }
            Ok(Err(e)) => {
                s.console.warn(format!("Recorder {}: {e}", f.name));
                s.recorder.status = Some((true, e, None));
            }
            Err(std::sync::mpsc::TryRecvError::Empty) => still.push(f),
            Err(std::sync::mpsc::TryRecvError::Disconnected) => {}
        }
    }
    s.recorder.finishing = still;
}

/// Block until every recording is finished and written (tests, quitting).
pub fn wait_all(s: &mut EditorState, gpu: Option<&Gpu>) {
    stop_all(s, gpu);
    while !s.recorder.finishing.is_empty() {
        poll_finishing(s);
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
}
