//! 2D collider shapes from Box/Circle/Capsule/Polygon/Edge Collider 2D components, attached to
//! the closest Rigidbody 2D up the hierarchy (static otherwise), with groups from the layer.

use glam::Vec2;
use hecs::Entity;
use rapier2d::prelude as rp;

use super::{Physics2D, pose, rv, world_pose};
use crate::scene::{
    BoxCollider2D, CapsuleCollider2D, CapsuleDirection2D, CircleCollider2D, Collider2DCommon,
    EdgeCollider2D, PolygonCollider2D, Scene,
};

/// Smallest size kept for degenerate shapes.
const MIN: f32 = 1e-3;

/// Collision groups of a collider on `layer` (the 3D Layer Collision Matrix).
fn groups(layers: &crate::settings::PhysicsSettings, layer: u8) -> rp::InteractionGroups {
    let layer = layer.min(31);
    rp::InteractionGroups::new(
        rp::Group::from_bits_retain(1 << layer),
        rp::Group::from_bits_retain(layers.collision_mask(layer)),
        rp::InteractionTestMode::And,
    )
}

/// The outline a Polygon Collider 2D uses: its points, else the sprite rect, else a unit square.
pub fn polygon_points(scene: &Scene, e: Entity, c: &PolygonCollider2D) -> Vec<Vec2> {
    if c.points.len() >= 3 {
        return c.points.clone();
    }
    let (min, max) =
        crate::sprites::local_bounds(scene, e).unwrap_or((Vec2::splat(-0.5), Vec2::splat(0.5)));
    vec![min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)]
}

impl Physics2D {
    pub(super) fn add_colliders(&mut self, scene: &Scene, objects: &[Entity]) {
        for &e in objects {
            let owner = self.owner(scene, e);
            let groups = groups(&self.layers, scene.game_object(e).layer);
            for (builder, common) in self.shapes(scene, e) {
                let mut b = builder
                    .density(0.0)
                    .friction(common.material.friction)
                    .restitution(common.material.bounciness)
                    .sensor(common.is_trigger)
                    .collision_groups(groups)
                    .active_events(rp::ActiveEvents::COLLISION_EVENTS);
                if common.is_trigger {
                    // Triggers also fire for kinematic and static bodies.
                    b = b.active_collision_types(rp::ActiveCollisionTypes::all());
                }
                self.insert(scene, e, owner, b, common.offset);
            }
        }
    }

    /// Insert a collider of `e` (offset in its local space) on `owner`'s body or in the world.
    fn insert(
        &mut self,
        scene: &Scene,
        e: Entity,
        owner: Option<Entity>,
        b: rp::ColliderBuilder,
        offset: Vec2,
    ) {
        let (p, a, s, _) = world_pose(scene, e);
        let world = pose(p, a) * pose(offset * s, 0.0);
        let parent = owner.and_then(|o| self.bodies.get(&o)).map(|b| b.handle);
        let h = match parent.and_then(|h| Some((h, *self.world.bodies.get(h)?.position()))) {
            Some((h, body)) => {
                let c = b.position(body.inv_mul(&world)).build();
                self.world.insert_collider(c, Some(h))
            }
            None => self.world.insert_collider(b.position(world).build(), None),
        };
        self.colliders.insert(h, e);
    }

    /// The object whose body `e`'s colliders attach to: itself or the closest parent with one.
    fn owner(&self, scene: &Scene, e: Entity) -> Option<Entity> {
        let mut cur = Some(e);
        while let Some(c) = cur {
            if self.bodies.contains_key(&c) {
                return Some(c);
            }
            cur = scene.parent(c);
        }
        None
    }

    /// Move `e`'s colliders to another layer (a script changed `gameObject.layer`).
    pub fn set_layer(&mut self, e: Entity, layer: u8) {
        let g = groups(&self.layers, layer);
        for (h, owner) in &self.colliders {
            if *owner == e
                && let Some(c) = self.world.colliders.get_mut(*h)
            {
                c.set_collision_groups(g);
            }
        }
    }

