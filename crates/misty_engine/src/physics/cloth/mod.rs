//! Cloth while playing: each object with a Cloth and a Skinned Mesh Renderer is simulated on
//! the background pool, one job per cloth per frame, and drawn through the skinning hook.
//!
//! **Why the CPU.** Cloths in a game scene are small (hundreds to a few thousand particles),
//! collide with scene colliders and wind that live on the CPU, and gameplay may read them; the
//! XPBD constraint passes are sequential per cloth, and running each cloth as its own job on the
//! background pool spreads several cloths over the cores. A compute shader would need the
//! colliders and wind uploaded, a graph-coloured constraint schedule and either a readback or a
//! render-side integration, and its work would land in the frame the renderer is already
//! budgeting. A solve of the sample flag takes well under a millisecond on one worker.
//!
//! **Never stalling the main thread.** Each frame the main thread only collects a finished job
//! (if any), publishes its vertices, snapshots the inputs (skin matrices, colliders, wind) and
//! starts the next job. A job that is still running is left alone: its time is added to the next
//! one (clamped), so the cloth lags a frame instead of the game waiting. The first job builds the
//! cloth's topology (welding, constraints, tethers) on the worker too.

pub mod flag;
pub mod solver;

use std::collections::HashMap;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, TryRecvError, channel};
use std::time::Instant;

use glam::{Mat4, Vec3};
use hecs::Entity;

use super::Physics;
use crate::models::{BlendShape, MeshSkin, Model};
use crate::render::mesh::{MeshData, Vertex};
use crate::scene::{CapsuleCollider, Cloth, Scene, SkinnedMeshRenderer, SphereCollider};
use solver::{Shape, State, StepParams, Topology, WindInput};

/// Wind Zone ▸ Main 1 blows this fast (m/s) on cloth.
pub const WIND_SPEED: f32 = 8.0;
/// Longest time one job simulates (a long hitch is not caught up).
const MAX_STEP: f32 = 1.0 / 15.0;

/// The simulated vertices of a Cloth (a component on its object while playing).
#[derive(Clone, Debug)]
pub struct ClothPose {
    /// Final vertices in the renderer's space (the mesh's vertex order).
    pub vertices: Arc<Vec<Vertex>>,
    /// Their bounds in the renderer's space.
    pub bounds: (Vec3, Vec3),
    /// Particle positions in world space (debug view, painting).
    pub particles: Arc<Vec<Vec3>>,
}

/// Cloth timings for the profiler and the docs.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ClothStats {
    pub cloths: usize,
    pub particles: usize,
    /// Worker time of the last finished job of every cloth, summed (ms).
    pub solve_ms: f32,
    /// Main-thread time of the last cloth update (collect, snapshot, start) (ms).
    pub main_thread_ms: f32,
    /// Frames a cloth's job was still running and the frame went on without it.
    pub late_frames: u64,
}

/// A cloth's source mesh (bind pose).
enum Source {
    Model(Arc<Model>, usize),
    Builtin(&'static MeshData),
}

impl Source {
    fn data(&self) -> &MeshData {
        match self {
            Self::Model(m, i) => &m.meshes[*i].data,
            Self::Builtin(d) => d,
        }
    }

    fn skin(&self) -> Option<&MeshSkin> {
        match self {
            Self::Model(m, i) => m.meshes[*i].skin.as_ref(),
            Self::Builtin(_) => None,
        }
    }

    fn shapes(&self) -> &[BlendShape] {
        match self {
            Self::Model(m, i) => &m.meshes[*i].blend_shapes,
            Self::Builtin(_) => &[],
        }
    }

