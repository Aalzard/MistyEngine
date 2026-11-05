//! Scene-view gizmos of a selected Skinned Mesh Renderer: the bounds used for culling (a world
//! line box) and the skeleton, painted over the image so bones inside the mesh stay visible —
//! a line from each bone to its parent bone and a dot at every joint.

use egui::{Color32, Rect, Stroke, Ui};
use glam::Vec3;
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Scene, SkinnedMeshRenderer};

use super::Lines;
use crate::picking::to_screen;
use crate::selection::Selection;

const BOUNDS: [f32; 4] = [0.75, 0.75, 0.75, 0.35];
const BONE: Color32 = Color32::from_rgb(90, 210, 255);
const JOINT: Color32 = Color32::from_rgb(235, 245, 255);

impl Lines {
    /// Bounds box in the root bone's space.
    pub(super) fn bones(&mut self, scene: &Scene, e: Entity) {
        let Ok(r) = scene.world.get::<&SkinnedMeshRenderer>(e) else {
            return;
        };
        if let Some(root) = r.root_bone.filter(|b| scene.contains(*b)) {
            let m = scene.world_matrix(root);
            self.wire_box(&m, r.bounds_center, r.bounds_extents.abs(), BOUNDS);
        }
    }
}

/// Bone segments of every selected Skinned Mesh Renderer as (parent joint, joint) pairs, plus
/// the joints, in world space.
pub fn skeleton(scene: &Scene, selection: &Selection) -> (Vec<(Vec3, Vec3)>, Vec<Vec3>) {
    let mut segments = Vec::new();
    let mut joints = Vec::new();
    // The selected objects and everything below them (selecting the model root shows it too).
    let renderers: Vec<Entity> = scene
        .world
        .query::<(Entity, &SkinnedMeshRenderer)>()
        .iter()
        .map(|(e, _)| e)
        .filter(|e| selection.list().iter().any(|s| scene.is_ancestor(*s, *e)))
        .collect();
    for e in renderers {
        let Ok(r) = scene.world.get::<&SkinnedMeshRenderer>(e) else {
            continue;
        };
        let bones: Vec<Entity> = r
            .bones
            .iter()
            .flatten()
            .copied()
            .filter(|b| scene.contains(*b))
            .collect();
        for &b in &bones {
            let p = scene.world_position(b);
            joints.push(p);
            // To the nearest ancestor that is also a bone.
            let mut cur = scene.parent(b);
            while let Some(a) = cur {
                if bones.contains(&a) {
                    segments.push((scene.world_position(a), p));
                    break;
                }
                cur = scene.parent(a);
            }
        }
    }
    (segments, joints)
}

/// Paint the skeletons of the selected Skinned Mesh Renderers over the Scene view.
pub fn paint_bones(ui: &Ui, rect: Rect, camera: &Camera, scene: &Scene, selection: &Selection) {
    let (segments, joints) = skeleton(scene, selection);
    if joints.is_empty() {
        return;
    }
    let painter = ui.painter_at(rect);
    for (a, b) in segments {
        if let (Some(pa), Some(pb)) = (to_screen(camera, rect, a), to_screen(camera, rect, b)) {
            painter.line_segment([pa, pb], Stroke::new(3.0, Color32::from_black_alpha(120)));
            painter.line_segment([pa, pb], Stroke::new(1.6, BONE));
        }
    }
    for p in joints {
        if let Some(s) = to_screen(camera, rect, p) {
            painter.circle(
                s,
                3.5,
                JOINT,
                Stroke::new(1.0, Color32::from_black_alpha(160)),
            );
        }
    }
}
