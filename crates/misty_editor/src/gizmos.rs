//! Scene-view gizmo lines: camera frustum, light ranges, and mesh wireframes.

use glam::{Mat4, Vec3};
use misty_engine::render::LineVertex;
use misty_engine::render::MeshDraw;
use misty_engine::render::mesh::primitive_edges;
use misty_engine::scene::{Camera, Light, LightType, Scene};

use crate::selection::Selection;

/// Wireframe draw-mode colour (dark lines).
pub const WIREFRAME: [f32; 4] = [0.05, 0.05, 0.05, 0.9];
const CAMERA: [f32; 4] = [0.85, 0.85, 0.85, 0.8];
const LIGHT: [f32; 4] = [1.0, 0.92, 0.45, 0.9];

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

    /// Camera frustum (shown when the camera is selected). The far plane is capped for readability.
    fn frustum(&mut self, m: &Mat4, cam: &Camera, color: [f32; 4]) {
        let aspect = 16.0 / 9.0;
        let (near, far) = (cam.near, cam.far.min(cam.near + 20.0));
        let corners = |d: f32| {
            let h = if cam.orthographic {
                cam.ortho_size
            } else {
                d * (cam.fov.to_radians() * 0.5).tan()
            };
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

    /// Directional: disc with parallel rays; Point: range spheres; Spot: cone.
    fn light_rays(&mut self, m: &Mat4, light: &Light, color: [f32; 4]) {
        match light.kind {
            LightType::Directional => {
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
            LightType::Point => {
                for axis in 0..3 {
                    self.circle(m, Vec3::ZERO, light.range, axis, color);
                }
            }
            LightType::Spot => {
                let d = light.range;
                let r = d * (light.spot_angle.to_radians() * 0.5).tan();
                self.circle(m, Vec3::Z * d, r, 2, color);
                for (x, y) in [(r, 0.0), (-r, 0.0), (0.0, r), (0.0, -r)] {
                    self.line(
                        m.transform_point3(Vec3::ZERO),
                        m.transform_point3(Vec3::new(x, y, d)),
                        color,
                    );
                }
            }
        }
    }
}

/// Lines for every active object; selected ones are highlighted.
pub fn scene_lines(scene: &Scene, selection: &Selection) -> Lines {
    let mut lines = Lines::default();
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) {
            continue;
        }
        let is_sel = selection.contains(e);
        let m = scene.world_matrix(e);
        // Unscaled matrix for camera/light gizmos.
        let (_, rot, pos) = m.to_scale_rotation_translation();
        let rigid = Mat4::from_rotation_translation(rot, pos);
        let w = &scene.world;
        if !is_sel {
            continue;
        }
        if let Ok(cam) = w.get::<&Camera>(e) {
            lines.frustum(&rigid, &cam, CAMERA);
        }
        if let Ok(light) = w.get::<&Light>(e) {
            lines.light_rays(&rigid, &light, LIGHT);
        }
    }
    lines
}

/// Mesh triangle edges for the Wireframe / Shaded Wireframe draw modes.
pub fn wireframe(lines: &mut Lines, meshes: &[MeshDraw], color: [f32; 4]) {
    for d in meshes {
        for [a, b] in primitive_edges(d.mesh) {
            lines.line(
                d.model.transform_point3(*a),
                d.model.transform_point3(*b),
                color,
            );
        }
    }
}
