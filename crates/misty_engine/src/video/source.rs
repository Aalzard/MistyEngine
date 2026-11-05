//! Opening a video file: the container and codec behind a clip, its frames on demand and its
//! audio track.

use std::sync::Arc;

use image::RgbaImage;

use super::avi::{Avi, AviCodec, with_huffman_tables};

/// What a clip is stored as.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoFormat {
    /// Motion-JPEG (or uncompressed) AVI with optional PCM audio.
    Avi,
    /// Animated GIF (no audio, per-frame delays).
    Gif,
}

/// Interleaved 16-bit PCM audio.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pcm {
    pub rate: u32,
    pub channels: u16,
    pub samples: Vec<i16>,
}

impl Pcm {
    /// Sample frames (one sample per channel).
    pub fn frames(&self) -> usize {
        self.samples.len() / self.channels.max(1) as usize
    }

    pub fn duration(&self) -> f64 {
        self.frames() as f64 / self.rate.max(1) as f64
    }

    /// The audio as a 16-bit PCM `.wav` file in memory (what the sound backend decodes).
    pub fn to_wav(&self) -> Vec<u8> {
        let data_len = (self.samples.len() * 2) as u32;
        let align = self.channels as u32 * 2;
        let mut v = Vec::with_capacity(44 + data_len as usize);
        v.extend_from_slice(b"RIFF");
        v.extend_from_slice(&(36 + data_len).to_le_bytes());
        v.extend_from_slice(b"WAVEfmt ");
        v.extend_from_slice(&16u32.to_le_bytes());
        v.extend_from_slice(&1u16.to_le_bytes());
        v.extend_from_slice(&self.channels.to_le_bytes());
        v.extend_from_slice(&self.rate.to_le_bytes());
        v.extend_from_slice(&(self.rate * align).to_le_bytes());
        v.extend_from_slice(&(align as u16).to_le_bytes());
        v.extend_from_slice(&16u16.to_le_bytes());
        v.extend_from_slice(b"data");
        v.extend_from_slice(&data_len.to_le_bytes());
        for s in &self.samples {
            v.extend_from_slice(&s.to_le_bytes());
        }
        v
    }
}

/// Facts about a clip (the Inspector's info line, `length`, `frameCount`, …).
#[derive(Clone, Debug, PartialEq)]
pub struct VideoInfo {
    pub format: VideoFormat,
    /// Codec name for display ("Motion JPEG", "GIF").
    pub codec: &'static str,
    pub width: u32,
    pub height: u32,
    pub frame_count: u64,
    /// Average frames per second.
    pub fps: f64,
    /// Seconds.
    pub length: f64,
    /// (sample rate, channels) of the audio track.
    pub audio: Option<(u32, u16)>,
}

enum Kind {
    Avi(Avi),
    /// Decoded frames (GIFs are small) with each frame's start time.
    Frames(Vec<RgbaImage>, Vec<f64>),
}

/// An opened clip. Cheap to share between the decoder thread and the player.
pub struct VideoSource {
    bytes: Arc<[u8]>,
    kind: Kind,
    info: VideoInfo,
}

/// Formats people commonly try that have no pure-Rust decoder here.
const UNSUPPORTED: [&str; 9] = [
    "mp4", "m4v", "mov", "webm", "mkv", "ogv", "wmv", "flv", "mpg",
];

/// Extensions the video player opens.
pub const EXTENSIONS: [&str; 2] = ["avi", "gif"];

/// The "this can't be played" message for a file extension, if it is a known video format
/// without a decoder.
pub fn unsupported_message(ext: &str) -> Option<String> {
    let e = ext.to_ascii_lowercase();
    UNSUPPORTED.contains(&e.as_str()).then(|| {
        format!(
            "Unsupported video format \".{e}\": Misty Engine decodes Motion-JPEG AVI (.avi) and \
             animated GIF (.gif). Convert the file, e.g. `ffmpeg -i input.{e} -c:v mjpeg -q:v 3 \
             -c:a pcm_s16le output.avi`."
        )
    })
}

fn extension(path: &str) -> String {
    std::path::Path::new(path)
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default()
}

/// Is `path` a playable video file (by extension)?
pub fn is_video(path: &str) -> bool {
    EXTENSIONS.contains(&extension(path).as_str())
}

/// Is `path` a video file, playable or not (the Project window shows both as video clips)?
pub fn is_video_like(path: &str) -> bool {
    let e = extension(path);
    EXTENSIONS.contains(&e.as_str()) || UNSUPPORTED.contains(&e.as_str())
}

impl VideoSource {
    /// Open a clip from its file bytes; `name` (file name or path) picks the format.
    pub fn open(name: &str, bytes: Arc<[u8]>) -> Result<Self, String> {
        let ext = extension(name);
        if let Some(m) = unsupported_message(&ext) {
            return Err(m);
        }
        let looks_gif = bytes.starts_with(b"GIF8");
        if ext == "gif" || looks_gif {
            return Self::open_gif(bytes);
        }
        let avi = Avi::parse(&bytes).map_err(|e| e.to_string())?;
        let info = VideoInfo {
            format: VideoFormat::Avi,
            codec: match avi.codec {
                AviCodec::Mjpeg => "Motion JPEG",
                AviCodec::Raw { .. } => "Uncompressed RGB",
            },
            width: avi.width,
            height: avi.height,
            frame_count: avi.frames.len() as u64,
            fps: avi.fps,
            length: avi.duration(),
            audio: avi.audio.map(|a| (a.rate, a.channels)),
        };
        Ok(Self {
            bytes,
            kind: Kind::Avi(avi),
            info,
        })
    }

