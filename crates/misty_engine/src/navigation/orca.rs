//! Reciprocal collision avoidance (ORCA): each agent picks the velocity closest to the one it
//! wants that keeps it from touching any neighbour within a time horizon, assuming the
//! neighbour takes its share of the avoiding. Every pair gives a half-plane of allowed
//! velocities; a small 2D linear program finds the best velocity inside all of them (or, when
//! they can't all hold, the one that breaks them the least). Static obstacles are hard
//! constraints the agent avoids alone.

use glam::Vec2;

const EPS: f32 = 1e-5;

/// A half-plane of allowed velocities: left of `direction` through `point`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Line {
    pub point: Vec2,
    pub direction: Vec2,
}

fn det(a: Vec2, b: Vec2) -> f32 {
    a.x * b.y - a.y * b.x
}

/// One agent as ORCA sees it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Body {
    pub position: Vec2,
    pub velocity: Vec2,
    pub radius: f32,
}

/// The allowed half-plane for `a` against `b` over `horizon` seconds; `share` is the part of
/// the avoiding `a` does (0.5 = reciprocal, 1 = alone). `dt` resolves overlaps already there.
pub fn pair_line(a: &Body, b: &Body, horizon: f32, dt: f32, share: f32) -> Line {
    let rel_pos = b.position - a.position;
    let rel_vel = a.velocity - b.velocity;
    let dist_sq = rel_pos.length_squared();
    let r = a.radius + b.radius;
    let r_sq = r * r;
    let inv_h = 1.0 / horizon.max(EPS);
    let (direction, u);
    if dist_sq > r_sq {
        // Velocity obstacle: a cone cut off by a circle at the horizon.
        let w = rel_vel - rel_pos * inv_h;
        let w_len_sq = w.length_squared();
        let dot1 = w.dot(rel_pos);
        if dot1 < 0.0 && dot1 * dot1 > r_sq * w_len_sq {
            // Closest to the cut-off circle.
            let w_len = w_len_sq.sqrt();
            let unit = w / w_len.max(EPS);
            direction = Vec2::new(unit.y, -unit.x);
            u = unit * (r * inv_h - w_len);
        } else {
            // Closest to a leg of the cone.
            let leg = (dist_sq - r_sq).sqrt();
            direction = if det(rel_pos, w) > 0.0 {
                Vec2::new(
                    rel_pos.x * leg - rel_pos.y * r,
                    rel_pos.x * r + rel_pos.y * leg,
                ) / dist_sq
            } else {
                -Vec2::new(
                    rel_pos.x * leg + rel_pos.y * r,
                    -rel_pos.x * r + rel_pos.y * leg,
                ) / dist_sq
            };
            u = direction * rel_vel.dot(direction) - rel_vel;
        }
    } else {
        // Already touching: get apart within this step.
        let inv_dt = 1.0 / dt.max(EPS);
        let w = rel_vel - rel_pos * inv_dt;
        let w_len = w.length();
        let unit = if w_len > EPS {
            w / w_len
        } else {
            (-rel_pos).try_normalize().unwrap_or(Vec2::X)
        };
        direction = Vec2::new(unit.y, -unit.x);
        u = unit * (r * inv_dt - w_len);
    }
    Line {
        point: a.velocity + u * share,
        direction,
    }
}

