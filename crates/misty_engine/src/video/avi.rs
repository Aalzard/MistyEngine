//! AVI (RIFF) container: reading Motion-JPEG / uncompressed video with PCM audio, and writing
//! Motion-JPEG + 16-bit PCM files (the generated test clips).
//!
//! The reader walks the `movi` list itself instead of trusting `idx1`, so files without an
//! index (or with a broken one) and OpenDML files split into `AVIX` parts still open.

/// Video codecs the decoder handles.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AviCodec {
    /// Each frame is a JPEG image (Huffman tables may be omitted, as MJPEG allows).
    Mjpeg,
    /// Uncompressed bottom-up BGR (24 bit) or BGRA (32 bit) rows.
    Raw { bits: u16 },
}

/// The audio stream's format (PCM only).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AviAudio {
    pub rate: u32,
    pub channels: u16,
    pub bits: u16,
}

/// One chunk's payload in the file.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Chunk {
    pub offset: usize,
    pub len: usize,
}

/// A parsed AVI file (the payload bytes stay in the caller's buffer).
#[derive(Clone, Debug, PartialEq)]
pub struct Avi {
    pub width: u32,
    pub height: u32,
    /// Frames per second (`rate / scale` of the video stream).
    pub fps: f64,
    pub codec: AviCodec,
    pub frames: Vec<Chunk>,
    pub audio: Option<AviAudio>,
    pub audio_chunks: Vec<Chunk>,
}

/// Why a file can't be played.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum AviError {
    NotAvi,
    Corrupt(String),
    /// The video stream uses a codec there is no decoder for (its FourCC).
    UnsupportedCodec(String),
    NoVideo,
}

impl std::fmt::Display for AviError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            AviError::NotAvi => write!(f, "not an AVI file"),
            AviError::Corrupt(m) => write!(f, "corrupt AVI file ({m})"),
            AviError::UnsupportedCodec(c) => write!(
                f,
                "unsupported video codec \"{c}\": AVI files must use Motion-JPEG (MJPG) or \
                 uncompressed frames"
            ),
            AviError::NoVideo => write!(f, "the AVI file has no video stream"),
        }
    }
}

fn u16_at(b: &[u8], o: usize) -> Option<u16> {
    Some(u16::from_le_bytes(b.get(o..o + 2)?.try_into().ok()?))
}

fn u32_at(b: &[u8], o: usize) -> Option<u32> {
    Some(u32::from_le_bytes(b.get(o..o + 4)?.try_into().ok()?))
}

fn fourcc(b: &[u8], o: usize) -> Option<[u8; 4]> {
    b.get(o..o + 4)?.try_into().ok()
}

fn corrupt(m: &str) -> AviError {
    AviError::Corrupt(m.into())
}

#[derive(Default)]
struct Stream {
    kind: [u8; 4],
    handler: [u8; 4],
    scale: u32,
    rate: u32,
    format: Vec<u8>,
}

