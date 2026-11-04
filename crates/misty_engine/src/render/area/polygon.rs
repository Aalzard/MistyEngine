//! Integral of a clamped cosine over a polygon (the LTC closed form), CPU copy of
//! `area_light.wgsl`: transform the corners, clip them to the upper hemisphere, then sum each
//! edge's angle × the z of its plane normal. The result is signed: positive when the polygon
//! winds counter-clockwise seen from above (the light's front face, see [`rect_corners`]).

use glam::{Mat3, Vec3};

/// Corners of a rectangle light from its centre and half axes (right × up = the emitting
/// direction), in the winding that integrates positive in front of it.
pub fn rect_corners(center: Vec3, half_right: Vec3, half_up: Vec3) -> [Vec3; 4] {
    [
        center - half_right + half_up,
        center + half_right + half_up,
        center + half_right - half_up,
        center - half_right - half_up,
    ]
}

/// Shading frame (tangent towards the view, bitangent, normal) as the rows of a matrix.
pub fn shading_frame(n: Vec3, v: Vec3) -> Mat3 {
    let mut t1 = v - n * n.dot(v);
    if t1.length_squared() < 1e-8 {
        t1 = n.any_orthonormal_vector();
    }
    let t1 = t1.normalize();
    let t2 = n.cross(t1);
    Mat3::from_cols(t1, t2, n).transpose()
}

/// θ / sin θ × the z of a × b, for unit vectors a, b with cos θ = a·b.
fn edge(a: Vec3, b: Vec3) -> f32 {
    let x = a.dot(b).clamp(-1.0, 1.0);
    let theta_over_sin = if x > 0.9999 {
        1.0 + (1.0 - x) / 3.0
    } else {
        x.acos() / (1.0 - x * x).max(1e-12).sqrt()
    };
    a.cross(b).z * theta_over_sin
}

/// The cosine integral (0..1 for a polygon filling the hemisphere) of corners already in the
/// shading frame, after `minv`.
pub fn integrate(minv: Mat3, corners: [Vec3; 4]) -> f32 {
    let q = corners.map(|c| minv * c);
    let mut clipped = [Vec3::ZERO; 5];
    let mut count = 0;
    for i in 0..4 {
        let (a, b) = (q[i], q[(i + 1) % 4]);
        if a.z >= 0.0 {
            clipped[count] = a;
            count += 1;
        }
        if (a.z >= 0.0) != (b.z >= 0.0) {
            clipped[count] = a.lerp(b, a.z / (a.z - b.z));
            count += 1;
        }
    }
    if count < 3 {
        return 0.0;
    }
    let mut sum = 0.0;
    for i in 0..count {
        let a = clipped[i].normalize();
        let b = clipped[(i + 1) % count].normalize();
        sum += edge(a, b);
    }
    sum / std::f32::consts::TAU
}
