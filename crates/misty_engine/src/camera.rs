//! Camera math: view and projection matrices (left-handed, depth 0..1).

use glam::camera::lh;
use glam::{EulerRot, Mat4, Quat, Vec3};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// Vertical field of view in degrees.
    Perspective { fov_y: f32 },
    /// Half of the vertical view height in world units.
    Orthographic { size: f32 },
}

#[derive(Clone, Copy, Debug)]
pub struct Camera {
    pub position: Vec3,
    pub rotation: Quat,
    pub projection: Projection,
    pub near: f32,
    pub far: f32,
}

impl Camera {
    pub fn forward(&self) -> Vec3 {
        self.rotation * Vec3::Z
    }

    pub fn up(&self) -> Vec3 {
        self.rotation * Vec3::Y
    }

    pub fn right(&self) -> Vec3 {
        self.rotation * Vec3::X
    }

    pub fn view(&self) -> Mat4 {
        lh::view::look_to_mat4(self.position, self.forward(), self.up())
    }

    pub fn projection(&self, aspect: f32) -> Mat4 {
        let aspect = aspect.max(1e-4);
        match self.projection {
            Projection::Perspective { fov_y } => {
                lh::proj::directx::perspective(fov_y.to_radians(), aspect, self.near, self.far)
            }
            Projection::Orthographic { size } => lh::proj::directx::orthographic(
                -size * aspect,
                size * aspect,
                -size,
                size,
                self.near,
                self.far,
            ),
        }
    }

    pub fn view_projection(&self, aspect: f32) -> Mat4 {
        self.projection(aspect) * self.view()
    }

    /// Project a world point to normalized screen coords (0..1, y down) and view depth.
    /// Returns `None` when the point is behind the camera.
    pub fn world_to_screen(&self, aspect: f32, p: Vec3) -> Option<(glam::Vec2, f32)> {
        let clip = self.view_projection(aspect) * p.extend(1.0);
        if clip.w <= 1e-5 {
            return None;
        }
        let ndc = clip.truncate() / clip.w;
        let depth = (p - self.position).dot(self.forward());
        if depth < self.near {
            return None;
        }
        Some((glam::vec2(ndc.x * 0.5 + 0.5, 0.5 - ndc.y * 0.5), depth))
    }
}

/// Rotation from Euler degrees, applied Z then X then Y.
pub fn euler_to_quat(degrees: Vec3) -> Quat {
    let r = degrees * (std::f32::consts::PI / 180.0);
    Quat::from_euler(EulerRot::YXZ, r.y, r.x, r.z)
}

/// Inverse of [`euler_to_quat`], in degrees.
pub fn quat_to_euler(q: Quat) -> Vec3 {
    let (y, x, z) = q.to_euler(EulerRot::YXZ);
    Vec3::new(x, y, z) * (180.0 / std::f32::consts::PI)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn positive_pitch_looks_down_positive_yaw_turns_right() {
        let down = euler_to_quat(Vec3::new(30.0, 0.0, 0.0)) * Vec3::Z;
        assert!(down.y < 0.0);
        let right = euler_to_quat(Vec3::new(0.0, 90.0, 0.0)) * Vec3::Z;
        assert!((right - Vec3::X).length() < 1e-5);
    }

    #[test]
    fn euler_round_trip() {
        let e = Vec3::new(20.0, -45.0, 10.0);
        let back = quat_to_euler(euler_to_quat(e));
        assert!((back - e).length() < 1e-3, "{back:?}");
    }

    #[test]
    fn point_in_front_projects_to_center() {
        let cam = Camera {
            position: Vec3::new(0.0, 0.0, -10.0),
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let (uv, depth) = cam.world_to_screen(1.0, Vec3::ZERO).unwrap();
        assert!((uv - glam::vec2(0.5, 0.5)).length() < 1e-5);
        assert!((depth - 10.0).abs() < 1e-4);
        assert!(
            cam.world_to_screen(1.0, Vec3::new(0.0, 0.0, -20.0))
                .is_none()
        );
    }
}
