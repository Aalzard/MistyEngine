//! Recorder settings (the optional Recorder package): what one recorder captures, how, and
//! where it writes. Only data lives here, so Timeline Recorder clips can carry it and scripts
//! can ask for a recording; the capture and the encoders are in the `misty_recorder` crate and
//! the editor drives them.
//!
//! - [`RecorderSettings`]: source, output format, resolution, frame rate, start / stop, output
//!   folder, file-name pattern and take number.
//! - [`expand_pattern`]: `<Scene>_<Take>_<Frame>` style file names.
//! - [`request`] / [`take_requests`]: `Recorder.Start()` / `Recorder.Stop()` from Lua; the
//!   editor picks them up after the frame (a built game has no Recorder and ignores them).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};

use serde::{Deserialize, Serialize};

/// What a recorder looks through.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    /// The game as the Scene Viewer shows it (the highest-Depth camera, with its canvas UI).
    #[default]
    SceneViewer,
    /// One camera of the scene, picked by object name.
    Camera,
    /// The Editor Scene view, gizmos and all.
    EditorScene,
}

impl Source {
    pub const ALL: [Source; 3] = [Self::SceneViewer, Self::Camera, Self::EditorScene];

    pub fn label(self) -> &'static str {
        match self {
            Self::SceneViewer => "Scene Viewer",
            Self::Camera => "Camera",
            Self::EditorScene => "Editor Scene",
        }
    }
}

/// The file a recorder writes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum OutputFormat {
    /// H.264 video in an MP4 file.
    #[default]
    Mp4,
    /// AV1 video in a WebM file.
    WebM,
    /// An animated GIF (256 colours a frame).
    Gif,
    /// One PNG a frame (lossless, with alpha).
    PngSequence,
    /// One JPG a frame.
    JpgSequence,
}

impl OutputFormat {
    pub const ALL: [OutputFormat; 5] = [
        Self::Mp4,
        Self::WebM,
        Self::Gif,
        Self::PngSequence,
        Self::JpgSequence,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Mp4 => "MP4 (H.264)",
            Self::WebM => "WebM (AV1)",
            Self::Gif => "GIF",
            Self::PngSequence => "PNG Sequence",
            Self::JpgSequence => "JPG Sequence",
        }
    }

    /// File extension (no dot).
    pub fn extension(self) -> &'static str {
        match self {
            Self::Mp4 => "mp4",
            Self::WebM => "webm",
            Self::Gif => "gif",
            Self::PngSequence => "png",
            Self::JpgSequence => "jpg",
        }
    }

    /// One file a frame (the pattern needs a `<Frame>`).
    pub fn is_sequence(self) -> bool {
        matches!(self, Self::PngSequence | Self::JpgSequence)
    }

    /// A video file.
    pub fn is_video(self) -> bool {
        matches!(self, Self::Mp4 | Self::WebM)
    }

    /// Can record the game's sound: inside the video (MP4, WebM), or as a WAV beside an image
    /// sequence. GIFs have no sound.
    pub fn has_audio(self) -> bool {
        self != Self::Gif
    }

    /// Video encoders want even sizes (4:2:0 chroma).
    pub fn needs_even_size(self) -> bool {
        self.is_video()
    }
}

/// When a recording starts and stops.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum RangeMode {
    /// From Start Recording until Stop Recording.
    #[default]
    Manual,
    /// Frames `start_frame..=end_frame` of Play mode (frame 0 is the first frame).
    FrameRange,
    /// Seconds `start_time..end_time` of Play mode, in capture time.
    TimeRange,
    /// While a Timeline Recorder clip plays (set on the clip).
    TimelineClip,
}

impl RangeMode {
    pub const ALL: [RangeMode; 4] = [
        Self::Manual,
        Self::FrameRange,
        Self::TimeRange,
        Self::TimelineClip,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Manual => "Manual",
            Self::FrameRange => "Frame Range",
            Self::TimeRange => "Time Range",
            Self::TimelineClip => "Timeline Clip",
        }
    }
}

/// Frame rates offered in the window (any whole number from 1 to 240 works).
pub const FRAME_RATES: [u32; 6] = [24, 25, 30, 50, 60, 120];

/// The default file name: scene, take, and (sequences) frame number.
pub const DEFAULT_PATTERN: &str = "<Scene>_<Take>";

