//! Physics.IgnoreCollision / IgnoreLayerCollision, Rigidbody.AddForceAtPosition /
//! AddExplosionForce / Sleep / WakeUp / IsSleeping and Collider.ClosestPoint.

use glam::Vec3;
use hecs::Entity;
use rapier3d::prelude as rp;

use super::Physics;
use super::convert::{gv, rv};
use super::layers;

/// The pair as stored in `Physics::ignored` (smaller id first).
pub(super) fn pair(a: Entity, b: Entity) -> (Entity, Entity) {
    if a.to_bits() <= b.to_bits() {
        (a, b)
    } else {
        (b, a)
    }
}

impl Physics {
    /// `Physics.IgnoreLayerCollision(a, b, ignore)`: tick or untick a box of the Layer
    /// Collision Matrix while playing (every collider's groups follow).
    pub fn ignore_layer_collision(&mut self, a: u8, b: u8, ignore: bool) {
        let (a, b) = (a.min(31), b.min(31));
        self.settings.set_layers_collide(a, b, !ignore);
        let handles: Vec<rp::ColliderHandle> = self.colliders.keys().copied().collect();
        for h in handles {
            if let Some(c) = self.world.colliders.get_mut(h) {
                let layer = c.collision_groups().memberships.bits().trailing_zeros() as u8;
                if layer < 32 {
                    c.set_collision_groups(layers::groups(&self.settings, layer));
                }
            }
        }
    }

    /// `Physics.GetIgnoreLayerCollision(a, b)`.
    pub fn layer_collision_ignored(&self, a: u8, b: u8) -> bool {
        !self.settings.layers_collide(a.min(31), b.min(31))
    }

    /// `Physics.IgnoreCollision(a, b, ignore)`: the colliders of objects `a` and `b` pass
    /// through each other (and their triggers stay quiet) until undone.
    pub fn ignore_collision(&mut self, a: Entity, b: Entity, ignore: bool) {
        if a == b {
            return;
        }
        if ignore {
            self.ignored.insert(pair(a, b));
        } else {
            self.ignored.remove(&pair(a, b));
        }
        self.refresh_filter_hooks(a);
        self.refresh_filter_hooks(b);
    }

    /// `Physics.GetIgnoreCollision(a, b)`.
    pub fn collision_ignored(&self, a: Entity, b: Entity) -> bool {
        self.ignored.contains(&pair(a, b))
    }

    /// Whether `e` takes part in an ignored pair.
    pub(super) fn has_ignored_pair(&self, e: Entity) -> bool {
        !self.ignored.is_empty() && self.ignored.iter().any(|(x, y)| *x == e || *y == e)
    }

    /// Turn the pair filter of `e`'s colliders on or off (it costs a call per contact pair).
    pub(super) fn refresh_filter_hooks(&mut self, e: Entity) {
        let on = self.has_ignored_pair(e);
        let filters =
            rp::ActiveHooks::FILTER_CONTACT_PAIRS | rp::ActiveHooks::FILTER_INTERSECTION_PAIR;
        for h in self.colliders_of.get(&e).into_iter().flatten() {
            if let Some(c) = self.world.colliders.get_mut(*h) {
                let hooks = c.active_hooks();
                c.set_active_hooks(if on { hooks | filters } else { hooks - filters });
                // Re-checks the pair (wakes what touches it).
                c.set_collision_groups(c.collision_groups());
            }
        }
    }

    /// Rigidbody.AddForceAtPosition: `impulse` (already scaled by the force mode) at a world
    /// point, so it also turns the body.
    pub fn add_impulse_at_position(&mut self, e: Entity, impulse: Vec3, point: Vec3) {
        if impulse.is_finite() && point.is_finite() {
            self.apply_impulse_at_point(e, impulse, point);
        }
    }

