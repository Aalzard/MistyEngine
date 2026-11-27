//! Inspector edits while playing reach the running physics world.
//!
//! Rigidbody and collider values were read once when Play started: changing a mass, drag,
//! constraint, collider size, trigger flag or Physics Material in the Inspector during Play
//! did nothing until the next Play. `refresh_object` copies the object's current values onto
//! its live body and colliders. Colliders are changed in place (no Exit/Enter events while a
//! slider is dragged); only a different number of colliders (one switched on or off) rebuilds
//! them.

use hecs::Entity;
use rapier3d::prelude as rp;

use super::Physics;
use crate::scene::{Interpolation, Rigidbody, Scene};

impl Physics {
    /// Copy `e`'s Rigidbody and collider components onto its live body and colliders.
    pub fn refresh_object(&mut self, scene: &Scene, e: Entity) {
        if !scene.contains(e) || !scene.active_in_hierarchy(e) {
            return;
        }
        let rb = scene.world.get::<&Rigidbody>(e).ok().map(|b| (*b).clone());
        if let Some(rb) = rb
            && self.bodies.contains_key(&e)
        {
            if self
                .bodies
                .get(&e)
                .is_some_and(|b| b.kinematic != rb.is_kinematic)
            {
                self.set_kinematic(e, rb.is_kinematic);
            }
            self.set_mass(e, rb.mass);
            self.set_use_gravity(e, rb.use_gravity);
            self.apply_body_settings(e, &rb);
            self.set_interpolate(e, rb.interpolation == Interpolation::Interpolate);
        }
        self.refresh_colliders(scene, e);
    }

    fn refresh_colliders(&mut self, scene: &Scene, e: Entity) {
        // Terrain Colliders and tree trunks also belong to their object: leave those alone.
        let w = &scene.world;
        if w.get::<&crate::terrain::TerrainCollider>(e).is_ok()
            || w.get::<&crate::terrain::Terrain>(e).is_ok()
        {
            return;
        }
        let built = self.colliders_for(scene, e);
        let handles = self.colliders_of.get(&e).cloned().unwrap_or_default();
        if handles.len() != built.len() {
            // Their contacts end with exits in the next step.
            self.departed.extend(handles.iter().map(|h| (*h, e)));
            for h in handles {
                self.world.remove_collider(h);
                self.forget_collider(h);
            }
            self.add_colliders(scene, &[e]);
            return;
        }
        for (h, (b, material)) in handles.iter().zip(built) {
            let Some(c) = self.world.colliders.get_mut(*h) else {
                continue;
            };
            c.set_shape(b.shape.clone());
            c.set_friction(b.friction);
            c.set_friction_combine_rule(b.friction_combine_rule);
            c.set_restitution(b.restitution);
            c.set_restitution_combine_rule(b.restitution_combine_rule);
            if c.is_sensor() != b.is_sensor {
                c.set_sensor(b.is_sensor);
                c.set_active_collision_types(if b.is_sensor {
                    rp::ActiveCollisionTypes::all()
                } else {
                    rp::ActiveCollisionTypes::default()
                });
            }
            let mut hooks = c.active_hooks();
            hooks.set(rp::ActiveHooks::MODIFY_SOLVER_CONTACTS, material.is_some());
            c.set_active_hooks(hooks);
            match material {
                Some(m) => {
                    self.materials.insert(*h, m);
                }
                None => {
                    self.materials.remove(h);
                }
            }
        }
    }
}
