//! `.pak` archives: a built game's assets and settings in one file.
//!
//! Layout (little endian): the 8-byte magic, a `u64` offset of the index, the blobs, then the
//! index: entry count (`u32`) and per entry the path (`u16` length + UTF-8), offset, stored size,
//! original size (`u64` each) and a codec byte (0 = stored, 1 = LZ4 block, 2 = zlib).
//! Paths are project-relative with forward slashes ("Assets/Scenes/Level1.scene").

use std::collections::BTreeMap;
use std::io::{Read, Seek, SeekFrom, Write};
use std::path::Path;
use std::sync::Mutex;

pub const MAGIC: &[u8; 8] = b"MISTYPK1";

/// How an entry's bytes are stored.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Codec {
    Stored = 0,
    Lz4 = 1,
    /// zlib (deflate): smaller than LZ4, slower to unpack. Asset bundles can pick it.
    Zlib = 2,
}

/// What a writer tries for each entry (kept only when it saves at least 1/16).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Compression {
    #[default]
    None,
    Lz4,
    Zlib,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PakEntry {
    pub offset: u64,
    pub stored: u64,
    pub size: u64,
    pub codec: Codec,
}

/// Formats that are already compressed: LZ4 would only cost load time.
const PRECOMPRESSED: [&str; 8] = ["png", "jpg", "jpeg", "ogg", "mp3", "glb", "avi", "gif"];

/// Writes an archive entry by entry.
pub struct PakWriter<W: Write + Seek> {
    out: W,
    index: Vec<(String, PakEntry)>,
    compression: Compression,
}

impl<W: Write + Seek> PakWriter<W> {
    /// Start an archive; `compress` = LZ4 for entries that shrink.
    pub fn new(out: W, compress: bool) -> std::io::Result<Self> {
        let compression = if compress {
            Compression::Lz4
        } else {
            Compression::None
        };
        Self::with_compression(out, compression)
    }

    /// Start an archive compressing entries with `compression` (when they shrink).
    pub fn with_compression(mut out: W, compression: Compression) -> std::io::Result<Self> {
        out.write_all(MAGIC)?;
        out.write_all(&0u64.to_le_bytes())?; // index offset, patched in `finish`
        Ok(Self {
            out,
            index: Vec::new(),
            compression,
        })
    }

    pub fn add(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        self.add_counted(path, data).map(|_| ())
    }

    /// [`PakWriter::add`]; returns the bytes the entry takes in the archive (build reports).
    pub fn add_counted(&mut self, path: &str, data: &[u8]) -> std::io::Result<u64> {
        let offset = self.out.stream_position()?;
        let ext = path.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
        let packed = match self.compression {
            _ if PRECOMPRESSED.contains(&ext.as_str()) => None,
            Compression::None => None,
            Compression::Lz4 => Some((lz4_flex::compress_prepend_size(data), Codec::Lz4)),
            Compression::Zlib => zlib(data).map(|c| (c, Codec::Zlib)),
        }
        // Keep it only when it saves at least 1/16.
        .filter(|(c, _)| c.len() + data.len() / 16 < data.len());
        let (bytes, codec) = match &packed {
            Some((c, codec)) => (c.as_slice(), *codec),
            None => (data, Codec::Stored),
        };
        self.out.write_all(bytes)?;
        self.index.push((
            path.to_owned(),
            PakEntry {
                offset,
                stored: bytes.len() as u64,
                size: data.len() as u64,
                codec,
            },
        ));
        Ok(bytes.len() as u64)
    }

    /// Write the index; returns the writer.
    pub fn finish(mut self) -> std::io::Result<W> {
        let at = self.out.stream_position()?;
        let mut index = Vec::new();
        index.extend_from_slice(&(self.index.len() as u32).to_le_bytes());
        for (path, e) in &self.index {
            let p = path.as_bytes();
            let len = u16::try_from(p.len())
                .map_err(|_| std::io::Error::other(format!("path too long: {path}")))?;
            index.extend_from_slice(&len.to_le_bytes());
            index.extend_from_slice(p);
            for v in [e.offset, e.stored, e.size] {
                index.extend_from_slice(&v.to_le_bytes());
            }
            index.push(e.codec as u8);
        }
        self.out.write_all(&index)?;
        self.out.seek(SeekFrom::Start(MAGIC.len() as u64))?;
        self.out.write_all(&at.to_le_bytes())?;
        self.out.seek(SeekFrom::End(0))?;
        self.out.flush()?;
        Ok(self.out)
    }
}

fn zlib(data: &[u8]) -> Option<Vec<u8>> {
    let mut enc = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    enc.write_all(data).ok()?;
    enc.finish().ok()
}

/// An opened archive; entries are read on demand.
pub struct Pak {
    file: Mutex<Box<dyn ReadSeek>>,
    entries: BTreeMap<String, PakEntry>,
}

pub trait ReadSeek: Read + Seek + Send {}
impl<T: Read + Seek + Send> ReadSeek for T {}

fn bad(msg: &str) -> std::io::Error {
    std::io::Error::new(std::io::ErrorKind::InvalidData, msg.to_owned())
}

impl Pak {
    pub fn open(path: &Path) -> std::io::Result<Self> {
        Self::from_reader(Box::new(std::io::BufReader::new(std::fs::File::open(
            path,
        )?)))
    }

