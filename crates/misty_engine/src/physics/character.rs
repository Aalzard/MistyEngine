//! Character Controller: a kinematic capsule moved by `Move` / `SimpleMove` through rapier's
//! kinematic character controller (slides along walls, climbs steps and slopes up to the limits).

use glam::Vec3;
use hecs::Entity;
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude as rp;

use super::convert::{gv, pose, rv};
use super::{Body, Physics, layers, world_pose};
use crate::scene::{CharacterController, Scene};

pub(super) struct Controller {
    settings: CharacterController,
    handle: rp::RigidBodyHandle,
    /// Capsule without its centre offset.
    shape: rp::SharedShape,
    /// Capsule centre in the object's frame (scaled).
    center: Vec3,
    layer: u8,
    grounded: bool,
    velocity: Vec3,
    /// SimpleMove's vertical speed (gravity accumulates while airborne).
    fall: f32,
}

/// The controller touched a collider while moving (OnControllerColliderHit).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerHit {
    pub controller: Entity,
    /// Object of the collider that was hit.
    pub other: Entity,
    pub point: Vec3,
    pub normal: Vec3,
    pub move_direction: Vec3,
    /// How far the controller moved before the hit.
    pub move_length: f32,
}

impl Controller {
    fn kcc(&self) -> KinematicCharacterController {
        let s = &self.settings;
        let r = self.shape.as_capsule().map_or(0.5, |c| c.radius);
        let slope = s.slope_limit.clamp(0.0, 90.0).to_radians();
        let step = s.step_offset.max(0.0);
        KinematicCharacterController {
            up: rp::Vector::Y,
            offset: CharacterLength::Absolute(s.skin_width.max(1e-4)),
            slide: true,
            autostep: (step > 0.0).then_some(CharacterAutostep {
                max_height: CharacterLength::Absolute(step),
                min_width: CharacterLength::Absolute(r * 0.5),
                include_dynamic_bodies: false,
            }),
            max_slope_climb_angle: slope,
            min_slope_slide_angle: slope,
            snap_to_ground: (step > 0.0).then_some(CharacterLength::Absolute(step)),
            ..Default::default()
        }
    }
}

impl Physics {
    /// Kinematic body + capsule collider for a Character Controller.
    pub(super) fn add_controller(&mut self, scene: &Scene, e: Entity, cc: CharacterController) {
        let (pos, rot, s) = world_pose(scene, e);
        let radius = (cc.radius * s.x.max(s.z)).max(1e-3);
        let half = (cc.height * s.y * 0.5 - radius).max(0.0);
        let handle = self.world.insert_body(
            rp::RigidBodyBuilder::kinematic_position_based()
                .pose(pose(pos, rot))
                .build(),
        );
        self.bodies.insert(
            e,
            Body {
                handle,
                kinematic: true,
                interpolate: false,
                prev: (pos, rot),
            },
        );
        let layer = scene.game_object(e).layer;
        let shape = rp::SharedShape::capsule_y(half, radius);
        let collider = super::colliders::offset(
            rp::ColliderBuilder::new(shape.clone()).friction(0.0),
            cc.center * s,
        )
        .collision_groups(layers::groups(&self.settings, layer))
        .active_events(rp::ActiveEvents::COLLISION_EVENTS);
        self.insert_collider(scene, e, Some(e), collider);
        self.controllers.insert(
            e,
            Controller {
                center: cc.center * s,
                settings: cc,
                handle,
                shape,
                layer,
                grounded: false,
                velocity: Vec3::ZERO,
                fall: 0.0,
            },
        );
    }

    pub fn is_character(&self, e: Entity) -> bool {
        self.controllers.contains_key(&e)
    }

    /// CharacterController.isGrounded: touching the ground after the last move.
    pub fn character_grounded(&self, e: Entity) -> bool {
        self.controllers.get(&e).is_some_and(|c| c.grounded)
    }

    /// CharacterController.velocity: the last move's displacement per second.
    pub fn character_velocity(&self, e: Entity) -> Vec3 {
        self.controllers.get(&e).map_or(Vec3::ZERO, |c| c.velocity)
    }

    /// CharacterController.Move: move by `motion` (world space) with collisions, update the
    /// Transform at once and return whether the controller is grounded.
    pub fn move_character(&mut self, scene: &mut Scene, e: Entity, motion: Vec3, dt: f32) -> bool {
        let Some(c) = self.controllers.get(&e) else {
            return false;
        };
        if !scene.contains(e) || !motion.is_finite() {
            return c.grounded;
        }
        if motion.length() < c.settings.min_move_distance {
            return c.grounded;
        }
        let (pos, rot, _) = world_pose(scene, e);
        let mask = self.settings.collision_mask(c.layer);
        let test = move |_: rp::ColliderHandle, col: &rp::Collider| {
            col.collision_groups().memberships.bits() & mask != 0
        };
        let filter = rp::QueryFilter::default()
            .exclude_rigid_body(c.handle)
            .exclude_sensors()
            .predicate(&test);
        let mut hits = Vec::new();
        let moved = c.kcc().move_shape(
            dt.max(1e-4),
            &self.world.query_pipeline_with_filter(filter),
            &*c.shape,
            &pose(pos + rot * c.center, rot),
            rv(motion),
            |hit| hits.push(hit),
        );
        let t = gv(moved.translation);
        let grounded = moved.grounded;
        let handle = c.handle;

        let new_pos = pos + t;
        scene.set_world_position(e, new_pos);
        if let Some(body) = self.world.bodies.get_mut(handle) {
            body.set_next_kinematic_position(pose(new_pos, rot));
        }
        let mut seen = Vec::new();
        for hit in hits {
            let Some(&other) = self.colliders.get(&hit.handle) else {
                continue;
            };
            if seen.contains(&hit.handle) {
                continue;
            }
            seen.push(hit.handle);
            self.controller_hits.push(ControllerHit {
                controller: e,
                other,
                point: gv(hit.hit.witness1),
                normal: gv(hit.hit.normal1),
                move_direction: motion.normalize_or_zero(),
                move_length: gv(hit.translation_applied).length(),
            });
        }
        if let Some(c) = self.controllers.get_mut(&e) {
            c.grounded = grounded;
            c.velocity = if dt > 0.0 { t / dt } else { Vec3::ZERO };
        }
        grounded
    }

    /// CharacterController.SimpleMove: walk at `speed` (units/second, Y ignored) with gravity.
    pub fn simple_move(&mut self, scene: &mut Scene, e: Entity, speed: Vec3, dt: f32) -> bool {
        let gravity = self.gravity().y;
        let Some(c) = self.controllers.get(&e) else {
            return false;
        };
        let fall = if c.grounded { 0.0 } else { c.fall } + gravity * dt;
        let motion = Vec3::new(speed.x, 0.0, speed.z) * dt + Vec3::Y * fall * dt;
        let grounded = self.move_character(scene, e, motion, dt);
        if let Some(c) = self.controllers.get_mut(&e) {
            c.fall = if grounded { 0.0 } else { fall };
        }
        grounded
    }

    /// Controller hits since the last call (for OnControllerColliderHit).
    pub fn take_controller_hits(&mut self) -> Vec<ControllerHit> {
        std::mem::take(&mut self.controller_hits)
    }

    /// Keep a controller's layer in step with `set_layer`.
    pub(super) fn set_controller_layer(&mut self, e: Entity, layer: u8) {
        if let Some(c) = self.controllers.get_mut(&e) {
            c.layer = layer;
        }
    }
}