    /// Shapes of `e`'s enabled 2D colliders, scaled by its world scale.
    fn shapes(&mut self, scene: &Scene, e: Entity) -> Vec<(rp::ColliderBuilder, Collider2DCommon)> {
        let w = &scene.world;
        let (_, _, s, _) = world_pose(scene, e);
        let mut out = Vec::new();
        if let Ok(c) = w.get::<&BoxCollider2D>(e)
            && c.common.enabled
        {
            let h = (c.size * s * 0.5).max(Vec2::splat(MIN));
            let b = if c.edge_radius > 0.0 {
                rp::ColliderBuilder::round_cuboid(h.x, h.y, c.edge_radius)
            } else {
                rp::ColliderBuilder::cuboid(h.x, h.y)
            };
            out.push((b, c.common));
        }
        if let Ok(c) = w.get::<&CircleCollider2D>(e)
            && c.common.enabled
        {
            let r = (c.radius * s.max_element()).max(MIN);
            out.push((rp::ColliderBuilder::ball(r), c.common));
        }
        if let Ok(c) = w.get::<&CapsuleCollider2D>(e)
            && c.common.enabled
        {
            let size = (c.size * s).max(Vec2::splat(MIN));
            let b = match c.direction {
                CapsuleDirection2D::Vertical => {
                    let r = size.x * 0.5;
                    rp::ColliderBuilder::capsule_y((size.y * 0.5 - r).max(0.0), r)
                }
                CapsuleDirection2D::Horizontal => {
                    let r = size.y * 0.5;
                    rp::ColliderBuilder::capsule_x((size.x * 0.5 - r).max(0.0), r)
                }
            };
            out.push((b, c.common));
        }
        if let Ok(c) = w.get::<&PolygonCollider2D>(e)
            && c.common.enabled
        {
            let pts: Vec<rp::Vector> = polygon_points(scene, e, &c)
                .into_iter()
                .map(|p| rv(p * s))
                .collect();
            match polygon(&pts) {
                Some(b) => out.push((b, c.common)),
                None => self.warnings.push(format!(
                    "'{}': Polygon Collider 2D needs at least 3 distinct points.",
                    scene.name(e)
                )),
            }
        }
        if let Ok(c) = w.get::<&crate::tilemap::TilemapCollider2D>(e)
            && c.common.enabled
            && let Some(b) = crate::tilemap::collider::builder(scene, e, &c, s)
        {
            out.push((b, c.common));
        }
        if let Ok(c) = w.get::<&EdgeCollider2D>(e)
            && c.common.enabled
        {
            let pts: Vec<rp::Vector> = c.points.iter().map(|p| rv(*p * s)).collect();
            if pts.len() < 2 {
                self.warnings.push(format!(
                    "'{}': Edge Collider 2D needs at least 2 points.",
                    scene.name(e)
                ));
            } else if c.edge_radius > 0.0 {
                // Rounded edges: one capsule per segment.
                let parts = pts
                    .windows(2)
                    .map(|w| {
                        let shape = rp::SharedShape::capsule(w[0], w[1], c.edge_radius);
                        (rp::Pose::IDENTITY, shape)
                    })
                    .collect();
                out.push((rp::ColliderBuilder::compound(parts), c.common));
            } else {
                out.push((rp::ColliderBuilder::polyline(pts, None), c.common));
            }
        }
        out
    }
}

/// A closed outline as one convex shape, or convex pieces when it is concave. Non-finite
/// points and repeated neighbours are dropped first (a NaN point sent the outline to the
/// convex decomposition, which asserted on zero pieces); None under 3 points or no area.
fn polygon(pts: &[rp::Vector]) -> Option<rp::ColliderBuilder> {
    let mut clean: Vec<rp::Vector> = Vec::with_capacity(pts.len());
    for p in pts.iter().filter(|p| p.is_finite()) {
        if clean.last().is_none_or(|q| (*p - *q).length_squared() > 1e-12) {
            clean.push(*p);
        }
    }
    while clean.len() > 1 && (clean[0] - clean[clean.len() - 1]).length_squared() <= 1e-12 {
        clean.pop();
    }
    let pts = clean.as_slice();
    if pts.len() < 3 {
        return None;
    }
    let n = pts.len();
    let area: f32 = (0..n).map(|i| pts[i].perp_dot(pts[(i + 1) % n])).sum();
    if !area.is_finite() || area.abs() < 1e-9 {
        return None;
    }
    if is_convex(pts) {
        return rp::ColliderBuilder::convex_hull(pts);
    }
    let n = pts.len() as u32;
    let segments: Vec<[u32; 2]> = (0..n).map(|i| [i, (i + 1) % n]).collect();
    Some(rp::ColliderBuilder::convex_decomposition(pts, &segments))
}

/// Every turn of the outline goes the same way.
fn is_convex(pts: &[rp::Vector]) -> bool {
    let n = pts.len();
    let mut sign = 0.0f32;
    for i in 0..n {
        let (a, b, c) = (pts[i], pts[(i + 1) % n], pts[(i + 2) % n]);
        let cross = (b - a).perp_dot(c - b);
        if cross.abs() < 1e-9 {
            continue;
        }
        if sign == 0.0 {
            sign = cross.signum();
        } else if cross.signum() != sign {
            return false;
        }
    }
    true
}
