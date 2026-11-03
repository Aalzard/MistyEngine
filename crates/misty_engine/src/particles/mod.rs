//! Particle System: settings component, pure simulation, and scene glue (update + render data).
//!
//! The saved [`ParticleSystem`] component holds the modules; the live particles are in a
//! runtime-only [`ParticleEmitter`] component on the same entity. It is not in the component
//! registry, so scene files, undo snapshots and the Play-mode copy never contain particles.

pub mod component;
pub mod rng;
pub mod shape;
pub mod sim;

#[cfg(test)]
mod tests;

pub use component::*;
pub use sim::{Particle, ParticleEmitter, PlayState};

use glam::{Mat4, Vec3};
use hecs::Entity;

use crate::render::{ParticleBatch, ParticleInstance};
use crate::scene::Scene;

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

/// Advance every active system by `dt` (Play mode). Systems seen for the first time start if
/// Play On Awake is set, so objects created while playing behave like scene objects.
pub fn update(scene: &mut Scene, dt: f32, gravity: Vec3) {
    for (e, world) in active_systems(scene) {
        let awake = scene
            .world
            .get::<&ParticleSystem>(e)
            .is_ok_and(|s| s.main.play_on_awake);
        ensure_emitter(scene, e, awake, gravity);
        step_one(scene, e, &world, dt, gravity);
    }
}

/// Advance one system (edit-mode preview of the selection).
pub fn step_one(scene: &mut Scene, e: Entity, world: &Mat4, dt: f32, gravity: Vec3) {
    if let Ok((system, emitter)) = scene
        .world
        .query_one_mut::<(&ParticleSystem, &mut ParticleEmitter)>(e)
    {
        emitter.step(system, world, gravity, dt);
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

/// Linear colour of a particle now: start colour × colour over lifetime × intensity.
fn particle_color(system: &ParticleSystem, p: &Particle) -> [f32; 4] {
    let mut c = p.color;
    if system.color_over_lifetime.enabled {
        let g = system
            .color_over_lifetime
            .gradient
            .evaluate(p.life_fraction());
        c = std::array::from_fn(|i| c[i] * g[i]);
    }
    let k = system.renderer.intensity.max(0.0);
    let lin = |v: f32| v.max(0.0).powf(2.2) * k;
    [lin(c[0]), lin(c[1]), lin(c[2]), c[3].clamp(0.0, 1.0)]
}

fn particle_size(system: &ParticleSystem, p: &Particle) -> f32 {
    let s = &system.size_over_lifetime;
    if s.enabled {
        let t = p.life_fraction();
        p.size * (s.start + (s.end - s.start) * t).max(0.0)
    } else {
        p.size
    }
}

/// Render data for every active system with live particles (world-space billboards).
pub fn batches(scene: &Scene) -> Vec<ParticleBatch> {
    let mut out = Vec::new();
    for (e, world) in active_systems(scene) {
        let Ok(system) = scene.world.get::<&ParticleSystem>(e) else {
            continue;
        };
        let Ok(emitter) = scene.world.get::<&ParticleEmitter>(e) else {
            continue;
        };
        if !system.renderer.enabled || emitter.particles.is_empty() {
            continue;
        }
        let local = system.main.simulation_space == SimulationSpace::Local;
        let textured = system.renderer.texture.is_some();
        let particles = emitter
            .particles
            .iter()
            .map(|p| {
                let pos = if local {
                    world.transform_point3(p.position)
                } else {
                    p.position
                };
                ParticleInstance {
                    position: pos.to_array(),
                    size: particle_size(&system, p),
                    color: particle_color(&system, p),
                    params: [if textured { 1.0 } else { 0.0 }, 0.0, 0.0, 0.0],
                }
            })
            .collect();
        out.push(ParticleBatch {
            origin: world.w_axis.truncate(),
            particles,
            texture: system.renderer.texture.clone(),
            additive: system.renderer.blend == ParticleBlend::Additive,
        });
    }
    out
}
