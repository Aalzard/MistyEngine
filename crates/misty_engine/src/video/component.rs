//! The Video Player component: what to play and where its frames and sound go.

use hecs::Entity;
use serde::{Deserialize, Serialize};

/// Where the clip comes from.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoSourceKind {
    /// A Video Clip asset of the project.
    #[default]
    VideoClip,
    /// A file path (absolute, project-relative or `file://`).
    Url,
}

/// Where frames are drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoRenderMode {
    /// Behind everything the camera draws (replaces the sky).
    CameraFarPlane,
    /// In front of everything the camera draws (under the UI).
    CameraNearPlane,
    /// Into a named runtime texture that materials and Raw Images can use.
    RenderTexture,
    /// Into a texture slot of the object's renderer.
    #[default]
    MaterialOverride,
    /// Nowhere: scripts read `texture` themselves.
    ApiOnly,
}

impl VideoRenderMode {
    pub const ALL: [VideoRenderMode; 5] = [
        VideoRenderMode::CameraFarPlane,
        VideoRenderMode::CameraNearPlane,
        VideoRenderMode::RenderTexture,
        VideoRenderMode::MaterialOverride,
        VideoRenderMode::ApiOnly,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoRenderMode::CameraFarPlane => "Camera Far Plane",
            VideoRenderMode::CameraNearPlane => "Camera Near Plane",
            VideoRenderMode::RenderTexture => "Render Texture",
            VideoRenderMode::MaterialOverride => "Material Override",
            VideoRenderMode::ApiOnly => "API Only",
        }
    }
}

/// How the picture fits a camera plane.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoAspectRatio {
    /// Pixel for pixel, centred.
    NoScaling,
    /// Full height, width follows the clip's aspect ratio.
    FitVertically,
    /// Full width, height follows.
    FitHorizontally,
    /// Whole picture visible (letterboxed).
    #[default]
    FitInside,
    /// Whole screen covered (cropped).
    FitOutside,
    /// Fill the screen, ignoring the aspect ratio.
    Stretch,
}

impl VideoAspectRatio {
    pub const ALL: [VideoAspectRatio; 6] = [
        VideoAspectRatio::NoScaling,
        VideoAspectRatio::FitVertically,
        VideoAspectRatio::FitHorizontally,
        VideoAspectRatio::FitInside,
        VideoAspectRatio::FitOutside,
        VideoAspectRatio::Stretch,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoAspectRatio::NoScaling => "No Scaling",
            VideoAspectRatio::FitVertically => "Fit Vertically",
            VideoAspectRatio::FitHorizontally => "Fit Horizontally",
            VideoAspectRatio::FitInside => "Fit Inside",
            VideoAspectRatio::FitOutside => "Fit Outside",
            VideoAspectRatio::Stretch => "Stretch",
        }
    }

    /// Size of a `video`-sized picture on a `screen`-sized plane (pixels).
    pub fn fit(self, video: glam::Vec2, screen: glam::Vec2) -> glam::Vec2 {
        let v = video.max(glam::Vec2::splat(1.0));
        let (sx, sy) = (screen.x / v.x, screen.y / v.y);
        match self {
            VideoAspectRatio::NoScaling => v,
            VideoAspectRatio::FitVertically => v * sy,
            VideoAspectRatio::FitHorizontally => v * sx,
            VideoAspectRatio::FitInside => v * sx.min(sy),
            VideoAspectRatio::FitOutside => v * sx.max(sy),
            VideoAspectRatio::Stretch => screen,
        }
    }
}

/// Where the clip's audio track goes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoAudioOutput {
    /// Muted.
    None,
    /// Straight to the speakers (2D, `direct_volume`).
    Direct,
    /// Through an Audio Source (its volume, 3D settings and mixer group).
    #[default]
    AudioSource,
}

impl VideoAudioOutput {
    pub const ALL: [VideoAudioOutput; 3] = [
        VideoAudioOutput::None,
        VideoAudioOutput::Direct,
        VideoAudioOutput::AudioSource,
    ];

    pub fn label(self) -> &'static str {
        match self {
            VideoAudioOutput::None => "None",
            VideoAudioOutput::Direct => "Direct",
            VideoAudioOutput::AudioSource => "Audio Source",
        }
    }
}

