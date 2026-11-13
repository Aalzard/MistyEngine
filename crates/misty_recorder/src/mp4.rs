//! MP4 files holding one H.264 video track, written as frames arrive: `ftyp`, then the samples
//! in an `mdat` (64-bit size, patched at the end), then the `moov` index (sample sizes, offsets,
//! key frames, timing). Every sample is its own chunk, so offsets are simply where each sample
//! landed. [`read_samples`] reads such a file back (tests, the frame count).

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::h264::{Encoded, H264, NAL_PPS, NAL_SPS};
use crate::{Frame, Output, Sink};

/// Ticks a frame: the timescale is `fps * 1000`, so frame times stay exact at any whole rate.
const FRAME_TICKS: u32 = 1000;

pub struct Mp4Sink {
    encoder: H264,
    writer: Mp4Writer,
}

impl Mp4Sink {
    pub fn create(out: &Output) -> Result<Self, String> {
        Ok(Self {
            encoder: H264::new(out.width, out.height, out.fps, out.quality)?,
            writer: Mp4Writer::create(&out.path(None), out.width, out.height, out.fps)?,
        })
    }
}

impl Sink for Mp4Sink {
    fn write(&mut self, frame: Frame) -> Result<(), String> {
        let e = self.encoder.encode(&frame)?;
        self.writer.add(&e)
    }

    fn finish(self: Box<Self>) -> Result<Vec<PathBuf>, String> {
        let path = self.writer.path.clone();
        self.writer.finish()?;
        Ok(vec![path])
    }
}

/// Builds MP4 boxes: `size`, `type`, body.
struct Boxes(Vec<u8>);

impl Boxes {
    fn new() -> Self {
        Self(Vec::new())
    }
    fn u8(&mut self, v: u8) -> &mut Self {
        self.0.push(v);
        self
    }
    fn u16(&mut self, v: u16) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u32(&mut self, v: u32) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn u64(&mut self, v: u64) -> &mut Self {
        self.0.extend_from_slice(&v.to_be_bytes());
        self
    }
    fn bytes(&mut self, b: &[u8]) -> &mut Self {
        self.0.extend_from_slice(b);
        self
    }
    fn zeros(&mut self, n: usize) -> &mut Self {
        self.0.resize(self.0.len() + n, 0);
        self
    }
    /// A box around what `body` writes.
    fn boxed(&mut self, kind: &[u8; 4], body: impl FnOnce(&mut Boxes)) -> &mut Self {
        let at = self.0.len();
        self.u32(0).bytes(kind);
        body(self);
        let size = (self.0.len() - at) as u32;
        self.0[at..at + 4].copy_from_slice(&size.to_be_bytes());
        self
    }
    /// A full box (version + flags).
    fn full(&mut self, kind: &[u8; 4], version: u8, flags: u32, body: impl FnOnce(&mut Boxes)) {
        self.boxed(kind, |b| {
            b.u32(((version as u32) << 24) | (flags & 0x00ff_ffff));
            body(b);
        });
    }
    fn matrix(&mut self) -> &mut Self {
        for v in [0x0001_0000u32, 0, 0, 0, 0x0001_0000, 0, 0, 0, 0x4000_0000] {
            self.u32(v);
        }
        self
    }
}

/// Writes an MP4 file sample by sample.
pub struct Mp4Writer {
    file: BufWriter<File>,
    path: PathBuf,
    width: u32,
    height: u32,
    fps: u32,
    sps: Option<Vec<u8>>,
    pps: Option<Vec<u8>>,
    /// Where the `mdat` box starts.
    mdat_at: u64,
    /// Next byte offset in the file.
    pos: u64,
    sizes: Vec<u32>,
    offsets: Vec<u64>,
    /// Frames each sample lasts (an empty encode lengthens the one before).
    durations: Vec<u32>,
    keys: Vec<u32>,
}

