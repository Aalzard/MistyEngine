//! MeadowDemo's texture memory with and without compression, and how close the blocks stay
//! to the images. Ignored (it decodes and encodes every texture of the scene):
//! `cargo test -p misty_engine --release meadow_texture_memory -- --ignored --nocapture`.

use std::collections::HashMap;
use std::path::PathBuf;

use super::*;

/// Peak signal-to-noise ratio (dB) of the colour channels.
fn psnr(a: &RgbaImage, b: &RgbaImage, channels: usize) -> f64 {
    let mut sum = 0.0f64;
    let mut n = 0usize;
    for (p, q) in a.pixels().zip(b.pixels()) {
        for c in 0..channels {
            let d = p[c] as f64 - q[c] as f64;
            sum += d * d;
            n += 1;
        }
    }
    let mse = sum / n.max(1) as f64;
    if mse == 0.0 {
        return 99.0;
    }
    10.0 * (255.0f64 * 255.0 / mse).log10()
}

#[test]
#[ignore]
fn meadow_texture_memory() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let deps = crate::asset_deps::collect(&root, &["Assets/Scenes/MeadowDemo.scene".to_owned()]);
    let mut uses: HashMap<String, Use> = HashMap::new();
    for rel in deps.files.iter().filter(|r| r.ends_with(".mat")) {
        let Ok(m) = crate::assets::Material::load(&root.join(rel)) else {
            continue;
        };
        for (map, usage) in [
            (&m.albedo_map, Use::Color),
            (&m.normal_map, Use::Normal),
            (&m.metallic_map, Use::Linear),
            (&m.occlusion_map, Use::Linear),
            (&m.emission_map, Use::Color),
        ] {
            if let Some(p) = map {
                uses.entry(p.clone()).or_insert(usage);
            }
        }
    }
    let platform = "Windows";
    let quality = crate::quality::QualitySettings::default();
    let level = &quality.levels[quality.default_level];
    // (category, rgba bytes, compressed bytes)
    let mut totals: HashMap<&str, (u64, u64, usize)> = HashMap::new();
    println!("texture | size | use | format | RGBA8 MB | BC MB | PSNR dB");
    for rel in deps.files.iter().filter(|r| crate::assets::is_texture(r)) {
        let abs = root.join(rel);
        let imp = TextureImporter::load(&abs);
        let Ok(full) = crate::render::texture::load_image(&abs) else {
            continue;
        };
        let img =
            crate::render::texture::fit(full, level.texture_size_limit(imp.max_size_for(platform)));
        let (w, h) = img.dimensions();
        let mips = if imp.generate_mip_maps {
            crate::render::texture::mip_count(w, h)
        } else {
            1
        };
        let rgba =
            crate::render::texture::texture_bytes(wgpu::TextureFormat::Rgba8Unorm, w, h, mips, 1);
        // Material textures go through the compressed upload; the terrain layers, trees and
        // grass detail atlases pack these files into textures of their own, compressed through
        // `generated` (their GPU memory: `render::made_textures_tests::made_textures_memory`).
        let (usage, category) = match uses.get(rel) {
            Some(u) => (*u, "material (compressed)"),
            None => {
                let lower = rel.to_ascii_lowercase();
                let cat = if lower.contains("tree") {
                    "trees (source files)"
                } else if lower.contains("flower") || lower.contains("grass") {
                    "grass and flowers (source files)"
                } else {
                    "terrain and other (source files)"
                };
                (Use::of(&imp), cat)
            }
        };
        let (format, bytes, quality_db) = match choose(&imp, platform, &img, usage) {
            Some(format) => {
                let c = encode(img.clone(), format, usage.srgb(&imp), imp.generate_mip_maps);
                let channels = if format == BcFormat::Bc5 { 2 } else { 3 };
                let db = psnr(&img, &c.decode(0), channels);
                (format.label(), c.bytes() as u64, db)
            }
            None => ("RGBA8", rgba, 99.0),
        };
        println!(
            "{rel} | {w}×{h} | {usage:?} | {format} | {:.2} | {:.2} | {quality_db:.1}",
            rgba as f64 / 1e6,
            bytes as f64 / 1e6
        );
        let t = totals.entry(category).or_default();
        t.0 += rgba;
        t.1 += bytes;
        t.2 += 1;
    }
    let mut cats: Vec<_> = totals.into_iter().collect();
    cats.sort();
    for (cat, (rgba, bc, n)) in cats {
        println!(
            "TOTAL {cat}: {n} textures, {:.1} MB → {:.1} MB ({:.0}% saved)",
            rgba as f64 / 1e6,
            bc as f64 / 1e6,
            100.0 * (1.0 - bc as f64 / rgba.max(1) as f64)
        );
    }
}
