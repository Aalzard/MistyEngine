//! CPU cubemaps: face/texel ↔ direction (the GPU's cube convention, faces +X −X +Y −Y +Z −Z),
//! bilinear lookups and box downsampling.

use glam::{Mat3, Quat, Vec3};

/// One cubemap level: `size`² linear RGB texels per face, rows top to bottom.
#[derive(Clone, Debug, PartialEq)]
pub struct Cube {
    pub size: u32,
    pub faces: [Vec<Vec3>; 6],
}

/// Unit direction through face coordinates `u`, `v` in -1..1 (v grows downwards).
pub fn direction(face: usize, u: f32, v: f32) -> Vec3 {
    match face {
        0 => Vec3::new(1.0, -v, -u),
        1 => Vec3::new(-1.0, -v, u),
        2 => Vec3::new(u, 1.0, v),
        3 => Vec3::new(u, -1.0, -v),
        4 => Vec3::new(u, -v, 1.0),
        _ => Vec3::new(-u, -v, -1.0),
    }
    .normalize()
}

/// Face and face coordinates (-1..1) of a direction.
pub fn face_uv(d: Vec3) -> (usize, f32, f32) {
    let a = d.abs();
    if a.x >= a.y && a.x >= a.z {
        let m = a.x.max(1e-20);
        if d.x > 0.0 {
            (0, -d.z / m, -d.y / m)
        } else {
            (1, d.z / m, -d.y / m)
        }
    } else if a.y >= a.z {
        let m = a.y.max(1e-20);
        if d.y > 0.0 {
            (2, d.x / m, d.z / m)
        } else {
            (3, d.x / m, -d.z / m)
        }
    } else {
        let m = a.z.max(1e-20);
        if d.z > 0.0 {
            (4, d.x / m, -d.y / m)
        } else {
            (5, -d.x / m, -d.y / m)
        }
    }
}

/// Camera rotation that renders face `face` (forward through the face centre; the image's
/// right and down match the face's u and v).
pub fn face_rotation(face: usize) -> Quat {
    let (forward, up) = match face {
        0 => (Vec3::X, Vec3::Y),
        1 => (Vec3::NEG_X, Vec3::Y),
        2 => (Vec3::Y, Vec3::NEG_Z),
        3 => (Vec3::NEG_Y, Vec3::Z),
        4 => (Vec3::Z, Vec3::Y),
        _ => (Vec3::NEG_Z, Vec3::Y),
    };
    // Left-handed camera: right = up × forward.
    Quat::from_mat3(&Mat3::from_cols(up.cross(forward), up, forward))
}

/// Centre of texel (x, y) in face coordinates.
pub fn texel_uv(size: u32, x: u32, y: u32) -> (f32, f32) {
    let s = size as f32;
    (
        (x as f32 + 0.5) / s * 2.0 - 1.0,
        (y as f32 + 0.5) / s * 2.0 - 1.0,
    )
}

/// Solid angle of a texel centred at (u, v) on a face of `size` texels.
pub fn texel_solid_angle(size: u32, u: f32, v: f32) -> f32 {
    let d = 2.0 / size as f32;
    d * d / (1.0 + u * u + v * v).powf(1.5)
}

impl Cube {
    pub fn new(size: u32) -> Self {
        let n = (size * size) as usize;
        Self {
            size,
            faces: std::array::from_fn(|_| vec![Vec3::ZERO; n]),
        }
    }

    /// Cube filled by `f(direction)` at each texel centre.
    pub fn from_fn(size: u32, f: impl Fn(Vec3) -> Vec3) -> Self {
        let mut c = Self::new(size);
        for (face, texels) in c.faces.iter_mut().enumerate() {
            for y in 0..size {
                for x in 0..size {
                    let (u, v) = texel_uv(size, x, y);
                    texels[(y * size + x) as usize] = f(direction(face, u, v));
                }
            }
        }
        c
    }

