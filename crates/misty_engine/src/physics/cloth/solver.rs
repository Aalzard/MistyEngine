//! The XPBD cloth solver: particles welded from the mesh vertices, distance (stretch) and
//! bending constraints, long-range attachments (tethers), max distances from the skinned
//! mesh, sphere and capsule collisions, aerodynamic wind and optional self-collision.
//!
//! Each step runs small sub-steps with one constraint pass each ("small steps" XPBD): predict
//! positions from velocities and forces, project the constraints with their compliance, then
//! derive the velocities from the moved positions. Pure data in, data out: no scene access, so
//! it runs on a worker thread and in tests.

use std::collections::{HashMap, HashSet};

use glam::Vec3;

use crate::render::mesh::MeshData;

/// Cloth weight per square metre (kg/m²): light cotton.
const DENSITY: f32 = 0.25;
/// Air density × drag coefficient for the wind force across the cloth.
const AIR_DRAG: f32 = 1.2;
/// Share of that drag along the cloth (skin friction): what makes a flag stream out in wind
/// blowing along it.
const TANGENT_DRAG: f32 = 0.12;
/// Compliance (m/N) of a stiffness of 0.5; stiffness 1 is rigid.
const STRETCH_COMPLIANCE: f32 = 0.05;
const BEND_COMPLIANCE: f32 = 0.5;
/// Positions closer than this are one particle (UV seams split vertices).
const WELD: f32 = 1e-5;

/// A distance constraint between two particles.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    pub a: u32,
    pub b: u32,
    pub rest: f32,
}

/// A free particle may not get further than `rest` from its pinned anchor particle.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tether {
    pub particle: u32,
    pub anchor: u32,
    pub rest: f32,
}

/// The cloth's structure, built once from the mesh.
#[derive(Clone, Debug, Default)]
pub struct Topology {
    /// Mesh vertex → particle.
    pub vertex_particle: Vec<u32>,
    /// Particle → its first mesh vertex.
    pub particle_vertex: Vec<u32>,
    /// Particle positions in the mesh (mesh space).
    pub rest: Vec<Vec3>,
    pub triangles: Vec<[u32; 3]>,
    pub stretch: Vec<Edge>,
    pub bend: Vec<Edge>,
    pub tethers: Vec<Tether>,
    /// Per particle: max distance from its skinned position (negative = free).
    pub max_distance: Vec<f32>,
    /// Per particle: 1 / mass (0 = pinned).
    pub inv_mass: Vec<f32>,
    /// Particle pairs joined by an edge (skipped by self-collision).
    pub linked: HashSet<(u32, u32)>,
}

fn key(a: u32, b: u32) -> (u32, u32) {
    (a.min(b), a.max(b))
}

