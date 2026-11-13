//! AV1 through rav1e (BSD-2-Clause, pure Rust, built without its nasm assembly): BT.601 4:2:0
//! limited range, the fastest speed preset, a key frame every two seconds. rav1e looks a few
//! frames ahead, so packets come out after the frames that made them; [`Av1::finish`] drains
//! the rest.

use rav1e::prelude::*;

use crate::Frame;

/// One AV1 temporal unit (one shown frame).
#[derive(Clone, Debug)]
pub struct Av1Packet {
    pub frame: u64,
    pub data: Vec<u8>,
    pub key: bool,
}

pub struct Av1 {
    ctx: Context<u8>,
    width: u32,
    height: u32,
}

/// RGBA8 (sRGB) to BT.601 limited-range Y, U, V planes (4:2:0, chroma averaged over 2x2).
pub fn rgba_to_yuv420(rgba: &[u8], w: usize, h: usize) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let (cw, ch) = (w.div_ceil(2), h.div_ceil(2));
    let mut y = vec![0u8; w * h];
    let mut u = vec![0u8; cw * ch];
    let mut v = vec![0u8; cw * ch];
    let px = |x: usize, yy: usize| {
        let i = (yy.min(h - 1) * w + x.min(w - 1)) * 4;
        (rgba[i] as i32, rgba[i + 1] as i32, rgba[i + 2] as i32)
    };
    for yy in 0..h {
        for x in 0..w {
            let (r, g, b) = px(x, yy);
            y[yy * w + x] = (((66 * r + 129 * g + 25 * b + 128) >> 8) + 16).clamp(0, 255) as u8;
        }
    }
    for cy in 0..ch {
        for cx in 0..cw {
            let mut s = (0, 0, 0);
            for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                let (r, g, b) = px(cx * 2 + dx, cy * 2 + dy);
                s = (s.0 + r, s.1 + g, s.2 + b);
            }
            let (r, g, b) = (s.0 / 4, s.1 / 4, s.2 / 4);
            u[cy * cw + cx] =
                (((-38 * r - 74 * g + 112 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
            v[cy * cw + cx] =
                (((112 * r - 94 * g - 18 * b + 128) >> 8) + 128).clamp(0, 255) as u8;
        }
    }
    (y, u, v)
}

impl Av1 {
    pub fn new(width: u32, height: u32, fps: u32, quality: u8) -> Result<Self, String> {
        let mut enc = EncoderConfig::with_speed_preset(10);
        enc.width = width as usize;
        enc.height = height as usize;
        enc.time_base = Rational::new(1, fps.max(1) as u64);
        enc.bitrate = crate::video_bitrate(width, height, fps, quality) as i32;
        enc.min_key_frame_interval = 1;
        enc.max_key_frame_interval = (fps.max(1) * 2) as u64;
        enc.chroma_sampling = ChromaSampling::Cs420;
        enc.pixel_range = PixelRange::Limited;
        enc.color_description = Some(ColorDescription {
            color_primaries: ColorPrimaries::BT601,
            transfer_characteristics: TransferCharacteristics::SRGB,
            matrix_coefficients: MatrixCoefficients::BT601,
        });
        let threads = std::thread::available_parallelism()
            .map_or(2, |n| n.get().saturating_sub(1))
            .clamp(1, 8);
        let cfg = Config::new().with_encoder_config(enc).with_threads(threads);
        let ctx = cfg
            .new_context::<u8>()
            .map_err(|e| format!("AV1 encoder: {e}"))?;
        Ok(Self { ctx, width, height })
    }

    /// The codec configuration (`av1C`) a container stores.
    pub fn sequence_header(&self) -> Vec<u8> {
        self.ctx.container_sequence_header()
    }

    /// Encode one frame; returns the packets that came out.
    pub fn encode(&mut self, frame: &Frame) -> Result<Vec<Av1Packet>, String> {
        if frame.width != self.width || frame.height != self.height {
            return Err(format!(
                "AV1: frame is {}x{}, the video {}x{}",
                frame.width, frame.height, self.width, self.height
            ));
        }
        let (w, h) = (frame.width as usize, frame.height as usize);
        let (y, u, v) = rgba_to_yuv420(&frame.rgba, w, h);
        let mut f = self.ctx.new_frame();
        f.planes[0].copy_from_raw_u8(&y, w, 1);
        f.planes[1].copy_from_raw_u8(&u, w.div_ceil(2), 1);
        f.planes[2].copy_from_raw_u8(&v, w.div_ceil(2), 1);
        self.ctx
            .send_frame(f)
            .map_err(|e| format!("AV1: {e}"))?;
        self.drain()
    }

    /// Flush the frames still in the look-ahead.
    pub fn finish(&mut self) -> Result<Vec<Av1Packet>, String> {
        self.ctx.flush();
        self.drain()
    }

    fn drain(&mut self) -> Result<Vec<Av1Packet>, String> {
        let mut out = Vec::new();
        loop {
            match self.ctx.receive_packet() {
                Ok(p) => out.push(Av1Packet {
                    frame: p.input_frameno,
                    key: p.frame_type == FrameType::KEY,
                    data: p.data,
                }),
                Err(EncoderStatus::Encoded) => continue,
                Err(EncoderStatus::NeedMoreData | EncoderStatus::LimitReached) => break,
                Err(e) => return Err(format!("AV1: {e}")),
            }
        }
        Ok(out)
    }
}