    /// Rigidbody.AddExplosionForce: the push (an impulse after the force mode) an explosion of
    /// `force` at `center` with `radius` (0: no limit) gives body `e`: away from the centre,
    /// which `upwards` moves down (so bodies are thrown up), falling off linearly to the edge.
    /// `None` when out of range or there is no body.
    pub fn explosion_push(
        &self,
        e: Entity,
        force: f32,
        center: Vec3,
        radius: f32,
        upwards: f32,
    ) -> Option<Vec3> {
        let body = self
            .bodies
            .get(&e)
            .and_then(|b| self.world.bodies.get(b.handle))?;
        let at = gv(body.center_of_mass());
        let from = center - Vec3::Y * upwards;
        let d = at - center;
        let dist = d.length();
        if radius > 0.0 && dist > radius {
            return None;
        }
        let falloff = if radius > 0.0 {
            1.0 - dist / radius
        } else {
            1.0
        };
        let dir = (at - from).normalize_or(Vec3::Y);
        Some(dir * force * falloff)
    }

    /// Rigidbody.Sleep / WakeUp.
    pub fn set_sleeping(&mut self, e: Entity, sleep: bool) {
        let Some(h) = self.bodies.get(&e).map(|b| b.handle) else {
            return;
        };
        if let Some(b) = self.world.bodies.get_mut(h) {
            if sleep {
                b.sleep();
            } else {
                b.wake_up(true);
            }
        }
    }

    /// Rigidbody.IsSleeping (false without a body).
    pub fn is_sleeping(&self, e: Entity) -> bool {
        self.bodies
            .get(&e)
            .and_then(|b| self.world.bodies.get(b.handle))
            .is_some_and(|b| b.is_sleeping())
    }

    /// Collider.ClosestPoint: the point on `e`'s colliders nearest `point` (`point` itself
    /// when inside one; `None` without colliders).
    pub fn closest_point(&self, e: Entity, point: Vec3) -> Option<Vec3> {
        let mut best: Option<(f32, Vec3)> = None;
        for h in self.colliders_of.get(&e)? {
            let Some(c) = self.world.colliders.get(*h) else {
                continue;
            };
            let p = c.shape().project_point(c.position(), rv(point), true);
            let q = if p.is_inside { point } else { gv(p.point) };
            let d = q.distance_squared(point);
            if best.is_none_or(|(bd, _)| d < bd) {
                best = Some((d, q));
            }
        }
        best.map(|(_, q)| q)
    }

    /// Rigidbody drag / angularDrag / constraints / collision detection changed by a script:
    /// copy them onto the live body.
    pub fn apply_body_settings(&mut self, e: Entity, rb: &crate::scene::Rigidbody) {
        let locks = super::locked_axes(rb);
        if let Some(b) = self.body_mut(e) {
            b.set_linear_damping(rb.drag.max(0.0));
            b.set_angular_damping(rb.angular_drag.max(0.0));
            if b.locked_axes() != locks {
                b.set_locked_axes(locks, true);
            }
            b.enable_ccd(rb.continuous);
        }
    }

    /// Rigidbody.interpolation changed by a script.
    pub fn set_interpolate(&mut self, e: Entity, on: bool) {
        let Some(b) = self.bodies.get_mut(&e) else {
            return;
        };
        if on
            && !b.interpolate
            && let Some(body) = self.world.bodies.get(b.handle)
        {
            b.prev = super::convert::parts(body.position());
        }
        b.interpolate = on;
    }

    /// Rigidbody.GetPointVelocity: the velocity of the body at a world point (zero without
    /// a body).
    pub fn point_velocity(&self, e: Entity, point: Vec3) -> Vec3 {
        self.body(e)
            .map(|b| gv(b.velocity_at_point(rv(point))))
            .unwrap_or_default()
    }

    /// Rigidbody.worldCenterOfMass (`None` without a body).
    pub fn world_center_of_mass(&self, e: Entity) -> Option<Vec3> {
        self.body(e).map(|b| gv(b.center_of_mass()))
    }
}
