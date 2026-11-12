//! Streamed clips (Load Type: Streaming, and Compressed In Memory): a worker thread decodes the
//! file a packet at a time into a lock-free ring the audio thread reads from, so a long music
//! track never sits decoded in memory. Seeks and loop points are handled by the worker; frames
//! carry a generation number so the reader drops whatever was decoded before a seek.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::Duration;

use symphonia::core::audio::conv::FromSample;
use symphonia::core::audio::sample::Sample;
use symphonia::core::audio::{Audio, AudioBuffer, GenericAudioBufferRef};
use symphonia::core::codecs::CodecParameters;
use symphonia::core::codecs::audio::AudioDecoder;
use symphonia::core::formats::probe::Hint;
use symphonia::core::formats::{FormatReader, SeekMode, SeekTo, TrackType};
use symphonia::core::io::{MediaSource, MediaSourceStream};
use symphonia::core::units::Timestamp;

use super::dsp::Frame;

/// Frames the ring holds (about 1.4 s at 48 kHz).
const RING: usize = 1 << 16;
/// No seek pending.
const NO_SEEK: u64 = u64::MAX;

/// Where a stream reads from.
pub enum StreamInput {
    /// A file on disk (Streaming).
    File(std::path::PathBuf),
    /// Encoded bytes kept in memory (Compressed In Memory).
    Memory(Arc<[u8]>),
}

struct Shared {
    /// `generation << 40 | frame` of the latest seek request.
    seek: AtomicU64,
    looping: AtomicBool,
    stop: AtomicBool,
    /// Generation whose end the worker reached (no loop): u32::MAX = not reached.
    ended: AtomicU32,
    /// Fold to mono while decoding (Force To Mono).
    mono: bool,
}

/// What [`StreamReader::pop`] found.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Pull {
    Frame(Frame),
    /// The worker hasn't caught up (the reader plays silence and waits).
    Underrun,
    /// End of a non-looping stream.
    End,
}

/// The audio thread's end of a stream.
pub struct StreamReader {
    rx: rtrb::Consumer<(u32, Frame)>,
    shared: Arc<Shared>,
    generation: u32,
    /// Sample rate of the file.
    pub rate: u32,
    /// Length in frames (0 when the file doesn't say).
    pub frames: u64,
}

impl Drop for StreamReader {
    fn drop(&mut self) {
        self.shared.stop.store(true, Ordering::Relaxed);
    }
}

impl StreamReader {
    /// Probe `input` (on this thread, so errors show up straight away) and start its worker
    /// at frame `start`.
    pub fn open(input: StreamInput, looping: bool, mono: bool, start: u64) -> Result<Self, String> {
        let source: Box<dyn MediaSource> = match &input {
            StreamInput::File(p) => Box::new(open_file(p)?),
            StreamInput::Memory(b) => Box::new(std::io::Cursor::new(b.clone())),
        };
        let dec = Decoder::new(source)?;
        let (rate, frames) = (dec.rate, dec.frames);
        let (tx, rx) = rtrb::RingBuffer::new(RING);
        let shared = Arc::new(Shared {
            seek: AtomicU64::new(if start > 0 { start } else { NO_SEEK }),
            looping: AtomicBool::new(looping),
            stop: AtomicBool::new(false),
            ended: AtomicU32::new(u32::MAX),
            mono,
        });
        let worker = shared.clone();
        std::thread::Builder::new()
            .name("misty-audio-stream".into())
            .spawn(move || run(dec, tx, worker))
            .map_err(|e| e.to_string())?;
        Ok(Self {
            rx,
            shared,
            generation: 0,
            rate,
            frames,
        })
    }

    /// Next frame of the current generation.
    pub fn pop(&mut self) -> Pull {
        while let Ok((g, f)) = self.rx.pop() {
            if g == self.generation {
                return Pull::Frame(f);
            }
        }
        if self.shared.ended.load(Ordering::Acquire) == self.generation {
            Pull::End
        } else {
            Pull::Underrun
        }
    }

