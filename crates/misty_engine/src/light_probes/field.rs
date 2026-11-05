//! Interpolating baked probes: the probes are tetrahedralized once (`delaunay`); a point finds
//! its tetrahedron by walking from the one it was in last time and blends the four probes' SH
//! with barycentric weights. Points outside the hull use the closest point of the nearest hull
//! face (three probes); fewer than four probes (or flat layouts) use the nearest probe.

use glam::{DVec3, Vec3};

use super::data::LightProbeData;
use super::delaunay::{self, Tetrahedralization};
use crate::render::probes::sh;

/// Up to four probes and their weights (unused slots have weight 0).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Weights {
    pub probes: [u32; 4],
    pub weights: [f32; 4],
}

impl Weights {
    fn single(i: u32) -> Self {
        Self {
            probes: [i, 0, 0, 0],
            weights: [1.0, 0.0, 0.0, 0.0],
        }
    }
}

/// Baked probes ready to sample.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct LightProbeField {
    pub positions: Vec<Vec3>,
    /// Irradiance / π coefficients per probe (radiance convolved with the cosine lobe), so
    /// `sh::eval` gives what a white diffuse surface reflects.
    pub irradiance: Vec<[Vec3; 9]>,
    pub tetra: Tetrahedralization,
}

impl LightProbeField {
    /// Probes at `positions` seeing radiance SH `radiance`.
    pub fn new(positions: Vec<Vec3>, radiance: &[[Vec3; 9]]) -> Self {
        let n = positions.len().min(radiance.len());
        let positions = positions[..n].to_vec();
        Self {
            tetra: delaunay::tetrahedralize(&positions),
            irradiance: radiance[..n]
                .iter()
                .map(|r| sh::cosine_convolve(*r))
                .collect(),
            positions,
        }
    }

    pub fn from_data(d: &LightProbeData) -> Self {
        let radiance: Vec<[Vec3; 9]> = (0..d.len()).map(|i| d.radiance(i)).collect();
        Self::new(d.positions.clone(), &radiance)
    }

    pub fn is_empty(&self) -> bool {
        self.positions.is_empty()
    }

    fn real(&self, i: u32) -> DVec3 {
        self.positions[i as usize].as_dvec3()
    }

    /// Probes and weights at `p`; `hint` is the tetrahedron to start from (kept per object
    /// between frames). `None` without probes.
    pub fn weights(&self, p: Vec3, hint: &mut u32) -> Option<Weights> {
        if self.positions.is_empty() {
            return None;
        }
        let pd = p.as_dvec3();
        if self.tetra.tets.is_empty() {
            return Some(Weights::single(self.nearest(pd)));
        }
        Some(match self.tetra.locate(pd, hint) {
            Some(t) => self.inside(t, pd),
            None => self.outside(pd),
        })
    }

    fn nearest(&self, p: DVec3) -> u32 {
        (0..self.positions.len() as u32)
            .min_by(|a, b| {
                self.real(*a)
                    .distance_squared(p)
                    .total_cmp(&self.real(*b).distance_squared(p))
            })
            .unwrap_or(0)
    }

    /// Barycentric weights in tetrahedron `t`: from the real positions, or from the jittered
    /// ones for slivers that are flat in reality (four coplanar grid probes).
    fn inside(&self, t: u32, p: DVec3) -> Weights {
        let v = self.tetra.tets[t as usize];
        let q = v.map(|i| self.real(i));
        let vol = delaunay::orient(q[0], q[1], q[2], q[3]);
        let scale = (q[1] - q[0])
            .length()
            .max((q[2] - q[0]).length())
            .max((q[3] - q[0]).length());
        let b = if vol.abs() > 1e-7 * scale * scale * scale {
            let mut b = [0.0; 4];
            for (i, w) in b.iter_mut().enumerate() {
                let mut r = q;
                r[i] = p;
                *w = delaunay::orient(r[0], r[1], r[2], r[3]) / vol;
            }
            b
        } else {
            self.tetra.barycentric(t, p)
        };
        normalized(v, b)
    }

    /// Closest point of the nearest hull face.
    fn outside(&self, p: DVec3) -> Weights {
        let mut best = (f64::INFINITY, [0u32; 3], [1.0, 0.0, 0.0]);
        for f in &self.tetra.hull {
            let (a, b, c) = (self.real(f[0]), self.real(f[1]), self.real(f[2]));
            let w = closest_on_triangle(p, a, b, c);
            let at = a * w[0] + b * w[1] + c * w[2];
            let d = at.distance_squared(p);
            if d < best.0 {
                best = (d, *f, w);
            }
        }
        let (_, f, w) = best;
        normalized([f[0], f[1], f[2], f[0]], [w[0], w[1], w[2], 0.0])
    }

    /// Interpolated irradiance SH at `p` (`None` without probes).
    pub fn sample(&self, p: Vec3, hint: &mut u32) -> Option<[Vec3; 9]> {
        let w = self.weights(p, hint)?;
        Some(self.blend(&w))
    }

    /// Weighted sum of the probes' irradiance SH.
    pub fn blend(&self, w: &Weights) -> [Vec3; 9] {
        let mut out = [Vec3::ZERO; 9];
        for (i, k) in w.probes.iter().zip(w.weights) {
            if k == 0.0 {
                continue;
            }
            for (o, c) in out.iter_mut().zip(self.irradiance[*i as usize]) {
                *o += c * k;
            }
        }
        out
    }
}