fn program1(
    lines: &[Line],
    n: usize,
    radius: f32,
    opt: Vec2,
    dir_opt: bool,
    out: &mut Vec2,
) -> bool {
    let l = lines[n];
    let dot = l.point.dot(l.direction);
    let disc = dot * dot + radius * radius - l.point.length_squared();
    if disc < 0.0 {
        return false;
    }
    let sq = disc.sqrt();
    let (mut t_left, mut t_right) = (-dot - sq, -dot + sq);
    for other in &lines[..n] {
        let denom = det(l.direction, other.direction);
        let numer = det(other.direction, l.point - other.point);
        if denom.abs() <= EPS {
            if numer < 0.0 {
                return false;
            }
            continue;
        }
        let t = numer / denom;
        if denom >= 0.0 {
            t_right = t_right.min(t);
        } else {
            t_left = t_left.max(t);
        }
        if t_left > t_right {
            return false;
        }
    }
    *out = if dir_opt {
        if opt.dot(l.direction) > 0.0 {
            l.point + l.direction * t_right
        } else {
            l.point + l.direction * t_left
        }
    } else {
        let t = l.direction.dot(opt - l.point);
        l.point + l.direction * t.clamp(t_left, t_right)
    };
    true
}

/// Best velocity within `radius` satisfying all lines; returns the first line it fails on.
fn program2(lines: &[Line], radius: f32, opt: Vec2, dir_opt: bool, out: &mut Vec2) -> usize {
    *out = if dir_opt {
        opt * radius
    } else if opt.length_squared() > radius * radius {
        opt.normalize() * radius
    } else {
        opt
    };
    for i in 0..lines.len() {
        if det(lines[i].direction, lines[i].point - *out) > 0.0 {
            let keep = *out;
            if !program1(lines, i, radius, opt, dir_opt, out) {
                *out = keep;
                return i;
            }
        }
    }
    lines.len()
}

/// When the lines can't all hold: the velocity that breaks the soft ones least (the first
/// `hard` lines are obstacles and always hold).
fn program3(lines: &[Line], hard: usize, begin: usize, radius: f32, out: &mut Vec2) {
    let mut distance = 0.0;
    for i in begin..lines.len() {
        if det(lines[i].direction, lines[i].point - *out) <= distance {
            continue;
        }
        let mut proj: Vec<Line> = lines[..hard].to_vec();
        for j in hard..i {
            let d = det(lines[i].direction, lines[j].direction);
            let point = if d.abs() <= EPS {
                if lines[i].direction.dot(lines[j].direction) > 0.0 {
                    continue;
                }
                (lines[i].point + lines[j].point) * 0.5
            } else {
                lines[i].point
                    + lines[i].direction
                        * (det(lines[j].direction, lines[i].point - lines[j].point) / d)
            };
            let direction = (lines[j].direction - lines[i].direction).normalize_or_zero();
            proj.push(Line { point, direction });
        }
        let keep = *out;
        let opt = Vec2::new(-lines[i].direction.y, lines[i].direction.x);
        if program2(&proj, radius, opt, true, out) < proj.len() {
            *out = keep;
        }
        distance = det(lines[i].direction, lines[i].point - *out);
    }
}

/// The velocity (at most `max_speed`) closest to `preferred` allowed by `lines`; the first
/// `hard` lines (obstacles) must hold.
pub fn solve(lines: &[Line], hard: usize, max_speed: f32, preferred: Vec2) -> Vec2 {
    let mut v = Vec2::ZERO;
    let failed = program2(lines, max_speed, preferred, false, &mut v);
    if failed < lines.len() {
        program3(lines, hard, failed, max_speed, &mut v);
    }
    v
}

/// Avoidance settings of one agent.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Params {
    pub max_speed: f32,
    /// Seconds ahead other agents are avoided.
    pub horizon: f32,
    /// Seconds ahead obstacles are avoided.
    pub obstacle_horizon: f32,
    pub dt: f32,
}

/// A tiny, fixed nudge for agent `seed`'s preferred velocity: perfectly symmetric meetings
/// (two agents exactly head-on) would otherwise stall.
pub fn nudge(seed: u64) -> Vec2 {
    let h = seed.wrapping_mul(0x9E37_79B9_7F4A_7C15) >> 40;
    let a = (h % 6283) as f32 / 1000.0;
    Vec2::new(a.cos(), a.sin()) * 1e-3
}

