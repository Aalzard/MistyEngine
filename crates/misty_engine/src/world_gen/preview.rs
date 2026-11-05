//! Preview images of a generation (the World Builder's Preview card and template thumbnails):
//! grayscale height, shaded relief (hillshade, slope and height tint, water) or the painted
//! biome textures, all top-down with +Z up the image.

use super::classify::WaterMap;
use crate::terrain::TerrainData;

/// What the preview image shows.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum PreviewMode {
    /// Grayscale height (stretched to the map's range).
    Height,
    /// Hillshade with slope and height tint, water in blue.
    #[default]
    Relief,
    /// The painted splat map in each layer's texture colour, hillshaded.
    Biomes,
}

impl PreviewMode {
    pub const ALL: [Self; 3] = [Self::Height, Self::Relief, Self::Biomes];
    pub fn label(self) -> &'static str {
        match self {
            Self::Height => "Height",
            Self::Relief => "Relief",
            Self::Biomes => "Biomes",
        }
    }
}

/// An sRGB RGBA8 image, `size²` pixels row by row (row 0 = the far, +Z edge).
#[derive(Clone, Debug, PartialEq)]
pub struct PreviewImage {
    pub size: usize,
    pub rgba: Vec<[u8; 4]>,
}

/// Light from the north-west, 45° up (unit vector, y up).
const SUN: [f32; 3] = [-0.5, std::f32::consts::FRAC_1_SQRT_2, 0.5];

/// Render `d` (and its wet texels) as a `size²` image. `layer_color` gives the average sRGB
/// colour (0..1) of a terrain layer for the Biomes mode.
pub fn render(
    d: &TerrainData,
    wet: Option<&WaterMap>,
    mode: PreviewMode,
    size: usize,
    layer_color: &dyn Fn(usize) -> [f32; 3],
) -> PreviewImage {
    let size = size.max(1);
    let res = d.heightmap_resolution;
    let (lo, hi) = d.height_range();
    let span = (hi - lo).max(1e-4);
    let colors: Vec<[f32; 3]> = (0..d.layers.len()).map(layer_color).collect();
    let step = d.spacing();
    let mut rgba = vec![[0u8; 4]; size * size];
    super::par_rows(&mut rgba, size, |row, out| {
        // Image rows go from +Z (top) to 0.
        let v = 1.0 - (row as f32 + 0.5) / size as f32;
        for (col, px) in out.iter_mut().enumerate() {
            let u = (col as f32 + 0.5) / size as f32;
            let (x, z) = (u * d.size.x, v * d.size.z);
            let h01 = sample01(d, res, u, v);
            let c = match mode {
                PreviewMode::Height => {
                    let g = ((h01 - lo) / span).clamp(0.0, 1.0);
                    [g, g, g]
                }
                PreviewMode::Relief | PreviewMode::Biomes => {
                    let h = |dx: f32, dz: f32| d.sample_height(x + dx, z + dz);
                    let gx = (h(step.x, 0.0) - h(-step.x, 0.0)) / (2.0 * step.x);
                    let gz = (h(0.0, step.y) - h(0.0, -step.y)) / (2.0 * step.y);
                    let n = normalize([-gx, 1.0, -gz]);
                    let lambert = (n[0] * SUN[0] + n[1] * SUN[1] + n[2] * SUN[2]).max(0.0);
                    let shade = 0.35 + 0.75 * lambert;
                    let slope = (gx * gx + gz * gz).sqrt().atan().to_degrees();
                    let ground = if mode == PreviewMode::Relief {
                        relief_tint(((h01 - lo) / span).clamp(0.0, 1.0), slope)
                    } else {
                        splat_color(d, &colors, u, v)
                    };
                    let mut c = ground.map(|g| g * shade);
                    if let Some(level) = wet.and_then(|w| water_level(w, u, v)) {
                        let depth = (level - h01 * d.size.y).max(0.0);
                        let k = (0.55 + depth / 20.0).min(0.9);
                        let water = [0.12, 0.32, 0.55];
                        c = [0, 1, 2].map(|i| c[i] * (1.0 - k) + water[i] * k);
                    }
                    c
                }
            };
            *px = [to_u8(c[0]), to_u8(c[1]), to_u8(c[2]), 255];
        }
    });
    PreviewImage { size, rgba }
}

fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0).round() as u8
}

fn normalize(v: [f32; 3]) -> [f32; 3] {
    let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-6);
    v.map(|c| c / l)
}

/// Bilinear normalised height at (u, v) in 0..1.
fn sample01(d: &TerrainData, res: usize, u: f32, v: f32) -> f32 {
    let max = (res - 1) as f32;
    let (fx, fz) = ((u * max).clamp(0.0, max), (v * max).clamp(0.0, max));
    let (x0, z0) = ((fx as usize).min(res - 2), (fz as usize).min(res - 2));
    let (a, b) = (fx - x0 as f32, fz - z0 as f32);
    let at = |x: usize, z: usize| d.heights[z * res + x];
    let top = at(x0, z0) + (at(x0 + 1, z0) - at(x0, z0)) * a;
    let bottom = at(x0, z0 + 1) + (at(x0 + 1, z0 + 1) - at(x0, z0 + 1)) * a;
    top + (bottom - top) * b
}

