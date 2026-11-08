//! Target Group: several objects a camera treats as one target. Each member has a weight (how
//! strongly it counts) and a radius (how much room it needs on screen). While playing, the
//! group object sits at the group's centre, so any body or aim can follow it; the Group
//! Composer also frames the group's bounding sphere.

use glam::Vec3;
use hecs::Entity;
use serde::{Deserialize, Serialize};

use crate::scene::components::entity_bits;
use crate::scene::{Scene, SceneComponent};

/// Where the group object is placed.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum GroupPositionMode {
    /// The centre of the members' bounding box.
    #[default]
    GroupCenter,
    /// The weighted average of the members' positions.
    GroupAverage,
}

impl GroupPositionMode {
    pub const ALL: [GroupPositionMode; 2] = [Self::GroupCenter, Self::GroupAverage];

    pub fn label(self) -> &'static str {
        match self {
            Self::GroupCenter => "Group Center",
            Self::GroupAverage => "Group Average",
        }
    }
}

/// One object in a group.
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GroupMember {
    #[serde(with = "entity_bits::option")]
    pub target: Option<Entity>,
    /// 0 = ignored, 1 = full; in between the member is pulled towards the average.
    pub weight: f32,
    /// Room the member needs around its position.
    pub radius: f32,
}

impl Default for GroupMember {
    fn default() -> Self {
        Self {
            target: None,
            weight: 1.0,
            radius: 0.5,
        }
    }
}

/// Target Group component.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct TargetGroup {
    pub position_mode: GroupPositionMode,
    pub members: Vec<GroupMember>,
}

impl SceneComponent for TargetGroup {
    const NAME: &'static str = "Target Group";
    const CATEGORY: &'static str = "Rendering";
}

/// A group's extent this frame.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupBounds {
    /// Centre of the bounding box of the (weighted) members with their radii.
    pub center: Vec3,
    /// Radius of the sphere around `center` holding every member with its radius.
    pub radius: f32,
    /// Weighted average of the member positions.
    pub average: Vec3,
}

impl GroupBounds {
    /// The point the group object stands on.
    pub fn position(&self, mode: GroupPositionMode) -> Vec3 {
        match mode {
            GroupPositionMode::GroupCenter => self.center,
            GroupPositionMode::GroupAverage => self.average,
        }
    }
}

impl TargetGroup {
    /// Members that count: in the scene, active and with a positive weight, as
    /// (position, weight, radius).
    fn live_members(&self, scene: &Scene, me: Entity) -> Vec<(Vec3, f32, f32)> {
        self.members
            .iter()
            .filter(|m| m.weight > 0.0)
            .filter_map(|m| {
                let t = m.target.filter(|t| *t != me && scene.contains(*t))?;
                scene
                    .active_in_hierarchy(t)
                    .then(|| (scene.world_position(t), m.weight, m.radius.max(0.0)))
            })
            .collect()
    }

    /// Bounds of the group (`me` is the group's own object), `None` without live members.
    pub fn bounds(&self, scene: &Scene, me: Entity) -> Option<GroupBounds> {
        let live = self.live_members(scene, me);
        if live.is_empty() {
            return None;
        }
        let total: f32 = live.iter().map(|m| m.1).sum();
        let average = live.iter().map(|m| m.0 * m.1).sum::<Vec3>() / total.max(1e-6);
        // Light members are pulled towards the average, so they matter less to the framing.
        let pts: Vec<(Vec3, f32)> = live
            .iter()
            .map(|&(p, w, r)| (average.lerp(p, w.min(1.0)), r * w.min(1.0)))
            .collect();
        let mut lo = Vec3::splat(f32::MAX);
        let mut hi = Vec3::splat(f32::MIN);
        for &(p, r) in &pts {
            lo = lo.min(p - Vec3::splat(r));
            hi = hi.max(p + Vec3::splat(r));
        }
        let center = (lo + hi) * 0.5;
        let radius = pts
            .iter()
            .map(|&(p, r)| p.distance(center) + r)
            .fold(0.0, f32::max);
        Some(GroupBounds {
            center,
            radius,
            average,
        })
    }
}

/// The bounds of the Target Group on `e`, if it has one with live members.
pub fn group_bounds(scene: &Scene, e: Entity) -> Option<GroupBounds> {
    let g = scene.world.get::<&TargetGroup>(e).ok()?;
    g.bounds(scene, e)
}

/// Move every active Target Group object to its group's position (Play mode, before cameras
/// solve).
pub fn place_groups(scene: &mut Scene) {
    let groups: Vec<(Entity, Vec3)> = scene
        .active_with::<TargetGroup>()
        .into_iter()
        .filter_map(|e| {
            let g = scene.world.get::<&TargetGroup>(e).ok()?;
            let b = g.bounds(scene, e)?;
            Some((e, b.position(g.position_mode)))
        })
        .collect();
    for (e, p) in groups {
        scene.set_world_position(e, p);
    }
}
