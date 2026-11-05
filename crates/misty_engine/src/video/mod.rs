//! Video: Video Clip assets, the Video Player component and its playback.
//!
//! Formats (all decoded in pure Rust, so every build target works without system codecs):
//! - **Motion-JPEG AVI** (`.avi`, MJPG or uncompressed frames, 8/16-bit PCM audio): every frame
//!   is a JPEG decoded with the image crate's JPEG decoder; seeking is exact and cheap.
//! - **Animated GIF** (`.gif`): no audio, per-frame delays.
//!
//! Modern codecs (H.264, VP9, AV1 in `.mp4` / `.webm` / `.mov` …) have no mature pure-Rust
//! decoder; such files show an "unsupported format" message telling how to convert them. The
//! [`transcode`] path turns any playable clip into a Motion-JPEG AVI (GIF → AVI, re-encode).
//!
//! Playback ([`VideoPlayback`]) decodes on a worker thread into a small frame queue
//! ([`decoder`]) and publishes frames as runtime textures ([`textures`]) that the renderers
//! upload to the GPU.

pub mod avi;
pub mod component;
pub mod decoder;
pub mod generate;
pub mod player;
pub mod source;
pub mod textures;

pub use component::{
    VIDEO_REFS, VideoAspectRatio, VideoAudioOutput, VideoMaterialProperty, VideoPlayer,
    VideoRenderMode, VideoSourceKind,
};
pub use player::{VideoEvent, VideoPlayback};
pub use source::{
    EXTENSIONS, Pcm, VideoFormat, VideoInfo, VideoSource, is_video, is_video_like,
    unsupported_message,
};

use serde::{Deserialize, Serialize};

/// Import settings of a Video Clip (stored in its `.meta` file).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct VideoImporter {
    /// Play the clip's audio track (off = always silent).
    pub import_audio: bool,
    /// JPEG quality used when transcoding (1–100).
    pub transcode_quality: u8,
}

impl Default for VideoImporter {
    fn default() -> Self {
        Self {
            import_audio: true,
            transcode_quality: 85,
        }
    }
}

impl VideoImporter {
    /// Settings of the clip at `path` (defaults without a `.meta` file).
    pub fn load(path: &std::path::Path) -> Self {
        crate::vfs::read_to_string(crate::asset_db::meta::meta_path(path))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write the settings into the clip's `.meta` file (keeping its GUID).
    pub fn save(&self, path: &std::path::Path) -> Result<(), String> {
        crate::asset_db::meta::write_settings(path, self)
    }
}

/// Re-encode a playable clip as a Motion-JPEG AVI (keeping its audio track and frame timing at
/// its average frame rate).
pub fn transcode(source: &VideoSource, quality: u8) -> Result<Vec<u8>, String> {
    let info = source.info();
    let fps = info.fps.round().max(1.0) as u32;
    let mut w = avi::AviWriter::new(info.width, info.height, fps, info.audio);
    let audio = source.audio();
    let frames = (info.length * fps as f64).round().max(1.0) as u64;
    for n in 0..frames {
        let t = n as f64 / fps as f64;
        let img = source.frame(source.frame_at(t))?;
        let rgb = image::DynamicImage::ImageRgba8(img).to_rgb8();
        let mut jpeg = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, quality.clamp(1, 100))
            .encode_image(&rgb)
            .map_err(|e| e.to_string())?;
        w.frame(&jpeg);
        if let Some(p) = &audio {
            let ch = p.channels.max(1) as u64;
            let rate = p.rate as u64;
            let (a, b) = (n * rate / fps as u64, (n + 1) * rate / fps as u64);
            let (a, b) = ((a * ch) as usize, ((b * ch) as usize).min(p.samples.len()));
            if a < b {
                w.audio(&p.samples[a..b]);
            }
        }
    }
    Ok(w.finish())
}

#[cfg(test)]
mod tests;