/// Hypsometric tint (lowland green → tan → grey → snow) darkened towards rock on steep ground.
fn relief_tint(t: f32, slope: f32) -> [f32; 3] {
    const STOPS: [(f32, [f32; 3]); 5] = [
        (0.0, [0.36, 0.52, 0.30]),
        (0.35, [0.55, 0.62, 0.36]),
        (0.6, [0.70, 0.62, 0.45]),
        (0.82, [0.62, 0.58, 0.55]),
        (1.0, [0.95, 0.95, 0.97]),
    ];
    let mut c = STOPS[0].1;
    for w in STOPS.windows(2) {
        let ((a, ca), (b, cb)) = (w[0], w[1]);
        if t >= a {
            let k = ((t - a) / (b - a)).clamp(0.0, 1.0);
            c = [0, 1, 2].map(|i| ca[i] + (cb[i] - ca[i]) * k);
        }
    }
    let rock = super::noise::smoothstep(25.0, 45.0, slope);
    let grey = [0.5, 0.47, 0.44];
    [0, 1, 2].map(|i| c[i] * (1.0 - rock) + grey[i] * rock)
}

/// Splat-weighted layer colour at (u, v).
fn splat_color(d: &TerrainData, colors: &[[f32; 3]], u: f32, v: f32) -> [f32; 3] {
    let n = d.alphamap_resolution;
    if colors.is_empty() || d.alphamap.len() < n * n {
        return [0.5, 0.5, 0.5];
    }
    let tx = ((u * n as f32) as usize).min(n - 1);
    let tz = ((v * n as f32) as usize).min(n - 1);
    let w = d.alphamap[tz * n + tx];
    let mut c = [0.0f32; 3];
    let mut sum = 0.0;
    for (l, col) in colors.iter().enumerate().take(4) {
        let k = w[l] as f32 / 255.0;
        sum += k;
        for i in 0..3 {
            c[i] += col[i] * k;
        }
    }
    if sum <= 1e-4 {
        return colors[0];
    }
    c.map(|x| x / sum)
}

/// Water surface over (u, v), if wet.
fn water_level(w: &WaterMap, u: f32, v: f32) -> Option<f32> {
    let n = w.res;
    if n == 0 {
        return None;
    }
    let tx = ((u * n as f32) as usize).min(n - 1);
    let tz = ((v * n as f32) as usize).min(n - 1);
    w.level.get(tz * n + tx).copied().flatten()
}

/// Average sRGB colour (0..1) of an RGBA image (Biomes mode layer colours).
pub fn average_color(img: &image::RgbaImage) -> [f32; 3] {
    let mut sum = [0u64; 3];
    let mut n = 0u64;
    for p in img.pixels() {
        for (s, c) in sum.iter_mut().zip(p.0) {
            *s += c as u64;
        }
        n += 1;
    }
    if n == 0 {
        return [0.5; 3];
    }
    sum.map(|s| s as f32 / n as f32 / 255.0)
}

/// Colour of a layer's diffuse texture: a generated preset texture by its stem, else `load`
/// (reads a project texture), else mid grey.
pub fn layer_color(
    layer: &crate::terrain::TerrainLayer,
    load: &dyn Fn(&str) -> Option<image::RgbaImage>,
) -> [f32; 3] {
    let Some(path) = layer.diffuse.as_deref() else {
        return [0.5; 3];
    };
    let stem = std::path::Path::new(path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    if path == super::biome::texture_path(&stem)
        && let Some(img) = super::textures::generate(&stem, 32)
    {
        return average_color(&img);
    }
    load(path).map_or([0.5; 3], |img| average_color(&img))
}

/// Downsample a `res²` heightmap to `n²` by bilinear lookup (correlation checks).
pub fn resample(heights: &[f32], res: usize, n: usize) -> Vec<f32> {
    let max = (res - 1) as f32;
    let mut out = Vec::with_capacity(n * n);
    for j in 0..n {
        for i in 0..n {
            let (fx, fz) = (
                i as f32 / (n - 1).max(1) as f32 * max,
                j as f32 / (n - 1).max(1) as f32 * max,
            );
            let (x0, z0) = ((fx as usize).min(res - 2), (fz as usize).min(res - 2));
            let (a, b) = (fx - x0 as f32, fz - z0 as f32);
            let at = |x: usize, z: usize| heights[z * res + x];
            let top = at(x0, z0) + (at(x0 + 1, z0) - at(x0, z0)) * a;
            let bottom = at(x0, z0 + 1) + (at(x0 + 1, z0 + 1) - at(x0, z0 + 1)) * a;
            out.push(top + (bottom - top) * b);
        }
    }
    out
}

/// Pearson correlation of two equally long series.
pub fn correlation(a: &[f32], b: &[f32]) -> f32 {
    let n = a.len().min(b.len()) as f64;
    if n < 2.0 {
        return 0.0;
    }
    let ma = a.iter().map(|v| *v as f64).sum::<f64>() / n;
    let mb = b.iter().map(|v| *v as f64).sum::<f64>() / n;
    let (mut cov, mut va, mut vb) = (0.0, 0.0, 0.0);
    for (x, y) in a.iter().zip(b) {
        let (dx, dy) = (*x as f64 - ma, *y as f64 - mb);
        cov += dx * dy;
        va += dx * dx;
        vb += dy * dy;
    }
    (cov / (va.sqrt() * vb.sqrt()).max(1e-12)) as f32
}
