//! Procedural, tileable ground textures for the biome presets (written as PNGs into
//! `Assets/WorldBuilder/Textures/` when the presets are created).

use image::{Rgba, RgbaImage};

use super::noise::hash01;

/// Smooth value noise on a wrapping lattice of `period` cells (0..1).
fn value(x: f32, y: f32, period: i32, seed: u32) -> f32 {
    let (x0, y0) = (x.floor(), y.floor());
    let s = |t: f32| t * t * (3.0 - 2.0 * t);
    let (u, v) = (s(x - x0), s(y - y0));
    let at = |dx: i32, dy: i32| {
        hash01(
            (x0 as i32 + dx).rem_euclid(period),
            (y0 as i32 + dy).rem_euclid(period),
            seed,
        )
    };
    let a = at(0, 0) + (at(1, 0) - at(0, 0)) * u;
    let b = at(0, 1) + (at(1, 1) - at(0, 1)) * u;
    a + (b - a) * v
}

/// Tileable fractal noise at pixel (px, py) of a `size` image (0..1).
fn fbm(px: u32, py: u32, size: u32, period: i32, octaves: u32, seed: u32) -> f32 {
    let (mut sum, mut amp, mut norm) = (0.0, 1.0, 0.0);
    for o in 0..octaves {
        let p = period << o;
        let k = p as f32 / size as f32;
        sum += amp * value(px as f32 * k, py as f32 * k, p, seed.wrapping_add(o));
        norm += amp;
        amp *= 0.5;
    }
    sum / norm
}

fn shade(base: [f32; 3], k: f32) -> Rgba<u8> {
    let c = base.map(|c| (c * k).clamp(0.0, 255.0).round() as u8);
    Rgba([c[0], c[1], c[2], 255])
}

/// The texture for a preset stem (see [`super::biome::TEXTURES`]); `None` for unknown names.
pub fn generate(stem: &str, size: u32) -> Option<RgbaImage> {
    let img = |f: &dyn Fn(u32, u32) -> Rgba<u8>| RgbaImage::from_fn(size, size, f);
    let grain = |x: u32, y: u32, seed: u32| hash01(x as i32, y as i32, seed) - 0.5;
    Some(match stem {
        "Grass" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 5, 11);
            let dry = fbm(x, y, size, 2, 2, 29);
            shade(
                [72.0 + 38.0 * dry, 112.0 + 12.0 * dry, 40.0],
                0.75 + 0.45 * n + 0.12 * grain(x, y, 3),
            )
        }),
        "Dirt" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 5, 41);
            let pebbles = (fbm(x, y, size, 16, 2, 43) - 0.6).max(0.0) * 2.0;
            shade(
                [104.0, 82.0, 58.0],
                0.7 + 0.45 * n + 0.3 * pebbles + 0.1 * grain(x, y, 5),
            )
        }),
        "Rock" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 5, 51);
            let r = 1.0 - (fbm(x, y, size, 8, 3, 77) * 2.0 - 1.0).abs();
            let tint = fbm(x, y, size, 2, 2, 91);
            shade(
                [116.0 + 14.0 * tint, 112.0 + 6.0 * tint, 106.0],
                0.5 + 0.45 * n + 0.35 * r * r,
            )
        }),
        "Sand" => img(&|x, y| {
            let n = fbm(x, y, size, 8, 4, 131);
            let t = (x as f32 + 6.0 * fbm(x, y, size, 2, 2, 17)) / size as f32;
            let ripple = (t * std::f32::consts::TAU * 6.0).sin();
            shade(
                [200.0, 182.0, 138.0],
                0.84 + 0.12 * n + 0.05 * ripple + 0.08 * grain(x, y, 7),
            )
        }),
        "Snow" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 4, 151);
            let sparkle = (grain(x, y, 9) + 0.5).powi(12) * 0.25;
            shade([228.0, 234.0, 244.0], 0.9 + 0.1 * n + sparkle)
        }),
        "DesertSand" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 4, 171);
            let t = (y as f32 + 10.0 * fbm(x, y, size, 2, 2, 173)) / size as f32;
            let ripple = (t * std::f32::consts::TAU * 8.0).sin();
            shade(
                [212.0, 164.0, 104.0],
                0.82 + 0.14 * n + 0.06 * ripple + 0.06 * grain(x, y, 11),
            )
        }),
        "RedRock" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 5, 191);
            // Horizontal strata.
            let band = ((y as f32 / size as f32 * 10.0 + 2.0 * fbm(x, y, size, 2, 2, 193))
                * std::f32::consts::TAU)
                .sin();
            shade([150.0, 82.0, 56.0], 0.62 + 0.4 * n + 0.12 * band)
        }),
        "Basalt" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 5, 211);
            // Dark rock with small vesicles and a faint sheen of cracks.
            let pits = (fbm(x, y, size, 16, 2, 213) - 0.62).max(0.0) * 3.0;
            let crack = 1.0 - (fbm(x, y, size, 6, 3, 217) * 2.0 - 1.0).abs();
            shade(
                [62.0, 60.0, 62.0],
                0.65 + 0.4 * n - 0.35 * pits + 0.25 * crack.powi(6) + 0.08 * grain(x, y, 13),
            )
        }),
        "Ash" => img(&|x, y| {
            let n = fbm(x, y, size, 4, 4, 231);
            let tint = fbm(x, y, size, 2, 2, 233);
            shade(
                [92.0 + 10.0 * tint, 86.0 + 4.0 * tint, 82.0],
                0.75 + 0.3 * n + 0.14 * grain(x, y, 17),
            )
        }),
        _ => return None,
    })
}
