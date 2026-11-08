//! Particle System: settings component, pure simulation, and scene glue (update + render data).
//!
//! The saved [`ParticleSystem`] component holds the modules; the live particles are in a
//! runtime-only [`ParticleEmitter`] component on the same entity. It is not in the component
//! registry, so scene files, undo snapshots and the Play-mode copy never contain particles.
//!
//! The glue gathers the scene's colliders for World collisions, steps every system and routes
//! the birth / collision / death events of systems with Sub Emitters to the child systems they
//! name (which then only emit when triggered).

pub mod component;
pub mod noise;
pub mod rng;
pub mod shape;
pub mod sim;

#[cfg(test)]
mod tests;

pub use component::*;
pub use sim::{
    Collider, ColliderShape, Inherit, Particle, ParticleEmitter, PlayState, StepEnv, SubEmitEvent,
    Trail,
};

use std::collections::HashSet;

use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::render::{ParticleBatch, ParticleInstance};
use crate::scene::Scene;
use sim::salt;

/// Gravity used when no physics world is around (edit-mode preview).
pub const DEFAULT_GRAVITY: Vec3 = Vec3::new(0.0, -9.81, 0.0);

/// Systems on active objects, with their world matrices.
fn active_systems(scene: &Scene) -> Vec<(Entity, Mat4)> {
    let mut list: Vec<Entity> = scene
        .world
        .query::<(Entity, &ParticleSystem)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    list.retain(|e| scene.active_in_hierarchy(*e));
    list.into_iter()
        .map(|e| (e, scene.world_matrix(e)))
        .collect()
}

/// Give `e` an emitter if it has none (started when `play` is true). False without a system.
pub fn ensure_emitter(scene: &mut Scene, e: Entity, play: bool, gravity: Vec3) -> bool {
    if scene.world.get::<&ParticleEmitter>(e).is_ok() {
        return true;
    }
    let Ok(system) = scene.world.get::<&ParticleSystem>(e).map(|s| (*s).clone()) else {
        return false;
    };
    let mut emitter = ParticleEmitter::new(&system);
    if play {
        emitter.play(&system, &scene.world_matrix(e), gravity);
    }
    scene.world.insert_one(e, emitter).is_ok()
}

/// Run `f` on the emitter and settings of `e`. A missing emitter is created first, already
/// playing when Play On Awake is set (so scripts see the same state as after the first frame).
pub fn with_emitter<R>(
    scene: &mut Scene,
    e: Entity,
    gravity: Vec3,
    f: impl FnOnce(&mut ParticleEmitter, &ParticleSystem, &Mat4) -> R,
) -> Option<R> {
    let awake = scene
        .world
        .get::<&ParticleSystem>(e)
        .is_ok_and(|s| s.main.play_on_awake);
    if !ensure_emitter(scene, e, awake, gravity) {
        return None;
    }
    let world = scene.world_matrix(e);
    let (system, emitter) = scene
        .world
        .query_one_mut::<(&ParticleSystem, &mut ParticleEmitter)>(e)
        .ok()?;
    Some(f(emitter, system, &world))
}

/// The child of `parent` called `name` with a Particle System (a sub emitter).
pub fn sub_emitter_entity(scene: &Scene, parent: Entity, name: &str) -> Option<Entity> {
    scene
        .children(parent)
        .into_iter()
        .find(|c| scene.name(*c) == name && scene.world.get::<&ParticleSystem>(*c).is_ok())
}

/// Systems some other system on the scene uses as a sub emitter.
fn sub_emitters(scene: &Scene) -> HashSet<Entity> {
    let mut out = HashSet::new();
    for (e, s) in scene.world.query::<(Entity, &ParticleSystem)>().iter() {
        if !s.sub_emitters.enabled {
            continue;
        }
        for sub in &s.sub_emitters.list {
            if let Some(c) = sub_emitter_entity(scene, e, &sub.emitter) {
                out.insert(c);
            }
        }
    }
    out
}

