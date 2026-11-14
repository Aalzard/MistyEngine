//! A recording's sound.
//!
//! - [`AudioFeed`]: the game's mix arrives through taps ([`misty_engine::audio::tap`]) in
//!   capture time; each sample carries the mix's sample clock, so the feed places it on the
//!   recording's own timeline (sample 0 = the first frame) instead of trusting when it arrived.
//!   Time no tap covers (no sound device, the game not started yet, a scene swapped) becomes
//!   silence, and the end is cut or padded to exactly the video's length: sound and picture
//!   cannot drift apart.
//! - [`Resampler`]: the device's rate (often 44.1 kHz) to the 48 kHz the encoders use.
//! - [`OpusStream`]: Opus (royalty-free, BSD-licensed) through `opus1`, pure Rust, for MP4 and
//!   WebM; [`WavWriter`]: 16-bit WAV next to image sequences.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::num::NonZeroU32;
use std::path::{Path, PathBuf};

use misty_engine::audio::MixTap;
use misty_engine::audio::dsp::Frame;

/// Sample rate of everything the Recorder writes.
pub const RATE: u32 = 48_000;
/// Channels (stereo, the mix's).
pub const CHANNELS: usize = 2;
/// Samples (per channel) in one Opus packet: 20 ms.
pub const OPUS_FRAME: usize = 960;
/// Capture time a tap may lag behind before the feed fills the gap with silence (the device
/// thread renders up to a second of capture time per block, so a live mix is never this late).
pub const LAG_SECONDS: f64 = 2.0;

/// Samples (per channel) at [`RATE`] in `seconds`.
pub fn samples_in(seconds: f64) -> u64 {
    (seconds.max(0.0) * RATE as f64).round() as u64
}

// ---- resampling -----------------------------------------------------------------------------

/// Streaming 4-point Hermite resampler from one rate to [`RATE`]. Output sample `k` is input
/// time `k * from / RATE`, so counts stay exact over any length.
pub struct Resampler {
    step: f64,
    /// Input position of the next output sample, relative to `input[1]`.
    pos: f64,
    /// Unused input, with one frame of history in front.
    input: Vec<Frame>,
}

impl Resampler {
    pub fn new(from: f32) -> Self {
        Self {
            step: from as f64 / RATE as f64,
            pos: 0.0,
            input: vec![Frame::ZERO],
        }
    }

    /// Feed `frames`, append what can be made to `out`.
    pub fn push(&mut self, frames: &[Frame], out: &mut Vec<Frame>) {
        self.input.extend_from_slice(frames);
        // Point i uses input[i - 1 ..= i + 2] (i counted from input[1]).
        while self.pos.floor() as usize + 3 < self.input.len() {
            let i = self.pos.floor() as usize + 1;
            let t = (self.pos - self.pos.floor()) as f32;
            let w = [
                self.input[i - 1],
                self.input[i],
                self.input[i + 1],
                self.input[i + 2],
            ];
            out.push(Frame {
                left: hermite(w.map(|f| f.left), t),
                right: hermite(w.map(|f| f.right), t),
            });
            self.pos += self.step;
        }
        let used = self.pos.floor() as usize;
        if used > 0 {
            self.input.drain(..used);
            self.pos -= used as f64;
        }
    }
}

fn hermite([x0, x1, x2, x3]: [f32; 4], t: f32) -> f32 {
    let c1 = 0.5 * (x2 - x0);
    let c2 = x0 - 2.5 * x1 + 2.0 * x2 - 0.5 * x3;
    let c3 = 0.5 * (x3 - x0) + 1.5 * (x1 - x2);
    ((c3 * t + c2) * t + c1) * t + x1
}

// ---- the feed -------------------------------------------------------------------------------

/// One tap and where its next sample lands.
struct Placed {
    tap: MixTap,
    resampler: Option<Resampler>,
    /// Timeline position (48 kHz samples from the first frame) of its next output sample.
    next: Option<i64>,
    /// Samples read before the first frame (dropped, but they count for the position).
    skipped: u64,
    /// Its game is gone and it is drained.
    done: bool,
    raw: Vec<Frame>,
    out: Vec<Frame>,
}

/// The recording's sound on its own timeline, built from mix taps.
#[derive(Default)]
pub struct AudioFeed {
    /// Capture-time seconds of the recording's first frame (`None` until it is captured).
    origin: Option<f64>,
    taps: Vec<Placed>,
    /// Samples (per channel) handed out so far.
    written: u64,
}

impl AudioFeed {
    pub fn new() -> Self {
        Self::default()
    }

    /// A tap on the mix (one per game session: Play mode and scene swaps make new ones).
    pub fn add_tap(&mut self, tap: MixTap) {
        self.taps.push(Placed {
            tap,
            resampler: None,
            next: None,
            skipped: 0,
            done: false,
            raw: Vec::new(),
            out: Vec::new(),
        });
    }

