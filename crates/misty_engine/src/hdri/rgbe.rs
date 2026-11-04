//! Radiance `.hdr` writer: shared-exponent RGBE texels, run-length encoded per channel
//! ("new" RLE scanlines), readable by any HDR viewer and by our own decoder.

use std::path::Path;

use glam::Vec3;

use super::HdrImage;

/// RGBE bytes of a linear colour (mantissas share the largest component's exponent).
pub fn rgbe(c: Vec3) -> [u8; 4] {
    let m = c.max_element();
    // Zero, tiny and NaN colours are black.
    if m.is_nan() || m <= 1e-32 {
        return [0; 4];
    }
    // m = f · 2^e with f in [0.5, 1).
    let e = m.log2().floor() as i32 + 1;
    let scale = 256.0 / 2f32.powi(e);
    let byte = |v: f32| (v.max(0.0) * scale).min(255.0) as u8;
    [
        byte(c.x),
        byte(c.y),
        byte(c.z),
        (e + 128).clamp(0, 255) as u8,
    ]
}

/// One channel of a scanline as runs (count > 128 = repeat) and literal chunks.
fn encode_channel(out: &mut Vec<u8>, data: &[u8]) {
    let mut i = 0;
    while i < data.len() {
        // Length of the run starting here.
        let run = data[i..]
            .iter()
            .take(127)
            .take_while(|&&b| b == data[i])
            .count();
        if run >= 3 {
            out.push(128 + run as u8);
            out.push(data[i]);
            i += run;
            continue;
        }
        // Literal bytes up to the next run of 3 (or 128 bytes).
        let start = i;
        while i < data.len() && i - start < 128 {
            if i + 2 < data.len() && data[i] == data[i + 1] && data[i] == data[i + 2] {
                break;
            }
            i += 1;
        }
        out.push((i - start) as u8);
        out.extend_from_slice(&data[start..i]);
    }
}

/// The whole file. Widths outside 8..32767 can't use RLE and are written flat.
pub fn encode(img: &HdrImage) -> Vec<u8> {
    let header = format!(
        "#?RADIANCE\n# Misty Engine\nFORMAT=32-bit_rle_rgbe\n\n-Y {} +X {}\n",
        img.height, img.width
    );
    let mut out = header.into_bytes();
    let rle = (8..32768).contains(&img.width);
    let mut channels: [Vec<u8>; 4] = Default::default();
    for row in img.pixels.chunks(img.width as usize) {
        let texels: Vec<[u8; 4]> = row.iter().map(|c| rgbe(*c)).collect();
        if !rle {
            out.extend(texels.iter().flatten());
            continue;
        }
        let w = img.width;
        out.extend_from_slice(&[2, 2, (w >> 8) as u8, (w & 0xff) as u8]);
        for (k, ch) in channels.iter_mut().enumerate() {
            ch.clear();
            ch.extend(texels.iter().map(|t| t[k]));
            encode_channel(&mut out, ch);
        }
    }
    out
}

pub fn save(path: &Path, img: &HdrImage) -> Result<(), String> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(path, encode(img)).map_err(|e| format!("{}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rgbe_keeps_values_within_a_percent() {
        for c in [
            Vec3::new(1.0, 0.5, 0.25),
            Vec3::new(1000.0, 20.0, 0.1),
            Vec3::splat(0.003),
        ] {
            let [r, g, b, e] = rgbe(c);
            let f = 2f32.powi(e as i32 - 128 - 8);
            let back = Vec3::new(r as f32 + 0.5, g as f32 + 0.5, b as f32 + 0.5) * f;
            let m = c.max_element();
            assert!((back - c).abs().max_element() < m * 0.01, "{c} → {back}");
        }
        assert_eq!(rgbe(Vec3::ZERO), [0; 4]);
    }

    /// Our file decodes with the image crate's reader (runs, literals, flat narrow images).
    #[test]
    fn written_files_decode() {
        let img = HdrImage::from_fn(300, 20, |d| {
            if d.y > 0.5 {
                Vec3::new(0.2, 0.4, 0.9)
            } else {
                Vec3::new(d.x.abs() * 5.0, 0.3, (d.z * 7.0).sin().abs())
            }
        });
        for img in [img.clone(), img.downsample(64)] {
            let back = HdrImage::decode(&encode(&img)).unwrap();
            assert_eq!((back.width, back.height), (img.width, img.height));
            for (a, b) in img.pixels.iter().zip(&back.pixels) {
                assert!((*a - *b).abs().max_element() <= a.max_element() * 0.01 + 1e-6);
            }
        }
    }
}
