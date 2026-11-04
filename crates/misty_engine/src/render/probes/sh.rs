//! Order-2 spherical harmonics (9 coefficients): project a cubemap and convolve it with the
//! cosine lobe, so evaluating the result at a normal gives Lambert irradiance / π (the radiance a
//! white diffuse surface reflects). Same basis and constants as `probes.wgsl::sh_eval`.

use glam::Vec3;

use super::cube::{Cube, direction, texel_solid_angle, texel_uv};

/// The nine real SH basis functions at unit direction `d`.
pub fn basis(d: Vec3) -> [f32; 9] {
    let (x, y, z) = (d.x, d.y, d.z);
    [
        0.282_095,
        0.488_603 * y,
        0.488_603 * z,
        0.488_603 * x,
        1.092_548 * x * y,
        1.092_548 * y * z,
        0.315_392 * (3.0 * z * z - 1.0),
        1.092_548 * x * z,
        0.546_274 * (x * x - y * y),
    ]
}

/// Cosine-lobe convolution per band divided by π: π, 2π/3, π/4 → 1, 2/3, 1/4.
const BAND: [f32; 9] = [
    1.0,
    2.0 / 3.0,
    2.0 / 3.0,
    2.0 / 3.0,
    0.25,
    0.25,
    0.25,
    0.25,
    0.25,
];

/// Radiance projected onto the basis (solid-angle weighted, normalised to 4π).
pub fn project(cube: &Cube) -> [Vec3; 9] {
    let mut sh = [Vec3::ZERO; 9];
    let mut total = 0.0;
    for (face, texels) in cube.faces.iter().enumerate() {
        for y in 0..cube.size {
            for x in 0..cube.size {
                let (u, v) = texel_uv(cube.size, x, y);
                let w = texel_solid_angle(cube.size, u, v);
                let c = texels[(y * cube.size + x) as usize] * w;
                for (s, b) in sh.iter_mut().zip(basis(direction(face, u, v))) {
                    *s += c * b;
                }
                total += w;
            }
        }
    }
    let norm = 4.0 * std::f32::consts::PI / total.max(1e-12);
    sh.map(|s| s * norm)
}

/// Diffuse coefficients of a cube: evaluate with [`eval`] for irradiance / π.
pub fn irradiance(cube: &Cube) -> [Vec3; 9] {
    cosine_convolve(project(cube))
}

/// Radiance coefficients → irradiance / π (the cosine lobe per band).
pub fn cosine_convolve(mut sh: [Vec3; 9]) -> [Vec3; 9] {
    for (s, k) in sh.iter_mut().zip(BAND) {
        *s *= k;
    }
    sh
}

/// Sum of coefficients × basis at normal `n`.
pub fn eval(sh: &[Vec3; 9], n: Vec3) -> Vec3 {
    sh.iter()
        .zip(basis(n.normalize_or(Vec3::Y)))
        .map(|(c, b)| *c * b)
        .sum()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Brute-force cosine-weighted irradiance / π from a cube.
    fn reference(cube: &Cube, n: Vec3) -> Vec3 {
        let mut sum = Vec3::ZERO;
        for (face, texels) in cube.faces.iter().enumerate() {
            for y in 0..cube.size {
                for x in 0..cube.size {
                    let (u, v) = texel_uv(cube.size, x, y);
                    let d = direction(face, u, v);
                    let w = texel_solid_angle(cube.size, u, v) * d.dot(n).max(0.0);
                    sum += texels[(y * cube.size + x) as usize] * w;
                }
            }
        }
        sum / std::f32::consts::PI
    }

    #[test]
    fn constant_environment_gives_its_colour() {
        let c = Cube::from_fn(16, |_| Vec3::new(0.2, 0.5, 1.0));
        let sh = irradiance(&c);
        for n in [Vec3::Y, Vec3::NEG_X, Vec3::new(0.3, -0.5, 0.8)] {
            assert!((eval(&sh, n) - Vec3::new(0.2, 0.5, 1.0)).length() < 1e-3);
        }
    }

    /// A red half-space (sky above is white): the projection matches brute-force irradiance
    /// within the usual order-2 error, and the colours tilt the right way.
    #[test]
    fn projection_matches_brute_force_irradiance() {
        let c = Cube::from_fn(24, |d| {
            if d.x > 0.0 {
                Vec3::new(1.0, 0.0, 0.0)
            } else {
                Vec3::splat(0.5) + Vec3::Y * d.y.max(0.0)
            }
        });
        let sh = irradiance(&c);
        for n in [
            Vec3::X,
            Vec3::NEG_X,
            Vec3::Y,
            Vec3::NEG_Y,
            Vec3::Z,
            Vec3::new(0.6, 0.6, -0.5).normalize(),
        ] {
            let (got, want) = (eval(&sh, n), reference(&c, n));
            assert!(
                (got - want).abs().max_element() < 0.08,
                "{n:?}: {got:?} vs {want:?}"
            );
        }
        assert!(eval(&sh, Vec3::X).x > eval(&sh, Vec3::NEG_X).x + 0.4);
        assert!(eval(&sh, Vec3::NEG_X).y > eval(&sh, Vec3::X).y);
    }
}