/// Clamp tiny negative weights (points on faces) and make them sum to one.
fn normalized(probes: [u32; 4], b: [f64; 4]) -> Weights {
    let b = b.map(|w| w.max(0.0));
    let sum: f64 = b.iter().sum();
    let weights = if sum > 0.0 {
        b.map(|w| (w / sum) as f32)
    } else {
        [1.0, 0.0, 0.0, 0.0]
    };
    Weights { probes, weights }
}

/// Barycentric coordinates of the point of triangle `abc` closest to `p`.
pub fn closest_on_triangle(p: DVec3, a: DVec3, b: DVec3, c: DVec3) -> [f64; 3] {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return [1.0, 0.0, 0.0];
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return [0.0, 1.0, 0.0];
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let v = d1 / (d1 - d3);
        return [1.0 - v, v, 0.0];
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return [0.0, 0.0, 1.0];
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let w = d2 / (d2 - d6);
        return [1.0 - w, 0.0, w];
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let w = (d4 - d3) / ((d4 - d3) + (d5 - d6));
        return [0.0, 1.0 - w, w];
    }
    let denom = 1.0 / (va + vb + vc);
    let (v, w) = (vb * denom, vc * denom);
    [1.0 - v - w, v, w]
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A field whose SH coefficient 0 (red) is a linear function of position, which barycentric
    /// interpolation reproduces exactly.
    fn linear_field(points: Vec<Vec3>) -> (LightProbeField, impl Fn(Vec3) -> f32) {
        let f = |p: Vec3| 1.0 + 0.3 * p.x - 0.2 * p.y + 0.1 * p.z;
        let radiance: Vec<[Vec3; 9]> = points
            .iter()
            .map(|p| {
                let mut c = [Vec3::ZERO; 9];
                c[0] = Vec3::new(f(*p), 0.5, 0.0);
                c
            })
            .collect();
        (LightProbeField::new(points, &radiance), f)
    }

    fn grid() -> Vec<Vec3> {
        let mut v = Vec::new();
        for x in 0..4 {
            for y in 0..3 {
                for z in 0..3 {
                    v.push(Vec3::new(x as f32 * 2.0, y as f32 * 1.5, z as f32 * 2.5));
                }
            }
        }
        v
    }

    fn random(n: usize) -> Vec<Vec3> {
        let mut s = 12345u64;
        let mut r = || {
            s = s
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            (s >> 40) as f32 / (1u64 << 24) as f32
        };
        (0..n)
            .map(|_| Vec3::new(r() * 8.0, r() * 3.0, r() * 5.0))
            .collect()
    }

    #[test]
    fn interpolation_is_exact_at_probes() {
        for points in [grid(), random(60)] {
            let (field, _) = linear_field(points.clone());
            let mut hint = 0;
            for (i, p) in points.iter().enumerate() {
                let got = field.sample(*p, &mut hint).unwrap();
                for (g, want) in got.iter().zip(field.irradiance[i]) {
                    assert!((*g - want).abs().max_element() < 1e-5, "probe {i}");
                }
                let w = field.weights(*p, &mut hint).unwrap();
                let top = w.weights.iter().cloned().fold(0.0, f32::max);
                assert!(top > 0.99999, "probe {i}: {w:?}");
            }
        }
    }

    #[test]
    fn interpolation_reproduces_linear_functions_inside() {
        let (field, f) = linear_field(grid());
        let mut hint = 0;
        for p in random(200) {
            let p = p * Vec3::new(0.75, 1.0, 1.0);
            let got = field.sample(p, &mut hint).unwrap()[0].x;
            assert!((got - f(p)).abs() < 1e-4, "{p}: {got} vs {}", f(p));
        }
    }

    #[test]
    fn outside_points_use_the_nearest_hull_face() {
        let (field, f) = linear_field(grid());
        let mut hint = 0;
        // Beyond +X: the value on the x = 6 face; below: on the floor; past a corner: the corner.
        for (p, on) in [
            (Vec3::new(9.0, 1.2, 2.0), Vec3::new(6.0, 1.2, 2.0)),
            (Vec3::new(3.0, -4.0, 1.0), Vec3::new(3.0, 0.0, 1.0)),
            (Vec3::new(-3.0, 9.0, -2.0), Vec3::new(0.0, 3.0, 0.0)),
        ] {
            let got = field.sample(p, &mut hint).unwrap()[0].x;
            assert!((got - f(on)).abs() < 1e-3, "{p}: {got} vs {}", f(on));
        }
    }

    #[test]
    fn few_probes_fall_back_to_the_nearest_and_none_gives_nothing() {
        let (field, f) = linear_field(vec![Vec3::ZERO, Vec3::new(4.0, 0.0, 0.0)]);
        let mut hint = 0;
        let got = field.sample(Vec3::new(3.5, 1.0, 0.0), &mut hint).unwrap()[0].x;
        assert!((got - f(Vec3::new(4.0, 0.0, 0.0))).abs() < 1e-6);
        assert!(
            LightProbeField::default()
                .sample(Vec3::ZERO, &mut hint)
                .is_none()
        );
    }

    #[test]
    fn walking_from_a_stale_hint_finds_the_same_tetrahedron() {
        let (field, _) = linear_field(random(80));
        let p = Vec3::new(4.0, 1.5, 2.5);
        let mut fresh = 0;
        let a = field.weights(p, &mut fresh).unwrap();
        for start in [0, 5, 17, 9999] {
            let mut hint = start;
            assert_eq!(field.weights(p, &mut hint).unwrap(), a);
            assert_eq!(hint, fresh);
        }
    }
}
