//! Blend tree weight maths: 1D, 2D Simple Directional, 2D Freeform Directional, 2D Freeform
//! Cartesian and Direct. Every function returns one weight per child, in the children's order.
//!
//! The 2D freeform types use gradient band interpolation (each motion's influence falls off
//! linearly towards every other motion; the weights are then normalised), in Cartesian space or in
//! a polar space where angle and magnitude count separately. Simple Directional blends the two
//! motions whose directions bracket the input with an optional motion at the centre.

use std::f32::consts::PI;

use glam::Vec2;

/// Positions closer than this count as the same point.
const EPS: f32 = 1e-5;
/// How much an angle difference counts against a magnitude difference (Freeform Directional).
const DIRECTION_SCALE: f32 = 2.0;

/// 1D: the two children whose thresholds bracket `value` are mixed linearly; outside the range
/// the nearest end child plays alone.
pub fn simple_1d(thresholds: &[f32], value: f32) -> Vec<f32> {
    let n = thresholds.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    let mut order: Vec<usize> = (0..n).collect();
    order.sort_by(|a, b| thresholds[*a].total_cmp(&thresholds[*b]));
    let th = |i: usize| thresholds[order[i]];
    if n == 1 || value <= th(0) {
        out[order[0]] = 1.0;
        return out;
    }
    if value >= th(n - 1) {
        out[order[n - 1]] = 1.0;
        return out;
    }
    let i = (0..n - 1).find(|&i| value <= th(i + 1)).unwrap_or(n - 2);
    let span = th(i + 1) - th(i);
    let w = if span > 0.0 {
        (value - th(i)) / span
    } else {
        1.0
    };
    out[order[i]] += 1.0 - w;
    out[order[i + 1]] += w;
    out
}

/// Counter-clockwise angle from `a` to `b`, in [0, 2π).
fn ccw(a: Vec2, b: Vec2) -> f32 {
    (b.y.atan2(b.x) - a.y.atan2(a.x)).rem_euclid(2.0 * PI)
}

/// 2D Simple Directional: each motion is a direction (one may sit at the centre). The input is
/// split between the two directions on either side of it; the part of the input short of the
/// line between them goes to the centre motion. Without a centre motion the two are normalised.
pub fn simple_directional(points: &[Vec2], p: Vec2) -> Vec<f32> {
    let n = points.len();
    let mut out = vec![0.0; n];
    if n == 0 {
        return out;
    }
    let centre = points.iter().position(|q| q.length() < EPS);
    let mut dirs: Vec<usize> = (0..n)
        .filter(|&i| points[i].length() >= EPS && Some(i) != centre)
        .collect();
    if dirs.is_empty() {
        out[centre.unwrap_or(0)] = 1.0;
        return out;
    }
    if p.length() < EPS {
        match centre {
            Some(c) => out[c] = 1.0,
            None => dirs.iter().for_each(|&i| out[i] = 1.0 / dirs.len() as f32),
        }
        return out;
    }
    dirs.sort_by(|a, b| {
        let ang = |i: usize| points[i].y.atan2(points[i].x);
        ang(*a).total_cmp(&ang(*b))
    });
    // The wedge (a → b counter-clockwise) holding the input.
    let m = dirs.len();
    let (a, b) = (0..m)
        .map(|k| (dirs[k], dirs[(k + 1) % m]))
        .find(|&(a, b)| {
            let wedge = if a == b {
                2.0 * PI
            } else {
                ccw(points[a], points[b])
            };
            ccw(points[a], p) <= wedge + EPS
        })
        .unwrap_or((dirs[0], dirs[0]));
    let (pa, pb) = (points[a], points[b]);
    let wedge = if a == b { 2.0 * PI } else { ccw(pa, pb) };
    let mut parts: Vec<(usize, f32)> = Vec::new();
    if wedge < PI - EPS {
        // p = wa·A + wb·B inside the triangle (centre, A, B); beyond the A–B line both shrink.
        let det = pa.perp_dot(pb);
        let wa = p.perp_dot(pb) / det;
        let wb = pa.perp_dot(p) / det;
        let (wa, wb) = (wa.max(0.0), wb.max(0.0));
        let s = wa + wb;
        if s > 1.0 {
            parts.push((a, wa / s));
            parts.push((b, wb / s));
        } else {
            parts.push((a, wa));
            parts.push((b, wb));
        }
    } else {
        // A gap of half a turn or more: only the nearer direction applies, by projection.
        let near = if ccw(pa, p) <= ccw(p, pb) { a } else { b };
        let q = points[near];
        parts.push((near, (p.dot(q) / q.length_squared()).clamp(0.0, 1.0)));
    }
    let sum: f32 = parts.iter().map(|(_, w)| w).sum();
    match centre {
        Some(c) => {
            parts.iter().for_each(|&(i, w)| out[i] += w);
            out[c] += (1.0 - sum).max(0.0);
        }
        None if sum > EPS => parts.iter().for_each(|&(i, w)| out[i] += w / sum),
        None => {
            // Pointing away from the only direction: it still plays (there is nothing else).
            out[parts[0].0] = 1.0;
        }
    }
    out
}

