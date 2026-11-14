//! The Recorder package's engine room: turning rendered frames into files.
//!
//! - [`schedule`]: which ticks of capture time become frames (frame and time ranges).
//! - [`readback`]: GPU → CPU copies through a ring of staging buffers, so a frame's pixels are
//!   picked up a few frames later instead of stalling the one that drew them.
//! - [`writer`]: a worker thread per recording that encodes and writes; the queue to it is
//!   bounded, so a slow encoder slows capture down (capture time stays fixed) and no frame is
//!   ever dropped.
//! - Encoders: [`sequence`] (PNG / JPG, on several threads), [`gif`], [`h264`] + [`mp4`]
//!   (MP4 H.264 through OpenH264 compiled into the editor) and [`webm`] + [`av1`] (WebM AV1
//!   through rav1e, pure Rust).
//! - [`audio`]: the game's sound from taps on the mix, kept in step with the frames by the
//!   mix's sample clock; Opus (pure Rust) in MP4 and WebM, a WAV beside image sequences.
//!
//! The settings themselves ([`RecorderSettings`]) live in the engine so Timeline clips and
//! scripts can carry them.

pub mod audio;
pub mod av1;
pub mod gif;
pub mod h264;
pub mod mp4;
pub mod readback;
pub mod schedule;
pub mod sequence;
pub mod session;
pub mod webm;
pub mod writer;

#[cfg(test)]
mod audio_tests;
#[cfg(test)]
mod gpu_tests;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

pub use misty_engine::recorder::{
    OutputFormat, PatternContext, RangeMode, RecorderSettings, Source, effective_pattern,
    expand_pattern,
};

/// One captured frame: tightly packed RGBA8 (sRGB), top row first.
#[derive(Clone, Debug, PartialEq)]
pub struct Frame {
    /// Frame number in capture time (0 = the first frame of the recording).
    pub index: u64,
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

impl Frame {
    /// A frame of one colour (tests, placeholders).
    pub fn solid(index: u64, width: u32, height: u32, rgba: [u8; 4]) -> Self {
        let mut px = Vec::with_capacity((width * height * 4) as usize);
        for _ in 0..width * height {
            px.extend_from_slice(&rgba);
        }
        Self {
            index,
            width,
            height,
            rgba: px,
        }
    }

    /// RGB8 copy (encoders without alpha).
    pub fn rgb(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.rgba.len() / 4 * 3);
        for p in self.rgba.as_chunks::<4>().0 {
            out.extend_from_slice(&p[..3]);
        }
        out
    }
}

/// Where an encoder writes and how.
#[derive(Clone, Debug)]
pub struct Output {
    pub format: OutputFormat,
    pub folder: PathBuf,
    /// File name without extension: the pattern with every wildcard filled in but `<Frame>`,
    /// which is [`FRAME_SLOT`] here (image sequences fill it per frame).
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    /// 1–100.
    pub quality: u8,
    /// Record the game's sound (formats that can hold it: see [`OutputFormat::has_audio`]).
    pub audio: bool,
}

/// Stands for `<Frame>` in [`Output::name`] until a frame number is known.
pub const FRAME_SLOT: &str = "FRAMENUMBERSLOT";

impl Output {
    /// The output for `settings`: folder (relative to `project`), name from its pattern.
    pub fn new(
        settings: &RecorderSettings,
        project: &Path,
        ctx: &PatternContext,
        width: u32,
        height: u32,
    ) -> Self {
        let pattern =
            effective_pattern(&settings.pattern, settings.format).replace("<Frame>", FRAME_SLOT);
        let folder = if settings.folder.trim().is_empty() {
            project.join("Recordings")
        } else {
            let f = PathBuf::from(settings.folder.trim());
            if f.is_absolute() { f } else { project.join(f) }
        };
        Self {
            format: settings.format,
            folder,
            name: expand_pattern(&pattern, ctx, None),
            width,
            height,
            fps: settings.frame_rate(),
            quality: settings.quality.clamp(1, 100),
            audio: settings.record_audio && settings.format.has_audio(),
        }
    }

    /// The file for frame `index` (sequences) or the whole recording (`None`).
    pub fn path(&self, index: Option<u64>) -> PathBuf {
        let name = match index {
            Some(i) => self.name.replace(FRAME_SLOT, &format!("{i:04}")),
            None => self
                .name
                .replace(FRAME_SLOT, "")
                .trim_end_matches(['_', '-', '.', ' '])
                .to_owned(),
        };
        let name = if name.is_empty() {
            "Recording".to_owned()
        } else {
            name
        };
        self.folder
            .join(format!("{name}.{}", self.format.extension()))
    }

    /// The sound file beside an image sequence.
    pub fn wav_path(&self) -> PathBuf {
        self.path(None).with_extension("wav")
    }
}

/// Something frames are written into.
pub trait Sink: Send {
    /// Write one frame (frames come in order; sizes never change mid-recording).
    fn write(&mut self, frame: Frame) -> Result<(), String>;
    /// Write sound: interleaved stereo at [`audio::RATE`], continuing where the last call
    /// stopped (sample 0 = the first frame). Formats without sound ignore it.
    fn write_audio(&mut self, samples: &[f32]) -> Result<(), String> {
        let _ = samples;
        Ok(())
    }
    /// Finish the file(s); returns what was written.
    fn finish(self: Box<Self>) -> Result<Vec<PathBuf>, String>;
}

/// The encoder for `out.format` (creates the folder).
pub fn open_sink(out: &Output) -> Result<Box<dyn Sink>, String> {
    std::fs::create_dir_all(&out.folder).map_err(|e| format!("{}: {e}", out.folder.display()))?;
    Ok(match out.format {
        OutputFormat::PngSequence | OutputFormat::JpgSequence => {
            Box::new(sequence::SequenceSink::create(out.clone())?)
        }
        OutputFormat::Gif => Box::new(gif::GifSink::create(out)?),
        OutputFormat::Mp4 => Box::new(mp4::Mp4Sink::create(out)?),
        OutputFormat::WebM => Box::new(webm::WebmSink::create(out)?),
    })
}

/// Video bit rate for `quality` (1–100) at a size and frame rate: about 0.02 to 0.3 bits a
/// pixel a frame.
pub fn video_bitrate(width: u32, height: u32, fps: u32, quality: u8) -> u32 {
    let bpp = 0.02 + 0.28 * (quality.clamp(1, 100) as f64 / 100.0).powi(2);
    let bps = width as f64 * height as f64 * fps as f64 * bpp;
    bps.clamp(200_000.0, 200_000_000.0) as u32
}