impl Topology {
    /// Build from `mesh` (with `scale` = the renderer's world scale, so lengths are in metres)
    /// and the per-vertex max distances (missing = free).
    pub fn build(mesh: &MeshData, max_distances: &[f32], scale: Vec3) -> Self {
        let mut t = Topology::default();
        // Weld vertices at the same position.
        let mut cells: HashMap<[i64; 3], u32> = HashMap::new();
        for (i, v) in mesh.vertices.iter().enumerate() {
            let p = Vec3::from(v.position) * scale;
            let q = (p / WELD).round();
            let k = [q.x as i64, q.y as i64, q.z as i64];
            let id = *cells.entry(k).or_insert_with(|| {
                t.rest.push(p);
                t.particle_vertex.push(i as u32);
                t.max_distance.push(f32::INFINITY);
                (t.rest.len() - 1) as u32
            });
            t.vertex_particle.push(id);
            // A welded particle takes the strictest max distance of its vertices.
            let d = max_distances.get(i).copied().unwrap_or(-1.0);
            let slot = &mut t.max_distance[id as usize];
            if d >= 0.0 {
                *slot = if slot.is_finite() { slot.min(d) } else { d };
            }
        }
        for d in &mut t.max_distance {
            if !d.is_finite() {
                *d = -1.0;
            }
        }
        let n = t.rest.len();
        // Triangles, masses from their areas, edges and the triangle on each side of an edge.
        let mut mass = vec![0.0f32; n];
        let mut edge_tris: HashMap<(u32, u32), Vec<u32>> = HashMap::new();
        for tri in mesh.indices.as_chunks::<3>().0 {
            let p = [
                t.vertex_particle[tri[0] as usize],
                t.vertex_particle[tri[1] as usize],
                t.vertex_particle[tri[2] as usize],
            ];
            if p[0] == p[1] || p[1] == p[2] || p[0] == p[2] {
                continue;
            }
            let (a, b, c) = (
                t.rest[p[0] as usize],
                t.rest[p[1] as usize],
                t.rest[p[2] as usize],
            );
            let area = (b - a).cross(c - a).length() * 0.5;
            for k in p {
                mass[k as usize] += area * DENSITY / 3.0;
            }
            for (x, y, z) in [(p[0], p[1], p[2]), (p[1], p[2], p[0]), (p[2], p[0], p[1])] {
                edge_tris.entry(key(x, y)).or_default().push(z);
            }
            t.triangles.push(p);
        }
        let mut edges: Vec<(u32, u32)> = edge_tris.keys().copied().collect();
        edges.sort_unstable();
        for (a, b) in &edges {
            let rest = (t.rest[*a as usize] - t.rest[*b as usize]).length();
            t.stretch.push(Edge { a: *a, b: *b, rest });
            t.linked.insert((*a, *b));
            // Bending: the two vertices opposite a shared edge keep their distance.
            if let Some(opp) = edge_tris.get(&(*a, *b))
                && opp.len() == 2
                && opp[0] != opp[1]
            {
                let (c, d) = (opp[0], opp[1]);
                let rest = (t.rest[c as usize] - t.rest[d as usize]).length();
                t.bend.push(Edge { a: c, b: d, rest });
            }
        }
        let floor = mass
            .iter()
            .copied()
            .filter(|m| *m > 0.0)
            .fold(f32::MAX, f32::min);
        let floor = if floor == f32::MAX { 1e-3 } else { floor };
        t.inv_mass = (0..n)
            .map(|i| {
                if t.max_distance[i] == 0.0 {
                    0.0
                } else {
                    1.0 / mass[i].max(floor * 0.1)
                }
            })
            .collect();
        t.build_tethers();
        t
    }

    /// Each free particle tethered to its nearest pinned particle (by distance in the mesh).
    fn build_tethers(&mut self) {
        let pinned: Vec<u32> = (0..self.rest.len() as u32)
            .filter(|i| self.inv_mass[*i as usize] == 0.0)
            .collect();
        if pinned.is_empty() {
            return;
        }
        for i in 0..self.rest.len() as u32 {
            if self.inv_mass[i as usize] == 0.0 {
                continue;
            }
            let p = self.rest[i as usize];
            let (anchor, rest) = pinned
                .iter()
                .map(|a| (*a, (self.rest[*a as usize] - p).length()))
                .fold((0, f32::MAX), |best, x| if x.1 < best.1 { x } else { best });
            self.tethers.push(Tether {
                particle: i,
                anchor,
                rest,
            });
        }
    }

    pub fn particle_count(&self) -> usize {
        self.rest.len()
    }
}

/// A collision shape in world space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Shape {
    Sphere { center: Vec3, radius: f32 },
    Capsule { a: Vec3, b: Vec3, radius: f32 },
}

impl Shape {
    /// Push `p` out of the shape (grown by `thickness`): the contact normal and the new point.
    fn push_out(&self, p: Vec3, thickness: f32) -> Option<(Vec3, Vec3)> {
        let (closest, radius) = match *self {
            Shape::Sphere { center, radius } => (center, radius),
            Shape::Capsule { a, b, radius } => {
                let ab = b - a;
                let t = ((p - a).dot(ab) / ab.length_squared().max(1e-12)).clamp(0.0, 1.0);
                (a + ab * t, radius)
            }
        };
        let r = radius + thickness;
        let d = p - closest;
        let len = d.length();
        if len >= r {
            return None;
        }
        let n = if len > 1e-7 { d / len } else { Vec3::Y };
        Some((n, closest + n * r))
    }
}