    /// Bilinear lookup in direction `d` (clamped at face edges).
    pub fn sample(&self, d: Vec3) -> Vec3 {
        let (face, u, v) = face_uv(d);
        let s = self.size as f32;
        let fx = ((u + 1.0) * 0.5 * s - 0.5).clamp(0.0, s - 1.0);
        let fy = ((v + 1.0) * 0.5 * s - 0.5).clamp(0.0, s - 1.0);
        let (x0, y0) = (fx as u32, fy as u32);
        let (x1, y1) = ((x0 + 1).min(self.size - 1), (y0 + 1).min(self.size - 1));
        let (tx, ty) = (fx - x0 as f32, fy - y0 as f32);
        let t = &self.faces[face];
        let at = |x: u32, y: u32| t[(y * self.size + x) as usize];
        let top = at(x0, y0).lerp(at(x1, y0), tx);
        let bottom = at(x0, y1).lerp(at(x1, y1), tx);
        top.lerp(bottom, ty)
    }

    /// Half-size cube by a 2×2 box filter.
    pub fn downsample(&self) -> Self {
        let size = (self.size / 2).max(1);
        let mut out = Self::new(size);
        let src = self.size;
        for (face, texels) in out.faces.iter_mut().enumerate() {
            let t = &self.faces[face];
            for y in 0..size {
                for x in 0..size {
                    let mut sum = Vec3::ZERO;
                    for (dx, dy) in [(0, 0), (1, 0), (0, 1), (1, 1)] {
                        let (sx, sy) = ((x * 2 + dx).min(src - 1), (y * 2 + dy).min(src - 1));
                        sum += t[(sy * src + sx) as usize];
                    }
                    texels[(y * size + x) as usize] = sum * 0.25;
                }
            }
        }
        out
    }

    /// This cube resampled to `size` (bilinear up, box down).
    pub fn resized(&self, size: u32) -> Self {
        if size == self.size {
            return self.clone();
        }
        if size < self.size {
            let mut c = self.downsample();
            while c.size > size {
                c = c.downsample();
            }
            return c;
        }
        Self::from_fn(size, |d| self.sample(d))
    }

    /// Mean of all texels (weighted by solid angle).
    pub fn average(&self) -> Vec3 {
        let (mut sum, mut weight) = (Vec3::ZERO, 0.0);
        for texels in &self.faces {
            for y in 0..self.size {
                for x in 0..self.size {
                    let (u, v) = texel_uv(self.size, x, y);
                    let w = texel_solid_angle(self.size, u, v);
                    sum += texels[(y * self.size + x) as usize] * w;
                    weight += w;
                }
            }
        }
        sum / weight.max(1e-12)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directions_round_trip_through_faces() {
        for face in 0..6 {
            for (u, v) in [(0.0, 0.0), (0.5, -0.3), (-0.9, 0.8)] {
                let d = direction(face, u, v);
                let (f, u2, v2) = face_uv(d);
                assert_eq!(f, face);
                assert!((u - u2).abs() < 1e-5 && (v - v2).abs() < 1e-5);
            }
        }
    }

    /// The camera of each face looks through its centre with image right/down along u/v.
    #[test]
    fn face_cameras_match_the_cube_layout() {
        for face in 0..6 {
            let q = face_rotation(face);
            assert!((q * Vec3::Z - direction(face, 0.0, 0.0)).length() < 1e-5);
            let right = direction(face, 0.02, 0.0) - direction(face, 0.0, 0.0);
            assert!(right.normalize().dot(q * Vec3::X) > 0.99, "face {face}");
            let down = direction(face, 0.0, 0.02) - direction(face, 0.0, 0.0);
            assert!(down.normalize().dot(q * Vec3::NEG_Y) > 0.99, "face {face}");
        }
    }

    #[test]
    fn solid_angles_cover_the_sphere() {
        let c = Cube::from_fn(16, |_| Vec3::ONE);
        let mut total = 0.0;
        for y in 0..16 {
            for x in 0..16 {
                let (u, v) = texel_uv(16, x, y);
                total += texel_solid_angle(16, u, v);
            }
        }
        assert!(
            (total * 6.0 - 4.0 * std::f32::consts::PI).abs() < 0.02,
            "{total}"
        );
        assert!((c.average() - Vec3::ONE).length() < 1e-4);
        let up = Cube::from_fn(8, |d| Vec3::splat(d.y.max(0.0)));
        assert!(up.sample(Vec3::Y).x > 0.9 && up.sample(Vec3::NEG_Y).x < 1e-6);
        assert_eq!(up.resized(2).size, 2);
        assert_eq!(up.resized(16).size, 16);
    }
}
