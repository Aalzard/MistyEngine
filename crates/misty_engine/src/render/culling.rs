//! Frustum culling: the six clip planes of a view-projection matrix (depth 0..1) tested
//! against world-space boxes. The test is conservative: a box is only rejected when it lies
//! wholly outside one plane, so nothing the GPU would rasterise is skipped.

use glam::{Mat4, Vec3, Vec4};

/// Axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Aabb {
    pub min: Vec3,
    pub max: Vec3,
}

impl Aabb {
    pub fn new(min: Vec3, max: Vec3) -> Self {
        Self { min, max }
    }

    /// World box of a local box under `m` (centre and extents, exact for affine matrices).
    pub fn transformed(&self, m: &Mat4) -> Self {
        let center = m.transform_point3((self.min + self.max) * 0.5);
        let half = (self.max - self.min) * 0.5;
        let extent = m.x_axis.truncate().abs() * half.x
            + m.y_axis.truncate().abs() * half.y
            + m.z_axis.truncate().abs() * half.z;
        Self::new(center - extent, center + extent)
    }

    /// Box around points grown by `radius` each (`None` when empty).
    pub fn around(points: impl Iterator<Item = (Vec3, f32)>) -> Option<Self> {
        points.fold(None, |acc: Option<Self>, (p, r)| {
            let b = Self::new(p - Vec3::splat(r), p + Vec3::splat(r));
            Some(match acc {
                Some(a) => Self::new(a.min.min(b.min), a.max.max(b.max)),
                None => b,
            })
        })
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Frustum {
    /// Inside where `dot(plane.xyz, p) + plane.w >= 0`.
    planes: [Vec4; 6],
}

impl Frustum {
    pub fn from_matrix(m: &Mat4) -> Self {
        let (r0, r1, r2, r3) = (m.row(0), m.row(1), m.row(2), m.row(3));
        let planes = [r3 + r0, r3 - r0, r3 + r1, r3 - r1, r2, r3 - r2].map(|p| {
            let len = p.truncate().length();
            if len > 0.0 { p / len } else { p }
        });
        Self { planes }
    }

    /// Whether any part of `b` may be inside.
    pub fn intersects(&self, b: &Aabb) -> bool {
        let center = (b.min + b.max) * 0.5;
        let half = (b.max - b.min) * 0.5;
        self.planes.iter().all(|p| {
            let n = p.truncate();
            let reach = half.dot(n.abs());
            // A little slack so float error never drops a box touching a plane.
            n.dot(center) + p.w + reach >= -1e-4 * (1.0 + reach)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use glam::Quat;

    fn camera() -> Camera {
        Camera {
            position: Vec3::ZERO,
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.3,
            far: 100.0,
        }
    }

    fn unit_at(p: Vec3) -> Aabb {
        Aabb::new(p - Vec3::splat(0.5), p + Vec3::splat(0.5))
    }

    #[test]
    fn boxes_inside_outside_and_crossing_planes() {
        let f = Frustum::from_matrix(&camera().view_projection(16.0 / 9.0));
        assert!(f.intersects(&unit_at(Vec3::new(0.0, 0.0, 10.0))));
        assert!(!f.intersects(&unit_at(Vec3::new(0.0, 0.0, -5.0))), "behind");
        assert!(
            !f.intersects(&unit_at(Vec3::new(0.0, 0.0, 150.0))),
            "past far"
        );
        assert!(!f.intersects(&unit_at(Vec3::new(40.0, 0.0, 10.0))), "right");
        assert!(
            !f.intersects(&unit_at(Vec3::new(0.0, -20.0, 10.0))),
            "below"
        );
        // Straddling the near plane and the left edge both count as visible.
        assert!(f.intersects(&unit_at(Vec3::new(0.0, 0.0, 0.2))));
        let edge_x = 10.0 * (30f32.to_radians().tan() * 16.0 / 9.0);
        assert!(f.intersects(&unit_at(Vec3::new(-edge_x - 0.4, 0.0, 10.0))));
    }

    #[test]
    fn orthographic_and_transformed_boxes() {
        let mut cam = camera();
        cam.projection = Projection::Orthographic { size: 5.0 };
        let f = Frustum::from_matrix(&cam.view_projection(1.0));
        assert!(f.intersects(&unit_at(Vec3::new(5.2, 0.0, 50.0))));
        assert!(!f.intersects(&unit_at(Vec3::new(6.0, 0.0, 50.0))));

        // A rotated, scaled cube's world box matches its transformed corners.
        let m = Mat4::from_scale_rotation_translation(
            Vec3::new(2.0, 1.0, 0.5),
            Quat::from_rotation_y(0.7) * Quat::from_rotation_x(0.3),
            Vec3::new(1.0, 2.0, 3.0),
        );
        let local = unit_at(Vec3::ZERO);
        let world = local.transformed(&m);
        let (mut lo, mut hi) = (Vec3::splat(f32::MAX), Vec3::splat(f32::MIN));
        for i in 0..8 {
            let c = Vec3::new(
                if i & 1 == 0 { -0.5 } else { 0.5 },
                if i & 2 == 0 { -0.5 } else { 0.5 },
                if i & 4 == 0 { -0.5 } else { 0.5 },
            );
            let p = m.transform_point3(c);
            lo = lo.min(p);
            hi = hi.max(p);
        }
        assert!(world.min.abs_diff_eq(lo, 1e-5) && world.max.abs_diff_eq(hi, 1e-5));
        let points = [(Vec3::ZERO, 1.0), (Vec3::X * 4.0, 0.5)];
        let b = Aabb::around(points.into_iter()).unwrap();
        assert_eq!(b, Aabb::new(Vec3::splat(-1.0), Vec3::new(4.5, 1.0, 1.0)));
    }
}
