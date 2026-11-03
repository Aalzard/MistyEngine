//! Lua `ParticleSystem`: Play/Stop/Pause/Clear/Emit plus isPlaying, isPaused, isStopped,
//! particleCount and time. Changes apply to the live emitter immediately.

use hecs::Entity;
use mlua::{Lua, Result, UserData, UserDataFields, UserDataMethods};

use super::World;
use super::api::{LGameObject, with_world};
use crate::particles::{self, ParticleEmitter, PlayState};

#[derive(Clone, Copy)]
pub struct LParticleSystem(pub Entity);

/// Run `f` on the object's emitter (created on first use).
fn emitter<R>(
    lua: &Lua,
    e: Entity,
    f: impl FnOnce(&mut ParticleEmitter, &particles::ParticleSystem, &glam::Mat4, glam::Vec3) -> R,
) -> Result<R> {
    with_world(lua, |w| {
        let gravity = w.physics.gravity();
        let World { scene, .. } = w;
        particles::with_emitter(scene, e, gravity, |em, sys, m| f(em, sys, m, gravity))
            .ok_or_else(|| mlua::Error::runtime("object was destroyed"))
    })?
}

/// Run `f` on the object's emitter only.
fn state<R>(lua: &Lua, e: Entity, f: impl FnOnce(&mut ParticleEmitter) -> R) -> Result<R> {
    emitter(lua, e, |em, _, _, _| f(em))
}

impl UserData for LParticleSystem {
    fn add_fields<F: UserDataFields<Self>>(f: &mut F) {
        f.add_field_method_get("isPlaying", |lua, p| state(lua, p.0, |em| em.is_playing()));
        f.add_field_method_get("isPaused", |lua, p| state(lua, p.0, |em| em.is_paused()));
        f.add_field_method_get("isStopped", |lua, p| {
            state(lua, p.0, |em| {
                em.state == PlayState::Stopped && em.count() == 0
            })
        });
        f.add_field_method_get("particleCount", |lua, p| state(lua, p.0, |em| em.count()));
        f.add_field_method_get("time", |lua, p| state(lua, p.0, |em| em.time));
        f.add_field_method_get("gameObject", |_, p| Ok(LGameObject(p.0)));
    }

    fn add_methods<M: UserDataMethods<Self>>(m: &mut M) {
        m.add_method("Play", |lua, p, ()| {
            emitter(lua, p.0, |em, sys, w, g| em.play(sys, w, g))
        });
        m.add_method("Stop", |lua, p, ()| state(lua, p.0, |em| em.stop()));
        m.add_method("Pause", |lua, p, ()| state(lua, p.0, |em| em.pause()));
        m.add_method("Clear", |lua, p, ()| state(lua, p.0, |em| em.clear()));
        m.add_method("Emit", |lua, p, n: u32| {
            emitter(lua, p.0, |em, sys, w, _| em.emit(sys, w, n))
        });
    }
}
