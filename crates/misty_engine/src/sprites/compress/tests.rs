//! BC encode/decode error bounds, format choice, the cache file and key, cache hits, and the
//! GPU upload of compressed levels.

use image::{Rgba, RgbaImage};

use super::*;
use crate::sprites::import::PlatformOverride;

/// A smooth colour gradient with a soft bump: what photos and painted textures look like.
fn gradient(w: u32, h: u32, alpha: bool) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let (u, v) = (x as f32 / w as f32, y as f32 / h as f32);
        let bump = ((u * 9.0).sin() * (v * 7.0).cos() * 0.5 + 0.5) * 40.0;
        let a = if alpha { (255.0 * u) as u8 } else { 255 };
        Rgba([
            (40.0 + 180.0 * u + bump) as u8,
            (60.0 + 150.0 * v) as u8,
            (200.0 - 120.0 * u * v + bump * 0.5) as u8,
            a,
        ])
    })
}

/// A tangent-space normal map (unit vectors, Z up).
fn normal_map(w: u32, h: u32) -> RgbaImage {
    RgbaImage::from_fn(w, h, |x, y| {
        let nx = (x as f32 * 0.3).sin() * 0.5;
        let ny = (y as f32 * 0.2).cos() * 0.4;
        let nz = (1.0 - nx * nx - ny * ny).max(0.0).sqrt();
        let e = |c: f32| ((c * 0.5 + 0.5) * 255.0).round() as u8;
        Rgba([e(nx), e(ny), e(nz), 255])
    })
}

/// Root-mean-square error per channel (0–255) over `channels`.
fn rmse(a: &RgbaImage, b: &RgbaImage, channels: std::ops::Range<usize>) -> f32 {
    assert_eq!(a.dimensions(), b.dimensions());
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in channels.clone() {
            let d = p[c] as f64 - q[c] as f64;
            sum += d * d;
            n += 1;
        }
    }
    (sum / n as f64).sqrt() as f32
}

#[test]
fn bc_formats_decode_within_error_bounds() {
    let img = gradient(64, 64, false);
    let alpha = gradient(64, 64, true);
    let bc1 = encode(img.clone(), BcFormat::Bc1, true, false);
    let bc3 = encode(alpha.clone(), BcFormat::Bc3, true, false);
    let bc7 = encode(img.clone(), BcFormat::Bc7, true, false);
    let e1 = rmse(&img, &bc1.decode(0), 0..3);
    let e3 = rmse(&alpha, &bc3.decode(0), 0..4);
    let e7 = rmse(&img, &bc7.decode(0), 0..3);
    assert!(e1 < 6.0, "BC1 RMSE {e1}");
    assert!(e3 < 6.0, "BC3 RMSE {e3}");
    assert!(e7 < 2.5, "BC7 RMSE {e7}");
    assert!(e7 < e1, "BC7 ({e7}) beats BC1 ({e1})");
    // BC3 alpha is interpolated (a ramp stays smooth).
    assert!(rmse(&alpha, &bc3.decode(0), 3..4) < 2.0);
    // Sizes: 8 bytes per 4×4 block for BC1, 16 for the others.
    assert_eq!(bc1.levels[0].len(), 16 * 16 * 8);
    assert_eq!(bc3.levels[0].len(), 16 * 16 * 16);

    // BC5 keeps X and Y of normal maps; the rebuilt Z stays close.
    let n = normal_map(64, 64);
    let bc5 = encode(n.clone(), BcFormat::Bc5, false, false);
    let d = bc5.decode(0);
    assert!(rmse(&n, &d, 0..2) < 2.0, "BC5 RMSE {}", rmse(&n, &d, 0..2));
    let mut worst = 0.0f32;
    for (p, q) in n.pixels().zip(d.pixels()) {
        let v = |c: u8| c as f32 / 255.0 * 2.0 - 1.0;
        let (x, y) = (v(q[0]), v(q[1]));
        let z = (1.0 - x * x - y * y).max(0.0).sqrt();
        let a = glam::Vec3::new(v(p[0]), v(p[1]), v(p[2])).normalize();
        let b = glam::Vec3::new(x, y, z).normalize();
        worst = worst.max(a.angle_between(b).to_degrees());
    }
    assert!(worst < 3.0, "worst normal error {worst}°");
}