    /// Frames decoded and waiting.
    pub fn buffered(&self) -> usize {
        self.rx.slots()
    }

    /// Whether the worker reached the end of the current generation (nothing more will come).
    pub fn ended(&self) -> bool {
        self.shared.ended.load(Ordering::Acquire) == self.generation
    }

    /// Restart decoding at `frame`; frames already decoded are dropped.
    pub fn seek(&mut self, frame: u64) {
        self.generation = (self.generation + 1) & 0x00FF_FFFF;
        let req = ((self.generation as u64) << 40) | frame.min((1 << 40) - 1);
        self.shared.seek.store(req, Ordering::Release);
    }

    pub fn set_looping(&self, looping: bool) {
        self.shared.looping.store(looping, Ordering::Relaxed);
    }
}

fn open_file(p: &Path) -> Result<std::fs::File, String> {
    std::fs::File::open(p).map_err(|e| format!("{}: {e}", p.display()))
}

/// Read the length and sample rate of an encoded clip without decoding it.
pub fn probe(input: StreamInput) -> Result<(u32, u64, usize), String> {
    let source: Box<dyn MediaSource> = match &input {
        StreamInput::File(p) => Box::new(open_file(p)?),
        StreamInput::Memory(b) => Box::new(std::io::Cursor::new(b.clone())),
    };
    let d = Decoder::new(source)?;
    Ok((d.rate, d.frames, d.channels))
}

struct Decoder {
    reader: Box<dyn FormatReader>,
    decoder: Box<dyn AudioDecoder>,
    track: u32,
    rate: u32,
    frames: u64,
    channels: usize,
}

impl Decoder {
    fn new(source: Box<dyn MediaSource>) -> Result<Self, String> {
        let mss = MediaSourceStream::new(source, Default::default());
        let reader = symphonia::default::get_probe()
            .probe(
                &Hint::default(),
                mss,
                Default::default(),
                Default::default(),
            )
            .map_err(|e| e.to_string())?;
        let track = reader
            .default_track(TrackType::Audio)
            .ok_or("no audio track")?;
        let Some(CodecParameters::Audio(params)) = track.codec_params.as_ref() else {
            return Err("no audio track".into());
        };
        let rate = params.sample_rate.ok_or("unknown sample rate")?;
        let channels = params.channels.as_ref().map_or(2, |c| c.count());
        let frames = track.num_frames.unwrap_or(0);
        let decoder = symphonia::default::get_codecs()
            .make_audio_decoder(params, &Default::default())
            .map_err(|e| e.to_string())?;
        let track = track.id;
        Ok(Self {
            reader,
            decoder,
            track,
            rate,
            frames,
            channels,
        })
    }

    /// Frames of the next packet (`None` at the end).
    fn next(&mut self, out: &mut Vec<Frame>) -> Option<()> {
        loop {
            let packet = match self.reader.next_packet() {
                Ok(Some(p)) if p.track_id == self.track => p,
                Ok(Some(_)) => continue,
                Ok(None) | Err(_) => return None,
            };
            // A damaged packet is skipped, not the end of the stream.
            if let Ok(buf) = self.decoder.decode(&packet) {
                frames_of(&buf, out);
                return Some(());
            }
        }
    }

    /// Seek to `frame`; returns how many decoded frames to drop to land exactly on it.
    fn seek(&mut self, frame: u64) -> u64 {
        let to = SeekTo::Timestamp {
            ts: Timestamp::new(frame as i64),
            track_id: self.track,
        };
        match self.reader.seek(SeekMode::Accurate, to) {
            Ok(s) => {
                self.decoder.reset();
                frame.saturating_sub(s.actual_ts.get().max(0) as u64)
            }
            Err(_) => 0,
        }
    }
}

