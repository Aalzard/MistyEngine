//! Nav Mesh Agents while playing: each agent keeps a position on the mesh, a path to its
//! destination and a velocity. Every frame it steers to the next corner (accelerating, braking
//! before the end), is pushed apart from other agents and obstacles (simple separation), moves
//! along the mesh surface and writes its Transform (base offset above the surface, turning
//! towards where it moves).

use std::collections::HashMap;
use std::path::Path;
use std::sync::Arc;

use glam::{Quat, Vec2, Vec3, Vec3Swizzles};
use hecs::Entity;
use serde::{Deserialize, Serialize};

use super::{NavMesh, NavMeshAgent, NavMeshObstacle, NavPath, ObstacleShape, PathStatus};
use crate::scene::Scene;

/// Avoidance quality; every level except None uses the same separation steering here.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum ObstacleAvoidance {
    None,
    LowQuality,
    MediumQuality,
    GoodQuality,
    #[default]
    HighQuality,
}

impl ObstacleAvoidance {
    pub const ALL: [ObstacleAvoidance; 5] = [
        Self::None,
        Self::LowQuality,
        Self::MediumQuality,
        Self::GoodQuality,
        Self::HighQuality,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::None => "None",
            Self::LowQuality => "Low Quality",
            Self::MediumQuality => "Medium Quality",
            Self::GoodQuality => "Good Quality",
            Self::HighQuality => "High Quality",
        }
    }
}

/// Corners closer than this are treated as reached.
const CORNER_REACHED: f32 = 0.1;

/// Live state of one agent.
#[derive(Clone, Debug, Default)]
pub struct AgentState {
    /// Position on the nav mesh surface (without the base offset).
    pub position: Vec3,
    poly: u32,
    pub on_mesh: bool,
    /// Requested destination, snapped onto the mesh.
    pub destination: Vec3,
    /// Remaining corners (the first is the next one to walk to).
    pub corners: Vec<Vec3>,
    pub status: PathStatus,
    pub velocity: Vec3,
    pub is_stopped: bool,
    /// Transform position written last frame (a script moving it means a warp).
    written: Vec3,
}

impl AgentState {
    pub fn has_path(&self) -> bool {
        !self.corners.is_empty()
    }

    /// Distance left along the path (0 without a path).
    pub fn remaining_distance(&self) -> f32 {
        let Some(first) = self.corners.first() else {
            return 0.0;
        };
        self.position.distance(*first)
            + self
                .corners
                .windows(2)
                .map(|w| w[0].distance(w[1]))
                .sum::<f32>()
    }
}

/// All agents of a running game plus the scene's nav mesh.
#[derive(Default)]
pub struct NavRuntime {
    mesh: Option<Arc<NavMesh>>,
    agents: HashMap<Entity, AgentState>,
    logs: Vec<String>,
}

fn agent_of(scene: &Scene, e: Entity) -> Option<NavMeshAgent> {
    if !scene.contains(e) || !scene.active_in_hierarchy(e) {
        return None;
    }
    let a = scene.world.get::<&NavMeshAgent>(e).ok()?;
    a.enabled.then(|| (*a).clone())
}

/// Move `v` towards `target` by at most `step`.
fn move_towards(v: Vec3, target: Vec3, step: f32) -> Vec3 {
    let d = target - v;
    let len = d.length();
    if len <= step || len < 1e-6 {
        target
    } else {
        v + d / len * step
    }
}

/// Yaw (degrees) whose forward is `dir` (x/z).
fn yaw_of(dir: Vec2) -> f32 {
    dir.x.atan2(dir.y).to_degrees()
}

impl NavRuntime {
    pub fn new(mesh: Option<Arc<NavMesh>>) -> Self {
        Self {
            mesh,
            ..Default::default()
        }
    }

    /// The scene's baked nav mesh (`scene.navigation.nav_mesh`, relative to `root`).
    pub fn load(scene: &Scene, root: &Path) -> Self {
        let Some(rel) = scene.navigation.nav_mesh.as_ref() else {
            return Self::default();
        };
        match super::store::load(&root.join(rel)) {
            Ok(m) => Self::new(Some(m)),
            Err(e) => Self {
                logs: vec![format!("Navigation: {e}")],
                ..Default::default()
            },
        }
    }