    pub fn from_reader(mut r: Box<dyn ReadSeek>) -> std::io::Result<Self> {
        let mut head = [0u8; 16];
        r.read_exact(&mut head)?;
        if &head[..8] != MAGIC {
            return Err(bad("not a Misty pak file"));
        }
        let at = u64::from_le_bytes(head[8..16].try_into().expect("8 bytes"));
        r.seek(SeekFrom::Start(at))?;
        let mut index = Vec::new();
        r.read_to_end(&mut index)?;
        let mut cur = index.as_slice();
        let mut take = |n: usize| -> std::io::Result<&[u8]> {
            if cur.len() < n {
                return Err(bad("truncated pak index"));
            }
            let (a, b) = cur.split_at(n);
            cur = b;
            Ok(a)
        };
        let count = u32::from_le_bytes(take(4)?.try_into().expect("4 bytes"));
        let mut entries = BTreeMap::new();
        for _ in 0..count {
            let len = u16::from_le_bytes(take(2)?.try_into().expect("2 bytes")) as usize;
            let path = String::from_utf8(take(len)?.to_vec()).map_err(|_| bad("bad path"))?;
            let mut v = [0u64; 3];
            for x in &mut v {
                *x = u64::from_le_bytes(take(8)?.try_into().expect("8 bytes"));
            }
            let codec = match take(1)?[0] {
                0 => Codec::Stored,
                1 => Codec::Lz4,
                2 => Codec::Zlib,
                _ => return Err(bad("unknown pak codec")),
            };
            let entry = PakEntry {
                offset: v[0],
                stored: v[1],
                size: v[2],
                codec,
            };
            entries.insert(path, entry);
        }
        Ok(Self {
            file: Mutex::new(r),
            entries,
        })
    }

    pub fn contains(&self, path: &str) -> bool {
        self.entries.contains_key(path)
    }

    /// Entry paths in sorted order.
    pub fn paths(&self) -> impl Iterator<Item = &str> {
        self.entries.keys().map(String::as_str)
    }

    pub fn entry(&self, path: &str) -> Option<&PakEntry> {
        self.entries.get(path)
    }

    /// The original bytes of an entry.
    pub fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        let e = self.entries.get(path).ok_or_else(|| {
            std::io::Error::new(std::io::ErrorKind::NotFound, format!("{path}: not in pak"))
        })?;
        let mut buf = vec![0u8; e.stored as usize];
        {
            let mut f = self.file.lock().unwrap_or_else(|e| e.into_inner());
            f.seek(SeekFrom::Start(e.offset))?;
            f.read_exact(&mut buf)?;
        }
        match e.codec {
            Codec::Stored => Ok(buf),
            Codec::Lz4 => lz4_flex::decompress_size_prepended(&buf)
                .map_err(|err| bad(&format!("{path}: {err}"))),
            Codec::Zlib => {
                let mut out = Vec::with_capacity(e.size as usize);
                flate2::read::ZlibDecoder::new(buf.as_slice())
                    .read_to_end(&mut out)
                    .map_err(|err| bad(&format!("{path}: {err}")))?;
                Ok(out)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn pak(compress: bool) -> Pak {
        let mut w = PakWriter::new(Cursor::new(Vec::new()), compress).unwrap();
        w.add("Assets/a.txt", &b"hello ".repeat(100)).unwrap();
        w.add("Assets/b.png", &[1, 2, 3]).unwrap();
        w.add("Assets/empty", &[]).unwrap();
        let bytes = w.finish().unwrap().into_inner();
        Pak::from_reader(Box::new(Cursor::new(bytes))).unwrap()
    }

    #[test]
    fn round_trips_with_and_without_compression() {
        for compress in [false, true] {
            let p = pak(compress);
            assert_eq!(p.read("Assets/a.txt").unwrap(), b"hello ".repeat(100));
            assert_eq!(p.read("Assets/b.png").unwrap(), [1, 2, 3]);
            assert!(p.read("Assets/empty").unwrap().is_empty());
            assert!(p.read("Assets/missing").is_err());
            let a = p.entry("Assets/a.txt").unwrap();
            assert_eq!(a.codec == Codec::Lz4, compress);
            assert_eq!(p.entry("Assets/b.png").unwrap().codec, Codec::Stored);
            assert_eq!(p.paths().count(), 3);
        }
        let p = pak(true);
        assert!(p.entry("Assets/a.txt").unwrap().stored < 600);
    }

    #[test]
    fn zlib_entries_round_trip() {
        let mut w =
            PakWriter::with_compression(Cursor::new(Vec::new()), Compression::Zlib).unwrap();
        w.add("Assets/a.txt", &b"misty ".repeat(200)).unwrap();
        w.add("Assets/tiny", b"x").unwrap();
        let p = Pak::from_reader(Box::new(Cursor::new(w.finish().unwrap().into_inner()))).unwrap();
        assert_eq!(p.entry("Assets/a.txt").unwrap().codec, Codec::Zlib);
        assert_eq!(p.read("Assets/a.txt").unwrap(), b"misty ".repeat(200));
        assert_eq!(p.entry("Assets/tiny").unwrap().codec, Codec::Stored);
        assert_eq!(p.read("Assets/tiny").unwrap(), b"x");
    }

    #[test]
    fn rejects_other_files() {
        let r = Pak::from_reader(Box::new(Cursor::new(b"PK\x03\x04 not a pak".to_vec())));
        assert!(r.is_err());
    }
}
