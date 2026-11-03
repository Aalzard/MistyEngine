//! Cascaded shadow maps for the directional light: practical split distances along the view,
//! one orthographic box per cascade around the slice's bounding sphere, snapped to whole
//! texels so shadows don't shimmer while the camera moves or turns.

use glam::{Mat4, Vec3};

use super::culling::Aabb;
use crate::camera::{Camera, Projection};
use crate::quality::ShadowSettings;

pub const MAX_CASCADES: usize = 4;
/// Casters further than this towards the light than a cascade's sphere are not drawn into it.
const MAX_CASTER_REACH: f32 = 1000.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cascade {
    pub view_proj: Mat4,
    /// View depth (metres along the camera's forward axis) where the cascade ends.
    pub far: f32,
    /// The map spans `2 × radius` metres.
    pub radius: f32,
    /// World size of one texel.
    pub texel: f32,
    /// Metres between the map's near and far depth planes.
    pub depth_range: f32,
}

/// Ends of each cascade between `near` and `far`: `lambda` mixes logarithmic (1) and uniform (0).
pub fn splits(near: f32, far: f32, count: usize, lambda: f32) -> Vec<f32> {
    let near = near.max(0.01);
    let far = far.max(near + 0.01);
    (1..=count)
        .map(|i| {
            let t = i as f32 / count as f32;
            let log = near * (far / near).powf(t);
            let uniform = near + (far - near) * t;
            lambda * log + (1.0 - lambda) * uniform
        })
        .collect()
}

/// Centre depth and radius of the sphere around the view slice between depths `a` and `b`.
/// Depends only on the projection, so it never changes while the camera moves or turns.
fn slice_sphere(camera: &Camera, aspect: f32, a: f32, b: f32) -> (f32, f32) {
    match camera.projection {
        Projection::Perspective { fov_y } => {
            // Squared half-diagonal of the slice per unit depth.
            let t = (fov_y.to_radians() * 0.5).tan();
            let k2 = t * t * (1.0 + aspect * aspect);
            let c = ((a + b) * 0.5 * (1.0 + k2)).min(b);
            let r = ((b - c).powi(2) + b * b * k2).sqrt();
            (c, r)
        }
        Projection::Orthographic { size } => {
            let half = Vec3::new(size * aspect, size, (b - a) * 0.5);
            ((a + b) * 0.5, half.length())
        }
    }
}