    pub fn mesh(&self) -> Option<&Arc<NavMesh>> {
        self.mesh.as_ref()
    }

    /// Console messages since the last call.
    pub fn take_logs(&mut self) -> Vec<String> {
        std::mem::take(&mut self.logs)
    }

    /// State of `e`'s agent, placing it on the mesh the first time.
    pub fn agent(&mut self, scene: &Scene, e: Entity) -> Option<&mut AgentState> {
        let comp = agent_of(scene, e)?;
        if !self.agents.contains_key(&e) {
            let mut st = AgentState::default();
            let at = scene.world_position(e) - Vec3::Y * comp.base_offset;
            if !self.place(&mut st, at) {
                self.logs.push(format!(
                    "'{}': failed to create agent because it is not close enough to the NavMesh",
                    scene.name(e)
                ));
            }
            st.written = scene.world_position(e);
            self.agents.insert(e, st);
        }
        self.agents.get_mut(&e)
    }

    /// Read-only state (None before the agent's first frame).
    pub fn state(&self, e: Entity) -> Option<&AgentState> {
        self.agents.get(&e)
    }

    /// Snap `st` onto the mesh near `at`.
    fn place(&self, st: &mut AgentState, at: Vec3) -> bool {
        let hit = self.mesh.as_ref().and_then(|m| {
            let reach = m.agent.height.max(m.cell_size * 6.0);
            m.nearest(at, reach)
        });
        st.on_mesh = hit.is_some();
        if let Some(h) = hit {
            st.position = h.position;
            st.poly = h.poly;
        }
        st.corners.clear();
        st.velocity = Vec3::ZERO;
        st.on_mesh
    }

    /// `SetDestination`: plan a path; false when the agent isn't on the mesh.
    pub fn set_destination(&mut self, scene: &Scene, e: Entity, target: Vec3) -> bool {
        let Some(mesh) = self.mesh.clone() else {
            self.logs.push(format!(
                "'{}': SetDestination needs a baked NavMesh (Window ▸ AI ▸ Navigation)",
                scene.name(e)
            ));
            return false;
        };
        let Some(st) = self.agent(scene, e) else {
            return false;
        };
        if !st.on_mesh {
            return false;
        }
        let path = mesh.find_path(st.position, target);
        st.status = path.status;
        if path.status == PathStatus::Invalid {
            st.corners.clear();
            return false;
        }
        st.destination = path.corners.last().copied().unwrap_or(st.position);
        st.corners = path.corners.into_iter().skip(1).collect();
        true
    }

    pub fn reset_path(&mut self, scene: &Scene, e: Entity) {
        if let Some(st) = self.agent(scene, e) {
            st.corners.clear();
        }
    }

    /// `Warp`: jump to the mesh point nearest `pos` (clears the path).
    pub fn warp(&mut self, scene: &mut Scene, e: Entity, pos: Vec3) -> bool {
        let Some(comp) = agent_of(scene, e) else {
            return false;
        };
        let mut st = self.agent(scene, e).cloned().unwrap_or_default();
        let ok = self.place(&mut st, pos);
        if ok {
            let world = st.position + Vec3::Y * comp.base_offset;
            scene.set_world_position(e, world);
            st.written = world;
        }
        self.agents.insert(e, st);
        ok
    }

    /// Corners still to walk, starting at the agent (for the editor's path gizmo).
    pub fn path_of(&self, e: Entity) -> Option<Vec<Vec3>> {
        let st = self.agents.get(&e)?;
        st.has_path().then(|| {
            std::iter::once(st.position)
                .chain(st.corners.iter().copied())
                .collect()
        })
    }

    /// A path between two points (`NavMesh.CalculatePath`).
    pub fn calculate_path(&self, a: Vec3, b: Vec3) -> NavPath {
        self.mesh
            .as_ref()
            .map_or_else(NavPath::invalid, |m| m.find_path(a, b))
    }