impl Mp4Writer {
    pub fn create(path: &Path, width: u32, height: u32, fps: u32) -> Result<Self, String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut file = BufWriter::new(File::create(path).map_err(err)?);
        let mut b = Boxes::new();
        b.boxed(b"ftyp", |b| {
            b.bytes(b"isom").u32(0x200);
            for brand in [b"isom", b"iso2", b"avc1", b"mp41"] {
                b.bytes(brand);
            }
        });
        let mdat_at = b.0.len() as u64;
        // 64-bit mdat header: size 1, "mdat", the real size (patched in finish()).
        b.u32(1).bytes(b"mdat").u64(0);
        file.write_all(&b.0).map_err(err)?;
        Ok(Self {
            file,
            path: path.to_path_buf(),
            width,
            height,
            fps: fps.max(1),
            sps: None,
            pps: None,
            mdat_at,
            pos: b.0.len() as u64,
            sizes: Vec::new(),
            offsets: Vec::new(),
            durations: Vec::new(),
            keys: Vec::new(),
        })
    }

    /// Add one encoded frame.
    pub fn add(&mut self, e: &Encoded) -> Result<(), String> {
        let mut sample = Vec::new();
        for nal in &e.nals {
            match Encoded::nal_type(nal) {
                NAL_SPS => {
                    self.sps.get_or_insert_with(|| nal.clone());
                }
                NAL_PPS => {
                    self.pps.get_or_insert_with(|| nal.clone());
                }
                _ => {
                    sample.extend_from_slice(&(nal.len() as u32).to_be_bytes());
                    sample.extend_from_slice(nal);
                }
            }
        }
        if sample.is_empty() {
            // Nothing to show for this frame: the previous one stays up a frame longer.
            if let Some(d) = self.durations.last_mut() {
                *d += 1;
            }
            return Ok(());
        }
        self.file
            .write_all(&sample)
            .map_err(|e| format!("{}: {e}", self.path.display()))?;
        self.offsets.push(self.pos);
        self.sizes.push(sample.len() as u32);
        self.durations.push(1);
        if e.key {
            self.keys.push(self.sizes.len() as u32);
        }
        self.pos += sample.len() as u64;
        Ok(())
    }

    /// Samples written so far.
    pub fn samples(&self) -> usize {
        self.sizes.len()
    }

    /// Patch the `mdat` size and write the index.
    pub fn finish(mut self) -> Result<(), String> {
        let path = self.path.clone();
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let (Some(sps), Some(pps)) = (self.sps.clone(), self.pps.clone()) else {
            return Err("MP4: no frames were encoded".into());
        };
        let moov = self.moov(&sps, &pps);
        self.file.write_all(&moov).map_err(err)?;
        let mdat_size = self.pos - self.mdat_at;
        self.file
            .seek(SeekFrom::Start(self.mdat_at + 8))
            .map_err(err)?;
        self.file.write_all(&mdat_size.to_be_bytes()).map_err(err)?;
        self.file.flush().map_err(err)?;
        Ok(())
    }

    fn moov(&self, sps: &[u8], pps: &[u8]) -> Vec<u8> {
        let timescale = self.fps * FRAME_TICKS;
        let frames: u64 = self.durations.iter().map(|d| *d as u64).sum();
        let duration = (frames * FRAME_TICKS as u64).min(u32::MAX as u64) as u32;
        let (w, h) = (self.width, self.height);
        let mut b = Boxes::new();
        b.boxed(b"moov", |b| {
            b.full(b"mvhd", 0, 0, |b| {
                b.u32(0).u32(0).u32(timescale).u32(duration);
                b.u32(0x0001_0000).u16(0x0100).zeros(10).matrix().zeros(24);
                b.u32(2);
            });
            b.boxed(b"trak", |b| {
                b.full(b"tkhd", 0, 3, |b| {
                    b.u32(0).u32(0).u32(1).u32(0).u32(duration);
                    b.zeros(8).u16(0).u16(0).u16(0).u16(0).matrix();
                    b.u32(w << 16).u32(h << 16);
                });
                b.boxed(b"mdia", |b| {
                    b.full(b"mdhd", 0, 0, |b| {
                        b.u32(0).u32(0).u32(timescale).u32(duration);
                        // "und", packed 5 bits a letter.
                        b.u16(0x55c4).u16(0);
                    });
                    b.full(b"hdlr", 0, 0, |b| {
                        b.u32(0).bytes(b"vide").zeros(12).bytes(b"VideoHandler\0");
                    });
                    b.boxed(b"minf", |b| {
                        b.full(b"vmhd", 0, 1, |b| {
                            b.zeros(8);
                        });
                        b.boxed(b"dinf", |b| {
                            b.full(b"dref", 0, 0, |b| {
                                b.u32(1);
                                b.full(b"url ", 0, 1, |_| {});
                            });
                        });
                        b.boxed(b"stbl", |b| self.stbl(b, sps, pps));
                    });
                });
            });
        });
        b.0
    }

    fn stbl(&self, b: &mut Boxes, sps: &[u8], pps: &[u8]) {
        let (w, h) = (self.width, self.height);
        b.full(b"stsd", 0, 0, |b| {
            b.u32(1);
            b.boxed(b"avc1", |b| {
                b.zeros(6).u16(1).zeros(16);
                b.u16(w as u16).u16(h as u16);
                b.u32(0x0048_0000).u32(0x0048_0000).u32(0).u16(1);
                let mut name = [0u8; 32];
                let label = b"Misty Recorder";
                name[0] = label.len() as u8;
                name[1..1 + label.len()].copy_from_slice(label);
                b.bytes(&name).u16(0x18).u16(0xffff);
                b.boxed(b"avcC", |b| {
                    b.u8(1)
                        .u8(sps.get(1).copied().unwrap_or(66))
                        .u8(sps.get(2).copied().unwrap_or(0))
                        .u8(sps.get(3).copied().unwrap_or(30))
                        .u8(0xff)
                        .u8(0xe1)
                        .u16(sps.len() as u16)
                        .bytes(sps)
                        .u8(1)
                        .u16(pps.len() as u16)
                        .bytes(pps);
                });
            });
        });
        // Frame durations, run-length coded.
        let mut runs: Vec<(u32, u32)> = Vec::new();
        for d in &self.durations {
            let delta = d * FRAME_TICKS;
            match runs.last_mut() {
                Some((n, v)) if *v == delta => *n += 1,
                _ => runs.push((1, delta)),
            }
        }
        b.full(b"stts", 0, 0, |b| {
            b.u32(runs.len() as u32);
            for (n, v) in &runs {
                b.u32(*n).u32(*v);
            }
        });
        b.full(b"stss", 0, 0, |b| {
            b.u32(self.keys.len() as u32);
            for k in &self.keys {
                b.u32(*k);
            }
        });
        b.full(b"stsc", 0, 0, |b| {
            b.u32(1).u32(1).u32(1).u32(1);
        });
        b.full(b"stsz", 0, 0, |b| {
            b.u32(0).u32(self.sizes.len() as u32);
            for s in &self.sizes {
                b.u32(*s);
            }
        });
        b.full(b"co64", 0, 0, |b| {
            b.u32(self.offsets.len() as u32);
            for o in &self.offsets {
                b.u64(*o);
            }
        });
    }
}