/// World colliders for Collision modules of the World type (empty when none needs them).
pub fn world_colliders(scene: &Scene) -> Vec<Collider> {
    use crate::scene::{BoxCollider, CapsuleCollider, MeshCollider, MeshFilter, SphereCollider};
    let needed = scene
        .world
        .query::<&ParticleSystem>()
        .iter()
        .any(|s| s.collision.enabled && s.collision.kind == CollisionType::World);
    if !needed {
        return Vec::new();
    }
    let mut out = Vec::new();
    let axes = |m: &Mat4| {
        let (x, y, z) = (
            m.x_axis.truncate(),
            m.y_axis.truncate(),
            m.z_axis.truncate(),
        );
        (
            [
                x.normalize_or(Vec3::X),
                y.normalize_or(Vec3::Y),
                z.normalize_or(Vec3::Z),
            ],
            Vec3::new(x.length(), y.length(), z.length()),
        )
    };
    let layer = |e: Entity| scene.game_object(e).layer;
    for (e, c) in scene.world.query::<(Entity, &BoxCollider)>().iter() {
        if !c.enabled || c.is_trigger || !scene.active_in_hierarchy(e) {
            continue;
        }
        let m = scene.world_matrix(e);
        let (ax, scale) = axes(&m);
        out.push(Collider {
            shape: ColliderShape::Box {
                center: m.transform_point3(c.center),
                axes: ax,
                half: c.size.abs() * 0.5 * scale,
            },
            layer: layer(e),
        });
    }
    for (e, c) in scene.world.query::<(Entity, &SphereCollider)>().iter() {
        if !c.enabled || c.is_trigger || !scene.active_in_hierarchy(e) {
            continue;
        }
        let m = scene.world_matrix(e);
        let (_, scale) = axes(&m);
        out.push(Collider {
            shape: ColliderShape::Sphere {
                center: m.transform_point3(c.center),
                radius: c.radius.abs() * scale.max_element(),
            },
            layer: layer(e),
        });
    }
    for (e, c) in scene.world.query::<(Entity, &CapsuleCollider)>().iter() {
        if !c.enabled || c.is_trigger || !scene.active_in_hierarchy(e) {
            continue;
        }
        let m = scene.world_matrix(e);
        let (_, scale) = axes(&m);
        let axis = match c.direction {
            0 => Vec3::X,
            2 => Vec3::Z,
            _ => Vec3::Y,
        };
        let radius = c.radius.abs() * scale.max_element();
        let half = (c.height * 0.5 - c.radius).max(0.0);
        out.push(Collider {
            shape: ColliderShape::Capsule {
                a: m.transform_point3(c.center - axis * half),
                b: m.transform_point3(c.center + axis * half),
                radius,
            },
            layer: layer(e),
        });
    }
    // Mesh colliders of primitives: their bounding box (a Plane is a thin floor).
    for (e, c, f) in scene
        .world
        .query::<(Entity, &MeshCollider, &MeshFilter)>()
        .iter()
    {
        if !c.enabled || c.is_trigger || !scene.active_in_hierarchy(e) {
            continue;
        }
        let Some(p) = f.mesh.as_ref().and_then(|m| m.primitive()) else {
            continue;
        };
        let m = scene.world_matrix(e);
        let (ax, scale) = axes(&m);
        out.push(Collider {
            shape: ColliderShape::Box {
                center: m.transform_point3(Vec3::ZERO),
                axes: ax,
                half: (p.half_extents() * scale).max(Vec3::splat(0.005)),
            },
            layer: layer(e),
        });
    }
    out
}

/// Advance every active system by `dt` (Play mode). Systems seen for the first time start if
/// Play On Awake is set, so objects created while playing behave like scene objects.
pub fn update(scene: &mut Scene, dt: f32, gravity: Vec3) {
    let colliders = world_colliders(scene);
    let subs = sub_emitters(scene);
    for (e, world) in active_systems(scene) {
        let awake = scene
            .world
            .get::<&ParticleSystem>(e)
            .is_ok_and(|s| s.main.play_on_awake);
        ensure_emitter(scene, e, awake || subs.contains(&e), gravity);
        let env = StepEnv {
            gravity,
            colliders: &colliders,
            emit: !subs.contains(&e),
        };
        step_with(scene, e, &world, dt, env);
    }
}