    /// Move every agent by one frame.
    pub fn update(&mut self, scene: &mut Scene, dt: f32) {
        let Some(mesh) = self.mesh.clone() else {
            return;
        };
        let agents: Vec<(Entity, NavMeshAgent)> = scene
            .walk()
            .into_iter()
            .filter_map(|(e, _)| agent_of(scene, e).map(|a| (e, a)))
            .collect();
        self.agents
            .retain(|e, _| agents.iter().any(|(a, _)| a == e));
        for (e, _) in &agents {
            self.agent(scene, *e);
        }
        if dt <= 0.0 {
            return;
        }
        let obstacles = obstacles(scene);
        let others: Vec<(Entity, Vec3, f32, i32)> = agents
            .iter()
            .filter(|(_, a)| a.obstacle_avoidance != ObstacleAvoidance::None)
            .filter_map(|(e, a)| {
                let st = self.agents.get(e)?;
                st.on_mesh
                    .then_some((*e, st.position, a.radius, a.priority))
            })
            .collect();
        for (e, comp) in &agents {
            let Some(mut st) = self.agents.get(e).cloned() else {
                continue;
            };
            // A script moved the object: start from there.
            let now = scene.world_position(*e);
            if now.distance(st.written) > 1e-3 {
                self.place(&mut st, now - Vec3::Y * comp.base_offset);
            }
            if st.on_mesh {
                let push = if comp.obstacle_avoidance == ObstacleAvoidance::None {
                    Vec3::ZERO
                } else {
                    separation(*e, &st, comp, &others, &obstacles)
                };
                step(&mesh, &mut st, comp, push, dt);
                write_transform(scene, *e, &mut st, comp, dt);
            }
            self.agents.insert(*e, st);
        }
        self.resolve_overlaps(scene, &mesh, &agents, &obstacles);
    }

    /// Push overlapping agents apart (and out of obstacles), staying on the mesh.
    fn resolve_overlaps(
        &mut self,
        scene: &mut Scene,
        mesh: &NavMesh,
        agents: &[(Entity, NavMeshAgent)],
        obstacles: &[Obstacle],
    ) {
        let avoiding: Vec<&(Entity, NavMeshAgent)> = agents
            .iter()
            .filter(|(e, a)| {
                a.obstacle_avoidance != ObstacleAvoidance::None
                    && self.agents.get(e).is_some_and(|s| s.on_mesh)
            })
            .collect();
        let mut offsets: HashMap<Entity, Vec3> = HashMap::new();
        for (i, (ea, a)) in avoiding.iter().enumerate() {
            let pa = self.agents[ea].position;
            for (eb, b) in avoiding.iter().skip(i + 1) {
                let pb = self.agents[eb].position;
                let d = (pa - pb).xz();
                let overlap = a.radius + b.radius - d.length();
                if overlap <= 0.0 || (pa.y - pb.y).abs() > a.height.max(b.height) {
                    continue;
                }
                let dir = d.try_normalize().unwrap_or(Vec2::X);
                let (wa, wb) = priority_weights(a.priority, b.priority);
                let push = Vec3::new(dir.x, 0.0, dir.y) * overlap;
                *offsets.entry(*ea).or_default() += push * wa;
                *offsets.entry(*eb).or_default() -= push * wb;
            }
            for o in obstacles {
                if let Some(out) = o.push_out(pa, a.radius) {
                    *offsets.entry(*ea).or_default() += out;
                }
            }
        }
        for (e, off) in offsets {
            let (Some(st), Some((_, comp))) = (
                self.agents.get_mut(&e),
                agents.iter().find(|(a, _)| *a == e),
            ) else {
                continue;
            };
            let (poly, p) = mesh.move_along(st.poly, st.position, st.position + off);
            st.poly = poly;
            st.position = p;
            let world = p + Vec3::Y * comp.base_offset;
            scene.set_world_position(e, world);
            st.written = world;
        }
    }
}

/// Share of an overlap each agent moves: the less important one (higher number) moves more.
fn priority_weights(a: i32, b: i32) -> (f32, f32) {
    match a.cmp(&b) {
        std::cmp::Ordering::Less => (0.2, 0.8),
        std::cmp::Ordering::Greater => (0.8, 0.2),
        std::cmp::Ordering::Equal => (0.5, 0.5),
    }
}