/// What [`read_samples`] finds in an MP4 file.
#[derive(Clone, Debug, Default)]
pub struct Mp4Info {
    pub width: u32,
    pub height: u32,
    pub timescale: u32,
    pub duration: u32,
    pub sps: Vec<u8>,
    pub pps: Vec<u8>,
    /// Each sample's NAL units (length prefixes removed).
    pub samples: Vec<Vec<Vec<u8>>>,
    pub keys: Vec<u32>,
}

fn be32(d: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_be_bytes(d.get(at..at + 4)?.try_into().ok()?))
}

fn be64(d: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_be_bytes(d.get(at..at + 8)?.try_into().ok()?))
}

/// The child boxes in `d[start..end]`: (type, body start, body end).
fn children(d: &[u8], start: usize, end: usize) -> Vec<([u8; 4], usize, usize)> {
    let mut out = Vec::new();
    let mut at = start;
    while at + 8 <= end {
        let Some(size) = be32(d, at) else { break };
        let kind: [u8; 4] = d[at + 4..at + 8].try_into().unwrap_or_default();
        let (body, size) = match size {
            1 => match be64(d, at + 8) {
                Some(s) => (at + 16, s as usize),
                None => break,
            },
            0 => (at + 8, end - at),
            s => (at + 8, s as usize),
        };
        if size < 8 || at + size > end {
            break;
        }
        out.push((kind, body, at + size));
        at += size;
    }
    out
}