/// Which texture slot Material Override replaces.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum VideoMaterialProperty {
    /// The albedo (main) texture.
    #[default]
    MainTexture,
    /// The emission map (a glowing screen; the material needs emission on).
    EmissionMap,
}

impl VideoMaterialProperty {
    pub fn label(self) -> &'static str {
        match self {
            VideoMaterialProperty::MainTexture => "Main Texture",
            VideoMaterialProperty::EmissionMap => "Emission Map",
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoPlayer {
    pub enabled: bool,
    pub source: VideoSourceKind,
    /// Video Clip asset ("Assets/Video/Intro.avi").
    pub clip: Option<String>,
    /// File path used when `source` is Url.
    pub url: String,
    pub play_on_awake: bool,
    /// Hold the clock until the first frame is ready (no skipped start).
    pub wait_for_first_frame: bool,
    pub looping: bool,
    /// Drop late frames to keep up with the clock (else every frame shows, running late).
    pub skip_on_drop: bool,
    /// Speed multiplier (0.01–10).
    pub playback_speed: f32,
    pub render_mode: VideoRenderMode,
    /// Camera Far / Near Plane: the camera (None = the game camera).
    #[serde(with = "crate::scene::components::entity_bits::option")]
    pub target_camera: Option<Entity>,
    /// Camera planes: opacity of the picture.
    pub target_camera_alpha: f32,
    /// Render Texture: the runtime texture name ("Screen" → `runtime://Screen`).
    pub target_texture: String,
    /// Material Override: the renderer (None = this object's).
    #[serde(with = "crate::scene::components::entity_bits::option")]
    pub target_renderer: Option<Entity>,
    pub target_material_property: VideoMaterialProperty,
    /// Material Override: which Materials element (0 = first).
    pub target_material_index: usize,
    pub aspect_ratio: VideoAspectRatio,
    pub audio_output: VideoAudioOutput,
    /// Audio Source output: the source (None = this object's).
    #[serde(with = "crate::scene::components::entity_bits::option")]
    pub audio_source: Option<Entity>,
    /// Direct output volume (0–1).
    pub direct_volume: f32,
    pub mute: bool,
    /// The `video://` texture the player is publishing while playing (not saved).
    #[serde(skip)]
    pub runtime_texture: Option<String>,
}

impl Default for VideoPlayer {
    fn default() -> Self {
        Self {
            enabled: true,
            source: VideoSourceKind::VideoClip,
            clip: None,
            url: String::new(),
            play_on_awake: true,
            wait_for_first_frame: true,
            looping: false,
            skip_on_drop: true,
            playback_speed: 1.0,
            render_mode: VideoRenderMode::MaterialOverride,
            target_camera: None,
            target_camera_alpha: 1.0,
            target_texture: String::new(),
            target_renderer: None,
            target_material_property: VideoMaterialProperty::MainTexture,
            target_material_index: 0,
            aspect_ratio: VideoAspectRatio::FitInside,
            audio_output: VideoAudioOutput::AudioSource,
            audio_source: None,
            direct_volume: 1.0,
            mute: false,
            runtime_texture: None,
        }
    }
}

impl VideoPlayer {
    /// The file to open: the clip, or the URL with `file://` stripped (None when unset).
    pub fn file(&self) -> Option<String> {
        match self.source {
            VideoSourceKind::VideoClip => self.clip.clone().filter(|c| !c.is_empty()),
            VideoSourceKind::Url => {
                let u = self.url.trim();
                let u = u.strip_prefix("file://").unwrap_or(u);
                (!u.is_empty()).then(|| u.to_owned())
            }
        }
    }

    /// The texture name frames are visible under while playing (Render Texture uses its
    /// target name, every other mode the player's own `video://` texture).
    pub fn output_texture(&self) -> Option<String> {
        match self.render_mode {
            VideoRenderMode::RenderTexture => super::textures::target_name(&self.target_texture),
            _ => self.runtime_texture.clone(),
        }
    }
}

/// GameObject reference fields of the Video Player (scene files store object indices).
pub const VIDEO_REFS: &[(&str, &str, &str)] = &[
    ("Video Player", "", "target_camera"),
    ("Video Player", "", "target_renderer"),
    ("Video Player", "", "audio_source"),
];
