//! WebM files holding one AV1 video track: the EBML header, a Segment (size patched at the end)
//! with Info (duration patched too) and Tracks, then Clusters of SimpleBlocks, a new cluster at
//! every key frame. Timestamps are in milliseconds. [`read_blocks`] reads such a file back.

use std::fs::File;
use std::io::{BufWriter, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

use crate::av1::{Av1, Av1Packet};
use crate::{Frame, Output, Sink};

mod id {
    pub const EBML: u32 = 0x1A45_DFA3;
    pub const EBML_VERSION: u32 = 0x4286;
    pub const EBML_READ_VERSION: u32 = 0x42F7;
    pub const EBML_MAX_ID: u32 = 0x42F2;
    pub const EBML_MAX_SIZE: u32 = 0x42F3;
    pub const DOC_TYPE: u32 = 0x4282;
    pub const DOC_TYPE_VERSION: u32 = 0x4287;
    pub const DOC_TYPE_READ_VERSION: u32 = 0x4285;
    pub const SEGMENT: u32 = 0x1853_8067;
    pub const INFO: u32 = 0x1549_A966;
    pub const TIMESTAMP_SCALE: u32 = 0x2A_D7B1;
    pub const DURATION: u32 = 0x4489;
    pub const MUXING_APP: u32 = 0x4D80;
    pub const WRITING_APP: u32 = 0x5741;
    pub const TRACKS: u32 = 0x1654_AE6B;
    pub const TRACK_ENTRY: u32 = 0xAE;
    pub const TRACK_NUMBER: u32 = 0xD7;
    pub const TRACK_UID: u32 = 0x73C5;
    pub const TRACK_TYPE: u32 = 0x83;
    pub const FLAG_LACING: u32 = 0x9C;
    pub const CODEC_ID: u32 = 0x86;
    pub const CODEC_PRIVATE: u32 = 0x63A2;
    pub const DEFAULT_DURATION: u32 = 0x23_E383;
    pub const VIDEO: u32 = 0xE0;
    pub const PIXEL_WIDTH: u32 = 0xB0;
    pub const PIXEL_HEIGHT: u32 = 0xBA;
    pub const CLUSTER: u32 = 0x1F43_B675;
    pub const TIMESTAMP: u32 = 0xE7;
    pub const SIMPLE_BLOCK: u32 = 0xA3;
}

fn put_id(out: &mut Vec<u8>, id: u32) {
    let b = id.to_be_bytes();
    let skip = b.iter().take_while(|x| **x == 0).count();
    out.extend_from_slice(&b[skip..]);
}

/// An EBML size in 8 bytes (always the long form: simple, and patchable).
fn put_size(out: &mut Vec<u8>, size: u64) {
    out.push(0x01);
    out.extend_from_slice(&size.to_be_bytes()[1..]);
}

fn element(out: &mut Vec<u8>, id: u32, body: &[u8]) {
    put_id(out, id);
    put_size(out, body.len() as u64);
    out.extend_from_slice(body);
}

fn uint(out: &mut Vec<u8>, id: u32, v: u64) {
    let b = v.to_be_bytes();
    let skip = b.iter().take_while(|x| **x == 0).count().min(7);
    element(out, id, &b[skip..]);
}

fn master(out: &mut Vec<u8>, id: u32, body: impl FnOnce(&mut Vec<u8>)) {
    let mut inner = Vec::new();
    body(&mut inner);
    element(out, id, &inner);
}

pub struct WebmSink {
    encoder: Av1,
    file: BufWriter<File>,
    path: PathBuf,
    fps: u32,
    /// Where the Segment's size and the Info duration sit, to patch at the end.
    segment_size_at: u64,
    segment_start: u64,
    duration_at: u64,
    pos: u64,
    cluster: Option<(u64, Vec<u8>)>,
    blocks: u64,
}

impl WebmSink {
    pub fn create(out: &Output) -> Result<Self, String> {
        let encoder = Av1::new(out.width, out.height, out.fps, out.quality)?;
        let path = out.path(None);
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let mut file = BufWriter::new(File::create(&path).map_err(err)?);
        let mut h = Vec::new();
        master(&mut h, id::EBML, |b| {
            uint(b, id::EBML_VERSION, 1);
            uint(b, id::EBML_READ_VERSION, 1);
            uint(b, id::EBML_MAX_ID, 4);
            uint(b, id::EBML_MAX_SIZE, 8);
            element(b, id::DOC_TYPE, b"webm");
            uint(b, id::DOC_TYPE_VERSION, 4);
            uint(b, id::DOC_TYPE_READ_VERSION, 2);
        });
        put_id(&mut h, id::SEGMENT);
        let segment_size_at = h.len() as u64;
        put_size(&mut h, 0);
        let segment_start = h.len() as u64;
        let mut info = Vec::new();
        uint(&mut info, id::TIMESTAMP_SCALE, 1_000_000);
        put_id(&mut info, id::DURATION);
        put_size(&mut info, 8);
        let duration_in_info = info.len();
        info.extend_from_slice(&0f64.to_be_bytes());
        element(&mut info, id::MUXING_APP, b"Misty Recorder");
        element(&mut info, id::WRITING_APP, b"Misty Engine");
        put_id(&mut h, id::INFO);
        put_size(&mut h, info.len() as u64);
        let duration_at = (h.len() + duration_in_info) as u64;
        h.extend_from_slice(&info);
        let header = encoder.sequence_header();
        master(&mut h, id::TRACKS, |b| {
            master(b, id::TRACK_ENTRY, |b| {
                uint(b, id::TRACK_NUMBER, 1);
                uint(b, id::TRACK_UID, 1);
                uint(b, id::TRACK_TYPE, 1);
                uint(b, id::FLAG_LACING, 0);
                element(b, id::CODEC_ID, b"V_AV1");
                if !header.is_empty() {
                    element(b, id::CODEC_PRIVATE, &header);
                }
                uint(
                    b,
                    id::DEFAULT_DURATION,
                    1_000_000_000 / out.fps.max(1) as u64,
                );
                master(b, id::VIDEO, |b| {
                    uint(b, id::PIXEL_WIDTH, out.width as u64);
                    uint(b, id::PIXEL_HEIGHT, out.height as u64);
                });
            });
        });
        file.write_all(&h).map_err(err)?;
        Ok(Self {
            encoder,
            file,
            fps: out.fps.max(1),
            segment_size_at,
            segment_start,
            duration_at,
            pos: h.len() as u64,
            cluster: None,
            blocks: 0,
            path,
        })
    }

    fn ms(&self, frame: u64) -> u64 {
        (frame as f64 * 1000.0 / self.fps as f64).round() as u64
    }

    fn close_cluster(&mut self) -> Result<(), String> {
        if let Some((_, body)) = self.cluster.take() {
            let mut c = Vec::new();
            element(&mut c, id::CLUSTER, &body);
            self.file
                .write_all(&c)
                .map_err(|e| format!("{}: {e}", self.path.display()))?;
            self.pos += c.len() as u64;
        }
        Ok(())
    }

    fn add(&mut self, p: Av1Packet) -> Result<(), String> {
        let t = self.ms(p.frame);
        let open = self
            .cluster
            .as_ref()
            .is_some_and(|(start, _)| !p.key && t - start < 30_000);
        if !open {
            self.close_cluster()?;
            let mut body = Vec::new();
            uint(&mut body, id::TIMESTAMP, t);
            self.cluster = Some((t, body));
        }
        let (start, body) = self.cluster.as_mut().expect("opened above");
        let mut block = vec![0x81];
        block.extend_from_slice(&((t - *start) as i16).to_be_bytes());
        block.push(if p.key { 0x80 } else { 0x00 });
        block.extend_from_slice(&p.data);
        element(body, id::SIMPLE_BLOCK, &block);
        self.blocks += 1;
        Ok(())
    }
}

impl Sink for WebmSink {
    fn write(&mut self, frame: Frame) -> Result<(), String> {
        for p in self.encoder.encode(&frame)? {
            self.add(p)?;
        }
        Ok(())
    }

    fn finish(mut self: Box<Self>) -> Result<Vec<PathBuf>, String> {
        for p in self.encoder.finish()? {
            self.add(p)?;
        }
        self.close_cluster()?;
        let path = self.path.clone();
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        let duration = self.ms(self.blocks) as f64;
        let mut size = Vec::new();
        put_size(&mut size, self.pos - self.segment_start);
        self.file
            .seek(SeekFrom::Start(self.segment_size_at))
            .map_err(err)?;
        self.file.write_all(&size).map_err(err)?;
        self.file
            .seek(SeekFrom::Start(self.duration_at))
            .map_err(err)?;
        self.file.write_all(&duration.to_be_bytes()).map_err(err)?;
        self.file.flush().map_err(err)?;
        Ok(vec![path])
    }
}

/// What [`read_blocks`] finds in a WebM file.
#[derive(Clone, Debug, Default)]
pub struct WebmInfo {
    pub doc_type: String,
    pub codec: String,
    pub width: u32,
    pub height: u32,
    pub duration_ms: f64,
    /// (timestamp ms, key frame, data) of every SimpleBlock.
    pub blocks: Vec<(u64, bool, Vec<u8>)>,
}

fn read_vint(d: &[u8], at: usize, keep_marker: bool) -> Option<(u64, usize)> {
    let first = *d.get(at)?;
    let len = first.leading_zeros() as usize + 1;
    if len > 8 {
        return None;
    }
    let mut v = if keep_marker {
        first as u64
    } else {
        (first as u64) & (0xff >> len)
    };
    for i in 1..len {
        v = (v << 8) | *d.get(at + i)? as u64;
    }
    Some((v, len))
}

fn be_uint(b: &[u8]) -> u64 {
    b.iter().fold(0, |v, x| (v << 8) | *x as u64)
}

/// Read a WebM written by [`WebmSink`].
pub fn read_blocks(path: &Path) -> Result<WebmInfo, String> {
    let d = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut info = WebmInfo::default();
    // Walk every element; descend into the masters this writer uses.
    let masters = [
        id::EBML,
        id::SEGMENT,
        id::INFO,
        id::TRACKS,
        id::TRACK_ENTRY,
        id::VIDEO,
        id::CLUSTER,
    ];
    let mut stack: Vec<(usize, u64)> = vec![(d.len(), 0)];
    let mut at = 0;
    while at < d.len() {
        while stack.len() > 1 && at >= stack.last().map_or(0, |s| s.0) {
            stack.pop();
        }
        let bad = || format!("{}: broken WebM at byte {at}", path.display());
        let (eid, il) = read_vint(&d, at, true).ok_or_else(bad)?;
        let (size, sl) = read_vint(&d, at + il, false).ok_or_else(bad)?;
        let body = at + il + sl;
        let end = body + size as usize;
        if end > d.len() {
            return Err(bad());
        }
        let eid = eid as u32;
        if masters.contains(&eid) {
            let cluster_time = stack.last().map_or(0, |s| s.1);
            stack.push((end, cluster_time));
            at = body;
            continue;
        }
        let b = &d[body..end];
        match eid {
            id::DOC_TYPE => info.doc_type = String::from_utf8_lossy(b).into_owned(),
            id::CODEC_ID => info.codec = String::from_utf8_lossy(b).into_owned(),
            id::PIXEL_WIDTH => info.width = be_uint(b) as u32,
            id::PIXEL_HEIGHT => info.height = be_uint(b) as u32,
            id::DURATION if b.len() == 8 => {
                info.duration_ms = f64::from_be_bytes(b.try_into().unwrap_or_default())
            }
            id::TIMESTAMP => {
                if let Some(top) = stack.last_mut() {
                    top.1 = be_uint(b);
                }
            }
            id::SIMPLE_BLOCK if b.len() >= 4 => {
                let base = stack.last().map_or(0, |s| s.1);
                let rel = i16::from_be_bytes([b[1], b[2]]) as i64;
                let key = b[3] & 0x80 != 0;
                info.blocks
                    .push(((base as i64 + rel) as u64, key, b[4..].to_vec()));
            }
            _ => {}
        }
        at = end;
    }
    Ok(info)
}