#[test]
fn bc6h_keeps_hdr_values() {
    let (w, h) = (16u32, 16u32);
    let mut src = Vec::new();
    let mut expect = Vec::new();
    for y in 0..h {
        for x in 0..w {
            // A sky-like HDR ramp: brightness from 0.1 to 12, a slowly changing tint.
            let k = 0.1 * 1.35f32.powi(x as i32);
            let tint = 1.0 - y as f32 * 0.02;
            let v = [k, k * 0.8 * tint, k * 0.6];
            expect.push(v);
            src.extend(v.map(half::f16::from_f32));
            src.push(half::f16::ONE);
        }
    }
    let c = encode_hdr(w, h, &src);
    assert_eq!(c.format, BcFormat::Bc6h);
    assert_eq!(c.levels[0].len(), 4 * 4 * 16);
    let out = decode_hdr(&c);
    // Errors relative to the brightest value of the pixel's 4×4 block (BC6H quantizes each
    // block between two endpoints).
    let block_max = |i: usize, k: usize| {
        let (bx, by) = ((i as u32 % w) / 4, (i as u32 / w) / 4);
        (0..16)
            .map(|j| expect[((by * 4 + j / 4) * w + bx * 4 + j % 4) as usize][k])
            .fold(0.0f32, f32::max)
    };
    for (i, e) in expect.iter().enumerate() {
        for k in 0..3 {
            let got = out[i * 4 + k];
            let rel = (got - e[k]).abs() / block_max(i, k);
            assert!(rel < 0.06, "pixel {i} channel {k}: {got} vs {}", e[k]);
        }
    }
}

#[test]
fn mips_and_odd_level_sizes_pad_to_blocks() {
    let c = encode(gradient(32, 8, false), BcFormat::Bc1, true, true);
    assert_eq!(
        c.levels.len() as u32,
        crate::render::texture::mip_count(32, 8)
    );
    // 32×8, 16×4, 8×2, 4×1, 2×1, 1×1: each at least one block row.
    let blocks: Vec<usize> = c.levels.iter().map(|l| l.len() / 8).collect();
    assert_eq!(blocks, [16, 4, 2, 1, 1, 1]);
    let last = c.decode(c.levels.len() - 1);
    assert_eq!(last.dimensions(), (1, 1));
}

#[test]
fn format_follows_settings_alpha_and_type() {
    let opaque = gradient(8, 8, false);
    let alpha = gradient(8, 8, true);
    let p = "Windows";
    let mut imp = TextureImporter::default();
    assert_eq!(choose(&imp, p, &opaque, Use::Color), Some(BcFormat::Bc1));
    assert_eq!(choose(&imp, p, &alpha, Use::Color), Some(BcFormat::Bc3));
    // A Default texture in a normal map slot gets BC5 too.
    assert_eq!(choose(&imp, p, &opaque, Use::Normal), Some(BcFormat::Bc5));
    imp.compression = Some(TextureCompression::HighQuality);
    assert_eq!(choose(&imp, p, &opaque, Use::Color), Some(BcFormat::Bc7));
    imp.set_type(TextureType::NormalMap);
    assert_eq!(choose(&imp, p, &opaque, Use::Color), Some(BcFormat::Bc5));
    // Sprites stay uncompressed unless asked; sizes off the block grid too.
    assert_eq!(
        choose(&TextureImporter::sprite(), p, &opaque, Use::Color),
        None
    );
    assert_eq!(
        choose(
            &TextureImporter::default(),
            p,
            &gradient(6, 8, false),
            Use::Color
        ),
        None
    );
    // A platform override wins.
    let mut o = TextureImporter::default();
    o.overrides.push(PlatformOverride {
        platform: "Linux".into(),
        max_size: 512,
        compression: TextureCompression::None,
    });
    assert_eq!(choose(&o, "Linux", &opaque, Use::Color), None);
    assert_eq!(
        choose(&o, "Windows", &opaque, Use::Color),
        Some(BcFormat::Bc1)
    );
    assert_eq!(o.max_size_for("Linux"), 512);
    assert_eq!(o.max_size_for("Windows"), 2048);
}

#[test]
fn cache_file_round_trips_and_rejects_damage() {
    let c = encode(gradient(16, 16, false), BcFormat::Bc7, true, true);
    let file = c.to_file();
    assert_eq!(Compressed::from_file(&file), Some(c.clone()));
    assert_eq!(Compressed::from_file(&file[..file.len() - 3]), None);
    let mut bad = file.clone();
    bad[8] = 42; // unknown format
    assert_eq!(Compressed::from_file(&bad), None);
    assert_eq!(Compressed::from_file(b"MISTYTC1"), None);
}

#[test]
fn cache_key_changes_with_content_and_settings() {
    let imp = TextureImporter::default();
    let k = cache_key(b"png bytes", &imp, "Windows", [64, 64], Use::Color);
    assert_eq!(
        k,
        cache_key(b"png bytes", &imp, "Windows", [64, 64], Use::Color)
    );
    assert_ne!(
        k,
        cache_key(b"png bytes!", &imp, "Windows", [64, 64], Use::Color)
    );
    assert_ne!(
        k,
        cache_key(b"png bytes", &imp, "Windows", [32, 32], Use::Color)
    );
    assert_ne!(
        k,
        cache_key(b"png bytes", &imp, "Windows", [64, 64], Use::Linear)
    );
    let hq = TextureImporter {
        compression: Some(TextureCompression::HighQuality),
        ..Default::default()
    };
    assert_ne!(
        k,
        cache_key(b"png bytes", &hq, "Windows", [64, 64], Use::Color)
    );
    assert_eq!(k.len(), 32);
}

