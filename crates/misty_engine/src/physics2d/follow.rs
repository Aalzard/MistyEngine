//! 2D colliders and non-dynamic bodies follow their object's Transform.
//!
//! Like 3D (`physics/follow.rs`): colliders without a Rigidbody 2D of their own (static ones,
//! and colliders on children of a body) were placed once when Play started, so a platform,
//! door or trigger moved by a script, an Animator or a Timeline kept colliding where it
//! started. Static and kinematic bodies moved by an Animator or a Timeline (which write the
//! Transform, not the body) stayed put too. Before each step these now catch up with their
//! Transform; colliders change in place (contacts carry on) and are rebuilt only when the
//! object's scale changed.

use glam::{Mat4, Vec2};
use hecs::Entity;
use rapier2d::prelude as rp;

use super::{Physics2D, pose, world_pose};
use crate::scene::{BodyType2D, Scene};

pub(super) struct Follower {
    /// The object whose body the colliders hang on (`None`: static, in world space).
    owner: Option<Entity>,
    /// Each collider with its offset in the object's local space.
    pub(super) handles: Vec<(rp::ColliderHandle, Vec2)>,
    /// The frame the colliders were last placed for (see `frame`).
    last: Mat4,
}

/// `e`'s frame: its world matrix when static, else relative to `owner`.
fn frame(scene: &Scene, e: Entity, owner: Option<Entity>) -> Mat4 {
    let Some(owner) = owner else {
        return scene.world_matrix(e);
    };
    let mut m = Mat4::IDENTITY;
    let mut cur = Some(e);
    while let Some(c) = cur {
        if c == owner {
            return m;
        }
        m = scene.transform(c).local_matrix() * m;
        cur = scene.parent(c);
    }
    scene.world_matrix(e)
}

fn scale_xy(m: &Mat4) -> Vec2 {
    Vec2::new(m.x_axis.truncate().length(), m.y_axis.truncate().length())
}

/// Position (metres) and angle (radians) differences that count as a move.
const MOVE_EPS: f32 = 1e-4;

impl Physics2D {
    /// Collider `h` of object `e` (offset in its local space) hangs on `owner`, or is static.
    pub(super) fn follow(
        &mut self,
        scene: &Scene,
        e: Entity,
        owner: Option<Entity>,
        h: rp::ColliderHandle,
        offset: Vec2,
    ) {
        let last = frame(scene, e, owner);
        let f = self.followers.entry(e).or_insert_with(|| Follower {
            owner,
            handles: Vec::new(),
            last,
        });
        f.owner = owner;
        f.last = last;
        f.handles.push((h, offset));
    }

    /// Catch moved colliders and non-dynamic bodies up with their Transforms. Call before a
    /// step.
    pub fn follow_transforms(&mut self, scene: &Scene) {
        self.follow_bodies(scene);
        if self.followers.is_empty() {
            return;
        }
        let mut moved = Vec::new();
        let mut rescaled = Vec::new();
        for (&e, f) in &mut self.followers {
            if !scene.contains(e) {
                continue;
            }
            let now = frame(scene, e, f.owner);
            if now == f.last || !now.is_finite() {
                continue;
            }
            let (old, new) = (scale_xy(&f.last), scale_xy(&now));
            f.last = now;
            if ((new - old).abs() - 1e-4 * old.abs().max(Vec2::ONE)).max_element() > 0.0 {
                rescaled.push(e);
            } else {
                moved.push(e);
            }
        }
        // A fixed order: rapier's results depend on the order colliders change in.
        moved.sort_unstable_by_key(|e| e.to_bits());
        rescaled.sort_unstable_by_key(|e| e.to_bits());
        for e in moved {
            self.place_follower(scene, e);
        }
        for e in rescaled {
            self.remove_colliders(e);
            self.add_colliders(scene, &[e]);
        }
    }

    /// Put `e`'s colliders where its Transform is now.
    fn place_follower(&mut self, scene: &Scene, e: Entity) {
        let Some(f) = self.followers.get(&e) else {
            return;
        };
        let (p, a, s, _) = world_pose(scene, e);
        let body = f
            .owner
            .and_then(|o| self.bodies.get(&o))
            .and_then(|b| self.world.bodies.get(b.handle))
            .map(|b| *b.position());
        for (h, offset) in &f.handles {
            let world = pose(p, a) * pose(*offset * s, 0.0);
            if let Some(c) = self.world.colliders.get_mut(*h) {
                match body {
                    Some(bp) if c.parent().is_some() => {
                        c.set_position_wrt_parent(bp.inv_mul(&world))
                    }
                    _ => c.set_position(world),
                }
            }
        }
    }

    /// Static and kinematic bodies whose Transform was moved by something other than a script
    /// (an Animator, a Timeline) jump there; scripts already move them when they set it. Only
    /// a Transform that changed since the last look and isn't where the body already is counts
    /// (several steps in one frame run before the Transforms are written back).
    fn follow_bodies(&mut self, scene: &Scene) {
        let differs = |(p, a): (Vec2, f32), (q, b): (Vec2, f32)| {
            let da = (a - b + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU)
                - std::f32::consts::PI;
            (p - q).length() > MOVE_EPS || da.abs() > MOVE_EPS
        };
        for (e, b) in &mut self.bodies {
            if b.kind == BodyType2D::Dynamic || b.target.is_some() || !scene.contains(*e) {
                continue;
            }
            let (p, a, _, _) = world_pose(scene, *e);
            if !differs((p, a), b.seen) {
                continue;
            }
            b.seen = (p, a);
            let Some(body) = self.world.bodies.get_mut(b.handle) else {
                continue;
            };
            let cur = body.position();
            if differs((p, a), (super::gv(cur.translation), cur.rotation.angle())) {
                body.set_position(pose(p, a), true);
            }
        }
    }
}