impl Avi {
    /// Parse the headers and find every frame and audio chunk.
    pub fn parse(b: &[u8]) -> Result<Avi, AviError> {
        if b.len() < 12 || &b[0..4] != b"RIFF" || &b[8..12] != b"AVI " {
            return Err(AviError::NotAvi);
        }
        let mut streams: Vec<Stream> = Vec::new();
        let mut movi: Vec<(usize, usize)> = Vec::new();
        // Top-level RIFF parts: the first `AVI `, then OpenDML `AVIX` continuations.
        let mut pos = 0;
        while pos + 12 <= b.len() {
            let id = fourcc(b, pos).ok_or_else(|| corrupt("truncated"))?;
            let size = u32_at(b, pos + 4).unwrap_or(0) as usize;
            let end = (pos + 8 + size).min(b.len());
            if &id == b"RIFF" {
                walk(b, pos + 12, end, &mut |id, list, start, end| {
                    match (&id, list.as_ref()) {
                        (b"LIST", Some(b"strl")) => streams.push(Stream::default()),
                        (b"LIST", Some(b"movi")) => movi.push((start, end)),
                        (b"strh", _) => {
                            if let Some(s) = streams.last_mut() {
                                s.kind = fourcc(b, start).unwrap_or_default();
                                s.handler = fourcc(b, start + 4).unwrap_or_default();
                                s.scale = u32_at(b, start + 20).unwrap_or(1);
                                s.rate = u32_at(b, start + 24).unwrap_or(0);
                            }
                        }
                        (b"strf", _) => {
                            if let Some(s) = streams.last_mut() {
                                s.format = b[start..end].to_vec();
                            }
                        }
                        _ => {}
                    }
                    // Don't descend into movi here (walked below with the stream numbers).
                    !matches!(list, Some(l) if &l == b"movi")
                });
            }
            pos = end + (end & 1);
        }
        let video = streams
            .iter()
            .position(|s| &s.kind == b"vids")
            .ok_or(AviError::NoVideo)?;
        let vs = &streams[video];
        let fmt = &vs.format;
        let width = u32_at(fmt, 4).ok_or_else(|| corrupt("video format"))?;
        let height = (u32_at(fmt, 8).ok_or_else(|| corrupt("video format"))? as i32).unsigned_abs();
        let bits = u16_at(fmt, 14).unwrap_or(24);
        let compression = fourcc(fmt, 16).unwrap_or_default();
        let is_mjpeg = |c: &[u8; 4]| {
            let up = c.to_ascii_uppercase();
            matches!(
                up.as_slice(),
                b"MJPG" | b"JPEG" | b"AVRN" | b"DMB1" | b"MJLS"
            )
        };
        let codec = if is_mjpeg(&compression) || is_mjpeg(&vs.handler) {
            AviCodec::Mjpeg
        } else if compression == [0; 4] || &compression == b"DIB " {
            match bits {
                24 | 32 => AviCodec::Raw { bits },
                _ => return Err(AviError::UnsupportedCodec(format!("RGB{bits}"))),
            }
        } else {
            let name = String::from_utf8_lossy(&compression).trim().to_owned();
            return Err(AviError::UnsupportedCodec(name));
        };
        let fps = if vs.scale > 0 && vs.rate > 0 {
            vs.rate as f64 / vs.scale as f64
        } else {
            25.0
        };
        let audio_stream = streams.iter().position(|s| &s.kind == b"auds");
        let audio = audio_stream.and_then(|i| {
            let f = &streams[i].format;
            let tag = u16_at(f, 0)?;
            let bits = u16_at(f, 14)?;
            // PCM (1) or WAVE_FORMAT_EXTENSIBLE (0xFFFE) carrying PCM, 8 or 16 bit.
            ((tag == 1 || tag == 0xFFFE) && (bits == 8 || bits == 16)).then_some(AviAudio {
                channels: u16_at(f, 2)?.max(1),
                rate: u32_at(f, 4)?.max(1),
                bits,
            })
        });
        fn tag(id: &[u8; 4], n: usize, suffixes: &[&[u8; 2]]) -> bool {
            let num = format!("{n:02}");
            id[0..2] == *num.as_bytes() && suffixes.iter().any(|s| id[2..4] == **s)
        }
        let is_video = |id: &[u8; 4]| tag(id, video, &[b"dc", b"db"]);
        let is_audio = |id: &[u8; 4]| audio_stream.is_some_and(|a| tag(id, a, &[b"wb"]));
        let mut frames = Vec::new();
        let mut audio_chunks = Vec::new();
        for (start, end) in movi {
            walk(b, start, end, &mut |id, _, s, e| {
                if is_video(&id) {
                    frames.push(Chunk {
                        offset: s,
                        len: e - s,
                    });
                } else if audio.is_some() && is_audio(&id) {
                    audio_chunks.push(Chunk {
                        offset: s,
                        len: e - s,
                    });
                }
                true
            });
        }
        if frames.is_empty() {
            return Err(corrupt("no frames"));
        }
        Ok(Avi {
            width,
            height,
            fps,
            codec,
            frames,
            audio,
            audio_chunks,
        })
    }

    /// Seconds of video.
    pub fn duration(&self) -> f64 {
        self.frames.len() as f64 / self.fps
    }

    /// The whole audio track as interleaved 16-bit samples.
    pub fn audio_samples(&self, b: &[u8]) -> Vec<i16> {
        let Some(a) = self.audio else {
            return Vec::new();
        };
        let mut out = Vec::new();
        for c in &self.audio_chunks {
            let data = &b[c.offset..c.offset + c.len];
            if a.bits == 16 {
                out.extend(
                    data.as_chunks::<2>()
                        .0
                        .iter()
                        .map(|p| i16::from_le_bytes(*p)),
                );
            } else {
                out.extend(data.iter().map(|&v| ((v as i16) - 128) << 8));
            }
        }
        out
    }
}