/// Advance one system (edit-mode preview of the selection).
pub fn step_one(scene: &mut Scene, e: Entity, world: &Mat4, dt: f32, gravity: Vec3) {
    let colliders = world_colliders(scene);
    let subs = sub_emitters(scene);
    let env = StepEnv {
        gravity,
        colliders: &colliders,
        emit: !subs.contains(&e),
    };
    step_with(scene, e, world, dt, env);
}

/// Step `e`, then hand its events to its sub emitters.
fn step_with(scene: &mut Scene, e: Entity, world: &Mat4, dt: f32, env: StepEnv) {
    let events = match scene
        .world
        .query_one_mut::<(&ParticleSystem, &mut ParticleEmitter)>(e)
    {
        Ok((system, emitter)) => {
            emitter.step_with(system, world, env, dt);
            if emitter.events.is_empty() {
                return;
            }
            (
                std::mem::take(&mut emitter.events),
                system.sub_emitters.clone(),
            )
        }
        Err(_) => return,
    };
    route_events(scene, e, &events.0, &events.1, env.gravity);
}

/// Emit into the sub emitters `e`'s events trigger.
pub fn route_events(
    scene: &mut Scene,
    e: Entity,
    events: &[SubEmitEvent],
    subs: &SubEmittersModule,
    gravity: Vec3,
) {
    for (i, sub) in subs.list.iter().enumerate() {
        let Some(child) = sub_emitter_entity(scene, e, &sub.emitter) else {
            continue;
        };
        ensure_emitter(scene, child, true, gravity);
        let world = scene.world_matrix(child);
        let Ok((system, emitter)) = scene
            .world
            .query_one_mut::<(&ParticleSystem, &mut ParticleEmitter)>(child)
        else {
            continue;
        };
        for (k, ev) in events.iter().enumerate() {
            if ev.trigger != sub.trigger {
                continue;
            }
            if sub.probability < 1.0 {
                // A fixed pick per event and entry (deterministic).
                let mut r = rng::Rng::new(
                    (ev.position.x.to_bits() as u64) << 32
                        ^ ev.position.z.to_bits() as u64
                        ^ ((k as u64) << 8)
                        ^ i as u64,
                );
                if r.next_f32() >= sub.probability {
                    continue;
                }
            }
            let inherit = Inherit {
                velocity: ev.velocity * sub.inherit_velocity,
                color: sub.inherit_color.then_some(ev.color),
                size: sub.inherit_size.then_some(ev.size),
            };
            emitter.emit_at(system, &world, ev.position, sub.count, inherit);
        }
    }
}

/// Remove every emitter (all particles), e.g. before Play starts.
pub fn clear_all(scene: &mut Scene) {
    let list: Vec<Entity> = scene
        .world
        .query::<(Entity, &ParticleEmitter)>()
        .iter()
        .map(|(e, _)| e)
        .collect();
    for e in list {
        let _ = scene.world.remove_one::<ParticleEmitter>(e);
    }
}

/// Current sRGB colour + alpha of a particle: start colour × colour over lifetime × colour
/// by speed.
pub fn particle_color_srgb(system: &ParticleSystem, p: &Particle) -> [f32; 4] {
    let mut c = p.color;
    let col = &system.color_over_lifetime;
    if col.enabled {
        let g = col.evaluate(p.life_fraction(), p.rand(salt::COLOR));
        c = std::array::from_fn(|i| c[i] * g[i]);
    }
    let cbs = &system.color_by_speed;
    if cbs.enabled {
        let g = cbs
            .gradient
            .evaluate(sim::speed_fraction(p.velocity.length(), cbs.range));
        c = std::array::from_fn(|i| c[i] * g[i]);
    }
    c
}

/// sRGB (+ HDR intensity) to the linear colour the renderer wants.
fn linear(system: &ParticleSystem, c: [f32; 4]) -> [f32; 4] {
    let k = system.renderer.intensity.max(0.0);
    let lin = |v: f32| v.max(0.0).powf(2.2) * k;
    [lin(c[0]), lin(c[1]), lin(c[2]), c[3].clamp(0.0, 1.0)]
}

/// Linear colour of a particle now.
fn particle_color(system: &ParticleSystem, p: &Particle) -> [f32; 4] {
    linear(system, particle_color_srgb(system, p))
}

