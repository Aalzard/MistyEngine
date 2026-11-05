//! New Shape generators. Every shape sits on the ground: its pivot is the centre of its base,
//! so a new shape drops onto the floor at the Scene view pivot.

use std::f32::consts::{PI, TAU};

use glam::{Vec2, Vec3};
use serde::{Deserialize, Serialize};

use super::{EditableMesh, Face, polygon_normal};

/// The New Shape popup's shapes with their parameters.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
pub enum Shape {
    Cube {
        size: Vec3,
    },
    Plane {
        width: f32,
        depth: f32,
        segments: u32,
    },
    Cylinder {
        radius: f32,
        height: f32,
        sides: u32,
    },
    Stairs {
        width: f32,
        height: f32,
        depth: f32,
        steps: u32,
    },
    Arch {
        radius: f32,
        thickness: f32,
        depth: f32,
        segments: u32,
    },
    Prism {
        width: f32,
        height: f32,
        depth: f32,
    },
    Torus {
        radius: f32,
        tube: f32,
        rings: u32,
        sides: u32,
    },
}

impl Shape {
    /// Every shape with default parameters, in popup order.
    pub const ALL: [Shape; 7] = [
        Shape::Cube { size: Vec3::ONE },
        Shape::Plane {
            width: 4.0,
            depth: 4.0,
            segments: 2,
        },
        Shape::Cylinder {
            radius: 0.5,
            height: 2.0,
            sides: 16,
        },
        Shape::Stairs {
            width: 2.0,
            height: 1.5,
            depth: 2.5,
            steps: 6,
        },
        Shape::Arch {
            radius: 1.5,
            thickness: 0.3,
            depth: 0.5,
            segments: 12,
        },
        Shape::Prism {
            width: 1.0,
            height: 1.0,
            depth: 1.0,
        },
        Shape::Torus {
            radius: 1.0,
            tube: 0.3,
            rings: 24,
            sides: 12,
        },
    ];

    pub fn name(&self) -> &'static str {
        match self {
            Shape::Cube { .. } => "Cube",
            Shape::Plane { .. } => "Plane",
            Shape::Cylinder { .. } => "Cylinder",
            Shape::Stairs { .. } => "Stairs",
            Shape::Arch { .. } => "Arch",
            Shape::Prism { .. } => "Prism",
            Shape::Torus { .. } => "Torus",
        }
    }

    /// The shape with these parameters.
    pub fn build(&self) -> EditableMesh {
        match *self {
            Shape::Cube { size } => cube(size),
            Shape::Plane {
                width,
                depth,
                segments,
            } => plane(width, depth, segments, segments),
            Shape::Cylinder {
                radius,
                height,
                sides,
            } => cylinder(radius, height, sides),
            Shape::Stairs {
                width,
                height,
                depth,
                steps,
            } => stairs(width, height, depth, steps),
            Shape::Arch {
                radius,
                thickness,
                depth,
                segments,
            } => arch(radius, thickness, depth, segments),
            Shape::Prism {
                width,
                height,
                depth,
            } => prism(width, height, depth),
            Shape::Torus {
                radius,
                tube,
                rings,
                sides,
            } => torus(radius, tube, rings, sides),
        }
    }
}

/// Reverse `verts` if their normal points away from `outward`.
fn oriented(positions: &[Vec3], mut verts: Vec<u32>, outward: Vec3) -> Vec<u32> {
    let n = polygon_normal(verts.iter().map(|v| positions[*v as usize]));
    if n.dot(outward) < 0.0 {
        verts.reverse();
    }
    verts
}

