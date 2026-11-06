//! Impostors: a tree's LOD 0 mesh rendered on the CPU from [`VIEWS`] directions around it
//! (level, orthographic) into one atlas row of cells, so far trees can be drawn as a single
//! camera-facing quad that shows the right side of the tree.
//!
//! Per cell: the albedo (texture × the same per-cluster variation as the mesh, sRGB) with the
//! coverage in alpha, and the shading normal in the cell's view frame (x = right, y = up,
//! z = towards the viewer) with the ambient occlusion in alpha. Lighting happens at draw time,
//! so impostors follow the sun like the meshes. Rendered at 2× and box filtered, so edges are
//! soft; transparent texels carry the colour of their neighbours (no dark fringes in mips).

use glam::{Vec2, Vec3};
use image::RgbaImage;

use super::TreeAsset;
use super::generate::TreeMesh;

/// Views around the tree (every 45°).
pub const VIEWS: u32 = 8;
/// Cell side in texels.
pub const CELL: u32 = 256;

/// Texture data the bake samples (built by the tree store from the asset's textures).
#[derive(Clone, Debug, Default)]
pub struct BakeTextures {
    /// The leaf atlas, small (its alpha is the leaves' outline).
    pub leaf: Option<RgbaImage>,
    /// Mean linear colour of the bark texture (None = mid brown).
    pub bark_mean: Option<[f32; 3]>,
}

#[derive(Clone, Debug)]
pub struct Impostor {
    /// `VIEWS` cells side by side (CELL × CELL each): sRGB albedo, alpha = coverage.
    pub albedo: RgbaImage,
    /// View-frame normal (rgb = n × 0.5 + 0.5), alpha = ambient occlusion.
    pub normal: RgbaImage,
    /// Quad size in object space (width across, height) and the height of its bottom edge.
    pub width: f32,
    pub height: f32,
    pub bottom: f32,
}

fn srgb(c: f32) -> f32 {
    let c = c.clamp(0.0, 1.0);
    if c <= 0.003_130_8 {
        c * 12.92
    } else {
        1.055 * c.powf(1.0 / 2.4) - 0.055
    }
}