fn find(d: &[u8], start: usize, end: usize, path: &[&[u8; 4]]) -> Option<(usize, usize)> {
    let (first, rest) = path.split_first()?;
    let (_, s, e) = children(d, start, end)
        .into_iter()
        .find(|(k, _, _)| k == *first)?;
    if rest.is_empty() {
        Some((s, e))
    } else {
        find(d, s, e, rest)
    }
}

/// Read an MP4 written by [`Mp4Writer`] (one video track, one sample a chunk).
pub fn read_samples(path: &Path) -> Result<Mp4Info, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let bad = || format!("{}: not an MP4 the Recorder wrote", path.display());
    if d.get(4..8) != Some(b"ftyp") {
        return Err(bad());
    }
    let mut info = Mp4Info::default();
    let (ms, _) = find(&d, 0, d.len(), &[b"moov", b"trak", b"mdia", b"mdhd"]).ok_or_else(bad)?;
    info.timescale = be32(&d, ms + 12).ok_or_else(bad)?;
    info.duration = be32(&d, ms + 16).ok_or_else(bad)?;
    let stbl = [b"moov", b"trak", b"mdia", b"minf", b"stbl"];
    let (ss, se) = find(&d, 0, d.len(), &stbl).ok_or_else(bad)?;
    let (ds, _) = find(&d, ss, se, &[b"stsd"]).ok_or_else(bad)?;
    // stsd: version/flags, count, then the avc1 box.
    let avc1 = ds + 8;
    if d.get(avc1 + 4..avc1 + 8) != Some(b"avc1") {
        return Err(bad());
    }
    let body = avc1 + 8;
    info.width = u16::from_be_bytes([d[body + 24], d[body + 25]]) as u32;
    info.height = u16::from_be_bytes([d[body + 26], d[body + 27]]) as u32;
    let avc1_end = avc1 + be32(&d, avc1).ok_or_else(bad)? as usize;
    let (cs, _) = find(&d, body + 78, avc1_end, &[b"avcC"]).ok_or_else(bad)?;
    let sps_len = u16::from_be_bytes([d[cs + 6], d[cs + 7]]) as usize;
    info.sps = d[cs + 8..cs + 8 + sps_len].to_vec();
    let p = cs + 8 + sps_len + 1;
    let pps_len = u16::from_be_bytes([d[p], d[p + 1]]) as usize;
    info.pps = d[p + 2..p + 2 + pps_len].to_vec();
    let (zs, _) = find(&d, ss, se, &[b"stsz"]).ok_or_else(bad)?;
    let count = be32(&d, zs + 8).ok_or_else(bad)? as usize;
    let sizes: Vec<usize> = (0..count)
        .map(|i| be32(&d, zs + 12 + i * 4).map(|s| s as usize))
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    let (os, _) = find(&d, ss, se, &[b"co64"]).ok_or_else(bad)?;
    let offsets: Vec<usize> = (0..count)
        .map(|i| be64(&d, os + 8 + i * 8).map(|o| o as usize))
        .collect::<Option<_>>()
        .ok_or_else(bad)?;
    if let Some((ks, _)) = find(&d, ss, se, &[b"stss"]) {
        let n = be32(&d, ks + 4).unwrap_or(0) as usize;
        info.keys = (0..n).filter_map(|i| be32(&d, ks + 8 + i * 4)).collect();
    }
    for (o, s) in offsets.iter().zip(&sizes) {
        let sample = d.get(*o..o + s).ok_or_else(bad)?;
        let mut nals = Vec::new();
        let mut at = 0;
        while at + 4 <= sample.len() {
            let n = be32(sample, at).ok_or_else(bad)? as usize;
            nals.push(sample.get(at + 4..at + 4 + n).ok_or_else(bad)?.to_vec());
            at += 4 + n;
        }
        info.samples.push(nals);
    }
    Ok(info)
}
