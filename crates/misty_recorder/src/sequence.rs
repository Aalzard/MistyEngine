//! PNG and JPG sequences: one file a frame, encoded on a few worker threads (PNG compression is
//! the slow part of a recording). Frame `n` goes to worker `n % workers`; each worker has a
//! short queue, so the recording waits for a busy worker instead of dropping its frame. The
//! sound, when kept, goes to a WAV file beside the frames (same name, no frame number).

use std::path::PathBuf;
use std::sync::mpsc::{SyncSender, sync_channel};
use std::thread::JoinHandle;

use crate::audio::WavWriter;
use crate::{Frame, Output, OutputFormat, Sink};

type Worker = (SyncSender<Frame>, JoinHandle<Result<Vec<PathBuf>, String>>);

pub struct SequenceSink {
    workers: Vec<Worker>,
    count: u64,
    error: Option<String>,
    wav: Option<WavWriter>,
}

/// Encode one frame to `path`.
pub fn write_image(out: &Output, frame: &Frame, path: &std::path::Path) -> Result<(), String> {
    let file = std::fs::File::create(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let w = std::io::BufWriter::new(file);
    let err = |e: image::ImageError| format!("{}: {e}", path.display());
    match out.format {
        OutputFormat::JpgSequence => {
            let mut enc = image::codecs::jpeg::JpegEncoder::new_with_quality(w, out.quality);
            enc.encode(
                &frame.rgb(),
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgb8,
            )
            .map_err(err)
        }
        _ => {
            use image::ImageEncoder;
            let enc = image::codecs::png::PngEncoder::new_with_quality(
                w,
                image::codecs::png::CompressionType::Fast,
                image::codecs::png::FilterType::Adaptive,
            );
            enc.write_image(
                &frame.rgba,
                frame.width,
                frame.height,
                image::ExtendedColorType::Rgba8,
            )
            .map_err(err)
        }
    }
}

impl SequenceSink {
    pub fn new(out: Output) -> Self {
        let n = std::thread::available_parallelism()
            .map_or(2, |n| n.get().saturating_sub(1))
            .clamp(1, 4);
        let workers = (0..n)
            .map(|i| {
                let (tx, rx) = sync_channel::<Frame>(2);
                let out = out.clone();
                let handle = std::thread::Builder::new()
                    .name(format!("misty recorder images {i}"))
                    .spawn(move || {
                        let mut written = Vec::new();
                        for frame in rx {
                            let path = out.path(Some(frame.index));
                            write_image(&out, &frame, &path)?;
                            written.push(path);
                        }
                        Ok(written)
                    })
                    .expect("spawn image writer");
                (tx, handle)
            })
            .collect();
        Self {
            workers,
            count: 0,
            error: None,
            wav: None,
        }
    }

    /// With the WAV for the sound when `out` keeps it.
    pub fn create(out: Output) -> Result<Self, String> {
        let wav = if out.audio {
            Some(WavWriter::create(&out.wav_path())?)
        } else {
            None
        };
        Ok(Self {
            wav,
            ..Self::new(out)
        })
    }
}

impl Sink for SequenceSink {
    fn write(&mut self, frame: Frame) -> Result<(), String> {
        if let Some(e) = &self.error {
            return Err(e.clone());
        }
        let i = (self.count % self.workers.len() as u64) as usize;
        self.count += 1;
        if self.workers[i].0.send(frame).is_err() {
            // The worker stopped on an error; finish() reports which.
            self.error = Some("an image writer stopped".into());
            return Err("an image writer stopped".into());
        }
        Ok(())
    }

    fn write_audio(&mut self, samples: &[f32]) -> Result<(), String> {
        match self.wav.as_mut() {
            Some(w) => w.write(samples),
            None => Ok(()),
        }
    }

    fn finish(self: Box<Self>) -> Result<Vec<PathBuf>, String> {
        let wav = self.wav.map(WavWriter::finish).transpose()?;
        let mut all = Vec::new();
        let mut first_err = None;
        for (tx, handle) in self.workers {
            drop(tx);
            match handle.join() {
                Ok(Ok(mut paths)) => all.append(&mut paths),
                Ok(Err(e)) => {
                    first_err.get_or_insert(e);
                }
                Err(_) => {
                    first_err.get_or_insert("an image writer crashed".to_owned());
                }
            }
        }
        if let Some(e) = first_err {
            return Err(e);
        }
        all.sort();
        all.extend(wav);
        Ok(all)
    }
}