/// Visit the chunks between `start` and `end`: `f(id, list type, payload start, payload end)`;
/// LIST chunks are descended into when `f` returns true.
fn walk(
    b: &[u8],
    start: usize,
    end: usize,
    f: &mut dyn FnMut([u8; 4], Option<[u8; 4]>, usize, usize) -> bool,
) {
    let mut pos = start;
    while pos + 8 <= end {
        let Some(id) = fourcc(b, pos) else { return };
        let size = u32_at(b, pos + 4).unwrap_or(0) as usize;
        let data = pos + 8;
        let stop = (data + size).min(end);
        if &id == b"LIST" || &id == b"RIFF" {
            let list = fourcc(b, data);
            if f(id, list, data + 4, stop) && data + 4 <= stop {
                walk(b, data + 4, stop, f);
            }
        } else {
            f(id, None, data, stop);
        }
        // Chunks are word aligned.
        pos = data + size + (size & 1);
    }
}

// ---- writing ---------------------------------------------------------------------------------

/// Builds a Motion-JPEG AVI with optional 16-bit PCM audio, interleaved frame by frame.
pub struct AviWriter {
    width: u32,
    height: u32,
    fps: u32,
    audio: Option<AviAudio>,
    movi: Vec<u8>,
    index: Vec<([u8; 4], u32, u32)>,
    frames: u32,
    audio_frames: u32,
    max_chunk: u32,
}

impl AviWriter {
    /// `audio`: sample rate and channel count of 16-bit PCM (`None` = silent clip).
    pub fn new(width: u32, height: u32, fps: u32, audio: Option<(u32, u16)>) -> Self {
        Self {
            width,
            height,
            fps: fps.max(1),
            audio: audio.map(|(rate, channels)| AviAudio {
                rate,
                channels,
                bits: 16,
            }),
            movi: Vec::new(),
            index: Vec::new(),
            frames: 0,
            audio_frames: 0,
            max_chunk: 0,
        }
    }

    fn chunk(&mut self, id: &[u8; 4], data: &[u8]) {
        // idx1 offsets count from the `movi` FourCC.
        let offset = 4 + self.movi.len() as u32;
        self.movi.extend_from_slice(id);
        self.movi
            .extend_from_slice(&(data.len() as u32).to_le_bytes());
        self.movi.extend_from_slice(data);
        if data.len() & 1 == 1 {
            self.movi.push(0);
        }
        self.index.push((*id, offset, data.len() as u32));
        self.max_chunk = self.max_chunk.max(data.len() as u32);
    }

    /// Append one JPEG-encoded frame.
    pub fn frame(&mut self, jpeg: &[u8]) {
        self.chunk(b"00dc", jpeg);
        self.frames += 1;
    }

    /// Append interleaved PCM samples (played alongside the frames written so far).
    pub fn audio(&mut self, samples: &[i16]) {
        let Some(a) = self.audio else { return };
        let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
        self.chunk(b"01wb", &bytes);
        self.audio_frames += (samples.len() / a.channels.max(1) as usize) as u32;
    }

