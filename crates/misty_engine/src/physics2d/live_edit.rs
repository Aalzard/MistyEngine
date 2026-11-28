//! Inspector edits while playing reach the running 2D physics world (see
//! `physics/live_edit.rs`): Rigidbody 2D and Collider 2D values were read once when Play
//! started.

use hecs::Entity;
use rapier2d::prelude as rp;

use super::{Physics2D, pose, world_pose};
use crate::scene::{CollisionDetection2D, Rigidbody2D, Scene};

impl Physics2D {
    /// Copy `e`'s Rigidbody 2D and Collider 2D components onto its live body and colliders.
    pub fn refresh_object(&mut self, scene: &Scene, e: Entity) {
        if !scene.contains(e) || !scene.active_in_hierarchy(e) {
            return;
        }
        let rb = scene
            .world
            .get::<&Rigidbody2D>(e)
            .ok()
            .map(|b| (*b).clone());
        if let Some(rb) = rb
            && self.bodies.contains_key(&e)
        {
            if self.body_type(e) != Some(rb.body_type) {
                self.set_body_type(e, rb.body_type);
            }
            let mut locked = rp::LockedAxes::empty();
            if rb.freeze_position[0] {
                locked |= rp::LockedAxes::TRANSLATION_LOCKED_X;
            }
            if rb.freeze_position[1] {
                locked |= rp::LockedAxes::TRANSLATION_LOCKED_Y;
            }
            if rb.freeze_rotation {
                locked |= rp::LockedAxes::ROTATION_LOCKED;
            }
            if let Some(b) = self.body_mut(e) {
                b.set_additional_mass(rb.mass.max(1e-4), true);
                b.set_linear_damping(rb.linear_drag.max(0.0));
                b.set_angular_damping(rb.angular_drag.max(0.0));
                b.set_gravity_scale(rb.gravity_scale, true);
                if b.locked_axes() != locked {
                    b.set_locked_axes(locked, true);
                }
                b.enable_ccd(rb.collision_detection == CollisionDetection2D::Continuous);
            }
        }
        self.refresh_colliders(scene, e);
    }

    fn refresh_colliders(&mut self, scene: &Scene, e: Entity) {
        // Tilemap colliders are rebuilt from their tiles (`refresh_tilemaps`).
        if scene
            .world
            .get::<&crate::tilemap::TilemapCollider2D>(e)
            .is_ok()
        {
            return;
        }
        let built = self.shapes(scene, e);
        let handles = self.colliders_of.get(&e).cloned().unwrap_or_default();
        // A new size or shape rebuilds the colliders (a shape swapped in place kept its old
        // contacts in rapier2d); offsets, materials and trigger flags change in place.
        let reshaped = handles.iter().zip(&built).any(|(h, (b, _))| {
            self.world.colliders.get(*h).is_none_or(|c| {
                c.shape().shape_type() != b.shape.shape_type()
                    || c.shape().compute_local_aabb() != b.shape.compute_local_aabb()
            })
        });
        if handles.len() != built.len() || reshaped {
            // Their contacts end with exits in the next step.
            self.departed.extend(handles.iter().map(|h| (*h, e)));
            self.remove_colliders(e);
            self.add_colliders(scene, &[e]);
            return;
        }
        let (p, a, s, _) = world_pose(scene, e);
        let body = self
            .owner(scene, e)
            .and_then(|o| self.bodies.get(&o))
            .and_then(|b| self.world.bodies.get(b.handle))
            .map(|b| *b.position());
        for (h, (_, common)) in handles.iter().zip(built) {
            let world = pose(p, a) * pose(common.offset * s, 0.0);
            let Some(c) = self.world.colliders.get_mut(*h) else {
                continue;
            };

            c.set_friction(common.material.friction);
            c.set_restitution(common.material.bounciness);
            if c.is_sensor() != common.is_trigger {
                c.set_sensor(common.is_trigger);
                c.set_active_collision_types(if common.is_trigger {
                    rp::ActiveCollisionTypes::all()
                } else {
                    rp::ActiveCollisionTypes::default()
                });
            }
            match body {
                Some(bp) if c.parent().is_some() => c.set_position_wrt_parent(bp.inv_mul(&world)),
                _ => c.set_position(world),
            }
            if let Some(parent) = c.parent()
                && let Some(b) = self.world.bodies.get_mut(parent)
            {
                // A sleeping body wouldn't notice a new offset or trigger flag.
                b.wake_up(true);
            }
            if let Some(f) = self.followers.get_mut(&e)
                && let Some(slot) = f.handles.iter_mut().find(|(x, _)| x == h)
            {
                slot.1 = common.offset;
            }
        }
    }
}
