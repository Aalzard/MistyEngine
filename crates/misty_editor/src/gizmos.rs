//! Scene-view gizmo lines: wire shapes for placeholder objects, camera frustum, light rays.

use glam::{Mat4, Vec3};
use misty_engine::camera::euler_to_quat;
use misty_engine::render::LineVertex;

use crate::actions::{LightKind, Primitive};
use crate::state::{ObjectKind, PlaceholderObject};

pub const SELECTED: [f32; 4] = [0.96, 0.54, 0.0, 1.0];
const WIRE: [f32; 4] = [0.75, 0.75, 0.75, 0.9];
const CAMERA: [f32; 4] = [0.85, 0.85, 0.85, 0.8];
const LIGHT: [f32; 4] = [1.0, 0.92, 0.45, 0.9];

pub fn object_matrix(o: &PlaceholderObject) -> Mat4 {
    Mat4::from_scale_rotation_translation(
        Vec3::from(o.scale),
        euler_to_quat(Vec3::from(o.rotation)),
        Vec3::from(o.position),
    )
}

/// Collects line segments in world space.
#[derive(Default)]
pub struct Lines(pub Vec<LineVertex>);

impl Lines {
    pub fn line(&mut self, a: Vec3, b: Vec3, color: [f32; 4]) {
        self.0.push(LineVertex {
            position: a.into(),
            color,
        });
        self.0.push(LineVertex {
            position: b.into(),
            color,
        });
    }

    fn polyline(&mut self, m: &Mat4, points: &[Vec3], closed: bool, color: [f32; 4]) {
        let n = points.len();
        let count = if closed { n } else { n.saturating_sub(1) };
        for i in 0..count {
            self.line(
                m.transform_point3(points[i]),
                m.transform_point3(points[(i + 1) % n]),
                color,
            );
        }
    }

    /// Circle of `radius` around `axis` (0 = X, 1 = Y, 2 = Z) at `center`, in local space.
    fn circle(&mut self, m: &Mat4, center: Vec3, radius: f32, axis: usize, color: [f32; 4]) {
        let pts: Vec<Vec3> = (0..32)
            .map(|i| {
                let a = i as f32 / 32.0 * std::f32::consts::TAU;
                let (s, c) = a.sin_cos();
                center
                    + match axis {
                        0 => Vec3::new(0.0, c, s),
                        1 => Vec3::new(c, 0.0, s),
                        _ => Vec3::new(c, s, 0.0),
                    } * radius
            })
            .collect();
        self.polyline(m, &pts, true, color);
    }

    fn wire_box(&mut self, m: &Mat4, half: Vec3, color: [f32; 4]) {
        let c = |x: f32, y: f32, z: f32| Vec3::new(x, y, z) * half;
        let corners = [
            c(-1., -1., -1.),
            c(1., -1., -1.),
            c(1., 1., -1.),
            c(-1., 1., -1.),
            c(-1., -1., 1.),
            c(1., -1., 1.),
            c(1., 1., 1.),
            c(-1., 1., 1.),
        ];
        for (a, b) in [
            (0, 1),
            (1, 2),
            (2, 3),
            (3, 0),
            (4, 5),
            (5, 6),
            (6, 7),
            (7, 4),
            (0, 4),
            (1, 5),
            (2, 6),
            (3, 7),
        ] {
            self.line(
                m.transform_point3(corners[a]),
                m.transform_point3(corners[b]),
                color,
            );
        }
    }