    /// The first frame was captured at capture-time second `at` (later calls change nothing).
    pub fn begin(&mut self, at: f64) {
        self.origin.get_or_insert(at);
    }

    pub fn started(&self) -> bool {
        self.origin.is_some()
    }

    /// Samples (per channel) handed out so far.
    pub fn written(&self) -> u64 {
        self.written
    }

    /// Interleaved stereo at [`RATE`] that is ready now that capture time reached `now`.
    pub fn pull(&mut self, now: f64) -> Vec<f32> {
        let mut out = Vec::new();
        self.take_taps(&mut out, None);
        if let Some(origin) = self.origin {
            // Time no tap covered (and none will this late): silence.
            let due = samples_in(now - LAG_SECONDS - origin);
            self.pad_to(due, &mut out);
        }
        out
    }

    /// Everything left, cut or padded to exactly `total` samples (per channel). Waits up to
    /// `wait` for a live mix to deliver its last block.
    pub fn finish(mut self, total: u64, wait: std::time::Duration) -> Vec<f32> {
        let mut out = Vec::new();
        let deadline = std::time::Instant::now() + wait;
        loop {
            self.take_taps(&mut out, Some(total));
            let live = self.taps.iter().any(|p| !p.tap.closed());
            if self.written >= total || !live || std::time::Instant::now() >= deadline {
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
        self.origin.get_or_insert(0.0);
        self.pad_to(total, &mut out);
        out
    }

    fn pad_to(&mut self, until: u64, out: &mut Vec<f32>) {
        if until > self.written {
            out.resize(out.len() + (until - self.written) as usize * CHANNELS, 0.0);
            self.written = until;
        }
    }

    /// Read every tap and place its samples (up to `limit` samples of timeline).
    fn take_taps(&mut self, out: &mut Vec<f32>, limit: Option<u64>) {
        let origin = self.origin;
        let mut written = self.written;
        for p in &mut self.taps {
            // Closed before reading: this read empties it for good.
            p.done = p.tap.closed();
            p.raw.clear();
            p.tap.read(&mut p.raw);
            let (Some(start), Some(rate)) = (p.tap.start(), p.tap.sample_rate()) else {
                continue;
            };
            // Before the first frame: all of it is earlier than the frame.
            let Some(origin) = origin else {
                p.skipped += p.raw.len() as u64;
                continue;
            };
            let skipped = p.skipped;
            let next = *p.next.get_or_insert_with(|| {
                let t = (start + skipped) as f64 / rate as f64 - origin;
                (t * RATE as f64).round() as i64
            });
            p.out.clear();
            if (rate - RATE as f32).abs() < 0.5 {
                p.out.extend_from_slice(&p.raw);
            } else {
                p.resampler
                    .get_or_insert_with(|| Resampler::new(rate))
                    .push(&p.raw, &mut p.out);
            }
            let mut at = next;
            for f in &p.out {
                let keep = at >= written as i64 && limit.is_none_or(|l| (at as u64) < l);
                if keep {
                    // A gap before it (a tap that started late): silence.
                    if at as u64 > written {
                        out.resize(out.len() + (at as u64 - written) as usize * CHANNELS, 0.0);
                        written = at as u64;
                    }
                    out.push(f.left);
                    out.push(f.right);
                    written += 1;
                }
                at += 1;
            }
            p.next = Some(at);
        }
        self.written = written;
        // Taps whose game is gone and that are drained.
        self.taps.retain(|p| !p.done);
    }
}

// ---- Opus -----------------------------------------------------------------------------------

/// Opus bit rate (stereo) for `quality` 1–100: 64 to 192 kb/s.
pub fn opus_bitrate(quality: u8) -> u32 {
    64_000 + 1_280 * quality.clamp(1, 100) as u32
}

/// An Opus encoder fed any number of samples, giving 20 ms packets.
pub struct OpusStream {
    encoder: opus1::OpusEncoder,
    pending: Vec<f32>,
    /// Samples (per channel) the decoder drops at the start (the encoder's delay).
    pub pre_skip: u16,
    /// Real samples (per channel) pushed.
    samples: u64,
    packet: Vec<u8>,
}

impl OpusStream {
    pub fn new(bitrate: u32) -> Result<Self, String> {
        let mut encoder = opus1::OpusEncoder::new(
            opus1::SampleRate::Hz48000,
            opus1::Channels::Stereo,
            opus1::Application::Audio,
        )
        .map_err(|e| format!("Opus: {e:?}"))?;
        if let Some(b) = NonZeroU32::new(bitrate) {
            encoder.set_bitrate(opus1::Bitrate::BitsPerSecond(b));
        }
        let pre_skip = encoder.lookahead() as u16;
        Ok(Self {
            encoder,
            pending: Vec::new(),
            pre_skip,
            samples: 0,
            packet: vec![0; 4000],
        })
    }

    /// Real samples (per channel) pushed so far.
    pub fn samples(&self) -> u64 {
        self.samples
    }

    /// Add interleaved stereo; returns the packets now complete.
    pub fn push(&mut self, interleaved: &[f32]) -> Result<Vec<Vec<u8>>, String> {
        self.samples += (interleaved.len() / CHANNELS) as u64;
        self.pending.extend_from_slice(interleaved);
        self.encode_full()
    }

    /// Flush: the encoder's delay and the last partial packet are filled with silence.
    pub fn finish(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let n = self.pending.len() + self.pre_skip as usize * CHANNELS;
        let whole = n.div_ceil(OPUS_FRAME * CHANNELS) * OPUS_FRAME * CHANNELS;
        self.pending.resize(whole, 0.0);
        self.encode_full()
    }

    fn encode_full(&mut self) -> Result<Vec<Vec<u8>>, String> {
        let mut packets = Vec::new();
        let len = OPUS_FRAME * CHANNELS;
        let mut at = 0;
        while at + len <= self.pending.len() {
            let n = self
                .encoder
                .encode(&self.pending[at..at + len], OPUS_FRAME, &mut self.packet)
                .map_err(|e| format!("Opus: {e:?}"))?;
            packets.push(self.packet[..n].to_vec());
            at += len;
        }
        self.pending.drain(..at);
        Ok(packets)
    }

    /// The Ogg / Matroska `OpusHead` (RFC 7845): version, channels, pre-skip, rate, gain,
    /// mapping family 0. Little-endian.
    pub fn head(&self) -> Vec<u8> {
        let mut h = b"OpusHead".to_vec();
        h.push(1);
        h.push(CHANNELS as u8);
        h.extend_from_slice(&self.pre_skip.to_le_bytes());
        h.extend_from_slice(&RATE.to_le_bytes());
        h.extend_from_slice(&0i16.to_le_bytes());
        h.push(0);
        h
    }
}

/// Decode Opus packets back to interleaved stereo with the pre-skip dropped (tests, checks).
pub fn decode_opus(packets: &[Vec<u8>], pre_skip: usize) -> Result<Vec<f32>, String> {
    let mut dec = opus1::OpusDecoder::new(opus1::SampleRate::Hz48000, opus1::Channels::Stereo)
        .map_err(|e| format!("Opus: {e:?}"))?;
    let mut out = Vec::new();
    let mut pcm = vec![0f32; 5760 * CHANNELS];
    for p in packets {
        let n = dec
            .decode(p, 5760, &mut pcm)
            .map_err(|e| format!("Opus: {e:?}"))?;
        out.extend_from_slice(&pcm[..n * CHANNELS]);
    }
    let skip = (pre_skip * CHANNELS).min(out.len());
    out.drain(..skip);
    Ok(out)
}

// ---- WAV ------------------------------------------------------------------------------------

/// 16-bit stereo WAV at [`RATE`], written as samples arrive (sizes patched at the end).
pub struct WavWriter {
    file: BufWriter<File>,
    pub path: PathBuf,
    bytes: u64,
}

impl WavWriter {
    pub fn create(path: &Path) -> Result<Self, String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut file = BufWriter::new(File::create(path).map_err(err)?);
        let mut h = Vec::with_capacity(44);
        h.extend_from_slice(b"RIFF");
        h.extend_from_slice(&0u32.to_le_bytes());
        h.extend_from_slice(b"WAVEfmt ");
        h.extend_from_slice(&16u32.to_le_bytes());
        h.extend_from_slice(&1u16.to_le_bytes());
        h.extend_from_slice(&(CHANNELS as u16).to_le_bytes());
        h.extend_from_slice(&RATE.to_le_bytes());
        h.extend_from_slice(&(RATE * CHANNELS as u32 * 2).to_le_bytes());
        h.extend_from_slice(&(CHANNELS as u16 * 2).to_le_bytes());
        h.extend_from_slice(&16u16.to_le_bytes());
        h.extend_from_slice(b"data");
        h.extend_from_slice(&0u32.to_le_bytes());
        file.write_all(&h).map_err(err)?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            bytes: 0,
        })
    }

    pub fn write(&mut self, interleaved: &[f32]) -> Result<(), String> {
        let mut b = Vec::with_capacity(interleaved.len() * 2);
        for s in interleaved {
            b.extend_from_slice(&((s.clamp(-1.0, 1.0) * 32767.0).round() as i16).to_le_bytes());
        }
        self.bytes += b.len() as u64;
        self.file
            .write_all(&b)
            .map_err(|e| format!("{}: {e}", self.path.display()))
    }

    pub fn finish(mut self) -> Result<PathBuf, String> {
        let path = self.path.clone();
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let data = self.bytes.min(u32::MAX as u64 - 36) as u32;
        self.file.seek(SeekFrom::Start(4)).map_err(err)?;
        self.file.write_all(&(data + 36).to_le_bytes()).map_err(err)?;
        self.file.seek(SeekFrom::Start(40)).map_err(err)?;
        self.file.write_all(&data.to_le_bytes()).map_err(err)?;
        self.file.flush().map_err(err)?;
        Ok(path)
    }
}