    pub fn finish(self) -> Vec<u8> {
        let list = |kind: &[u8; 4], body: &[u8]| {
            let mut v = Vec::with_capacity(body.len() + 12);
            v.extend_from_slice(b"LIST");
            v.extend_from_slice(&(body.len() as u32 + 4).to_le_bytes());
            v.extend_from_slice(kind);
            v.extend_from_slice(body);
            v
        };
        let chunk = |id: &[u8; 4], body: &[u8]| {
            let mut v = Vec::with_capacity(body.len() + 8);
            v.extend_from_slice(id);
            v.extend_from_slice(&(body.len() as u32).to_le_bytes());
            v.extend_from_slice(body);
            if body.len() & 1 == 1 {
                v.push(0);
            }
            v
        };
        let w32 = |v: &mut Vec<u8>, x: u32| v.extend_from_slice(&x.to_le_bytes());
        let streams = 1 + self.audio.is_some() as u32;

        let mut avih = Vec::new();
        w32(&mut avih, 1_000_000 / self.fps);
        w32(&mut avih, self.max_chunk * self.fps);
        w32(&mut avih, 0);
        w32(&mut avih, 0x10); // AVIF_HASINDEX
        w32(&mut avih, self.frames);
        w32(&mut avih, 0);
        w32(&mut avih, streams);
        w32(&mut avih, self.max_chunk);
        w32(&mut avih, self.width);
        w32(&mut avih, self.height);
        avih.extend_from_slice(&[0; 16]);

        let strh = |kind: &[u8; 4], handler: &[u8; 4], scale: u32, rate: u32, len: u32, ss: u32| {
            let mut v = Vec::new();
            v.extend_from_slice(kind);
            v.extend_from_slice(handler);
            v.extend_from_slice(&[0; 8]); // flags, priority, language
            v.extend_from_slice(&0u32.to_le_bytes()); // initial frames
            v.extend_from_slice(&scale.to_le_bytes());
            v.extend_from_slice(&rate.to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes()); // start
            v.extend_from_slice(&len.to_le_bytes());
            v.extend_from_slice(&0u32.to_le_bytes()); // suggested buffer
            v.extend_from_slice(&u32::MAX.to_le_bytes()); // quality
            v.extend_from_slice(&ss.to_le_bytes());
            v.extend_from_slice(&[0; 8]); // rcFrame
            v
        };
        let mut bih = Vec::new();
        w32(&mut bih, 40);
        w32(&mut bih, self.width);
        w32(&mut bih, self.height);
        bih.extend_from_slice(&1u16.to_le_bytes());
        bih.extend_from_slice(&24u16.to_le_bytes());
        bih.extend_from_slice(b"MJPG");
        w32(&mut bih, self.width * self.height * 3);
        bih.extend_from_slice(&[0; 16]);
        let mut strl_v = chunk(
            b"strh",
            &strh(b"vids", b"MJPG", 1, self.fps, self.frames, 0),
        );
        strl_v.extend(chunk(b"strf", &bih));
        let mut hdrl = chunk(b"avih", &avih);
        hdrl.extend(list(b"strl", &strl_v));
        if let Some(a) = self.audio {
            let align = a.channels as u32 * 2;
            let mut wf = Vec::new();
            wf.extend_from_slice(&1u16.to_le_bytes());
            wf.extend_from_slice(&a.channels.to_le_bytes());
            w32(&mut wf, a.rate);
            w32(&mut wf, a.rate * align);
            wf.extend_from_slice(&(align as u16).to_le_bytes());
            wf.extend_from_slice(&16u16.to_le_bytes());
            let mut strl_a = chunk(
                b"strh",
                &strh(b"auds", &[0; 4], 1, a.rate, self.audio_frames, align),
            );
            strl_a.extend(chunk(b"strf", &wf));
            hdrl.extend(list(b"strl", &strl_a));
        }
        let mut idx = Vec::new();
        for (id, offset, len) in &self.index {
            idx.extend_from_slice(id);
            w32(&mut idx, if id[2..4] == *b"dc" { 0x10 } else { 0 });
            w32(&mut idx, *offset);
            w32(&mut idx, *len);
        }
        let mut body = b"AVI ".to_vec();
        body.extend(list(b"hdrl", &hdrl));
        body.extend(list(b"movi", &self.movi));
        body.extend(chunk(b"idx1", &idx));
        let mut out = b"RIFF".to_vec();
        out.extend_from_slice(&(body.len() as u32).to_le_bytes());
        out.extend(body);
        out
    }
}

// ---- Motion-JPEG Huffman tables --------------------------------------------------------------

