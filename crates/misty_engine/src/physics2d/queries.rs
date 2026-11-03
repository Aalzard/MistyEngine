//! 2D scene queries: Physics2D.Raycast / RaycastAll / OverlapCircle / OverlapBox / OverlapPoint.
//!
//! Masks select layers by bit. Triggers are hit unless Physics 2D ▸ Queries Hit Triggers is off;
//! rays starting inside a collider report it at distance 0 unless Queries Start In Colliders is
//! off (then it is skipped).

use glam::Vec2;
use hecs::Entity;
use rapier2d::prelude as rp;

use super::{Physics2D, gv, pose, rv};

/// One ray hit.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct RaycastHit2D {
    /// Object of the collider that was hit.
    pub entity: Entity,
    /// Object whose Rigidbody 2D the collider belongs to, if any.
    pub body: Option<Entity>,
    pub point: Vec2,
    pub normal: Vec2,
    pub distance: f32,
    /// Distance as a fraction of the ray length (0 for infinite rays).
    pub fraction: f32,
}

/// Predicate: the collider's layer is in `mask`.
fn in_mask(mask: u32) -> impl Fn(rp::ColliderHandle, &rp::Collider) -> bool {
    move |_, c| c.collision_groups().memberships.bits() & mask != 0
}

impl Physics2D {
    fn filter<'a, F>(&self, test: &'a F) -> rp::QueryFilter<'a>
    where
        F: Fn(rp::ColliderHandle, &rp::Collider) -> bool,
    {
        let f = rp::QueryFilter::default().predicate(test);
        if self.settings.queries_hit_triggers {
            f
        } else {
            f.exclude_sensors()
        }
    }

    /// The object with the Rigidbody 2D a collider is attached to.
    fn body_of(&self, h: rp::ColliderHandle) -> Option<Entity> {
        let parent = self.world.colliders.get(h)?.parent()?;
        self.bodies
            .iter()
            .find(|(_, b)| b.handle == parent)
            .map(|(e, _)| *e)
    }

    /// Every collider along the ray within `distance`, nearest first.
    pub fn raycast_all(
        &self,
        origin: Vec2,
        direction: Vec2,
        distance: f32,
        mask: u32,
    ) -> Vec<RaycastHit2D> {
        let dir = direction.normalize_or_zero();
        if dir == Vec2::ZERO || distance.is_nan() || distance < 0.0 {
            return Vec::new();
        }
        let test = in_mask(mask);
        let ray = rp::Ray::new(rv(origin), rv(dir));
        let inside_ok = self.settings.queries_start_in_colliders;
        let mut hits: Vec<RaycastHit2D> = self
            .world
            .intersect_ray(ray, distance.min(f32::MAX), true, self.filter(&test))
            .filter(|(_, _, i)| inside_ok || i.time_of_impact > 0.0)
            .filter_map(|(h, _, i)| {
                let t = i.time_of_impact;
                let normal = if t == 0.0 { -dir } else { gv(i.normal) };
                Some(RaycastHit2D {
                    entity: *self.colliders.get(&h)?,
                    body: self.body_of(h),
                    point: origin + dir * t,
                    normal,
                    distance: t,
                    fraction: if distance.is_finite() && distance > 0.0 {
                        t / distance
                    } else {
                        0.0
                    },
                })
            })
            .collect();
        hits.sort_by(|a, b| a.distance.total_cmp(&b.distance));
        hits
    }

    /// The nearest collider along the ray.
    pub fn raycast(
        &self,
        origin: Vec2,
        direction: Vec2,
        distance: f32,
        mask: u32,
    ) -> Option<RaycastHit2D> {
        self.raycast_all(origin, direction, distance, mask)
            .into_iter()
            .next()
    }

    /// Objects of colliders overlapping a shape placed at `center` rotated by `degrees`.
    fn overlap(&self, shape: &dyn rp::Shape, center: Vec2, degrees: f32, mask: u32) -> Vec<Entity> {
        let test = in_mask(mask);
        self.world
            .intersect_shape(
                pose(center, degrees.to_radians()),
                shape,
                self.filter(&test),
            )
            .filter_map(|(h, _)| self.colliders.get(&h).copied())
            .collect()
    }

    pub fn overlap_circle_all(&self, center: Vec2, radius: f32, mask: u32) -> Vec<Entity> {
        if radius.is_nan() || radius < 0.0 {
            return Vec::new();
        }
        self.overlap(&rp::Ball::new(radius.max(1e-4)), center, 0.0, mask)
    }

    pub fn overlap_box_all(
        &self,
        center: Vec2,
        size: Vec2,
        degrees: f32,
        mask: u32,
    ) -> Vec<Entity> {
        let h = (size * 0.5).max(Vec2::splat(1e-4));
        self.overlap(&rp::Cuboid::new(rv(h)), center, degrees, mask)
    }

    pub fn overlap_point_all(&self, point: Vec2, mask: u32) -> Vec<Entity> {
        let test = in_mask(mask);
        self.world
            .intersect_point(rv(point), self.filter(&test))
            .filter_map(|(h, _)| self.colliders.get(&h).copied())
            .collect()
    }
}