/// The scene's wind as the cloth feels it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WindInput {
    /// Direction × speed (m/s).
    pub velocity: Vec3,
    /// 0+: fast swirls (fraction of the speed).
    pub turbulence: f32,
    /// 0+: gust strength (fraction of the speed).
    pub pulse_magnitude: f32,
    /// Gusts per second.
    pub pulse_frequency: f32,
}

impl WindInput {
    /// Wind velocity at `p` and time `t` (smooth, deterministic).
    pub fn at(&self, p: Vec3, t: f32) -> Vec3 {
        let speed = self.velocity.length();
        if speed < 1e-6 {
            return Vec3::ZERO;
        }
        let dir = self.velocity / speed;
        let along = p.dot(dir);
        let gust = 1.0
            + self.pulse_magnitude
                * 0.5
                * (t * self.pulse_frequency * std::f32::consts::TAU - along * 0.4).sin();
        let swirl = Vec3::new(
            (t * 5.3 + p.x * 2.1 + p.y * 0.7).sin(),
            (t * 4.1 + p.y * 1.7 + p.z * 0.9 + 1.0).sin() * 0.6,
            (t * 6.7 + p.z * 2.3 + p.x * 1.1 + 2.0).sin(),
        );
        dir * speed * gust.max(0.0) + swirl * (speed * self.turbulence * 0.35)
    }
}

/// Settings of one step.
#[derive(Clone, Debug, PartialEq)]
pub struct StepParams {
    pub dt: f32,
    pub substeps: u32,
    pub gravity: Vec3,
    /// Constant + random acceleration (m/s²).
    pub acceleration: Vec3,
    pub random_acceleration: Vec3,
    pub stretching_stiffness: f32,
    pub bending_stiffness: f32,
    pub tethers: bool,
    /// Velocity lost per second (0–1).
    pub damping: f32,
    pub friction: f32,
    pub thickness: f32,
    pub self_collision: Option<f32>,
    pub wind: WindInput,
    /// Simulation time at the start of the step (wind gusts).
    pub time: f32,
    /// Random seed for this step.
    pub seed: u32,
}

impl Default for StepParams {
    fn default() -> Self {
        Self {
            dt: 1.0 / 60.0,
            substeps: 4,
            gravity: Vec3::new(0.0, -9.81, 0.0),
            acceleration: Vec3::ZERO,
            random_acceleration: Vec3::ZERO,
            stretching_stiffness: 1.0,
            bending_stiffness: 0.5,
            tethers: true,
            damping: 0.1,
            friction: 0.5,
            thickness: 0.02,
            self_collision: None,
            wind: WindInput::default(),
            time: 0.0,
            seed: 1,
        }
    }
}

/// Particle positions and velocities (world space).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct State {
    pub x: Vec<Vec3>,
    pub v: Vec<Vec3>,
}

impl State {
    /// At rest on the targets.
    pub fn new(targets: &[Vec3]) -> Self {
        Self {
            x: targets.to_vec(),
            v: vec![Vec3::ZERO; targets.len()],
        }
    }

    /// Kinetic energy with the topology's masses (pinned particles left out).
    pub fn kinetic_energy(&self, t: &Topology) -> f32 {
        self.v
            .iter()
            .zip(&t.inv_mass)
            .filter(|(_, w)| **w > 0.0)
            .map(|(v, w)| 0.5 * v.length_squared() / w)
            .sum()
    }

    /// Largest edge length / rest length.
    pub fn max_stretch(&self, t: &Topology) -> f32 {
        t.stretch
            .iter()
            .filter(|e| e.rest > 1e-6)
            .map(|e| (self.x[e.a as usize] - self.x[e.b as usize]).length() / e.rest)
            .fold(0.0, f32::max)
    }
}

