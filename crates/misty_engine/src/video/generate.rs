//! A generated test clip: colour bars scrolling sideways, a progress block, the frame number
//! as a row of black / white squares, and a sine beep at the start of every second, written as
//! a Motion-JPEG AVI with PCM audio. Used by the samples and the decoder tests.

use image::{Rgb, RgbImage};

use super::avi::AviWriter;

/// Settings of a generated clip.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TestPattern {
    pub width: u32,
    pub height: u32,
    pub fps: u32,
    pub seconds: u32,
    /// Audio sample rate (mono, 16-bit); 0 = no audio track.
    pub sample_rate: u32,
    /// Beep pitch in Hz.
    pub beep_hz: f32,
    /// Beep length in seconds (at the start of each second).
    pub beep_seconds: f32,
    pub jpeg_quality: u8,
}

impl Default for TestPattern {
    fn default() -> Self {
        Self {
            width: 320,
            height: 180,
            fps: 24,
            seconds: 4,
            sample_rate: 22050,
            beep_hz: 880.0,
            beep_seconds: 0.15,
            jpeg_quality: 85,
        }
    }
}

/// The eight bar colours (white, yellow, cyan, green, magenta, red, blue, black).
pub const BARS: [[u8; 3]; 8] = [
    [235, 235, 235],
    [235, 235, 16],
    [16, 235, 235],
    [16, 235, 16],
    [235, 16, 235],
    [235, 16, 16],
    [16, 16, 235],
    [16, 16, 16],
];

/// Bits of the frame number drawn along the bottom.
pub const ID_BITS: u32 = 10;

impl TestPattern {
    pub fn frame_count(&self) -> u32 {
        self.fps * self.seconds
    }

    fn bar_width(&self) -> u32 {
        (self.width / 8).max(1)
    }

    /// Pixels the bars move per frame.
    fn speed(&self) -> u32 {
        (self.width / 80).max(1)
    }

    /// Height of the bar area (the bottom quarter holds the progress and id rows).
    fn bars_height(&self) -> u32 {
        self.height * 3 / 4
    }

    /// Size and top of the id squares.
    fn id_square(&self) -> (u32, u32) {
        let s = (self.width / (ID_BITS + 2)).min(self.height / 8).max(2);
        (s, self.height - s - 2)
    }

    /// Index into [`BARS`] of the bar at column `x` in frame `n`.
    pub fn bar_at(&self, x: u32, n: u32) -> usize {
        let shifted = (x + self.width * 8 - (n * self.speed()) % (self.width * 8)) % self.width;
        ((shifted / self.bar_width()) as usize).min(7)
    }

    /// Centre pixel of the id square for bit `bit` (bit 0 = leftmost = most significant).
    pub fn id_pixel(&self, bit: u32) -> (u32, u32) {
        let (s, top) = self.id_square();
        (s + bit * s + s / 2, top + s / 2)
    }

    /// Frame `n` as an image.
    pub fn frame(&self, n: u32) -> RgbImage {
        let (w, h) = (self.width, self.height);
        let mut img = RgbImage::from_pixel(w, h, Rgb([24, 24, 28]));
        let bars_h = self.bars_height();
        for y in 0..bars_h {
            for x in 0..w {
                img.put_pixel(x, y, Rgb(BARS[self.bar_at(x, n)]));
            }
        }
        // Progress block sweeping left to right across the clip.
        let frames = self.frame_count().max(1);
        let block = (w / 16).max(2);
        let bx = (n * (w - block)) / frames.saturating_sub(1).max(1);
        let (s, id_top) = self.id_square();
        let py = bars_h + 2;
        for y in py..(py + block / 2).min(id_top.saturating_sub(1)) {
            for x in bx..(bx + block).min(w) {
                img.put_pixel(x, y, Rgb([255, 200, 40]));
            }
        }
        // Frame number in binary, most significant bit first.
        for bit in 0..ID_BITS {
            let on = (n >> (ID_BITS - 1 - bit)) & 1 == 1;
            let c = if on { 250 } else { 5 };
            let x0 = s + bit * s;
            for y in id_top + 1..id_top + s - 1 {
                for x in x0 + 1..x0 + s - 1 {
                    if x < w && y < h {
                        img.put_pixel(x, y, Rgb([c, c, c]));
                    }
                }
            }
        }
        img
    }

    /// Read the frame number back from a decoded frame (None if the squares are unclear).
    pub fn read_id(&self, img: &image::RgbaImage) -> Option<u32> {
        let mut n = 0;
        for bit in 0..ID_BITS {
            let (x, y) = self.id_pixel(bit);
            let v = img.get_pixel(x, y)[0];
            n <<= 1;
            match v {
                0..=60 => {}
                190..=255 => n |= 1,
                _ => return None,
            }
        }
        Some(n)
    }

    /// Audio sample `i` (mono): the beep for the first `beep_seconds` of each second.
    pub fn sample(&self, i: u64) -> i16 {
        let rate = self.sample_rate.max(1) as u64;
        let in_second = (i % rate) as f32 / rate as f32;
        if in_second >= self.beep_seconds {
            return 0;
        }
        let t = i as f32 / rate as f32;
        let v = (t * self.beep_hz * std::f32::consts::TAU).sin();
        (v * 12000.0) as i16
    }

    /// Encode the whole clip as a Motion-JPEG AVI.
    pub fn encode(&self) -> Vec<u8> {
        let audio = (self.sample_rate > 0).then_some((self.sample_rate, 1));
        let mut w = AviWriter::new(self.width, self.height, self.fps, audio);
        let rate = self.sample_rate as u64;
        for n in 0..self.frame_count() {
            let img = self.frame(n);
            let mut jpeg = Vec::new();
            let mut enc =
                image::codecs::jpeg::JpegEncoder::new_with_quality(&mut jpeg, self.jpeg_quality);
            enc.encode_image(&img).expect("in-memory JPEG encoding");
            w.frame(&jpeg);
            if rate > 0 {
                // The samples that play during this frame.
                let fps = self.fps as u64;
                let (a, b) = (n as u64 * rate / fps, (n as u64 + 1) * rate / fps);
                let samples: Vec<i16> = (a..b).map(|i| self.sample(i)).collect();
                w.audio(&samples);
            }
        }
        w.finish()
    }
}