/// sRGB to linear (one channel).
pub fn lin(c: f32) -> f32 {
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

/// Per-cluster colour variation (same as `tree_leaf_variation` in tree.wgsl): warmer and
/// lighter or cooler and darker clusters.
pub fn leaf_variation(color: Vec3, v: f32, amount: f32) -> Vec3 {
    let k = (v - 0.5) * amount;
    color * Vec3::new(1.0 + 0.32 * k, 1.0 + 0.14 * k, 1.0 - 0.2 * k) * (1.0 + 0.3 * k)
}

/// Bilinear sample (linear colour, alpha) of an sRGB image at uv (clamped).
fn sample(img: &RgbaImage, uv: Vec2) -> (Vec3, f32) {
    let (w, h) = img.dimensions();
    let x = (uv.x * w as f32 - 0.5).clamp(0.0, w as f32 - 1.0);
    let y = (uv.y * h as f32 - 0.5).clamp(0.0, h as f32 - 1.0);
    let (x0, y0) = (x.floor() as u32, y.floor() as u32);
    let (x1, y1) = ((x0 + 1).min(w - 1), (y0 + 1).min(h - 1));
    let (fx, fy) = (x - x0 as f32, y - y0 as f32);
    let px = |x, y| {
        let p = img.get_pixel(x, y);
        (
            Vec3::new(
                lin(p[0] as f32 / 255.0),
                lin(p[1] as f32 / 255.0),
                lin(p[2] as f32 / 255.0),
            ),
            p[3] as f32 / 255.0,
        )
    };
    let (a, b, c, d) = (px(x0, y0), px(x1, y0), px(x0, y1), px(x1, y1));
    let top = (a.0.lerp(b.0, fx), a.1 + (b.1 - a.1) * fx);
    let bottom = (c.0.lerp(d.0, fx), c.1 + (d.1 - c.1) * fx);
    (top.0.lerp(bottom.0, fy), top.1 + (bottom.1 - top.1) * fy)
}

/// One supersampled view being rasterised.
struct Canvas {
    side: usize,
    depth: Vec<f32>,
    color: Vec<Vec3>,
    normal: Vec<Vec3>,
    ao: Vec<f32>,
}

/// Bake the impostor of `mesh` (LOD 0) within the bounds.
pub fn bake(t: &TreeAsset, mesh: &TreeMesh, min: Vec3, max: Vec3, tex: &BakeTextures) -> Impostor {
    let half = min
        .x
        .abs()
        .max(max.x.abs())
        .max(min.z.abs())
        .max(max.z.abs())
        * 1.02
        + 0.05;
    let (bottom, top) = (min.y.min(0.0), max.y + 0.05);
    let height = top - bottom;
    let ss = 2usize;
    let side = CELL as usize * ss;
    let mut albedo = RgbaImage::new(CELL * VIEWS, CELL);
    let mut normal = RgbaImage::new(CELL * VIEWS, CELL);
    let leaf_tint = Vec3::from(t.leaf_color.map(lin));
    let bark =
        Vec3::from(tex.bark_mean.unwrap_or([0.09, 0.07, 0.05])) * Vec3::from(t.bark_color.map(lin));
    for view in 0..VIEWS {
        let a = view as f32 / VIEWS as f32 * std::f32::consts::TAU;
        // Camera towards +c, looking back at the tree.
        let c = Vec3::new(a.cos(), 0.0, a.sin());
        let right = (-c).cross(Vec3::Y).normalize();
        let mut cv = Canvas {
            side,
            depth: vec![f32::MIN; side * side],
            color: vec![Vec3::ZERO; side * side],
            normal: vec![Vec3::Z; side * side],
            ao: vec![1.0; side * side],
        };
        let project = |p: Vec3| {
            Vec3::new(
                (p.dot(right) / half * 0.5 + 0.5) * side as f32,
                (1.0 - (p.y - bottom) / height) * side as f32,
                p.dot(c),
            )
        };
        let to_view = |n: Vec3| Vec3::new(n.dot(right), n.y, n.dot(c));
        for (range, leaf) in [(mesh.bark.clone(), false), (mesh.leaves.clone(), true)] {
            let tris = mesh.indices[range.start as usize..range.end as usize]
                .as_chunks::<3>()
                .0;
            for tri in tris {
                let v = tri.map(|i| &mesh.vertices[i as usize]);
                let s = v.map(|v| project(Vec3::from(v.position)));
                raster(&mut cv, s, |b| {
                    let uv = Vec2::from(v[0].uv) * b.x
                        + Vec2::from(v[1].uv) * b.y
                        + Vec2::from(v[2].uv) * b.z;
                    let n = Vec3::from(v[0].normal) * b.x
                        + Vec3::from(v[1].normal) * b.y
                        + Vec3::from(v[2].normal) * b.z;
                    let ao = (v[0].extra[0] as f32 * b.x
                        + v[1].extra[0] as f32 * b.y
                        + v[2].extra[0] as f32 * b.z)
                        / 255.0;
                    if leaf {
                        let (col, alpha) = match &tex.leaf {
                            Some(img) => sample(img, uv),
                            None => (Vec3::new(0.1, 0.16, 0.04), 1.0),
                        };
                        if alpha < 0.5 {
                            return None;
                        }
                        let variation = v[0].extra[1] as f32 / 255.0;
                        let col = leaf_variation(col * leaf_tint, variation, t.leaf_variation);
                        Some((col, to_view(n.normalize_or(Vec3::Y)), ao))
                    } else {
                        Some((bark, to_view(n.normalize_or(Vec3::Y)), ao))
                    }
                });
            }
        }
        resolve(&cv, ss, view, &mut albedo, &mut normal);
    }
    dilate(&mut albedo, &mut normal);
    Impostor {
        albedo,
        normal,
        width: half * 2.0,
        height,
        bottom,
    }
}

/// Fill the covered pixels of one triangle (screen xy, depth z: larger = nearer). `shade`
/// gets the barycentrics and returns the surface, or None for a hole (alpha test).
fn raster(cv: &mut Canvas, s: [Vec3; 3], mut shade: impl FnMut(Vec3) -> Option<(Vec3, Vec3, f32)>) {
    let area = (s[1].x - s[0].x) * (s[2].y - s[0].y) - (s[2].x - s[0].x) * (s[1].y - s[0].y);
    if area.abs() < 1e-6 {
        return;
    }
    let lo_x = s
        .iter()
        .map(|p| p.x)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let hi_x = (s.iter().map(|p| p.x).fold(f32::MIN, f32::max).ceil() as usize).min(cv.side);
    let lo_y = s
        .iter()
        .map(|p| p.y)
        .fold(f32::MAX, f32::min)
        .floor()
        .max(0.0) as usize;
    let hi_y = (s.iter().map(|p| p.y).fold(f32::MIN, f32::max).ceil() as usize).min(cv.side);
    for y in lo_y..hi_y {
        for x in lo_x..hi_x {
            let (px, py) = (x as f32 + 0.5, y as f32 + 0.5);
            let w0 = ((s[1].x - px) * (s[2].y - py) - (s[2].x - px) * (s[1].y - py)) / area;
            let w1 = ((s[2].x - px) * (s[0].y - py) - (s[0].x - px) * (s[2].y - py)) / area;
            let w2 = 1.0 - w0 - w1;
            if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                continue;
            }
            let z = s[0].z * w0 + s[1].z * w1 + s[2].z * w2;
            let i = y * cv.side + x;
            if z <= cv.depth[i] {
                continue;
            }
            if let Some((color, n, ao)) = shade(Vec3::new(w0, w1, w2)) {
                cv.depth[i] = z;
                cv.color[i] = color;
                cv.normal[i] = n;
                cv.ao[i] = ao;
            }
        }
    }
}