/// Away from `from` plus a sideways part when it's ahead (so head-on meetings pass each other
/// instead of stalling); `strength` 0..1 grows with closeness.
fn avoid(pos: Vec3, from: Vec3, heading: Vec2, strength: f32) -> Vec2 {
    let away = (pos - from).xz().try_normalize().unwrap_or(Vec2::ZERO);
    let ahead = (from - pos).xz();
    let mut v = away;
    if heading != Vec2::ZERO && ahead.dot(heading) > 0.0 {
        // Pass on the side away from the other (right when dead ahead).
        let right = Vec2::new(heading.y, -heading.x);
        let lateral = ahead.dot(right);
        let side = if lateral > 0.2 * ahead.length() {
            -right
        } else {
            right
        };
        v += side;
    }
    v * strength
}

/// Steering away from nearby agents and obstacles (added to the desired velocity).
fn separation(
    me: Entity,
    st: &AgentState,
    comp: &NavMeshAgent,
    others: &[(Entity, Vec3, f32, i32)],
    obstacles: &[Obstacle],
) -> Vec3 {
    let heading = st
        .corners
        .first()
        .map_or(Vec2::ZERO, |c| (*c - st.position).xz().normalize_or_zero());
    let mut push = Vec2::ZERO;
    for (e, p, r, prio) in others {
        if *e == me || (p.y - st.position.y).abs() > comp.height {
            continue;
        }
        // Agents standing at their goal only react to near contact.
        let moving = st.has_path()
            && !st.is_stopped
            && st.remaining_distance() > comp.stopping_distance + 0.05;
        let range = (comp.radius + r) * if moving { 2.0 } else { 1.1 };
        let len = (st.position - *p).xz().length();
        if len >= range {
            continue;
        }
        let (w, _) = priority_weights(comp.priority, *prio);
        push += avoid(st.position, *p, heading, (1.0 - len / range) * w * 2.0);
    }
    for o in obstacles {
        let (c, dist) = o.closest(st.position);
        let range = comp.radius * 2.0 + 0.5;
        if dist < range {
            let center = Vec3::new(o.center.x, st.position.y, o.center.y);
            let from = if dist > 1e-5 { c } else { center };
            push += avoid(st.position, from, heading, 1.0 - dist / range);
        }
    }
    let push = push.clamp_length_max(1.0) * comp.speed;
    Vec3::new(push.x, 0.0, push.y)
}

/// Steer, accelerate and move one agent along its path on the mesh.
fn step(mesh: &NavMesh, st: &mut AgentState, comp: &NavMeshAgent, push: Vec3, dt: f32) {
    while st.corners.len() > 1 && st.position.xz().distance(st.corners[0].xz()) < CORNER_REACHED {
        st.corners.remove(0);
    }
    let mut desired = Vec3::ZERO;
    let remaining = st.remaining_distance();
    let mut arrive_at = None;
    if let Some(&target) = st.corners.first()
        && !st.is_stopped
    {
        let last = st.corners.len() == 1;
        if last && remaining <= comp.stopping_distance.max(0.0) + 1e-3 {
            // Arrived: an exact stop drops the path.
            if comp.stopping_distance <= 1e-3 {
                st.position = Vec3::new(target.x, st.position.y, target.z);
                st.corners.clear();
            }
            st.velocity = Vec3::ZERO;
        } else {
            let dir = (target - st.position).xz().normalize_or_zero();
            let mut speed = comp.speed.max(0.0);
            if comp.auto_braking {
                let left = (remaining - comp.stopping_distance).max(0.0);
                speed = speed.min((2.0 * comp.acceleration.max(0.01) * left).sqrt());
            }
            desired = Vec3::new(dir.x, 0.0, dir.y) * speed;
            if last {
                arrive_at = Some(target);
            }
        }
    }
    let desired = (desired + push).clamp_length_max(comp.speed.max(0.0));
    st.velocity = move_towards(st.velocity, desired, comp.acceleration.max(0.0) * dt);
    let mut to = st.position + st.velocity * dt;
    // Don't overshoot the destination.
    if let Some(end) = arrive_at {
        let left = (end - st.position).xz().length();
        if st.velocity.length() * dt >= left - comp.stopping_distance && left > 0.0 {
            to = if comp.stopping_distance > 0.0 {
                st.position + (to - st.position).clamp_length_max(left - comp.stopping_distance)
            } else {
                end
            };
        }
    }
    let (poly, p) = mesh.move_along(st.poly, st.position, to);
    st.poly = poly;
    st.position = p;
}