    /// Read and open a file (project path through the virtual file system, or absolute).
    pub fn open_file(path: &std::path::Path) -> Result<Self, String> {
        let name = path.to_string_lossy().into_owned();
        if let Some(m) = unsupported_message(&extension(&name)) {
            return Err(m);
        }
        let bytes = crate::vfs::read(path).map_err(|e| format!("{name}: {e}"))?;
        Self::open(&name, bytes.into())
    }

    fn open_gif(bytes: Arc<[u8]>) -> Result<Self, String> {
        use image::AnimationDecoder;
        let decoder = image::codecs::gif::GifDecoder::new(std::io::Cursor::new(bytes.to_vec()))
            .map_err(|e| format!("GIF: {e}"))?;
        let mut images = Vec::new();
        let mut starts = Vec::new();
        let mut t = 0.0;
        for f in decoder.into_frames() {
            let f = f.map_err(|e| format!("GIF: {e}"))?;
            let (n, d) = f.delay().numer_denom_ms();
            // Browsers treat delays under 20 ms as 100 ms; do the same.
            let mut delay = n as f64 / d.max(1) as f64 / 1000.0;
            if delay < 0.02 {
                delay = 0.1;
            }
            starts.push(t);
            t += delay;
            images.push(f.into_buffer());
        }
        let first = images.first().ok_or("GIF: no frames")?;
        let info = VideoInfo {
            format: VideoFormat::Gif,
            codec: "GIF",
            width: first.width(),
            height: first.height(),
            frame_count: images.len() as u64,
            fps: images.len() as f64 / t.max(1e-6),
            length: t,
            audio: None,
        };
        Ok(Self {
            bytes,
            kind: Kind::Frames(images, starts),
            info,
        })
    }

    pub fn info(&self) -> &VideoInfo {
        &self.info
    }

    /// Start time of frame `n` in seconds.
    pub fn frame_time(&self, n: u64) -> f64 {
        match &self.kind {
            Kind::Avi(a) => n as f64 / a.fps,
            Kind::Frames(_, starts) => starts.get(n as usize).copied().unwrap_or(self.info.length),
        }
    }

    /// The frame showing at `t` seconds (clamped to the clip).
    pub fn frame_at(&self, t: f64) -> u64 {
        let last = self.info.frame_count.saturating_sub(1);
        match &self.kind {
            // Tiny epsilon so t = n / fps lands on frame n despite rounding.
            Kind::Avi(a) => ((t.max(0.0) * a.fps + 1e-6).floor() as u64).min(last),
            Kind::Frames(_, starts) => {
                (starts.partition_point(|s| *s <= t + 1e-9) as u64).saturating_sub(1)
            }
        }
    }

    /// Decode frame `n` as sRGB RGBA.
    pub fn frame(&self, n: u64) -> Result<RgbaImage, String> {
        match &self.kind {
            Kind::Frames(images, _) => images
                .get(n as usize)
                .cloned()
                .ok_or_else(|| format!("frame {n} out of range")),
            Kind::Avi(a) => {
                let c = a
                    .frames
                    .get(n as usize)
                    .ok_or_else(|| format!("frame {n} out of range"))?;
                let data = &self.bytes[c.offset..c.offset + c.len];
                match a.codec {
                    AviCodec::Mjpeg => {
                        let jpeg = with_huffman_tables(data);
                        image::load_from_memory_with_format(&jpeg, image::ImageFormat::Jpeg)
                            .map(|i| i.to_rgba8())
                            .map_err(|e| format!("frame {n}: {e}"))
                    }
                    AviCodec::Raw { bits } => raw_frame(data, a.width, a.height, bits)
                        .ok_or_else(|| format!("frame {n}: truncated")),
                }
            }
        }
    }

    /// The whole audio track (`None` without one).
    pub fn audio(&self) -> Option<Pcm> {
        let Kind::Avi(a) = &self.kind else {
            return None;
        };
        let fmt = a.audio?;
        Some(Pcm {
            rate: fmt.rate,
            channels: fmt.channels,
            samples: a.audio_samples(&self.bytes),
        })
    }
}

/// Bottom-up BGR(A) rows (4-byte aligned) to top-down RGBA.
fn raw_frame(data: &[u8], w: u32, h: u32, bits: u16) -> Option<RgbaImage> {
    let bpp = bits as usize / 8;
    let stride = (w as usize * bpp).div_ceil(4) * 4;
    if data.len() < stride * h as usize {
        return None;
    }
    Some(RgbaImage::from_fn(w, h, |x, y| {
        let row = (h - 1 - y) as usize * stride;
        let p = &data[row + x as usize * bpp..];
        image::Rgba([p[2], p[1], p[0], 255])
    }))
}