/// XPBD distance projection; `alpha` = compliance / h².
fn solve_edge(x: &mut [Vec3], w: &[f32], e: &Edge, alpha: f32) {
    let (a, b) = (e.a as usize, e.b as usize);
    let wsum = w[a] + w[b];
    if wsum <= 0.0 {
        return;
    }
    let d = x[a] - x[b];
    let len = d.length();
    if len < 1e-9 {
        return;
    }
    let c = len - e.rest;
    let dl = -c / (wsum + alpha);
    let n = d / len;
    x[a] += n * (dl * w[a]);
    x[b] -= n * (dl * w[b]);
}

/// Compliance of a 0–1 stiffness (1 = rigid).
fn compliance(stiffness: f32, base: f32) -> f32 {
    let s = stiffness.clamp(1e-3, 1.0);
    (1.0 / s - 1.0) * base
}

fn hash(mut v: u32) -> u32 {
    v ^= v >> 16;
    v = v.wrapping_mul(0x7feb_352d);
    v ^= v >> 15;
    v = v.wrapping_mul(0x846c_a68b);
    v ^ (v >> 16)
}

/// -1..1 per particle and axis.
fn random(seed: u32, i: u32, axis: u32) -> f32 {
    let h = hash(seed.wrapping_mul(0x9e37_79b9) ^ i.wrapping_mul(0x85eb_ca6b) ^ axis);
    h as f32 / u32::MAX as f32 * 2.0 - 1.0
}

/// Advance `state` by `p.dt`. `from` / `to` are each particle's skinned target at the start and
/// end of the step (max distances and pins follow them).
pub fn step(
    t: &Topology,
    state: &mut State,
    from: &[Vec3],
    to: &[Vec3],
    shapes: &[Shape],
    p: &StepParams,
) {
    let n = t.particle_count();
    if n == 0 || state.x.len() != n {
        return;
    }
    let steps = p.substeps.max(1);
    let h = p.dt / steps as f32;
    if h <= 0.0 {
        return;
    }
    let w = &t.inv_mass;
    let stretch_alpha = compliance(p.stretching_stiffness, STRETCH_COMPLIANCE) / (h * h);
    let bend_alpha = compliance(p.bending_stiffness, BEND_COMPLIANCE) / (h * h);
    let damping = (1.0 - p.damping.clamp(0.0, 1.0)).powf(h);
    let mut prev = vec![Vec3::ZERO; n];
    let mut wind_dv = vec![Vec3::ZERO; n];
    for s in 0..steps {
        let k = (s + 1) as f32 / steps as f32;
        let target = |i: usize| from[i].lerp(to[i], k);
        let time = p.time + h * s as f32;
        // Wind on each triangle: drag along its normal, shared by its corners.
        wind_dv.iter_mut().for_each(|v| *v = Vec3::ZERO);
        if p.wind.velocity != Vec3::ZERO {
            for tri in &t.triangles {
                let [a, b, c] = tri.map(|i| i as usize);
                let cross = (state.x[b] - state.x[a]).cross(state.x[c] - state.x[a]);
                let area2 = cross.length();
                if area2 < 1e-12 {
                    continue;
                }
                let normal = cross / area2;
                let centre = (state.x[a] + state.x[b] + state.x[c]) / 3.0;
                let vel = (state.v[a] + state.v[b] + state.v[c]) / 3.0;
                let rel = p.wind.at(centre, time) - vel;
                let along = rel.dot(normal);
                let tangent = rel - normal * along;
                let area = area2 * 0.5;
                let force = normal * (AIR_DRAG * 0.5 * area * along * along.abs())
                    + tangent * (AIR_DRAG * 0.5 * area * TANGENT_DRAG * tangent.length());
                for i in [a, b, c] {
                    wind_dv[i] += force / 3.0;
                }
            }
        }
        // Predict.
        for i in 0..n {
            prev[i] = state.x[i];
            if w[i] == 0.0 {
                state.x[i] = target(i);
                continue;
            }
            let rnd = Vec3::new(
                random(p.seed.wrapping_add(s), i as u32, 0),
                random(p.seed.wrapping_add(s), i as u32, 1),
                random(p.seed.wrapping_add(s), i as u32, 2),
            ) * p.random_acceleration;
            let acc = p.gravity + p.acceleration + rnd;
            state.v[i] += (acc + wind_dv[i] * w[i]) * h;
            state.x[i] += state.v[i] * h;
        }
        // Constraints.
        for e in &t.stretch {
            solve_edge(&mut state.x, w, e, stretch_alpha);
        }
        for e in &t.bend {
            solve_edge(&mut state.x, w, e, bend_alpha);
        }
        if p.tethers {
            for th in &t.tethers {
                let (i, a) = (th.particle as usize, th.anchor as usize);
                let d = state.x[i] - state.x[a];
                let len = d.length();
                let limit = th.rest * 1.02;
                if len > limit {
                    state.x[i] = state.x[a] + d * (limit / len);
                }
            }
        }
        if let Some(dist) = p.self_collision {
            self_collide(t, &mut state.x, dist.max(1e-4));
        }
        for i in 0..n {
            if w[i] == 0.0 {
                continue;
            }
            let md = t.max_distance[i];
            if md > 0.0 {
                let tg = target(i);
                let d = state.x[i] - tg;
                let len = d.length();
                if len > md {
                    state.x[i] = tg + d * (md / len);
                }
            }
            for shape in shapes {
                if let Some((normal, out)) = shape.push_out(state.x[i], p.thickness) {
                    // Friction: lose part of this sub-step's sliding along the surface.
                    let moved = out - prev[i];
                    let slide = moved - normal * moved.dot(normal);
                    state.x[i] = out - slide * p.friction.clamp(0.0, 1.0);
                }
            }
        }
        // Velocities from the moved positions.
        for i in 0..n {
            state.v[i] = if w[i] == 0.0 {
                (state.x[i] - prev[i]) / h
            } else {
                (state.x[i] - prev[i]) / h * damping
            };
        }
    }
}