/// One recorder.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecorderSettings {
    pub name: String,
    /// Records when Start Recording is pressed (off: skipped).
    pub enabled: bool,
    pub source: Source,
    /// [`Source::Camera`]: the camera object's name.
    pub camera: String,
    pub format: OutputFormat,
    /// Record at the Scene Viewer's size (the Editor Scene's for that source).
    pub match_game: bool,
    pub width: u32,
    pub height: u32,
    /// Frames per second of capture time.
    pub fps: u32,
    pub range: RangeMode,
    pub start_frame: u32,
    pub end_frame: u32,
    pub start_time: f32,
    pub end_time: f32,
    /// Output folder, relative to the project (or absolute).
    pub folder: String,
    /// File name without extension, with wildcards (see [`expand_pattern`]).
    pub pattern: String,
    /// The take written next.
    pub take: u32,
    /// Add one to `take` after every recording.
    pub auto_increment: bool,
    /// Draw the scene's canvas UI (Scene Viewer and Camera sources).
    pub include_ui: bool,
    /// 1–100: JPG quality and the video bit rate.
    pub quality: u8,
    /// Record the game's sound (Opus in MP4 and WebM, a WAV beside image sequences). Older
    /// files' `audio` (always off: sound wasn't recorded then) is ignored.
    pub record_audio: bool,
}

impl Default for RecorderSettings {
    fn default() -> Self {
        Self {
            name: "Movie Recorder".into(),
            enabled: true,
            source: Source::SceneViewer,
            camera: String::new(),
            format: OutputFormat::Mp4,
            match_game: true,
            width: 1920,
            height: 1080,
            fps: 30,
            range: RangeMode::Manual,
            start_frame: 0,
            end_frame: 299,
            start_time: 0.0,
            end_time: 10.0,
            folder: "Recordings".into(),
            pattern: DEFAULT_PATTERN.into(),
            take: 1,
            auto_increment: true,
            include_ui: true,
            quality: 75,
            record_audio: true,
        }
    }
}

impl RecorderSettings {
    /// A new recorder writing `format`, named after it.
    pub fn new(format: OutputFormat) -> Self {
        let name = match format {
            OutputFormat::Mp4 | OutputFormat::WebM => "Movie Recorder",
            OutputFormat::Gif => "GIF Recorder",
            OutputFormat::PngSequence | OutputFormat::JpgSequence => "Image Sequence Recorder",
        };
        Self {
            name: name.into(),
            format,
            ..Default::default()
        }
    }

    /// Frames per second, kept between 1 and 240.
    pub fn frame_rate(&self) -> u32 {
        self.fps.clamp(1, 240)
    }

    /// Seconds one captured frame advances the game.
    pub fn frame_dt(&self) -> f32 {
        1.0 / self.frame_rate() as f32
    }

    /// The frames recorded, `first..=last` counted from the start of the recording; `None` for
    /// open-ended recordings (Manual, Timeline Clip).
    pub fn frame_range(&self) -> Option<(u64, u64)> {
        let fps = self.frame_rate() as f64;
        match self.range {
            RangeMode::Manual | RangeMode::TimelineClip => None,
            RangeMode::FrameRange => {
                let first = self.start_frame as u64;
                Some((first, (self.end_frame as u64).max(first)))
            }
            RangeMode::TimeRange => {
                // Frame n shows time n / fps: [start, end) holds round(start·fps) up to the
                // last frame before end.
                let first = (self.start_time.max(0.0) as f64 * fps).round() as u64;
                let end = (self.end_time.max(0.0) as f64 * fps).round() as u64;
                Some((first, end.max(first + 1) - 1))
            }
        }
    }

    /// Output size for a game view of `game` pixels: the custom size or the game's, made even
    /// for video formats and at least 2×2.
    pub fn output_size(&self, game: (u32, u32)) -> (u32, u32) {
        let (w, h) = if self.match_game {
            game
        } else {
            (self.width, self.height)
        };
        let (mut w, mut h) = (w.clamp(2, 8192), h.clamp(2, 8192));
        if self.format.needs_even_size() {
            w &= !1;
            h &= !1;
        }
        (w, h)
    }
}

/// What the wildcards of a file-name pattern stand for.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PatternContext<'a> {
    pub scene: &'a str,
    pub recorder: &'a str,
    pub project: &'a str,
    pub take: u32,
    pub width: u32,
    pub height: u32,
    pub source: &'a str,
}

/// Wildcards a pattern may use (the window lists them).
pub const WILDCARDS: [&str; 7] = [
    "<Scene>",
    "<Take>",
    "<Frame>",
    "<Recorder>",
    "<Project>",
    "<Resolution>",
    "<Source>",
];

/// `pattern` with its wildcards filled in: `<Scene>`, `<Take>` (three digits), `<Frame>`
/// (four digits; empty when `frame` is `None`), `<Recorder>`, `<Project>`, `<Resolution>`
/// (`1920x1080`) and `<Source>`. Characters a file name can't hold become `_`. An empty result
/// falls back to `Recording`.
pub fn expand_pattern(pattern: &str, ctx: &PatternContext, frame: Option<u64>) -> String {
    let frame_text = frame.map(|f| format!("{f:04}")).unwrap_or_default();
    let mut out = pattern
        .replace("<Scene>", ctx.scene)
        .replace("<Take>", &format!("{:03}", ctx.take))
        .replace("<Frame>", &frame_text)
        .replace("<Recorder>", ctx.recorder)
        .replace("<Project>", ctx.project)
        .replace("<Resolution>", &format!("{}x{}", ctx.width, ctx.height))
        .replace("<Source>", ctx.source);
    if frame.is_none() {
        // A pattern ending "_<Frame>" for a video leaves no dangling separator.
        out = out.trim_end_matches(['_', '-', '.', ' ']).to_owned();
    }
    let clean: String = out
        .chars()
        .map(|c| {
            if c.is_control() || "<>:\"/\\|?*".contains(c) {
                '_'
            } else {
                c
            }
        })
        .collect();
    let clean = clean.trim().to_owned();
    if clean.is_empty() {
        "Recording".into()
    } else {
        clean
    }
}