    fn primitive(&mut self, m: &Mat4, p: Primitive, color: [f32; 4]) {
        match p {
            Primitive::Cube => self.wire_box(m, Vec3::splat(0.5), color),
            Primitive::Sphere => {
                for axis in 0..3 {
                    self.circle(m, Vec3::ZERO, 0.5, axis, color);
                }
            }
            Primitive::Capsule | Primitive::Cylinder => {
                let h = if p == Primitive::Capsule { 0.5 } else { 1.0 };
                self.circle(m, Vec3::Y * h, 0.5, 1, color);
                self.circle(m, -Vec3::Y * h, 0.5, 1, color);
                for (x, z) in [(0.5, 0.0), (-0.5, 0.0), (0.0, 0.5), (0.0, -0.5)] {
                    self.line(
                        m.transform_point3(Vec3::new(x, h, z)),
                        m.transform_point3(Vec3::new(x, -h, z)),
                        color,
                    );
                }
                if p == Primitive::Capsule {
                    self.circle(m, Vec3::Y * h, 0.5, 0, color);
                    self.circle(m, -Vec3::Y * h, 0.5, 0, color);
                }
            }
            Primitive::Plane => self.wire_box(m, Vec3::new(5.0, 0.0, 5.0), color),
            Primitive::Quad => self.wire_box(m, Vec3::new(0.5, 0.5, 0.0), color),
        }
    }

    /// Camera frustum pyramid (shown when the camera is selected).
    fn frustum(&mut self, m: &Mat4, color: [f32; 4]) {
        let (near, far, fov, aspect) = (0.3_f32, 5.0_f32, 60.0_f32, 16.0 / 9.0);
        let corners = |d: f32| {
            let h = d * (fov.to_radians() * 0.5).tan();
            let w = h * aspect;
            [
                Vec3::new(-w, -h, d),
                Vec3::new(w, -h, d),
                Vec3::new(w, h, d),
                Vec3::new(-w, h, d),
            ]
        };
        let (n, f) = (corners(near), corners(far));
        self.polyline(m, &n, true, color);
        self.polyline(m, &f, true, color);
        for i in 0..4 {
            self.line(m.transform_point3(n[i]), m.transform_point3(f[i]), color);
        }
    }

    /// Directional light: disc with parallel rays along +Z.
    fn light_rays(&mut self, m: &Mat4, kind: LightKind, color: [f32; 4]) {
        match kind {
            LightKind::Directional => {
                self.circle(m, Vec3::ZERO, 0.5, 2, color);
                for i in 0..8 {
                    let a = i as f32 / 8.0 * std::f32::consts::TAU;
                    let p = Vec3::new(a.cos(), a.sin(), 0.0) * 0.5;
                    self.line(
                        m.transform_point3(p),
                        m.transform_point3(p + Vec3::Z * 2.0),
                        color,
                    );
                }
            }
            LightKind::Point => {
                for axis in 0..3 {
                    self.circle(m, Vec3::ZERO, 10.0, axis, color);
                }
            }
            LightKind::Spot => {
                let r = 10.0 * (15.0_f32).to_radians().tan();
                self.circle(m, Vec3::Z * 10.0, r, 2, color);
                for (x, y) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                    self.line(
                        m.transform_point3(Vec3::ZERO),
                        m.transform_point3(Vec3::new(x, y, 10.0)),
                        color,
                    );
                }
            }
        }
    }
}

/// Lines for every object; selected ones are highlighted.
pub fn scene_lines(objects: &[PlaceholderObject], selected: Option<usize>) -> Lines {
    let mut lines = Lines::default();
    for (i, o) in objects.iter().enumerate() {
        if !o.active {
            continue;
        }
        let is_sel = selected == Some(i);
        let m = object_matrix(o);
        // Unscaled matrix for icon-like gizmos.
        let rigid = Mat4::from_rotation_translation(
            euler_to_quat(Vec3::from(o.rotation)),
            Vec3::from(o.position),
        );
        match o.kind {
            ObjectKind::Primitive(p) => {
                lines.primitive(&m, p, if is_sel { SELECTED } else { WIRE })
            }
            ObjectKind::Camera if is_sel => lines.frustum(&rigid, CAMERA),
            ObjectKind::Light(kind) if is_sel => lines.light_rays(&rigid, kind, LIGHT),
            _ => {}
        }
    }
    lines
}
