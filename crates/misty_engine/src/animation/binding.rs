//! Binding curves to scene objects: child paths, reading and writing animated properties.

use glam::Vec3;
use hecs::Entity;

use super::clip::{AnimationClip, Property};
use crate::scene::{Light, Scene};

/// The object at `path` below `root` ("" = root, "Arm/Hand" = child Arm's child Hand).
pub fn resolve(scene: &Scene, root: Entity, path: &str) -> Option<Entity> {
    let mut cur = root;
    for name in path.split('/').filter(|s| !s.is_empty()) {
        cur = scene
            .children(cur)
            .into_iter()
            .find(|c| scene.game_object(*c).name == name)?;
    }
    scene.contains(cur).then_some(cur)
}

/// Path of `e` relative to `root`, or `None` if `e` is not `root` or below it.
pub fn path_of(scene: &Scene, root: Entity, e: Entity) -> Option<String> {
    let mut names = Vec::new();
    let mut cur = e;
    while cur != root {
        names.push(scene.name(cur));
        cur = scene.parent(cur)?;
    }
    names.reverse();
    Some(names.join("/"))
}

/// Whether `e` has the component `p` animates.
pub fn has(scene: &Scene, e: Entity, p: Property) -> bool {
    match p.component() {
        "Light" => scene.world.get::<&Light>(e).is_ok(),
        "Humanoid" => false,
        "SkinnedMeshRenderer" => scene
            .world
            .get::<&crate::scene::SkinnedMeshRenderer>(e)
            .is_ok_and(|r| match p {
                Property::BlendShape(i) => (i as usize) < r.blend_shape_count(),
                _ => false,
            }),
        _ => scene.contains(e),
    }
}

/// Current value of `p` on `e` (booleans are 0 or 1).
pub fn read(scene: &Scene, e: Entity, p: Property) -> Option<f32> {
    if !scene.contains(e) {
        return None;
    }
    let t = scene.world.get::<&crate::scene::Transform>(e).ok()?;
    let axis = |v: Vec3, i: u8| v.to_array().get(i as usize).copied();
    match p {
        Property::BlendShape(i) => scene
            .world
            .get::<&crate::scene::SkinnedMeshRenderer>(e)
            .ok()
            .map(|r| r.blend_shape_weight(i as usize)),
        Property::Position(i) => axis(t.position, i),
        Property::Rotation(i) => axis(t.euler(), i),
        Property::Scale(i) => axis(t.scale, i),
        Property::Active => Some(if scene.game_object(e).active {
            1.0
        } else {
            0.0
        }),
        // Humanoid curves are resolved through an Avatar, never read from an object.
        Property::HumanRotation(_) | Property::HumanPosition(_) => None,
        Property::LightIntensity | Property::LightRange | Property::LightColor(_) => {
            let l = scene.world.get::<&Light>(e).ok()?;
            match p {
                Property::LightIntensity => Some(l.intensity),
                Property::LightRange => Some(l.range),
                Property::LightColor(c) => l.color.get(c as usize).copied(),
                _ => None,
            }
        }
    }
}

/// Write `value` to `p` on `e`. Rotation axes go through Euler angles, so several axis curves
/// can be applied one after another.
pub fn write(scene: &mut Scene, e: Entity, p: Property, value: f32) {
    if !scene.contains(e) {
        return;
    }
    match p {
        Property::Position(i) | Property::Rotation(i) | Property::Scale(i) => {
            let mut t = scene.transform(e);
            let i = (i as usize).min(2);
            match p {
                Property::Position(_) => t.position[i] = value,
                Property::Scale(_) => t.scale[i] = value,
                _ => {
                    let mut euler = t.euler();
                    euler[i] = value;
                    t.set_euler(euler);
                }
            }
            scene.set_transform(e, t);
        }
        Property::Active => scene.game_object_mut(e).active = value >= 0.5,
        Property::HumanRotation(_) | Property::HumanPosition(_) => {}
        Property::BlendShape(i) => {
            if let Ok(mut r) = scene.world.get::<&mut crate::scene::SkinnedMeshRenderer>(e) {
                r.set_blend_shape_weight(i as usize, value);
            }
        }
        Property::LightIntensity | Property::LightRange | Property::LightColor(_) => {
            if let Ok(mut l) = scene.world.get::<&mut Light>(e) {
                match p {
                    Property::LightIntensity => l.intensity = value.max(0.0),
                    Property::LightRange => l.range = value.max(0.0),
                    Property::LightColor(c) => {
                        if let Some(ch) = l.color.get_mut(c as usize) {
                            *ch = value.clamp(0.0, 1.0);
                        }
                    }
                    _ => {}
                }
            }
        }
    }
}

/// Pose the objects below `root` as the clip is at `time` (curves with missing objects are skipped).
pub fn sample(scene: &mut Scene, root: Entity, clip: &AnimationClip, time: f32) {
    for curve in &clip.curves {
        if curve.keys.is_empty() {
            continue;
        }
        if let Some(e) = resolve(scene, root, &curve.path) {
            write(scene, e, curve.property, curve.evaluate(time));
        }
    }
}
