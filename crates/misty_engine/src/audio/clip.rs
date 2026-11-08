//! Audio Clip import settings (stored in the clip's `.meta` file) and clip information.
//!
//! Load types, as in the familiar engine:
//! - **Decompress On Load**: decoded once into memory when first played (fast, most memory).
//! - **Compressed In Memory**: the encoded file stays in memory; each voice decodes it as it
//!   plays on a worker thread (little memory, some CPU per voice).
//! - **Streaming**: read and decoded from disk as it plays (least memory; for long music).
//!
//! Formats are whatever the built-in pure-Rust decoders read: WAV (PCM), OGG Vorbis and MP3.

use std::path::Path;

use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum LoadType {
    #[default]
    DecompressOnLoad,
    CompressedInMemory,
    Streaming,
}

impl LoadType {
    pub const ALL: [Self; 3] = [
        Self::DecompressOnLoad,
        Self::CompressedInMemory,
        Self::Streaming,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::DecompressOnLoad => "Decompress On Load",
            Self::CompressedInMemory => "Compressed In Memory",
            Self::Streaming => "Streaming",
        }
    }
}

/// Import settings of an Audio Clip.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioImporter {
    pub load_type: LoadType,
    /// Fold stereo clips to mono when loading.
    pub force_to_mono: bool,
}

impl AudioImporter {
    /// Settings of the clip at `path` (defaults without a `.meta` file).
    pub fn load(path: &Path) -> Self {
        crate::vfs::read_to_string(crate::asset_db::meta::meta_path(path))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    /// Write the settings into the clip's `.meta` file (keeping its GUID).
    pub fn save(&self, path: &Path) -> Result<(), String> {
        crate::asset_db::meta::write_settings(path, self)
    }
}

/// What the Inspector shows about a clip file.
#[derive(Clone, Debug, PartialEq)]
pub struct ClipInfo {
    pub format: &'static str,
    pub rate: u32,
    pub channels: usize,
    pub seconds: f32,
}

impl ClipInfo {
    /// "22050 Hz, Mono, 0.500 s".
    pub fn summary(&self) -> String {
        let layout = match self.channels {
            1 => "Mono",
            2 => "Stereo",
            _ => "Multichannel",
        };
        format!("{} Hz, {layout}, {:.3} s", self.rate, self.seconds)
    }
}

/// Format, rate, channels and length of the clip at `path` (`None` if it can't be read).
pub fn info(path: &Path) -> Option<ClipInfo> {
    let bytes = crate::vfs::read(path).ok()?;
    let format = match path
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase())
        .as_deref()
    {
        Some("wav") => "WAV (PCM)",
        Some("ogg") => "Vorbis (OGG)",
        Some("mp3") => "MP3",
        _ => "Unknown",
    };
    let (rate, frames, channels) =
        super::stream::probe(super::stream::StreamInput::Memory(bytes.into())).ok()?;
    Some(ClipInfo {
        format,
        rate,
        channels,
        seconds: frames as f32 / rate.max(1) as f32,
    })
}