#[test]
fn bake_writes_the_cache_once_then_hits() {
    let root = std::env::temp_dir().join(format!("misty_tex_bake_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    gradient(128, 64, false)
        .save(root.join("Assets/Wall.png"))
        .unwrap();
    // Max Size 128 and two quality levels (full and half texture size): two entries.
    TextureImporter {
        max_size: 128,
        ..Default::default()
    }
    .save(&root.join("Assets/Wall.png"))
    .unwrap();
    let mut quality = crate::quality::QualitySettings::default();
    quality.levels.truncate(1);
    quality.levels[0].texture_quality = crate::quality::TextureQuality::Full;
    let mut half = quality.levels[0].clone();
    half.texture_quality = crate::quality::TextureQuality::Half;
    quality.levels.push(half);
    let first = bake(&root, "Assets/Wall.png", "Windows", &quality, &[]).unwrap();
    assert_eq!(first.entries.len(), 2);
    assert_eq!(first.encoded, 2);
    let again = bake(&root, "Assets/Wall.png", "Windows", &quality, &[]).unwrap();
    assert_eq!(again.entries, first.entries);
    assert_eq!(again.encoded, 0, "cache hits");
    let sizes: Vec<(u32, u32)> = first
        .entries
        .iter()
        .map(|e| {
            let key = e
                .trim_start_matches("Library/TextureCache/")
                .trim_end_matches(".mtc");
            let c = cached(&root, key).expect("cached");
            assert_eq!(c.format, BcFormat::Bc1);
            assert!(c.srgb);
            (c.width, c.height)
        })
        .collect();
    assert!(
        sizes.contains(&(128, 64)) && sizes.contains(&(64, 32)),
        "{sizes:?}"
    );
    // Changing the settings makes new entries; uncompressed textures have none.
    TextureImporter {
        compression: Some(TextureCompression::None),
        ..Default::default()
    }
    .save(&root.join("Assets/Wall.png"))
    .unwrap();
    assert!(
        bake(&root, "Assets/Wall.png", "Windows", &quality, &[])
            .unwrap()
            .entries
            .is_empty()
    );
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn packed_entries_read_back_from_a_pak() {
    let root = std::env::temp_dir().join(format!("misty_tex_pak_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    normal_map(64, 64)
        .save(root.join("Assets/Bumps.png"))
        .unwrap();
    let mut imp = TextureImporter::default();
    imp.set_type(TextureType::NormalMap);
    imp.save(&root.join("Assets/Bumps.png")).unwrap();
    let quality = crate::quality::QualitySettings::default();
    let baked = bake(&root, "Assets/Bumps.png", "Windows", &quality, &[]).unwrap();
    assert!(!baked.entries.is_empty());
    let mut w = crate::pak::PakWriter::new(std::io::Cursor::new(Vec::new()), true).unwrap();
    for e in &baked.entries {
        w.add(e, &std::fs::read(root.join(e)).unwrap()).unwrap();
    }
    let pak = crate::pak::Pak::from_reader(Box::new(std::io::Cursor::new(
        w.finish().unwrap().into_inner(),
    )))
    .unwrap();
    for e in &baked.entries {
        let c = Compressed::from_file(&pak.read(e).unwrap()).expect("blocks");
        assert_eq!(c.format, BcFormat::Bc5);
        assert!(!c.srgb);
        assert_eq!(c.wgpu_format(), wgpu::TextureFormat::Bc5RgUnorm);
    }
    std::fs::remove_dir_all(root).ok();
}

/// The GPU takes the levels as they are laid out (sizes and block rows of every mip).
#[test]
fn compressed_levels_upload_without_validation_errors() {
    let Some((device, queue)) = crate::render::gpu_tests::headless() else {
        eprintln!("skipped: no GPU");
        return;
    };
    if !device
        .features()
        .contains(wgpu::Features::TEXTURE_COMPRESSION_BC)
    {
        eprintln!("skipped: the adapter has no BC compression");
        return;
    }
    for format in [BcFormat::Bc1, BcFormat::Bc3, BcFormat::Bc5, BcFormat::Bc7] {
        let c = encode(gradient(64, 32, true), format, true, true);
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        let view = crate::render::texture::upload_compressed(&device, &queue, "test", &c);
        assert!(pollster::block_on(scope.pop()).is_none(), "{format:?}");
        let t = view.texture();
        assert_eq!(t.format(), c.wgpu_format());
        assert_eq!(t.mip_level_count(), c.levels.len() as u32);
        assert_eq!(
            crate::render::texture::texture_bytes(t.format(), 64, 32, t.mip_level_count(), 1),
            c.bytes() as u64
        );
    }
}