/// The pattern a recorder of `format` uses: image sequences get `_<Frame>` when it has none, so
/// frames don't overwrite each other.
pub fn effective_pattern(pattern: &str, format: OutputFormat) -> String {
    let p = if pattern.trim().is_empty() {
        DEFAULT_PATTERN
    } else {
        pattern
    };
    if format.is_sequence() && !p.contains("<Frame>") {
        format!("{p}_<Frame>")
    } else {
        p.to_owned()
    }
}

/// A script's request to the editor's Recorder.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Request {
    /// Start every enabled recorder (or the one with this name).
    Start(Option<String>),
    Stop,
}

static REQUESTS: Mutex<Vec<Request>> = Mutex::new(Vec::new());
static RECORDING: AtomicBool = AtomicBool::new(false);

/// Queue a request (Lua `Recorder.Start` / `Recorder.Stop`).
pub fn request(r: Request) {
    if let Ok(mut q) = REQUESTS.lock() {
        q.push(r);
    }
}

/// The requests since the last call (the editor, once a frame).
pub fn take_requests() -> Vec<Request> {
    REQUESTS
        .lock()
        .map(|mut q| std::mem::take(&mut *q))
        .unwrap_or_default()
}

/// The editor says whether a recording is running (Lua `Recorder.isRecording`).
pub fn set_recording(on: bool) {
    RECORDING.store(on, Ordering::Relaxed);
}

pub fn is_recording() -> bool {
    RECORDING.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frame_ranges_hold_exactly_the_frames_asked_for() {
        let mut s = RecorderSettings {
            range: RangeMode::FrameRange,
            start_frame: 10,
            end_frame: 19,
            ..Default::default()
        };
        assert_eq!(s.frame_range(), Some((10, 19)));
        s.range = RangeMode::TimeRange;
        s.fps = 30;
        s.start_time = 0.0;
        s.end_time = 2.0;
        // 60 frames: 0..=59.
        assert_eq!(s.frame_range(), Some((0, 59)));
        s.start_time = 1.0;
        s.end_time = 1.0;
        // An empty range still records one frame.
        assert_eq!(s.frame_range(), Some((30, 30)));
        s.range = RangeMode::Manual;
        assert_eq!(s.frame_range(), None);
    }

    #[test]
    fn patterns_fill_wildcards_and_clean_names() {
        let ctx = PatternContext {
            scene: "Level:1",
            recorder: "Movie",
            project: "Game",
            take: 7,
            width: 640,
            height: 360,
            source: "Scene Viewer",
        };
        assert_eq!(
            expand_pattern("<Scene>_<Take>_<Frame>", &ctx, Some(12)),
            "Level_1_007_0012"
        );
        assert_eq!(
            expand_pattern("<Scene>_<Take>_<Frame>", &ctx, None),
            "Level_1_007"
        );
        assert_eq!(
            expand_pattern("<Recorder> <Resolution> <Project>", &ctx, None),
            "Movie 640x360 Game"
        );
        assert_eq!(expand_pattern("<Frame>", &ctx, None), "Recording");
        assert_eq!(
            effective_pattern("<Scene>", OutputFormat::PngSequence),
            "<Scene>_<Frame>"
        );
        assert_eq!(effective_pattern("<Scene>", OutputFormat::Mp4), "<Scene>");
    }

    #[test]
    fn output_sizes_are_even_for_video() {
        let s = RecorderSettings {
            match_game: true,
            ..Default::default()
        };
        assert_eq!(s.output_size((801, 451)), (800, 450));
        let gif = RecorderSettings {
            format: OutputFormat::Gif,
            match_game: false,
            width: 321,
            height: 241,
            ..Default::default()
        };
        assert_eq!(gif.output_size((1, 1)), (321, 241));
    }

    #[test]
    fn settings_round_trip_and_fill_missing_fields() {
        let s = RecorderSettings::new(OutputFormat::Gif);
        let text = serde_json::to_string(&s).unwrap();
        assert_eq!(serde_json::from_str::<RecorderSettings>(&text).unwrap(), s);
        let old: RecorderSettings = serde_json::from_str(r#"{"name":"Old","fps":24}"#).unwrap();
        assert_eq!(old.fps, 24);
        assert_eq!(old.folder, "Recordings");
    }
}
