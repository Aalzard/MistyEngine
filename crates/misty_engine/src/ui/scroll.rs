//! Scroll Rect maths: content bounds against the viewport, the offset that brings content back,
//! rubber-banding, normalised positions and the elastic spring. Canvas units, y up.

use glam::Vec2;

use super::layout::Rect;
use crate::scene::MovementType;

/// Content rect grown to at least the viewport's size around the content pivot, so small
/// content stays put instead of jittering.
pub fn content_bounds(content: Rect, view: Rect, pivot: Vec2) -> Rect {
    let mut b = content;
    let diff = (view.size() - content.size()).max(Vec2::ZERO);
    b.min -= diff * pivot;
    b.max += diff * (Vec2::ONE - pivot);
    b
}

/// How far the content (moved by `delta`) must move back to stay inside its limits.
pub fn offset(bounds: Rect, view: Rect, delta: Vec2, horizontal: bool, vertical: bool) -> Vec2 {
    let mut off = Vec2::ZERO;
    for (axis, on) in [(0, horizontal), (1, vertical)] {
        if !on {
            continue;
        }
        let min = bounds.min[axis] + delta[axis];
        let max = bounds.max[axis] + delta[axis];
        if min > view.min[axis] {
            off[axis] = view.min[axis] - min;
        } else if max < view.max[axis] {
            off[axis] = view.max[axis] - max;
        }
    }
    off
}

/// Drag distance past the edge after rubber-banding (approaches the view size).
pub fn rubber(over: f32, view_size: f32) -> f32 {
    if view_size <= 0.0 {
        return 0.0;
    }
    (1.0 - 1.0 / (over.abs() * 0.55 / view_size + 1.0)) * view_size * over.signum()
}

/// 0..1 scroll position per axis (x: 0 = left, y: 1 = top).
pub fn normalized(bounds: Rect, view: Rect) -> Vec2 {
    let mut n = Vec2::ZERO;
    for axis in 0..2 {
        let span = bounds.size()[axis] - view.size()[axis];
        n[axis] = if span.abs() < 1e-3 {
            if view.min[axis] > bounds.min[axis] {
                1.0
            } else {
                0.0
            }
        } else {
            (view.min[axis] - bounds.min[axis]) / span
        };
    }
    n
}

/// Content move that puts `axis` at normalised position `n`.
pub fn delta_for(bounds: Rect, view: Rect, axis: usize, n: f32) -> f32 {
    let span = bounds.size()[axis] - view.size()[axis];
    let target_min = view.min[axis] - n * span;
    target_min - bounds.min[axis]
}

/// Critically damped spring towards `target`, settling in about `time` seconds.
pub fn smooth_damp(current: f32, target: f32, velocity: &mut f32, time: f32, dt: f32) -> f32 {
    let time = time.max(1e-4);
    let omega = 2.0 / time;
    let x = omega * dt;
    let decay = 1.0 / (1.0 + x + 0.48 * x * x + 0.235 * x * x * x);
    let change = current - target;
    let temp = (*velocity + omega * change) * dt;
    *velocity = (*velocity - omega * temp) * decay;
    let out = target + (change + temp) * decay;
    // Don't overshoot.
    if (target - current > 0.0) == (out > target) {
        *velocity = 0.0;
        return target;
    }
    out
}

/// Content move for this frame while not dragging: inertia, then the movement type's limits.
/// Returns (delta, new velocity).
#[allow(clippy::too_many_arguments)]
pub fn settle(
    bounds: Rect,
    view: Rect,
    velocity: Vec2,
    movement: MovementType,
    elasticity: f32,
    inertia: bool,
    deceleration: f32,
    axes: (bool, bool),
    dt: f32,
) -> (Vec2, Vec2) {
    let mut vel = velocity;
    let mut delta = Vec2::ZERO;
    let off = offset(bounds, view, Vec2::ZERO, axes.0, axes.1);
    for axis in 0..2 {
        if !(if axis == 0 { axes.0 } else { axes.1 }) {
            vel[axis] = 0.0;
            continue;
        }
        if movement == MovementType::Elastic && off[axis] != 0.0 {
            let mut v = vel[axis];
            let moved = smooth_damp(0.0, off[axis], &mut v, elasticity, dt);
            delta[axis] = moved;
            vel[axis] = if moved.abs() < 0.01 { 0.0 } else { v };
            if (off[axis] - moved).abs() < 0.1 {
                delta[axis] = off[axis];
                vel[axis] = 0.0;
            }
        } else if inertia {
            vel[axis] *= deceleration.clamp(0.0, 1.0).powf(dt);
            if vel[axis].abs() < 1.0 {
                vel[axis] = 0.0;
            }
            delta[axis] = vel[axis] * dt;
        } else {
            vel[axis] = 0.0;
        }
    }
    if movement == MovementType::Clamped {
        delta += offset(bounds, view, delta, axes.0, axes.1);
    }
    (delta, vel)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(x0: f32, y0: f32, x1: f32, y1: f32) -> Rect {
        Rect::new(Vec2::new(x0, y0), Vec2::new(x1, y1))
    }

    #[test]
    fn bounds_offsets_and_positions() {
        let view = r(0.0, 0.0, 100.0, 100.0);
        // Tall content with its top at the view's top.
        let content = r(0.0, -200.0, 100.0, 100.0);
        let b = content_bounds(content, view, Vec2::new(0.0, 1.0));
        assert_eq!(b, content);
        assert_eq!(normalized(b, view).y, 1.0);
        assert_eq!(offset(b, view, Vec2::new(0.0, -30.0), false, true).y, 30.0);
        assert_eq!(offset(b, view, Vec2::new(0.0, 250.0), false, true).y, -50.0);
        assert_eq!(delta_for(b, view, 1, 0.0), 200.0);
        // Small content is padded to the view size.
        let small = content_bounds(r(0.0, 50.0, 100.0, 100.0), view, Vec2::new(0.0, 1.0));
        assert_eq!(small, view);
        assert!(rubber(1000.0, 100.0) < 100.0 && rubber(-10.0, 100.0) < 0.0);
    }

    #[test]
    fn elastic_springs_back_and_clamped_stops() {
        let view = r(0.0, 0.0, 100.0, 100.0);
        // Content pulled 40 units below the top edge.
        let b = r(0.0, -240.0, 100.0, 60.0);
        let mut vel = Vec2::ZERO;
        let mut moved = 0.0;
        for _ in 0..120 {
            let bb = r(0.0, -240.0 + moved, 100.0, 60.0 + moved);
            let (d, v) = settle(
                bb,
                view,
                vel,
                MovementType::Elastic,
                0.1,
                true,
                0.135,
                (false, true),
                1.0 / 60.0,
            );
            moved += d.y;
            vel = v;
        }
        assert!((moved - 40.0).abs() < 0.01, "{moved}");
        let (d, _) = settle(
            b,
            view,
            Vec2::new(0.0, -500.0),
            MovementType::Clamped,
            0.1,
            true,
            0.135,
            (false, true),
            0.1,
        );
        assert!((d.y - 40.0).abs() < 1e-3, "clamped back into place: {d:?}");
        let mut v = 0.0;
        assert!((smooth_damp(0.0, 10.0, &mut v, 0.1, 10.0) - 10.0).abs() < 0.01);
    }
}