/// Size of a particle now: start size × size over lifetime × size by speed × noise.
pub fn particle_size(system: &ParticleSystem, p: &Particle) -> f32 {
    let mut size = p.size * p.noise_size;
    let s = &system.size_over_lifetime;
    if s.enabled {
        size *= s
            .curve()
            .evaluate(p.life_fraction(), p.rand(salt::SIZE))
            .max(0.0);
    }
    let sbs = &system.size_by_speed;
    if sbs.enabled {
        let f = sim::speed_fraction(p.velocity.length(), sbs.range);
        size *= sbs.size.evaluate(f, p.rand(salt::SIZE_SPEED)).max(0.0);
    }
    size
}

/// Texture-sheet UV rectangle of a particle (the whole texture without the module).
pub fn particle_uv(system: &ParticleSystem, p: &Particle) -> [f32; 4] {
    let ts = &system.texture_sheet_animation;
    if !ts.enabled {
        return [0.0, 0.0, 1.0, 1.0];
    }
    ts.uv(
        p.life_fraction(),
        [
            p.rand(salt::SHEET),
            p.rand(salt::SHEET + 1),
            p.rand(salt::SHEET + 2),
        ],
    )
}

/// Render data for every active system with live particles (world-space billboards).
pub fn batches(scene: &Scene) -> Vec<ParticleBatch> {
    batches_where(scene, |_| true)
}

/// Instance shape flags (`params.x`).
const SHAPE_DOT: f32 = 0.0;
const SHAPE_TEXTURE: f32 = 1.0;
const SHAPE_STRIP: f32 = 2.0;
/// Instance modes (`params.z`).
const MODE_SEGMENT: f32 = 4.0;

/// [`batches`] of the systems `keep` accepts (a camera's culling mask).
pub fn batches_where(scene: &Scene, keep: impl Fn(Entity) -> bool) -> Vec<ParticleBatch> {
    let mut out = Vec::new();
    for (e, world) in active_systems(scene) {
        if !keep(e) {
            continue;
        }
        let Ok(system) = scene.world.get::<&ParticleSystem>(e) else {
            continue;
        };
        let Ok(emitter) = scene.world.get::<&ParticleEmitter>(e) else {
            continue;
        };
        if !system.renderer.enabled {
            continue;
        }
        let local = system.main.simulation_space == SimulationSpace::Local;
        let to_world = |v: Vec3| if local { world.transform_point3(v) } else { v };
        let to_world_dir = |v: Vec3| if local { world.transform_vector3(v) } else { v };
        let textured = system.renderer.texture.is_some();
        let r = &system.renderer;
        let mode = match r.render_mode {
            RenderMode::Billboard => 0.0,
            RenderMode::Stretched => 1.0,
            RenderMode::Horizontal => 2.0,
            RenderMode::Vertical => 3.0,
        };
        if !emitter.particles.is_empty() {
            let particles = emitter
                .particles
                .iter()
                .map(|p| {
                    let vel = to_world_dir(p.velocity);
                    ParticleInstance {
                        position: to_world(p.position).to_array(),
                        size: particle_size(&system, p),
                        color: particle_color(&system, p),
                        params: [
                            if textured { SHAPE_TEXTURE } else { SHAPE_DOT },
                            p.rotation.to_radians(),
                            mode,
                            r.length_scale,
                        ],
                        uv: particle_uv(&system, p),
                        axis: [vel.x, vel.y, vel.z, r.speed_scale],
                    }
                })
                .collect();
            out.push(ParticleBatch {
                origin: world.w_axis.truncate(),
                particles,
                texture: r.texture.clone(),
                additive: r.blend == ParticleBlend::Additive,
            });
        }
        let trails = trail_instances(&system, &emitter, &world);
        if !trails.is_empty() {
            out.push(ParticleBatch {
                origin: world.w_axis.truncate(),
                particles: trails,
                texture: system.trails.texture.clone(),
                additive: r.blend == ParticleBlend::Additive,
            });
        }
    }
    out
}