/// The standard Huffman tables (JPEG spec Annex K.3) MJPEG frames may leave out, as one DHT
/// segment.
fn standard_dht() -> Vec<u8> {
    fn run(lo: u8, hi: u8) -> impl Iterator<Item = u8> {
        lo..=hi
    }
    let dc_l_bits = [0u8, 1, 5, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0, 0, 0];
    let dc_c_bits = [0u8, 3, 1, 1, 1, 1, 1, 1, 1, 1, 1, 0, 0, 0, 0, 0];
    let dc_vals: Vec<u8> = (0..12).collect();
    let ac_l_bits = [0u8, 2, 1, 3, 3, 2, 4, 3, 5, 5, 4, 4, 0, 0, 1, 0x7d];
    let mut ac_l: Vec<u8> = vec![
        0x01, 0x02, 0x03, 0x00, 0x04, 0x11, 0x05, 0x12, 0x21, 0x31, 0x41, 0x06, 0x13, 0x51, 0x61,
        0x07, 0x22, 0x71, 0x14, 0x32, 0x81, 0x91, 0xa1, 0x08, 0x23, 0x42, 0xb1, 0xc1, 0x15, 0x52,
        0xd1, 0xf0, 0x24, 0x33, 0x62, 0x72, 0x82, 0x09, 0x0a, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x25,
        0x26, 0x27, 0x28, 0x29, 0x2a, 0x34, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
    ];
    for hi in [0x40u8, 0x50, 0x60, 0x70, 0x80] {
        ac_l.extend(run(hi + 3, hi + 0xa));
    }
    for hi in [0x90u8, 0xa0, 0xb0, 0xc0, 0xd0] {
        ac_l.extend(run(hi + 2, hi + 0xa));
    }
    ac_l.extend(run(0xe1, 0xea));
    ac_l.extend(run(0xf1, 0xfa));
    let ac_c_bits = [0u8, 2, 1, 2, 4, 4, 3, 4, 7, 5, 4, 4, 0, 1, 2, 0x77];
    let mut ac_c: Vec<u8> = vec![
        0x00, 0x01, 0x02, 0x03, 0x11, 0x04, 0x05, 0x21, 0x31, 0x06, 0x12, 0x41, 0x51, 0x07, 0x61,
        0x71, 0x13, 0x22, 0x32, 0x81, 0x08, 0x14, 0x42, 0x91, 0xa1, 0xb1, 0xc1, 0x09, 0x23, 0x33,
        0x52, 0xf0, 0x15, 0x62, 0x72, 0xd1, 0x0a, 0x16, 0x24, 0x34, 0xe1, 0x25, 0xf1, 0x17, 0x18,
        0x19, 0x1a, 0x26, 0x27, 0x28, 0x29, 0x2a, 0x35, 0x36, 0x37, 0x38, 0x39, 0x3a,
    ];
    for hi in [0x40u8, 0x50, 0x60, 0x70] {
        ac_c.extend(run(hi + 3, hi + 0xa));
    }
    for hi in [0x80u8, 0x90, 0xa0, 0xb0, 0xc0, 0xd0, 0xe0, 0xf0] {
        ac_c.extend(run(hi + 2, hi + 0xa));
    }
    let mut body = Vec::new();
    for (class, bits, vals) in [
        (0x00u8, &dc_l_bits, &dc_vals),
        (0x01, &dc_c_bits, &dc_vals),
        (0x10, &ac_l_bits, &ac_l),
        (0x11, &ac_c_bits, &ac_c),
    ] {
        body.push(class);
        body.extend_from_slice(bits);
        body.extend_from_slice(vals);
    }
    let mut seg = vec![0xFF, 0xC4];
    seg.extend_from_slice(&((body.len() + 2) as u16).to_be_bytes());
    seg.extend(body);
    seg
}

/// A JPEG frame with the standard Huffman tables inserted when it has none (MJPEG allows
/// leaving them out); frames that carry their own tables are returned as they are.
pub fn with_huffman_tables(jpeg: &[u8]) -> std::borrow::Cow<'_, [u8]> {
    if jpeg.len() < 4 || jpeg[0] != 0xFF || jpeg[1] != 0xD8 {
        return jpeg.into();
    }
    // Walk the marker segments up to the start of scan.
    let mut pos = 2;
    while pos + 4 <= jpeg.len() {
        if jpeg[pos] != 0xFF {
            break;
        }
        let marker = jpeg[pos + 1];
        match marker {
            0xC4 => return jpeg.into(),
            0xDA => break,
            0xD8 | 0x01 | 0xD0..=0xD7 => pos += 2,
            _ => {
                let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
                pos += 2 + len;
            }
        }
    }
    let mut out = Vec::with_capacity(jpeg.len() + 432);
    out.extend_from_slice(&jpeg[..2]);
    out.extend(standard_dht());
    out.extend_from_slice(&jpeg[2..]);
    out.into()
}

/// The JPEG without its DHT segments (tests: MJPEG cameras write frames like this).
#[cfg(test)]
pub fn strip_huffman_tables(jpeg: &[u8]) -> Vec<u8> {
    let mut out = jpeg[..2].to_vec();
    let mut pos = 2;
    while pos + 4 <= jpeg.len() && jpeg[pos] == 0xFF {
        let marker = jpeg[pos + 1];
        if marker == 0xDA {
            break;
        }
        let len = u16::from_be_bytes([jpeg[pos + 2], jpeg[pos + 3]]) as usize;
        if marker != 0xC4 {
            out.extend_from_slice(&jpeg[pos..pos + 2 + len]);
        }
        pos += 2 + len;
    }
    out.extend_from_slice(&jpeg[pos..]);
    out
}
