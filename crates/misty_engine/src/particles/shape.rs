//! Emission shapes: a start position and direction in the emitter's local space (+Z = axis).

use glam::Vec3;

use super::component::{ShapeKind, ShapeModule};
use super::rng::Rng;

/// Uniform random unit vector.
pub fn random_direction(rng: &mut Rng) -> Vec3 {
    let z = rng.signed();
    let a = rng.next_f32() * std::f32::consts::TAU;
    let r = (1.0 - z * z).max(0.0).sqrt();
    Vec3::new(r * a.cos(), r * a.sin(), z)
}

/// Uniform random point inside a unit sphere.
fn random_in_sphere(rng: &mut Rng) -> Vec3 {
    random_direction(rng) * rng.next_f32().cbrt()
}

/// Local start position and unit direction for one particle.
pub fn sample(shape: &ShapeModule, rng: &mut Rng) -> (Vec3, Vec3) {
    if !shape.enabled {
        // No shape: from the origin in a random direction.
        return (Vec3::ZERO, random_direction(rng));
    }
    let radius = shape.radius.max(0.0);
    match shape.shape {
        ShapeKind::Sphere => {
            let p = random_in_sphere(rng) * radius;
            let d = p.try_normalize().unwrap_or_else(|| random_direction(rng));
            (p, d)
        }
        ShapeKind::Hemisphere => {
            let mut p = random_in_sphere(rng) * radius;
            p.z = p.z.abs();
            let d = p.try_normalize().unwrap_or_else(|| {
                let mut d = random_direction(rng);
                d.z = d.z.abs();
                d
            });
            (p, d)
        }
        ShapeKind::Cone => {
            // Point on the base disc; particles further out tilt further, up to the cone angle.
            let f = rng.next_f32().sqrt();
            let phi = rng.next_f32() * std::f32::consts::TAU;
            let (s, c) = phi.sin_cos();
            let p = Vec3::new(c, s, 0.0) * f * radius;
            // A point emitter (radius 0) spreads randomly within the cone.
            let spread = if radius > 0.0 {
                f
            } else {
                rng.next_f32().sqrt()
            };
            let theta = shape.angle.clamp(0.0, 90.0).to_radians() * spread;
            let d = Vec3::new(c * theta.sin(), s * theta.sin(), theta.cos());
            (p, d)
        }
        ShapeKind::Box => {
            let p = Vec3::new(rng.signed(), rng.signed(), rng.signed()) * shape.box_size * 0.5;
            (p, Vec3::Z)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shapes_stay_inside_their_volume() {
        let mut rng = Rng::new(7);
        let mut shape = ShapeModule {
            radius: 2.0,
            angle: 30.0,
            box_size: Vec3::new(1.0, 2.0, 3.0),
            ..Default::default()
        };
        for kind in ShapeKind::ALL {
            shape.shape = kind;
            for _ in 0..500 {
                let (p, d) = sample(&shape, &mut rng);
                assert!(
                    (d.length() - 1.0).abs() < 1e-4,
                    "{kind:?} direction is unit"
                );
                match kind {
                    ShapeKind::Sphere => assert!(p.length() <= 2.0 + 1e-4),
                    ShapeKind::Hemisphere => assert!(p.length() <= 2.0 + 1e-4 && p.z >= 0.0),
                    ShapeKind::Cone => {
                        assert!(p.z == 0.0 && p.length() <= 2.0 + 1e-4);
                        let angle = d.z.clamp(-1.0, 1.0).acos().to_degrees();
                        assert!(angle <= 30.0 + 1e-3, "inside the cone: {angle}");
                    }
                    ShapeKind::Box => {
                        assert!(p.abs().cmple(Vec3::new(0.5, 1.0, 1.5)).all());
                        assert_eq!(d, Vec3::Z);
                    }
                }
            }
        }
    }
}