/// Normalise influences into weights (all zero: the nearest point plays alone).
fn normalise(points: &[Vec2], p: Vec2, mut h: Vec<f32>) -> Vec<f32> {
    let sum: f32 = h.iter().sum();
    if sum > EPS {
        h.iter_mut().for_each(|w| *w /= sum);
    } else if let Some(i) =
        (0..points.len()).min_by(|a, b| points[*a].distance(p).total_cmp(&points[*b].distance(p)))
    {
        h.iter_mut().for_each(|w| *w = 0.0);
        h[i] = 1.0;
    }
    h
}

/// 2D Freeform Cartesian: gradient bands in plain x/y space. Good for motions that do not
/// represent directions (e.g. lean amount and aim height).
pub fn freeform_cartesian(points: &[Vec2], p: Vec2) -> Vec<f32> {
    let h = (0..points.len())
        .map(|i| {
            let pi = points[i];
            (0..points.len())
                .filter(|&j| j != i)
                .filter_map(|j| {
                    let d = points[j] - pi;
                    let len2 = d.length_squared();
                    (len2 > EPS * EPS).then(|| (1.0 - (p - pi).dot(d) / len2).clamp(0.0, 1.0))
                })
                .fold(1.0f32, f32::min)
        })
        .collect();
    normalise(points, p, h)
}

/// Signed angle from `a` to `b` (0 when either is the zero vector).
fn signed_angle(a: Vec2, b: Vec2) -> f32 {
    if a.length() < EPS || b.length() < EPS {
        return 0.0;
    }
    a.perp_dot(b).atan2(a.dot(b))
}

/// 2D Freeform Directional: gradient bands in polar space (magnitude difference relative to the
/// pair's mean magnitude, and angle). Motions represent directions and speeds, several motions
/// may share a direction (walk and run forward), and one may sit at the centre.
pub fn freeform_directional(points: &[Vec2], p: Vec2) -> Vec<f32> {
    let mag_p = p.length();
    let h = (0..points.len())
        .map(|i| {
            let pi = points[i];
            let mag_i = pi.length();
            let mut w = 1.0f32;
            for (j, &pj) in points.iter().enumerate() {
                if j == i {
                    continue;
                }
                let mag_j = pj.length();
                let mean = (mag_i + mag_j) * 0.5;
                if mean < EPS {
                    continue;
                }
                let ij = Vec2::new(
                    (mag_j - mag_i) / mean,
                    signed_angle(pi, pj) * DIRECTION_SCALE,
                );
                // A centre motion has no direction: only magnitude counts from it.
                let ip = Vec2::new(
                    (mag_p - mag_i) / mean,
                    signed_angle(pi, p) * DIRECTION_SCALE,
                );
                let len2 = ij.length_squared();
                if len2 < EPS * EPS {
                    continue;
                }
                w = w.min((1.0 - ip.dot(ij) / len2).clamp(0.0, 1.0));
            }
            w
        })
        .collect();
    normalise(points, p, h)
}

/// Direct: each child's weight is its own parameter (clamped at 0), optionally normalised so
/// the weights sum to 1.
pub fn direct(values: &[f32], normalize: bool) -> Vec<f32> {
    let mut out: Vec<f32> = values.iter().map(|v| v.max(0.0)).collect();
    let sum: f32 = out.iter().sum();
    if normalize && sum > EPS {
        out.iter_mut().for_each(|w| *w /= sum);
    }
    out
}