/// Box-filter the supersampled canvas into cell `view` of the atlas.
fn resolve(cv: &Canvas, ss: usize, view: u32, albedo: &mut RgbaImage, normal: &mut RgbaImage) {
    let n = (ss * ss) as f32;
    for y in 0..CELL as usize {
        for x in 0..CELL as usize {
            let (mut col, mut nor, mut ao, mut cover) = (Vec3::ZERO, Vec3::ZERO, 0.0, 0.0);
            for sy in 0..ss {
                for sx in 0..ss {
                    let i = (y * ss + sy) * cv.side + x * ss + sx;
                    if cv.depth[i] > f32::MIN {
                        col += cv.color[i];
                        nor += cv.normal[i];
                        ao += cv.ao[i];
                        cover += 1.0;
                    }
                }
            }
            let px = view * CELL + x as u32;
            if cover > 0.0 {
                let c = col / cover;
                let nn = (nor / cover).normalize_or(Vec3::Z);
                let enc = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                albedo.put_pixel(
                    px,
                    y as u32,
                    image::Rgba([
                        enc(srgb(c.x)),
                        enc(srgb(c.y)),
                        enc(srgb(c.z)),
                        enc(cover / n),
                    ]),
                );
                normal.put_pixel(
                    px,
                    y as u32,
                    image::Rgba([
                        enc(nn.x * 0.5 + 0.5),
                        enc(nn.y * 0.5 + 0.5),
                        enc(nn.z * 0.5 + 0.5),
                        enc(ao / cover),
                    ]),
                );
            }
        }
    }
}

/// Spread the colours of covered texels into the empty ones (alpha stays 0), so filtering and
/// mips never pull in black: a few rings of neighbours around the outline, the cell's mean
/// colour further out. Stays within each cell.
fn dilate(albedo: &mut RgbaImage, normal: &mut RgbaImage) {
    let (w, h) = albedo.dimensions();
    let filled: Vec<bool> = albedo.pixels().map(|p| p[3] > 0).collect();
    for cell in 0..w / CELL {
        let (mut col, mut nor, mut count) = ([0u64; 3], [0u64; 4], 0u64);
        for y in 0..h {
            for x in cell * CELL..(cell + 1) * CELL {
                if filled[(y * w + x) as usize] {
                    let (a, b) = (albedo.get_pixel(x, y), normal.get_pixel(x, y));
                    for k in 0..3 {
                        col[k] += a[k] as u64;
                    }
                    for k in 0..4 {
                        nor[k] += b[k] as u64;
                    }
                    count += 1;
                }
            }
        }
        let count = count.max(1);
        let a = col.map(|c| (c / count) as u8);
        let b = nor.map(|c| (c / count) as u8);
        for y in 0..h {
            for x in cell * CELL..(cell + 1) * CELL {
                if !filled[(y * w + x) as usize] {
                    albedo.put_pixel(x, y, image::Rgba([a[0], a[1], a[2], 0]));
                    normal.put_pixel(x, y, image::Rgba([b[0], b[1], b[2], 255]));
                }
            }
        }
    }
    let mut filled = filled;
    for _ in 0..6 {
        let mut changed = false;
        let mut next = filled.clone();
        for y in 0..h {
            for x in 0..w {
                let i = (y * w + x) as usize;
                if filled[i] {
                    continue;
                }
                let cell = x / CELL;
                let (mut col, mut nor, mut count) = ([0u32; 3], [0u32; 4], 0u32);
                for (dx, dy) in [(-1i32, 0i32), (1, 0), (0, -1), (0, 1)] {
                    let (nx, ny) = (x as i32 + dx, y as i32 + dy);
                    if nx < 0
                        || ny < 0
                        || nx >= w as i32
                        || ny >= h as i32
                        || nx as u32 / CELL != cell
                    {
                        continue;
                    }
                    let j = (ny as u32 * w + nx as u32) as usize;
                    if !filled[j] {
                        continue;
                    }
                    let a = albedo.get_pixel(nx as u32, ny as u32);
                    let b = normal.get_pixel(nx as u32, ny as u32);
                    for k in 0..3 {
                        col[k] += a[k] as u32;
                    }
                    for k in 0..4 {
                        nor[k] += b[k] as u32;
                    }
                    count += 1;
                }
                if count > 0 {
                    let a = col.map(|c| (c / count) as u8);
                    albedo.put_pixel(x, y, image::Rgba([a[0], a[1], a[2], 0]));
                    let b = nor.map(|c| (c / count) as u8);
                    normal.put_pixel(x, y, image::Rgba([b[0], b[1], b[2], b[3]]));
                    next[i] = true;
                    changed = true;
                }
            }
        }
        filled = next;
        if !changed {
            break;
        }
    }
}
