//! Colliders without a body of their own follow their object's Transform.
//!
//! Static colliders (no Rigidbody on the object or a parent) and colliders on children of a
//! body were placed once when Play started: a door, platform or trigger moved by a script,
//! an Animator or a Timeline kept colliding where it started, and a body's child moved
//! inside it (an animated arm) kept its collider at the old offset. Each step now looks at
//! those objects' frames (world, or relative to their body) and moves the colliders that
//! changed; a changed scale rebuilds their shapes.

use glam::{Mat4, Vec3};
use hecs::Entity;
use rapier3d::prelude as rp;

use super::Physics;
use super::convert::pose;
use super::world_pose;
use crate::scene::Scene;

pub(super) struct Follower {
    /// The object whose body the colliders hang on (`None`: static, in world space).
    owner: Option<Entity>,
    handles: Vec<rp::ColliderHandle>,
    /// The frame the colliders were last placed for (see `frame`).
    last: Mat4,
}

/// Scale changes smaller than this (relative) keep the shapes.
const SCALE_EPS: f32 = 1e-4;

/// `e`'s frame: its world matrix when static, else relative to `owner` (the local matrices
/// between them, so a moving body alone doesn't count as a change).
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
    // Not under its owner any more (re-parented): compare world frames instead.
    scene.world_matrix(e)
}

fn scale_of(m: &Mat4) -> Vec3 {
    Vec3::new(
        m.x_axis.truncate().length(),
        m.y_axis.truncate().length(),
        m.z_axis.truncate().length(),
    )
}

impl Physics {
    /// Collider `h` of object `e` (hanging on `owner`'s body, or static) follows `e`.
    pub(super) fn follow(
        &mut self,
        scene: &Scene,
        e: Entity,
        owner: Option<Entity>,
        h: rp::ColliderHandle,
    ) {
        let last = frame(scene, e, owner);
        let f = self.followers.entry(e).or_insert_with(|| Follower {
            owner,
            handles: Vec::new(),
            last,
        });
        f.owner = owner;
        f.last = last;
        f.handles.push(h);
    }

    /// Collider `h` of object `e` left the world.
    pub(super) fn forget_follower(&mut self, e: Entity, h: rp::ColliderHandle) {
        if let Some(f) = self.followers.get_mut(&e) {
            f.handles.retain(|x| *x != h);
            if f.handles.is_empty() {
                self.followers.remove(&e);
            }
        }
    }

    /// Move (or reshape) the followers whose frame changed since the last step.
    pub(super) fn follow_transforms(&mut self, scene: &Scene) {
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
            let (old, new) = (scale_of(&f.last), scale_of(&now));
            f.last = now;
            if ((new - old).abs() - SCALE_EPS * old.abs().max(Vec3::ONE)).max_element() > 0.0 {
                rescaled.push(e);
            }
            moved.push(e);
        }
        // A fixed order: rapier's results depend on the order colliders change in.
        moved.sort_unstable_by_key(|e| e.to_bits());
        for e in rescaled {
            self.reshape_follower(scene, e);
        }
        for e in moved {
            self.place_follower(scene, e);
        }
    }

    /// Put `e`'s colliders where its Transform is now.
    fn place_follower(&mut self, scene: &Scene, e: Entity) {
        let Some(f) = self.followers.get(&e) else {
            return;
        };
        let (cp, cr, _) = world_pose(scene, e);
        let body = f
            .owner
            .filter(|o| scene.contains(*o))
            .and_then(|o| self.bodies.get(&o).map(|b| (o, b.handle)));
        let iso = match body {
            Some((o, _)) => {
                let (bp, br, _) = world_pose(scene, o);
                pose(br.inverse() * (cp - bp), br.inverse() * cr)
            }
            None => pose(cp, cr),
        };
        for h in &f.handles {
            if let Some(c) = self.world.colliders.get_mut(*h) {
                if c.parent().is_some() {
                    c.set_position_wrt_parent(iso);
                } else {
                    c.set_position(iso);
                }
            }
        }
        // (rapier wakes the bodies touching a moved collider.)
    }

    /// `e`'s scale changed: rebuild its colliders' shapes (sizes include the world scale).
    fn reshape_follower(&mut self, scene: &Scene, e: Entity) {
        let shapes: Vec<rp::SharedShape> = self
            .colliders_for(scene, e)
            .into_iter()
            .map(|(b, _)| b.shape)
            .collect();
        let Some(f) = self.followers.get(&e) else {
            return;
        };
        // The same components in the same order (a component switched on or off in the
        // meantime is rebuilt when it is re-added instead).
        if shapes.len() != f.handles.len() {
            return;
        }
        for (h, shape) in f.handles.iter().zip(shapes) {
            if let Some(c) = self.world.colliders.get_mut(*h) {
                c.set_shape(shape);
            }
        }
    }
}