/// A 2D profile (`pts`, any winding) swept straight between depths `d0` and `d1`.
/// `to3(p, depth)` places a profile point; `axis` is the sweep direction. `caps` are polygons
/// over profile indices closing both ends; `smooth[k]` is the smoothing group of the side face
/// along profile edge k → k+1.
fn sweep(
    pts: &[Vec2],
    caps: &[Vec<usize>],
    smooth: &[u32],
    to3: impl Fn(Vec2, f32) -> Vec3,
    axis: Vec3,
    (d0, d1): (f32, f32),
) -> EditableMesh {
    let n = pts.len();
    let mut positions: Vec<Vec3> = pts.iter().map(|p| to3(*p, d0)).collect();
    positions.extend(pts.iter().map(|p| to3(*p, d1)));
    let mut faces = Vec::new();
    for cap in caps {
        let front: Vec<u32> = cap.iter().map(|i| *i as u32).collect();
        let back: Vec<u32> = cap.iter().map(|i| (*i + n) as u32).collect();
        faces.push(Face::new(oriented(&positions, front, -axis)));
        faces.push(Face::new(oriented(&positions, back, axis)));
    }
    // Signed area tells which side of each profile edge is outside.
    let area: f32 = (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum();
    for k in 0..n {
        let (a, b) = (pts[k], pts[(k + 1) % n]);
        let d = b - a;
        let out2 = if area > 0.0 {
            Vec2::new(d.y, -d.x)
        } else {
            Vec2::new(-d.y, d.x)
        };
        let mid = (a + b) * 0.5;
        let outward = to3(mid + out2, d0) - to3(mid, d0);
        let k1 = (k + 1) % n;
        let verts = vec![k as u32, k1 as u32, (k1 + n) as u32, (k + n) as u32];
        faces.push(Face {
            smoothing: smooth.get(k).copied().unwrap_or(0),
            ..Face::new(oriented(&positions, verts, outward))
        });
    }
    EditableMesh::new(positions, faces)
}

/// Box `size` with its base centred on the origin.
pub fn cube(size: Vec3) -> EditableMesh {
    let (hx, hz) = (size.x * 0.5, size.z * 0.5);
    let pts = [
        Vec2::new(-hx, 0.0),
        Vec2::new(hx, 0.0),
        Vec2::new(hx, size.y),
        Vec2::new(-hx, size.y),
    ];
    sweep(
        &pts,
        &[vec![0, 1, 2, 3]],
        &[],
        |p, d| Vec3::new(p.x, p.y, d),
        Vec3::Z,
        (-hz, hz),
    )
}

/// Flat grid facing +Y, `sx` × `sz` quads (open: one-sided, not closed).
pub fn plane(width: f32, depth: f32, sx: u32, sz: u32) -> EditableMesh {
    let (sx, sz) = (sx.max(1), sz.max(1));
    let mut positions = Vec::new();
    for j in 0..=sz {
        for i in 0..=sx {
            positions.push(Vec3::new(
                (i as f32 / sx as f32 - 0.5) * width,
                0.0,
                (j as f32 / sz as f32 - 0.5) * depth,
            ));
        }
    }
    let at = |i: u32, j: u32| j * (sx + 1) + i;
    let mut faces = Vec::new();
    for j in 0..sz {
        for i in 0..sx {
            let verts = vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            faces.push(Face::new(oriented(&positions, verts, Vec3::Y)));
        }
    }
    EditableMesh::new(positions, faces)
}

/// Upright cylinder, smooth sides, flat caps.
pub fn cylinder(radius: f32, height: f32, sides: u32) -> EditableMesh {
    let sides = sides.max(3) as usize;
    let pts: Vec<Vec2> = (0..sides)
        .map(|i| Vec2::from_angle(i as f32 / sides as f32 * TAU) * radius)
        .collect();
    sweep(
        &pts,
        &[(0..sides).collect()],
        &vec![1; sides],
        |p, d| Vec3::new(p.x, d, p.y),
        Vec3::Y,
        (0.0, height),
    )
}

/// Straight stairs climbing along +Z: a solid block with `steps` treads.
pub fn stairs(width: f32, height: f32, depth: f32, steps: u32) -> EditableMesh {
    let n = steps.max(1);
    // Side profile in (z, y): along the floor, up the back, then down the steps.
    let z = |i: u32| depth * i as f32 / n as f32 - depth * 0.5;
    let y = |i: u32| height * i as f32 / n as f32;
    let mut pts = vec![Vec2::new(z(0), 0.0), Vec2::new(z(n), 0.0)];
    for i in (1..=n).rev() {
        pts.push(Vec2::new(z(i), y(i)));
        pts.push(Vec2::new(z(i - 1), y(i)));
    }
    let hx = width * 0.5;
    sweep(
        &pts,
        &[(0..pts.len()).collect()],
        &[],
        |p, d| Vec3::new(d, p.y, p.x),
        Vec3::X,
        (-hx, hx),
    )
}

/// Half-ring arch standing on its two feet, opening along X; `radius` to the outside.
pub fn arch(radius: f32, thickness: f32, depth: f32, segments: u32) -> EditableMesh {
    let s = segments.max(2) as usize;
    let inner = (radius - thickness).max(radius * 0.05);
    // Outer arc 0 → π, then inner arc π → 0.
    let mut pts: Vec<Vec2> = (0..=s)
        .map(|i| Vec2::from_angle(i as f32 / s as f32 * PI) * radius)
        .collect();
    pts.extend(
        (0..=s)
            .rev()
            .map(|i| Vec2::from_angle(i as f32 / s as f32 * PI) * inner),
    );
    let count = pts.len();
    // One quad per segment on each end, so every cap face is convex.
    let caps: Vec<Vec<usize>> = (0..s)
        .map(|i| vec![i, i + 1, count - 2 - i, count - 1 - i])
        .collect();
    // Outer arc edges 0..s smooth (group 1), feet hard, inner arc smooth (group 2).
    let mut smooth = vec![0; count];
    for (k, g) in smooth.iter_mut().enumerate() {
        if k < s {
            *g = 1;
        } else if k > s && k < count - 1 {
            *g = 2;
        }
    }
    let hz = depth * 0.5;
    sweep(
        &pts,
        &caps,
        &smooth,
        |p, d| Vec3::new(p.x, p.y, d),
        Vec3::Z,
        (-hz, hz),
    )
}

/// Triangular prism (a roof): ridge along Z.
pub fn prism(width: f32, height: f32, depth: f32) -> EditableMesh {
    let hx = width * 0.5;
    let pts = [
        Vec2::new(-hx, 0.0),
        Vec2::new(hx, 0.0),
        Vec2::new(0.0, height),
    ];
    let hz = depth * 0.5;
    sweep(
        &pts,
        &[vec![0, 1, 2]],
        &[],
        |p, d| Vec3::new(p.x, p.y, d),
        Vec3::Z,
        (-hz, hz),
    )
}

/// Flat ring lying on the ground (axis +Y), smooth all over.
pub fn torus(radius: f32, tube: f32, rings: u32, sides: u32) -> EditableMesh {
    let (rings, sides) = (rings.max(3), sides.max(3));
    let mut positions = Vec::new();
    for i in 0..rings {
        let a = i as f32 / rings as f32 * TAU;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        for j in 0..sides {
            let b = j as f32 / sides as f32 * TAU;
            positions.push(dir * (radius + tube * b.cos()) + Vec3::Y * (tube + tube * b.sin()));
        }
    }
    let at = |i: u32, j: u32| (i % rings) * sides + (j % sides);
    let mut faces = Vec::new();
    for i in 0..rings {
        let a = (i as f32 + 0.5) / rings as f32 * TAU;
        let dir = Vec3::new(a.cos(), 0.0, a.sin());
        for j in 0..sides {
            let verts = vec![at(i, j), at(i + 1, j), at(i + 1, j + 1), at(i, j + 1)];
            let center = verts.iter().map(|v| positions[*v as usize]).sum::<Vec3>() / 4.0;
            let outward = center - (dir * radius + Vec3::Y * tube);
            faces.push(Face {
                smoothing: 1,
                ..Face::new(oriented(&positions, verts, outward))
            });
        }
    }
    EditableMesh::new(positions, faces)
}
