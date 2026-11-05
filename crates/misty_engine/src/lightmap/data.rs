//! One baked lightmap and its file (`Lightmap-N.lightmap` next to the scene): linear HDR RGB
//! (irradiance in the renderer's light units, so `albedo × texel` is the diffuse light).
//!
//! Layout (little endian): magic `MLMP`, u32 version, u32 width, u32 height, u64 compressed
//! length, then zlib of the texels (rows top to bottom = v from 0 to 1) as f16 RGB with the
//! bytes split into low / high planes, which compresses HDR data far better than interleaved
//! halves.

use std::io::{Read, Write};
use std::path::Path;

use glam::Vec3;

const MAGIC: &[u8; 4] = b"MLMP";
const VERSION: u32 = 1;

#[derive(Clone, Debug, PartialEq)]
pub struct LightmapData {
    pub width: u32,
    pub height: u32,
    /// Row-major, `width × height`.
    pub texels: Vec<Vec3>,
}

impl LightmapData {
    pub fn new(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            texels: vec![Vec3::ZERO; (width * height) as usize],
        }
    }

    pub fn get(&self, x: u32, y: u32) -> Vec3 {
        self.texels[(y * self.width + x) as usize]
    }

    /// Bilinear sample at `uv` (clamped), like the GPU does it.
    pub fn sample(&self, uv: glam::Vec2) -> Vec3 {
        let p = uv * glam::Vec2::new(self.width as f32, self.height as f32) - 0.5;
        let (x0, y0) = (p.x.floor(), p.y.floor());
        let (fx, fy) = (p.x - x0, p.y - y0);
        let at = |x: f32, y: f32| {
            let x = (x as i32).clamp(0, self.width as i32 - 1) as u32;
            let y = (y as i32).clamp(0, self.height as i32 - 1) as u32;
            self.get(x, y)
        };
        let top = at(x0, y0).lerp(at(x0 + 1.0, y0), fx);
        let bottom = at(x0, y0 + 1.0).lerp(at(x0 + 1.0, y0 + 1.0), fx);
        top.lerp(bottom, fy)
    }

    /// RGBA16F texels for upload (alpha 1).
    pub fn rgba_f16(&self) -> Vec<half::f16> {
        let mut out = Vec::with_capacity(self.texels.len() * 4);
        for t in &self.texels {
            out.extend(
                [t.x, t.y, t.z, 1.0]
                    .map(half::f16::from_f32)
                    .iter()
                    .copied(),
            );
        }
        out
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        let err = |e: std::io::Error| format!("{}: {e}", path.display());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(err)?;
        }
        std::fs::write(path, self.to_bytes()).map_err(err)
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        let halves: Vec<u16> = self
            .texels
            .iter()
            .flat_map(|c| {
                c.to_array()
                    .map(|v| half::f16::from_f32(v.max(0.0)).to_bits())
            })
            .collect();
        let mut planes: Vec<u8> = halves.iter().map(|h| *h as u8).collect();
        planes.extend(halves.iter().map(|h| (h >> 8) as u8));
        let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::best());
        z.write_all(&planes).expect("writing to memory");
        let packed = z.finish().expect("writing to memory");
        let mut out = Vec::with_capacity(packed.len() + 24);
        out.extend_from_slice(MAGIC);
        for v in [VERSION, self.width, self.height] {
            out.extend_from_slice(&v.to_le_bytes());
        }
        out.extend_from_slice(&(packed.len() as u64).to_le_bytes());
        out.extend_from_slice(&packed);
        out
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let bytes = crate::vfs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&bytes).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < 24 || &bytes[..4] != MAGIC {
            return Err("not a lightmap file".into());
        }
        let u32_at = |i: usize| u32::from_le_bytes(bytes[i..i + 4].try_into().expect("4 bytes"));
        let (version, width, height) = (u32_at(4), u32_at(8), u32_at(12));
        if version != VERSION {
            return Err(format!("unsupported version {version}"));
        }
        if !(1..=8192).contains(&width) || !(1..=8192).contains(&height) {
            return Err("bad size".into());
        }
        let len = u64::from_le_bytes(bytes[16..24].try_into().expect("8 bytes")) as usize;
        let packed = bytes.get(24..24 + len).ok_or("file is truncated")?;
        let mut planes = Vec::new();
        flate2::read::ZlibDecoder::new(packed)
            .read_to_end(&mut planes)
            .map_err(|e| e.to_string())?;
        let n = (width * height * 3) as usize;
        if planes.len() != n * 2 {
            return Err("pixel data has the wrong size".into());
        }
        let value = |i: usize| {
            half::f16::from_bits(planes[i] as u16 | (planes[n + i] as u16) << 8).to_f32()
        };
        let texels = (0..(width * height) as usize)
            .map(|i| Vec3::new(value(i * 3), value(i * 3 + 1), value(i * 3 + 2)))
            .collect();
        Ok(Self {
            width,
            height,
            texels,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn files_round_trip_at_half_precision() {
        let mut m = LightmapData::new(16, 8);
        for (i, t) in m.texels.iter_mut().enumerate() {
            *t = Vec3::new(i as f32 * 0.1, 2.5, (i % 3) as f32);
        }
        let path = std::env::temp_dir().join("misty_lightmap_round_trip/Lightmap-0.lightmap");
        m.save(&path).unwrap();
        let back = LightmapData::load(&path).unwrap();
        assert_eq!((back.width, back.height), (16, 8));
        for (a, b) in back.texels.iter().zip(&m.texels) {
            assert!((*a - *b).abs().max_element() <= b.max_element() * 1e-3 + 1e-4);
        }
        assert!(LightmapData::parse(b"nope").is_err());
    }
}
