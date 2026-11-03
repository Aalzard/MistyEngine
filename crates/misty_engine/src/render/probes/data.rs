//! Baked probe data and its file (`ReflectionProbe-N.probe` next to the scene): the prefiltered
//! roughness levels as RGB half floats plus the SH9 diffuse coefficients.
//!
//! Layout (little endian): magic `MPRB`, u32 version, u32 face size, u32 level count,
//! 27 × f32 SH (RGB per coefficient), u64 compressed length, then zlib of every level's faces
//! (+X −X +Y −Y +Z −Z, rows top to bottom) as f16 RGB with the bytes split into planes, which
//! compresses HDR data far better than interleaved halves.

use std::io::{Read, Write};
use std::path::Path;

use glam::Vec3;

use super::cube::Cube;
use super::{prefilter, sh};

const MAGIC: &[u8; 4] = b"MPRB";
const VERSION: u32 = 1;

/// One probe ready to upload: roughness levels (level 0 sharp) and diffuse SH.
#[derive(Clone, Debug, PartialEq)]
pub struct ProbeData {
    pub levels: Vec<Cube>,
    /// Irradiance / π coefficients (see `sh::eval`).
    pub sh: [Vec3; 9],
}

impl ProbeData {
    /// Prefilter a capture (`samples` GGX samples per texel) and project its SH.
    pub fn from_capture(base: &Cube, samples: u32) -> Self {
        // SH from a small copy: diffuse lighting has no detail anyway.
        let small = base.resized(base.size.min(32));
        Self {
            sh: sh::irradiance(&small),
            levels: prefilter::prefilter(base, samples),
        }
    }

    /// Face size of level 0.
    pub fn size(&self) -> u32 {
        self.levels.first().map_or(0, |l| l.size)
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        let mut halves = Vec::new();
        for level in &self.levels {
            for face in &level.faces {
                for c in face {
                    halves.extend(c.to_array().map(|v| half::f16::from_f32(v).to_bits()));
                }
            }
        }
        // Low bytes then high bytes.
        let mut planes: Vec<u8> = halves.iter().map(|h| *h as u8).collect();
        planes.extend(halves.iter().map(|h| (h >> 8) as u8));
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&planes).map_err(err)?;
        let packed = z.finish().map_err(err)?;

        let mut out = Vec::with_capacity(packed.len() + 160);
        out.extend_from_slice(MAGIC);
        for v in [VERSION, self.size(), self.levels.len() as u32] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        for c in &self.sh {
            for v in c.to_array() {
                out.extend_from_slice(&v.to_le_bytes());
            }
        }
        out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        out.extend_from_slice(&packed);
        std::fs::write(path, out).map_err(err)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    fn parse(bytes: &[u8]) -> Result<Self, String> {
        let mut r = Reader { bytes, at: 0 };
        let mut take = |n: usize| r.take(n);
        if take(4)? != MAGIC {
            return Err("not a reflection probe file".into());
        }
        let mut u32s = [0u32; 3];
        for v in &mut u32s {
            *v = u32::from_le_bytes(take(4)?.try_into().expect("4 bytes"));
        }
        let [version, size, count] = u32s;
        if version != VERSION {
            return Err(format!("unsupported version {version}"));
        }
        if !(1..=4096).contains(&size) || count == 0 || count > 16 {
            return Err("bad size".into());
        }
        let mut sh = [Vec3::ZERO; 9];
        for c in &mut sh {
            for i in 0..3 {
                c[i] = f32::from_le_bytes(take(4)?.try_into().expect("4 bytes"));
            }
        }
        let len = u64::from_le_bytes(take(8)?.try_into().expect("8 bytes")) as usize;
        let mut planes = Vec::new();
        flate2::read::ZlibDecoder::new(take(len)?)
            .read_to_end(&mut planes)
            .map_err(|e| e.to_string())?;
        let texels: usize = (0..count)
            .map(|i| 6 * ((size >> i).max(1) as usize).pow(2))
            .sum();
        let n = texels * 3;
        if planes.len() != n * 2 {
            return Err("pixel data has the wrong size".into());
        }
        let value = |i: usize| half::f16::from_bits(planes[i] as u16 | (planes[n + i] as u16) << 8);
        let mut next = 0usize;
        let levels = (0..count)
            .map(|i| {
                let mut c = Cube::new((size >> i).max(1));
                for face in &mut c.faces {
                    for t in face.iter_mut() {
                        *t = Vec3::new(
                            value(next).to_f32(),
                            value(next + 1).to_f32(),
                            value(next + 2).to_f32(),
                        );
                        next += 3;
                    }
                }
                c
            })
            .collect();
        Ok(Self { levels, sh })
    }
}

/// Cursor over the file bytes.
struct Reader<'a> {
    bytes: &'a [u8],
    at: usize,
}

impl<'a> Reader<'a> {
    fn take(&mut self, n: usize) -> Result<&'a [u8], String> {
        let s = self
            .bytes
            .get(self.at..self.at + n)
            .ok_or("file is truncated")?;
        self.at += n;
        Ok(s)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip_at_half_precision() {
        let base = Cube::from_fn(64, |d| Vec3::new(d.x.max(0.0) * 4.0, 0.25, d.y * d.y));
        let data = ProbeData::from_capture(&base, 16);
        let path = std::env::temp_dir().join("misty_probe_round_trip/ReflectionProbe-0.probe");
        data.save(&path).unwrap();
        let back = ProbeData::load(&path).unwrap();
        assert_eq!(back.sh, data.sh);
        assert_eq!(back.levels.len(), data.levels.len());
        for (a, b) in back.levels.iter().zip(&data.levels) {
            assert_eq!(a.size, b.size);
            for (fa, fb) in a.faces.iter().zip(&b.faces) {
                for (x, y) in fa.iter().zip(fb) {
                    assert!((*x - *y).abs().max_element() <= y.abs().max_element() * 1e-3 + 1e-4);
                }
            }
        }
        // Compressed well below the raw half-float size.
        let raw = 6 * 64 * 64 * 6 * 4 / 3;
        assert!(std::fs::metadata(&path).unwrap().len() < raw as u64);
        assert!(ProbeData::parse(b"nope").is_err());
    }
}