    fn of(r: &crate::scene::MeshRef) -> Option<Self> {
        if let Some((m, i)) = crate::skinning::model_mesh(r) {
            return Some(Self::Model(m, i));
        }
        r.primitive()
            .map(|p| Self::Builtin(crate::render::mesh::primitive(p)))
    }
}

/// Simulation state, owned by the main thread between jobs and by the job while it runs.
struct Sim {
    topology: Option<Arc<Topology>>,
    /// Max distances and scale the topology was built with.
    built_for: (Vec<f32>, Vec3),
    state: State,
    targets: Vec<Vec3>,
    last_world: Option<Mat4>,
    time: f32,
    seed: u32,
    solve_ms: f32,
    output: Option<ClothPose>,
}

/// What a job needs from the scene.
struct Inputs {
    world: Mat4,
    matrices: Vec<Mat4>,
    weights: Vec<f32>,
    shapes: Vec<Shape>,
    params: StepParams,
    max_distances: Vec<f32>,
    world_velocity_scale: f32,
}

struct Run {
    source: Arc<Source>,
    sim: Option<Box<Sim>>,
    job: Option<Receiver<Box<Sim>>>,
    /// Time not simulated yet (frames the job was late).
    pending: f32,
}

/// The playing cloths.
#[derive(Default)]
pub struct ClothSystem {
    runs: HashMap<Entity, Run>,
    stats: ClothStats,
}

impl ClothSystem {
    /// Cloths among `objects`: objects with an enabled Cloth and a Skinned Mesh Renderer.
    pub(super) fn add(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let Ok(c) = scene.world.get::<&Cloth>(e) else {
                continue;
            };
            if !c.enabled {
                continue;
            }
            let Some(source) = scene
                .world
                .get::<&SkinnedMeshRenderer>(e)
                .ok()
                .and_then(|r| r.mesh.clone())
                .and_then(|m| Source::of(&m))
            else {
                continue;
            };
            self.runs.insert(
                e,
                Run {
                    source: Arc::new(source),
                    sim: Some(Box::new(Sim {
                        topology: None,
                        built_for: (Vec::new(), Vec3::ONE),
                        state: State::default(),
                        targets: Vec::new(),
                        last_world: None,
                        time: 0.0,
                        seed: e.id(),
                        solve_ms: 0.0,
                        output: None,
                    })),
                    job: None,
                    pending: 0.0,
                },
            );
        }
    }

    pub(super) fn remove(&mut self, e: Entity) {
        self.runs.remove(&e);
    }

    pub fn len(&self) -> usize {
        self.runs.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runs.is_empty()
    }
}

/// World-space collision shapes of a Cloth's collider lists.
fn shapes(scene: &Scene, c: &Cloth) -> Vec<Shape> {
    let mut out = Vec::new();
    for e in c.sphere_colliders.iter().flatten() {
        if !scene.contains(*e) || !scene.active_in_hierarchy(*e) {
            continue;
        }
        if let Ok(s) = scene.world.get::<&SphereCollider>(*e)
            && s.enabled
        {
            let m = scene.world_matrix(*e);
            let scale = m.to_scale_rotation_translation().0.abs().max_element();
            out.push(Shape::Sphere {
                center: m.transform_point3(s.center),
                radius: s.radius * scale,
            });
        }
    }
    for e in c.capsule_colliders.iter().flatten() {
        if !scene.contains(*e) || !scene.active_in_hierarchy(*e) {
            continue;
        }
        if let Ok(s) = scene.world.get::<&CapsuleCollider>(*e)
            && s.enabled
        {
            let m = scene.world_matrix(*e);
            let scale = m.to_scale_rotation_translation().0.abs();
            let axis = s.direction.min(2) as usize;
            let (a, b) = ((axis + 1) % 3, (axis + 2) % 3);
            let r = s.radius * scale[a].max(scale[b]);
            let half = (s.height * scale[axis] * 0.5 - r).max(0.0);
            let mut dir = Vec3::ZERO;
            dir[axis] = 1.0;
            let (_, rot, _) = m.to_scale_rotation_translation();
            let centre = m.transform_point3(s.center);
            let d = rot * dir * half;
            out.push(Shape::Capsule {
                a: centre - d,
                b: centre + d,
                radius: r,
            });
        }
    }
    out
}

/// The scene's Wind Zone as cloth wind, × `scale`.
fn wind(scene: &Scene, scale: f32) -> WindInput {
    match crate::scene::wind_zone::scene_wind(scene) {
        Some(w) => WindInput {
            velocity: w.direction.normalize_or(Vec3::Z) * (w.main * WIND_SPEED * scale),
            turbulence: w.turbulence * 0.5,
            pulse_magnitude: w.pulse_magnitude * 0.6,
            pulse_frequency: w.pulse_frequency * 0.5,
        },
        None => WindInput::default(),
    }
}