/// Keep particles that aren't linked by an edge `dist` apart (spatial hash).
fn self_collide(t: &Topology, x: &mut [Vec3], dist: f32) {
    let cell = |p: Vec3| {
        let q = (p / dist).floor();
        (q.x as i32, q.y as i32, q.z as i32)
    };
    let mut grid: HashMap<(i32, i32, i32), Vec<u32>> = HashMap::new();
    for (i, p) in x.iter().enumerate() {
        grid.entry(cell(*p)).or_default().push(i as u32);
    }
    let w = &t.inv_mass;
    for i in 0..x.len() {
        let c = cell(x[i]);
        for dx in -1..=1 {
            for dy in -1..=1 {
                for dz in -1..=1 {
                    let Some(list) = grid.get(&(c.0 + dx, c.1 + dy, c.2 + dz)) else {
                        continue;
                    };
                    for &j in list {
                        let j = j as usize;
                        if j <= i || t.linked.contains(&key(i as u32, j as u32)) {
                            continue;
                        }
                        let wsum = w[i] + w[j];
                        if wsum <= 0.0 {
                            continue;
                        }
                        let d = x[i] - x[j];
                        let len = d.length();
                        if len >= dist || len < 1e-9 {
                            continue;
                        }
                        let push = d / len * (dist - len) / wsum;
                        x[i] += push * w[i];
                        x[j] -= push * w[j];
                    }
                }
            }
        }
    }
}

/// Area-weighted particle normals.
pub fn normals(t: &Topology, x: &[Vec3]) -> Vec<Vec3> {
    let mut out = vec![Vec3::ZERO; x.len()];
    for tri in &t.triangles {
        let [a, b, c] = tri.map(|i| i as usize);
        let n = (x[b] - x[a]).cross(x[c] - x[a]);
        out[a] += n;
        out[b] += n;
        out[c] += n;
    }
    out.iter().map(|n| n.normalize_or(Vec3::Y)).collect()
}

#[cfg(test)]
#[path = "solver_tests.rs"]
mod tests;