fn frames_of(buf: &GenericAudioBufferRef, out: &mut Vec<Frame>) {
    match buf {
        GenericAudioBufferRef::U8(b) => push(b, out),
        GenericAudioBufferRef::U16(b) => push(b, out),
        GenericAudioBufferRef::U24(b) => push(b, out),
        GenericAudioBufferRef::U32(b) => push(b, out),
        GenericAudioBufferRef::S8(b) => push(b, out),
        GenericAudioBufferRef::S16(b) => push(b, out),
        GenericAudioBufferRef::S24(b) => push(b, out),
        GenericAudioBufferRef::S32(b) => push(b, out),
        GenericAudioBufferRef::F32(b) => push(b, out),
        GenericAudioBufferRef::F64(b) => push(b, out),
    }
}

fn push<S: Sample>(b: &AudioBuffer<S>, out: &mut Vec<Frame>)
where
    f32: FromSample<S>,
{
    let conv = |s: &S| f32::from_sample(*s);
    match (b.plane(0), b.plane(1)) {
        (Some(l), Some(r)) => out.extend(
            l.iter()
                .zip(r.iter())
                .map(|(l, r)| Frame::new(conv(l), conv(r))),
        ),
        (Some(m), None) => out.extend(m.iter().map(|s| Frame::from_mono(conv(s)))),
        _ => {}
    }
}

/// The worker: decode ahead of the reader, follow seeks and loops, stop with the reader.
fn run(mut dec: Decoder, mut tx: rtrb::Producer<(u32, Frame)>, sh: Arc<Shared>) {
    let mut generation = 0u32;
    let mut last_seek = NO_SEEK;
    let mut skip = 0u64;
    let mut pending: Vec<Frame> = Vec::new();
    let mut sent = 0usize;
    let mut at_end = false;
    // Looped back to the start and nothing decoded since: a stream with no playable frames
    // ends instead of seeking back forever.
    let mut empty_loop = false;
    loop {
        if sh.stop.load(Ordering::Relaxed) {
            return;
        }
        let req = sh.seek.load(Ordering::Acquire);
        if req != last_seek {
            last_seek = req;
            generation = (req >> 40) as u32;
            skip = dec.seek(req & ((1 << 40) - 1));
            pending.clear();
            sent = 0;
            at_end = false;
            empty_loop = false;
            sh.ended.store(u32::MAX, Ordering::Release);
        }
        if sent < pending.len() {
            while sent < pending.len() && tx.push((generation, pending[sent])).is_ok() {
                sent += 1;
            }
            if sent < pending.len() {
                std::thread::sleep(Duration::from_millis(2));
            }
            continue;
        }
        if at_end {
            if sh.looping.load(Ordering::Relaxed) && !empty_loop {
                skip = dec.seek(0);
                at_end = false;
                empty_loop = true;
            } else {
                sh.ended.store(generation, Ordering::Release);
                std::thread::sleep(Duration::from_millis(5));
            }
            continue;
        }
        pending.clear();
        sent = 0;
        if dec.next(&mut pending).is_none() {
            at_end = true;
            continue;
        }
        if !pending.is_empty() {
            empty_loop = false;
        }
        if skip > 0 {
            let n = (skip as usize).min(pending.len());
            pending.drain(..n);
            skip -= n as u64;
        }
        if sh.mono {
            for f in &mut pending {
                *f = f.as_mono();
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_looping_stream_without_frames_ends_instead_of_spinning() {
        let empty: Arc<[u8]> = super::super::wav::synth(0.0, 48000, |_| 0.0).into();
        let Ok(mut r) = StreamReader::open(StreamInput::Memory(empty), true, false, 0) else {
            return; // refused up front: nothing to spin on
        };
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match r.pop() {
                Pull::End => break,
                Pull::Frame(_) => panic!("an empty clip has no frames"),
                Pull::Underrun => {
                    assert!(
                        std::time::Instant::now() < deadline,
                        "the worker never ended"
                    );
                    std::thread::sleep(Duration::from_millis(5));
                }
            }
        }
    }
}