/// One job: build the topology if needed, step, produce the drawn vertices.
fn run_job(source: &Source, mut sim: Box<Sim>, inp: Inputs) -> Box<Sim> {
    let t0 = Instant::now();
    let data = source.data();
    let (_, _, translation) = inp.world.to_scale_rotation_translation();
    let scale = inp.world.to_scale_rotation_translation().0.abs();
    let key = (inp.max_distances.clone(), scale);
    if sim.topology.is_none() || sim.built_for != key {
        sim.topology = Some(Arc::new(Topology::build(data, &inp.max_distances, scale)));
        sim.built_for = key;
        sim.state = State::default();
    }
    let topo = sim.topology.clone().expect("built above");
    // Skinned (or bind) vertex positions → each particle's target in world space.
    let posed: Vec<Vec3> = if inp.matrices.is_empty() && inp.weights.is_empty() {
        data.vertices
            .iter()
            .map(|v| Vec3::from(v.position))
            .collect()
    } else {
        crate::skinning::deform(
            data,
            source.skin(),
            source.shapes(),
            &inp.matrices,
            &inp.weights,
        )
        .iter()
        .map(|v| Vec3::from(v.position))
        .collect()
    };
    let targets: Vec<Vec3> = topo
        .particle_vertex
        .iter()
        .map(|v| inp.world.transform_point3(posed[*v as usize]))
        .collect();
    if sim.state.x.len() != targets.len() {
        sim.state = State::new(&targets);
        sim.targets = targets.clone();
    }
    // Movement of the object the cloth doesn't lag behind.
    if let Some(last) = sim.last_world {
        let (_, _, before) = last.to_scale_rotation_translation();
        let shift = (translation - before) * (1.0 - inp.world_velocity_scale.clamp(0.0, 1.0));
        if shift != Vec3::ZERO {
            for (x, w) in sim.state.x.iter_mut().zip(&topo.inv_mass) {
                if *w > 0.0 {
                    *x += shift;
                }
            }
        }
    }
    sim.last_world = Some(inp.world);
    let mut params = inp.params;
    params.time = sim.time;
    params.seed = sim.seed;
    let from = std::mem::take(&mut sim.targets);
    let from = if from.len() == targets.len() {
        from
    } else {
        targets.clone()
    };
    solver::step(&topo, &mut sim.state, &from, &targets, &inp.shapes, &params);
    sim.targets = targets;
    sim.time += params.dt;
    sim.seed = sim.seed.wrapping_add(params.substeps.max(1));
    // Drawn vertices in the renderer's space.
    let inv = inp.world.inverse();
    let normals = solver::normals(&topo, &sim.state.x);
    let mut lo = Vec3::splat(f32::MAX);
    let mut hi = Vec3::splat(f32::MIN);
    let vertices: Vec<Vertex> = data
        .vertices
        .iter()
        .enumerate()
        .map(|(i, v)| {
            let p = topo.vertex_particle[i] as usize;
            let pos = inv.transform_point3(sim.state.x[p]);
            lo = lo.min(pos);
            hi = hi.max(pos);
            let n = inv.transform_vector3(normals[p]).normalize_or(Vec3::Y);
            let t = Vec3::new(v.tangent[0], v.tangent[1], v.tangent[2]);
            let t = (t - n * t.dot(n)).normalize_or(n.any_orthonormal_vector());
            Vertex {
                position: pos.into(),
                normal: n.into(),
                uv: v.uv,
                uv2: v.uv2,
                tangent: [t.x, t.y, t.z, v.tangent[3]],
            }
        })
        .collect();
    if vertices.is_empty() {
        lo = Vec3::ZERO;
        hi = Vec3::ZERO;
    }
    sim.output = Some(ClothPose {
        vertices: Arc::new(vertices),
        bounds: (lo, hi),
        particles: Arc::new(sim.state.x.clone()),
    });
    sim.solve_ms = t0.elapsed().as_secs_f32() * 1000.0;
    sim
}

