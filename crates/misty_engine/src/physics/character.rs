//! Character Controller: a kinematic capsule moved by `Move` / `SimpleMove` through rapier's
//! kinematic character controller (slides along walls, climbs steps and slopes up to the limits).

use glam::Vec3;
use hecs::Entity;
use rapier3d::control::{CharacterAutostep, CharacterLength, KinematicCharacterController};
use rapier3d::prelude as rp;

use super::convert::{gv, pose, rv};
use super::{Body, Physics, layers, world_mirror, world_pose};
use crate::scene::{CharacterController, Scene};

pub(super) struct Controller {
    settings: CharacterController,
    handle: rp::RigidBodyHandle,
    /// The capsule collider (resized by `set_controller_size`).
    collider: rp::ColliderHandle,
    /// Capsule without its centre offset.
    shape: rp::SharedShape,
    /// Capsule centre in the object's frame (scaled).
    center: Vec3,
    layer: u8,
    grounded: bool,
    /// Normal of the ground under the capsule after the last move (up while airborne).
    ground_normal: Vec3,
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

/// Walking speed on a slope relative to flat ground, from Tobler's hiking function
/// (`6·e^(-3.5·|grade + 0.05|)` km/h): fastest on a gentle 3° descent (~1.19×), slower uphill
/// and on steep descents. `grade` is rise over run along the walking direction.
pub fn tobler_factor(grade: f32) -> f32 {
    ((-3.5 * (grade + 0.05).abs()).exp() / (-3.5f32 * 0.05).exp()).max(0.05)
}

/// `motion` with its horizontal part scaled for walking over ground with `normal`, blended by
/// `strength` (0 = unchanged, 1 = Tobler's function).
pub fn slope_scaled(motion: Vec3, normal: Vec3, strength: f32) -> Vec3 {
    let strength = strength.clamp(0.0, 1.0);
    let flat = Vec3::new(motion.x, 0.0, motion.z);
    let len = flat.length();
    if strength <= 0.0 || len < 1e-6 || normal.y < 0.1 {
        return motion;
    }
    let dir = flat / len;
    let grade = -(normal.x * dir.x + normal.z * dir.z) / normal.y;
    let k = 1.0 + strength * (tobler_factor(grade) - 1.0);
    Vec3::new(motion.x * k, motion.y, motion.z * k)
}

impl Physics {
    /// Kinematic body + capsule collider for a Character Controller.
    pub(super) fn add_controller(&mut self, scene: &Scene, e: Entity, cc: CharacterController) {
        let (pos, rot, s) = world_pose(scene, e);
        let radius = (cc.radius * s.x.max(s.z)).clamp(1e-3, super::MAX_EXTENT);
        let half = (cc.height * s.y * 0.5 - radius).clamp(0.0, super::MAX_EXTENT);
        let handle = self.world.insert_body(
            rp::RigidBodyBuilder::kinematic_position_based()
                .pose(pose(pos, rot))
                .build(),
        );
        self.owners.insert(e);
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
        // A mirrored object (negative scale) mirrors the capsule's centre, as collider centres do.
        let center = cc.center * s * world_mirror(scene, e);
        let collider = super::colliders::offset(
            rp::ColliderBuilder::new(shape.clone()).friction(0.0),
            center,
        )
        .collision_groups(layers::groups(&self.settings, layer))
        .active_events(rp::ActiveEvents::COLLISION_EVENTS);
        let collider = self.insert_collider(scene, e, Some(e), collider);
        self.controllers.insert(
            e,
            Controller {
                center,
                settings: cc,
                handle,
                collider,
                shape,
                layer,
                grounded: false,
                ground_normal: Vec3::Y,
                velocity: Vec3::ZERO,
                fall: 0.0,
            },
        );
    }

    pub fn is_character(&self, e: Entity) -> bool {
        self.controllers.contains_key(&e)
    }

    /// The controller's capsule follows its component again after a script changed `height`,
    /// `radius` or `center` (crouching). The object's position stays: lower the centre too
    /// to keep the feet on the floor.
    pub fn set_controller_size(&mut self, scene: &Scene, e: Entity) {
        let Ok(cc) = scene
            .world
            .get::<&CharacterController>(e)
            .map(|c| CharacterController::clone(&c))
        else {
            return;
        };
        let Some(c) = self.controllers.get_mut(&e) else {
            return;
        };
        let (_, _, s) = world_pose(scene, e);
        let radius = (cc.radius * s.x.max(s.z)).clamp(1e-3, super::MAX_EXTENT);
        let half = (cc.height * s.y * 0.5 - radius).clamp(0.0, super::MAX_EXTENT);
        let shape = rp::SharedShape::capsule_y(half, radius);
        let center = cc.center * s * world_mirror(scene, e);
        c.settings = cc;
        c.shape = shape.clone();
        c.center = center;
        if let Some(col) = self.world.colliders.get_mut(c.collider) {
            if center == Vec3::ZERO {
                col.set_shape(shape);
            } else {
                col.set_shape(rp::SharedShape::compound(vec![(
                    pose(center, glam::Quat::IDENTITY),
                    shape,
                )]));
            }
        }
    }

    /// CharacterController.isGrounded: touching the ground after the last move.
    pub fn character_grounded(&self, e: Entity) -> bool {
        self.controllers.get(&e).is_some_and(|c| c.grounded)
    }

    /// CharacterController.groundNormal: the ground's normal under the controller after the
    /// last move (up while airborne).
    pub fn character_ground_normal(&self, e: Entity) -> Vec3 {
        self.controllers
            .get(&e)
            .map_or(Vec3::Y, |c| c.ground_normal)
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
        let motion = if c.grounded {
            slope_scaled(motion, c.ground_normal, c.settings.slope_speed)
        } else {
            motion
        };
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
        let ground_normal = if grounded {
            // Straight down from the capsule's centre, a little past its feet and step reach.
            let reach = c
                .shape
                .as_capsule()
                .map_or(1.0, |cap| cap.half_height() + cap.radius)
                + c.settings.step_offset.max(0.0)
                + c.settings.skin_width.max(1e-4)
                + 0.05;
            let ray = rp::Ray::new(rv(pos + rot * c.center + t), rv(Vec3::NEG_Y));
            self.world
                .cast_ray_and_get_normal(&ray, reach, true, filter)
                .map(|(_, hit)| gv(hit.normal))
                .filter(|n| n.y > 0.1)
                .unwrap_or(Vec3::Y)
        } else {
            Vec3::Y
        };

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
            c.ground_normal = ground_normal;
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
    /// Whether moves left controller hits to report.
    pub fn has_controller_hits(&self) -> bool {
        !self.controller_hits.is_empty()
    }

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