/// Fit up to `settings.cascades` shadow boxes for a light travelling along `dir`.
/// `casters` = world bounds of the shadow casters (they pull each box's near plane back).
pub fn fit(
    camera: &Camera,
    aspect: f32,
    dir: Vec3,
    settings: &ShadowSettings,
    casters: &[(Vec3, Vec3)],
    size: u32,
) -> Vec<Cascade> {
    let count = (settings.cascades as usize).clamp(1, MAX_CASCADES);
    let near = camera.near.max(0.01);
    let far = settings.distance.min(camera.far).max(near + 0.1);
    let ends = splits(near, far, count, settings.split_lambda);
    let dir = dir.normalize_or(Vec3::NEG_Y);
    let up = if dir.y.abs() > 0.99 { Vec3::Z } else { Vec3::Y };
    // Fixed origin: the texel grid stays put in the world.
    let light_view = glam::camera::lh::view::look_to_mat4(Vec3::ZERO, dir, up);
    let boxes: Vec<Aabb> = casters
        .iter()
        .map(|(lo, hi)| Aabb::new(*lo, *hi).transformed(&light_view))
        .collect();
    let forward = camera.forward();

    let mut out = Vec::with_capacity(count);
    let mut start = near;
    let mut prev_end = near;
    for end in ends {
        let (depth, radius) = slice_sphere(camera, aspect, start, end);
        // Quantised so float noise can't change the texel size from frame to frame, plus two
        // texels each side so snapping never pushes the sphere off the map.
        let radius = (radius * 16.0).ceil() / 16.0 * size as f32 / (size as f32 - 4.0);
        let texel = 2.0 * radius / size as f32;
        let center = light_view.transform_point3(camera.position + forward * depth);
        let x = (center.x / texel).floor() * texel;
        let y = (center.y / texel).floor() * texel;
        let (lo, hi) = (center.z - radius, center.z + radius);
        // Casters over this cascade's footprint may stand between it and the light.
        let reach = boxes
            .iter()
            .filter(|b| {
                b.max.x >= x - radius
                    && b.min.x <= x + radius
                    && b.max.y >= y - radius
                    && b.min.y <= y + radius
                    && b.min.z < hi
            })
            .fold(lo, |z, b| z.min(b.min.z))
            .max(lo - MAX_CASTER_REACH);
        let z_near = reach - 0.5;
        let proj = glam::camera::lh::proj::directx::orthographic(
            x - radius,
            x + radius,
            y - radius,
            y + radius,
            z_near,
            hi,
        );
        out.push(Cascade {
            view_proj: proj * light_view,
            far: end,
            radius,
            texel,
            depth_range: hi - z_near,
        });
        // The next cascade also covers this one's blend band.
        let band = settings.cascade_blend * (end - prev_end);
        start = (end - band).max(near);
        prev_end = end;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn camera(position: Vec3, yaw: f32) -> Camera {
        Camera {
            position,
            rotation: Quat::from_rotation_y(yaw),
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.3,
            far: 1000.0,
        }
    }

    fn sun() -> Vec3 {
        Vec3::new(0.3, -0.4, 0.8).normalize()
    }

    #[test]
    fn splits_grow_and_end_at_the_shadow_distance() {
        let s = splits(0.3, 150.0, 4, 0.8);
        assert_eq!(s.len(), 4);
        assert!(s.windows(2).all(|w| w[0] < w[1]), "{s:?}");
        assert!((s[3] - 150.0).abs() < 1e-3);
        // Mostly logarithmic: the first cascade is short.
        assert!(s[0] < 15.0, "{s:?}");
        let uniform = splits(1.0, 101.0, 4, 0.0);
        assert!((uniform[0] - 26.0).abs() < 1e-3);
    }

    #[test]
    fn every_slice_corner_is_inside_its_cascade() {
        let cam = camera(Vec3::new(3.0, 2.0, -5.0), 0.7);
        let settings = ShadowSettings::default();
        let cascades = fit(&cam, 16.0 / 9.0, sun(), &settings, &[], 2048);
        assert_eq!(cascades.len(), 4);
        let inv = cam.view_projection(16.0 / 9.0).inverse();
        let mut start = cam.near;
        for c in &cascades {
            for z in [start, c.far] {
                // NDC depth of view depth z (perspective, depth 0..1).
                let ndc_z = cam.far / (cam.far - cam.near) * (1.0 - cam.near / z);
                for (x, y) in [(-1.0, -1.0), (1.0, -1.0), (-1.0, 1.0), (1.0, 1.0)] {
                    let p = inv.project_point3(Vec3::new(x, y, ndc_z));
                    let s = c.view_proj.project_point3(p);
                    assert!(
                        s.x.abs() <= 1.0 && s.y.abs() <= 1.0 && (0.0..=1.0).contains(&s.z),
                        "{s:?}"
                    );
                }
            }
            start = c.far;
        }
    }

    #[test]
    fn moving_the_camera_moves_the_map_by_whole_texels() {
        let settings = ShadowSettings::default();
        let a = fit(&camera(Vec3::ZERO, 0.2), 1.5, sun(), &settings, &[], 2048);
        let b = fit(
            &camera(Vec3::new(0.37, 0.0, 0.11), 0.9),
            1.5,
            sun(),
            &settings,
            &[],
            2048,
        );
        for (a, b) in a.iter().zip(&b) {
            // Same size whatever the view direction (bounding sphere).
            assert_eq!(a.radius, b.radius);
            // A fixed world point lands on the same sub-texel position in both maps.
            let p = Vec3::new(1.234, 0.5, 2.345);
            let (pa, pb) = (a.view_proj.project_point3(p), b.view_proj.project_point3(p));
            let texels = (pa - pb).truncate() * 2048.0 * 0.5;
            let frac = texels - texels.round();
            assert!(frac.abs().max_element() < 1e-2, "{texels:?}");
        }
    }

    #[test]
    fn casters_towards_the_light_extend_the_depth_range() {
        let cam = camera(Vec3::ZERO, 0.0);
        let settings = ShadowSettings {
            cascades: 1,
            distance: 20.0,
            ..Default::default()
        };
        let plain = fit(&cam, 1.0, Vec3::NEG_Y, &settings, &[], 1024)[0];
        // A tall tower right above the view slice.
        let tower = [(Vec3::new(-1.0, 0.0, 5.0), Vec3::new(1.0, 200.0, 7.0))];
        let tall = fit(&cam, 1.0, Vec3::NEG_Y, &settings, &tower, 1024)[0];
        assert!(tall.depth_range > plain.depth_range + 150.0);
        let top = tall.view_proj.project_point3(Vec3::new(0.0, 199.0, 6.0));
        assert!((0.0..=1.0).contains(&top.z), "{top:?}");
    }
}