/// Place the object at the agent position (+ base offset) and turn it towards the velocity.
fn write_transform(
    scene: &mut Scene,
    e: Entity,
    st: &mut AgentState,
    comp: &NavMeshAgent,
    dt: f32,
) {
    let world = st.position + Vec3::Y * comp.base_offset;
    scene.set_world_position(e, world);
    st.written = world;
    let v = st.velocity.xz();
    if v.length() > 0.05 {
        let current = scene.world_rotation(e) * Vec3::Z;
        let from = yaw_of(current.xz().try_normalize().unwrap_or(Vec2::Y));
        let to = yaw_of(v);
        let diff = (to - from + 540.0).rem_euclid(360.0) - 180.0;
        let turn = diff.clamp(-comp.angular_speed * dt, comp.angular_speed * dt);
        let yaw = (from + turn).to_radians();
        scene.set_world_rotation(e, Quat::from_rotation_y(yaw));
    }
}

/// A Nav Mesh Obstacle's footprint on x/z.
pub(super) struct Obstacle {
    center: Vec2,
    /// Box axes (unit) and half sizes; a capsule is a circle (`radius`).
    axes: [Vec2; 2],
    half: Vec2,
    radius: f32,
    capsule: bool,
}

impl Obstacle {
    /// Closest point of the footprint to `p` (with p's height) and the distance to it.
    fn closest(&self, p: Vec3) -> (Vec3, f32) {
        let q = p.xz() - self.center;
        let c = if self.capsule {
            q.clamp_length_max(self.radius)
        } else {
            let (u, v) = (q.dot(self.axes[0]), q.dot(self.axes[1]));
            self.axes[0] * u.clamp(-self.half.x, self.half.x)
                + self.axes[1] * v.clamp(-self.half.y, self.half.y)
        };
        let world = Vec3::new(c.x + self.center.x, p.y, c.y + self.center.y);
        (world, (q - c).length())
    }

    /// Offset moving a circle of radius `r` at `p` out of the footprint (None = clear).
    fn push_out(&self, p: Vec3, r: f32) -> Option<Vec3> {
        let (c, dist) = self.closest(p);
        if dist >= r {
            return None;
        }
        let d = (p - c).xz();
        let dir = if dist > 1e-5 {
            d / dist
        } else {
            // Inside: leave through the nearest side.
            let q = p.xz() - self.center;
            q.try_normalize().unwrap_or(Vec2::X)
        };
        let depth = if dist > 1e-5 {
            r - dist
        } else {
            r + self.half.max_element().max(self.radius)
        };
        Some(Vec3::new(dir.x, 0.0, dir.y) * depth)
    }
}

/// Footprints of the enabled Nav Mesh Obstacles.
pub(super) fn obstacles(scene: &Scene) -> Vec<Obstacle> {
    scene
        .walk()
        .into_iter()
        .filter(|(e, _)| scene.active_in_hierarchy(*e))
        .filter_map(|(e, _)| {
            let o = scene.world.get::<&NavMeshObstacle>(e).ok()?;
            o.enabled.then(|| obstacle_footprint(scene, e, &o))
        })
        .collect()
}

fn obstacle_footprint(scene: &Scene, e: Entity, o: &NavMeshObstacle) -> Obstacle {
    let m = scene.world_matrix(e);
    let (scale, rot, _) = m.to_scale_rotation_translation();
    let center = m.transform_point3(o.center).xz();
    let x = (rot * Vec3::X).xz().try_normalize().unwrap_or(Vec2::X);
    Obstacle {
        center,
        axes: [x, Vec2::new(-x.y, x.x)],
        half: (o.size.xz() * scale.xz().abs() * 0.5).max(Vec2::ZERO),
        radius: o.radius * scale.x.abs().max(scale.z.abs()),
        capsule: o.shape == ObstacleShape::Capsule,
    }
}