impl Physics {
    /// Collect finished cloth jobs and start the next ones (once per frame, after animation).
    pub(super) fn update_cloth(&mut self, scene: &mut Scene, dt: f32) {
        if self.cloth.runs.is_empty() {
            return;
        }
        let t0 = Instant::now();
        let gravity = self.gravity();
        let entities: Vec<Entity> = self.cloth.runs.keys().copied().collect();
        let mut solve_ms = 0.0;
        let mut particles = 0;
        for e in entities {
            if !scene.contains(e) {
                self.cloth.runs.remove(&e);
                continue;
            }
            let run = self.cloth.runs.get_mut(&e).expect("listed above");
            if let Some(rx) = &run.job {
                match rx.try_recv() {
                    Ok(sim) => {
                        run.sim = Some(sim);
                        run.job = None;
                    }
                    Err(TryRecvError::Empty) => {
                        run.pending += dt;
                        self.cloth.stats.late_frames += 1;
                        continue;
                    }
                    Err(TryRecvError::Disconnected) => {
                        self.cloth.runs.remove(&e);
                        continue;
                    }
                }
            }
            let Some(sim) = run.sim.as_mut() else {
                continue;
            };
            if let Some(out) = sim.output.take() {
                let _ = scene.world.insert_one(e, out);
            }
            solve_ms += sim.solve_ms;
            particles += sim.topology.as_ref().map_or(0, |t| t.particle_count());
            let Ok(c) = scene.world.get::<&Cloth>(e).map(|c| (*c).clone()) else {
                continue;
            };
            if !c.enabled || !scene.active_in_hierarchy(e) {
                continue;
            }
            let step = (run.pending + dt).min(MAX_STEP);
            run.pending = 0.0;
            if step <= 0.0 {
                continue;
            }
            let Some(inputs) = inputs(scene, e, &c, gravity, step) else {
                continue;
            };
            let sim = run.sim.take().expect("checked above");
            let source = run.source.clone();
            let (tx, rx) = channel();
            run.job = Some(rx);
            crate::background::pool().spawn(move || {
                let _ = tx.send(run_job(&source, sim, inputs));
            });
        }
        self.cloth.stats.cloths = self.cloth.runs.len();
        self.cloth.stats.particles = particles;
        self.cloth.stats.solve_ms = solve_ms;
        self.cloth.stats.main_thread_ms = t0.elapsed().as_secs_f32() * 1000.0;
    }

    /// Wait for the cloth jobs in flight and publish their results (tests, captures).
    pub fn finish_cloth(&mut self, scene: &mut Scene) {
        for (e, run) in &mut self.cloth.runs {
            if let Some(rx) = run.job.take()
                && let Ok(mut sim) = rx.recv()
            {
                if let Some(out) = sim.output.take() {
                    let _ = scene.world.insert_one(*e, out);
                }
                run.sim = Some(sim);
            }
        }
    }

    pub fn cloth_stats(&self) -> ClothStats {
        self.cloth.stats
    }

    /// Number of playing cloths.
    pub fn cloth_count(&self) -> usize {
        self.cloth.len()
    }
}

/// Snapshot what a job of cloth `e` needs.
fn inputs(scene: &Scene, e: Entity, c: &Cloth, gravity: Vec3, dt: f32) -> Option<Inputs> {
    let r = scene.world.get::<&SkinnedMeshRenderer>(e).ok()?.clone();
    let world = scene.world_matrix(e);
    if world.determinant().abs() < 1e-12 {
        return None;
    }
    let (matrices, weights) =
        match crate::skinning::compute_pose(scene, e, &r, &|x| scene.world_matrix(x)) {
            Some(p) => (p.matrices, p.weights),
            None => (Vec::new(), Vec::new()),
        };
    let substeps = (dt * c.solver_frequency.max(30.0)).ceil().clamp(1.0, 32.0) as u32;
    Some(Inputs {
        world,
        matrices,
        weights,
        shapes: shapes(scene, c),
        params: StepParams {
            dt,
            substeps,
            gravity: if c.use_gravity { gravity } else { Vec3::ZERO },
            acceleration: c.external_acceleration,
            random_acceleration: c.random_acceleration,
            stretching_stiffness: c.stretching_stiffness,
            bending_stiffness: c.bending_stiffness,
            tethers: c.use_tethers,
            damping: c.damping,
            friction: c.friction,
            thickness: c.thickness.max(0.0),
            self_collision: c.self_collision.then_some(c.self_collision_distance),
            wind: wind(scene, c.wind_scale),
            time: 0.0,
            seed: 0,
        },
        max_distances: c.max_distances.clone(),
        world_velocity_scale: c.world_velocity_scale,
    })
}

#[cfg(test)]
#[path = "runtime_tests.rs"]
mod runtime_tests;
