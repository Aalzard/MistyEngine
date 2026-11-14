//! Animated GIF: a 256-colour palette per frame (NeuQuant), looping forever. GIF delays are in
//! hundredths of a second, so frame rates that don't divide 100 alternate delays to keep the
//! right average (30 fps: 3, 3, 4, ...).

use std::fs::File;
use std::io::BufWriter;
use std::path::PathBuf;

use image::codecs::gif::{GifEncoder, Repeat};

use crate::{Frame, Output, Sink};

pub struct GifSink {
    encoder: GifEncoder<BufWriter<File>>,
    path: PathBuf,
    fps: u32,
    frames: u64,
}

/// Hundredths of a second frame `n` (0-based) lasts at `fps`.
pub fn delay_cs(n: u64, fps: u32) -> u32 {
    let at = |k: u64| (k as f64 * 100.0 / fps.max(1) as f64).round() as u64;
    (at(n + 1) - at(n)).max(1) as u32
}

impl GifSink {
    pub fn create(out: &Output) -> Result<Self, String> {
        let path = out.path(None);
        let file = File::create(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        // Speed 10 of 1-30: a good palette without taking seconds a frame.
        let mut encoder = GifEncoder::new_with_speed(BufWriter::new(file), 10);
        encoder
            .set_repeat(Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            encoder,
            path,
            fps: out.fps,
            frames: 0,
        })
    }
}

impl Sink for GifSink {
    fn write(&mut self, frame: Frame) -> Result<(), String> {
        let img = image::RgbaImage::from_raw(frame.width, frame.height, frame.rgba)
            .ok_or("GIF: frame size doesn't match its pixels")?;
        let cs = delay_cs(self.frames, self.fps);
        let delay = image::Delay::from_numer_denom_ms(cs * 10, 1);
        self.encoder
            .encode_frame(image::Frame::from_parts(img, 0, 0, delay))
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.frames += 1;
        Ok(())
    }

    fn finish(self: Box<Self>) -> Result<Vec<PathBuf>, String> {
        let path = self.path.clone();
        // Dropping the encoder writes the trailer and flushes the file.
        drop(self.encoder);
        Ok(vec![path])
    }
}
