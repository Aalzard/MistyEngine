//! H.264 through OpenH264 (Cisco, BSD-2-Clause), compiled from the source the `openh264` crate
//! bundles, so the editor needs no system library or downloaded codec. Frames are converted to
//! BT.601 4:2:0, encoded as Constrained Baseline with a key frame every two seconds and frame
//! skipping off (every frame in is a frame out).

use openh264::OpenH264API;
use openh264::encoder::{
    BitRate, Encoder, EncoderConfig, FrameRate, IntraFramePeriod, RateControlMode, UsageType,
    VuiConfig,
};
use openh264::formats::{RgbaSliceU8, YUVBuffer};

use crate::Frame;

/// NAL unit types the MP4 writer cares about.
pub const NAL_SLICE: u8 = 1;
pub const NAL_IDR: u8 = 5;
pub const NAL_SPS: u8 = 7;
pub const NAL_PPS: u8 = 8;

pub struct H264 {
    encoder: Encoder,
    width: u32,
    height: u32,
}

/// One encoded frame: NAL units without start codes.
#[derive(Clone, Debug, Default)]
pub struct Encoded {
    pub nals: Vec<Vec<u8>>,
    pub key: bool,
}

impl Encoded {
    pub fn nal_type(nal: &[u8]) -> u8 {
        nal.first().map_or(0, |b| b & 0x1f)
    }
}

/// Split an Annex-B byte stream (`00 00 01` / `00 00 00 01` start codes) into NAL units.
pub fn split_annex_b(data: &[u8]) -> Vec<&[u8]> {
    let mut starts = Vec::new();
    let mut i = 0;
    while i + 3 <= data.len() {
        if data[i] == 0 && data[i + 1] == 0 && data[i + 2] == 1 {
            starts.push((i, i + 3));
            i += 3;
        } else {
            i += 1;
        }
    }
    let mut out = Vec::new();
    for (k, &(_, body)) in starts.iter().enumerate() {
        let mut end = starts.get(k + 1).map_or(data.len(), |s| s.0);
        // The zero before a 4-byte start code belongs to it.
        while end > body && data[end - 1] == 0 && k + 1 < starts.len() {
            end -= 1;
        }
        if end > body {
            out.push(&data[body..end]);
        }
    }
    out
}

impl H264 {
    pub fn new(width: u32, height: u32, fps: u32, quality: u8) -> Result<Self, String> {
        let config = EncoderConfig::new()
            .bitrate(BitRate::from_bps(crate::video_bitrate(
                width, height, fps, quality,
            )))
            .max_frame_rate(FrameRate::from_hz(fps as f32))
            .rate_control_mode(RateControlMode::Bitrate)
            .usage_type(UsageType::CameraVideoRealTime)
            .skip_frames(false)
            .intra_frame_period(IntraFramePeriod::from_num_frames(fps * 2))
            .vui(VuiConfig::bt601());
        let encoder = Encoder::with_api_config(OpenH264API::from_source(), config)
            .map_err(|e| format!("H.264 encoder: {e}"))?;
        Ok(Self {
            encoder,
            width,
            height,
        })
    }

    pub fn encode(&mut self, frame: &Frame) -> Result<Encoded, String> {
        if frame.width != self.width || frame.height != self.height {
            return Err(format!(
                "H.264: frame is {}x{}, the video {}x{}",
                frame.width, frame.height, self.width, self.height
            ));
        }
        let rgba = RgbaSliceU8::new(&frame.rgba, (frame.width as usize, frame.height as usize));
        let yuv = YUVBuffer::from_rgb8_source(rgba);
        let bits = self
            .encoder
            .encode(&yuv)
            .map_err(|e| format!("H.264: {e}"))?;
        let mut out = Encoded::default();
        for l in 0..bits.num_layers() {
            let Some(layer) = bits.layer(l) else { continue };
            for n in 0..layer.nal_count() {
                let Some(nal) = layer.nal_unit(n) else {
                    continue;
                };
                for unit in split_annex_b(nal) {
                    if Encoded::nal_type(unit) == NAL_IDR {
                        out.key = true;
                    }
                    out.nals.push(unit.to_vec());
                }
            }
        }
        Ok(out)
    }
}