/// New velocity for `me` among `neighbours` (body, share) and static obstacle points.
pub fn velocity(
    me: &Body,
    preferred: Vec2,
    neighbours: &[(Body, f32)],
    obstacles: &[Vec2],
    p: &Params,
) -> Vec2 {
    let (max_speed, horizon, obstacle_horizon, dt) =
        (p.max_speed, p.horizon, p.obstacle_horizon, p.dt);
    let mut lines = Vec::with_capacity(neighbours.len() + obstacles.len());
    for &p in obstacles {
        let b = Body {
            position: p,
            velocity: Vec2::ZERO,
            radius: 0.0,
        };
        lines.push(pair_line(me, &b, obstacle_horizon, dt, 1.0));
    }
    let hard = lines.len();
    for (b, share) in neighbours {
        if *share > 0.0 {
            lines.push(pair_line(me, b, horizon, dt, *share));
        }
    }
    solve(&lines, hard, max_speed, preferred)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Two streams of agents cross at right angles: nobody overlaps on the way and everyone
    /// arrives.
    #[test]
    fn crossing_agents_never_overlap() {
        let radius = 0.4;
        let speed = 1.5;
        let mut pos = Vec::new();
        let mut goals = Vec::new();
        for i in 0..5 {
            let o = i as f32 * 1.2 - 2.4;
            pos.push(Vec2::new(-8.0, o));
            goals.push(Vec2::new(8.0, o));
            pos.push(Vec2::new(o, -8.0));
            goals.push(Vec2::new(o, 8.0));
        }
        let n = pos.len();
        let mut vel = vec![Vec2::ZERO; n];
        let dt = 1.0 / 30.0;
        let mut min_gap = f32::MAX;
        for _ in 0..(30 * 30) {
            let bodies: Vec<Body> = (0..n)
                .map(|i| Body {
                    position: pos[i],
                    velocity: vel[i],
                    radius,
                })
                .collect();
            let mut next = vel.clone();
            for i in 0..n {
                let to = goals[i] - pos[i];
                let pref = if to.length() > speed * dt {
                    to.normalize() * speed + nudge(i as u64)
                } else {
                    to / dt
                };
                let neighbours: Vec<(Body, f32)> = (0..n)
                    .filter(|&j| j != i && pos[j].distance(pos[i]) < 10.0)
                    .map(|j| (bodies[j], 0.5))
                    .collect();
                let p = Params {
                    max_speed: speed,
                    horizon: 4.0,
                    obstacle_horizon: 1.0,
                    dt,
                };
                next[i] = velocity(&bodies[i], pref, &neighbours, &[], &p);
            }
            vel = next;
            for i in 0..n {
                pos[i] += vel[i] * dt;
            }
            for i in 0..n {
                for j in i + 1..n {
                    min_gap = min_gap.min(pos[i].distance(pos[j]) - 2.0 * radius);
                }
            }
        }
        assert!(min_gap > -0.02, "agents overlapped by {}", -min_gap);
        for i in 0..n {
            assert!(
                pos[i].distance(goals[i]) < 0.3,
                "agent {i} at {} not {}",
                pos[i],
                goals[i]
            );
        }
    }

    /// An agent heading straight for a static point goes round it.
    #[test]
    fn obstacles_are_hard_constraints() {
        let me = Body {
            position: Vec2::new(-2.0, 0.01),
            velocity: Vec2::new(1.0, 0.0),
            radius: 0.5,
        };
        let p = Params {
            max_speed: 1.0,
            horizon: 2.0,
            obstacle_horizon: 2.0,
            dt: 0.1,
        };
        let v = velocity(&me, Vec2::new(1.0, 0.0), &[], &[Vec2::ZERO], &p);
        assert!(v.y.abs() > 0.1 || v.x < 0.8, "{v}");
        let free = velocity(&me, Vec2::new(0.0, 1.0), &[], &[Vec2::new(5.0, 5.0)], &p);
        assert!((free - Vec2::new(0.0, 1.0)).length() < 1e-4, "{free}");
    }
}