/// A camera-facing segment from `a` to `b` (widths and colours of both ends' start).
fn segment(
    a: Vec3,
    b: Vec3,
    width: [f32; 2],
    color: [f32; 4],
    u: [f32; 2],
    textured: bool,
) -> ParticleInstance {
    let d = b - a;
    ParticleInstance {
        position: a.to_array(),
        size: width[0],
        color,
        params: [
            if textured { SHAPE_TEXTURE } else { SHAPE_STRIP },
            0.0,
            MODE_SEGMENT,
            0.0,
        ],
        uv: [u[0], 0.0, u[1] - u[0], 1.0],
        axis: [d.x, d.y, d.z, width[1]],
    }
}

/// Trail segments of a system (Particles mode: one ribbon per particle through its points;
/// Ribbon mode: one ribbon through every particle in birth order).
pub fn trail_instances(
    system: &ParticleSystem,
    emitter: &ParticleEmitter,
    world: &Mat4,
) -> Vec<ParticleInstance> {
    let tr = &system.trails;
    if !tr.enabled {
        return Vec::new();
    }
    let local = system.main.simulation_space == SimulationSpace::Local;
    let textured = tr.texture.is_some();
    let width_of = |p: &Particle, t: f32| {
        let base = if tr.size_affects_width {
            particle_size(system, p)
        } else {
            1.0
        };
        base * tr
            .width_over_trail
            .evaluate(t, p.rand(salt::TRAIL))
            .max(0.0)
    };
    let color_of = |p: &Particle, t: f32| {
        let base = if tr.inherit_particle_color {
            particle_color_srgb(system, p)
        } else {
            [1.0; 4]
        };
        let life = tr.color_over_lifetime.evaluate(p.life_fraction());
        let along = tr.color_over_trail.evaluate(t);
        let c: [f32; 4] = std::array::from_fn(|i| base[i] * life[i] * along[i]);
        linear(system, c)
    };
    let mut out = Vec::new();
    match tr.mode {
        TrailMode::Particles => {
            let by_id: std::collections::HashMap<u32, &Particle> =
                emitter.particles.iter().map(|p| (p.id, p)).collect();
            for (id, trail) in &emitter.trails {
                let p = match (by_id.get(id), &trail.last) {
                    (Some(p), _) => **p,
                    (None, Some(last)) => *last,
                    (None, None) => continue,
                };
                let to_world = |v: Vec3| {
                    if local && !tr.world_space {
                        world.transform_point3(v)
                    } else {
                        v
                    }
                };
                // The live particle is the head; then its points, newest first.
                let head = if trail.alive {
                    Some(if local {
                        world.transform_point3(p.position)
                    } else {
                        p.position
                    })
                } else {
                    None
                };
                let pts: Vec<Vec3> = head
                    .into_iter()
                    .chain(trail.points.iter().map(|q| to_world(q.position)))
                    .collect();
                if pts.len() < 2 {
                    continue;
                }
                let last = (pts.len() - 1) as f32;
                for k in 0..pts.len() - 1 {
                    let (t0, t1) = (k as f32 / last, (k + 1) as f32 / last);
                    let c = color_of(&p, t0);
                    out.push(segment(
                        pts[k],
                        pts[k + 1],
                        [width_of(&p, t0), width_of(&p, t1)],
                        c,
                        [t0, t1],
                        textured,
                    ));
                }
            }
        }
        TrailMode::Ribbon => {
            let mut list: Vec<&Particle> = emitter.particles.iter().collect();
            // Newest first, like a trail's head.
            list.sort_by_key(|p| std::cmp::Reverse(p.id));
            if list.len() < 2 {
                return out;
            }
            let last = (list.len() - 1) as f32;
            let pos = |p: &Particle| {
                if local {
                    world.transform_point3(p.position)
                } else {
                    p.position
                }
            };
            for k in 0..list.len() - 1 {
                let (a, b) = (list[k], list[k + 1]);
                let (t0, t1) = (k as f32 / last, (k + 1) as f32 / last);
                out.push(segment(
                    pos(a),
                    pos(b),
                    [width_of(a, t0), width_of(b, t1)],
                    color_of(a, t0),
                    [t0, t1],
                    textured,
                ));
            }
        }
    }
    out
}
