//! Starting scenes. A tutorial starts in a fresh Untitled scene (never a sample scene); these
//! add what it needs on top of the default camera and sun.

use glam::Vec3;
use misty_engine::hecs::Entity;
use misty_engine::scene::{
    Light, MeshFilter, MeshRenderer, PostProcessVolume, PrimitiveMesh, Scene, Shadows, Transform,
};

use crate::state::EditorState;

/// Known starting scene names (tests check every tutorial uses one).
#[cfg(test)]
pub const NAMES: [&str; 3] = ["empty", "post_processing", "god_rays"];

/// Fill the new scene for `name`; false for an unknown name.
pub fn setup(s: &mut EditorState, name: &str) -> bool {
    match name {
        "" | "empty" => {}
        "post_processing" => {
            outdoor(&mut s.scene, 12.0);
            s.scene.spawn("Effects", None);
        }
        "god_rays" => {
            outdoor(&mut s.scene, 18.0);
            if let Some(sun) = s.scene.find_by_name("Directional Light") {
                let _ = s.scene.world.get::<&mut Light>(sun).map(|mut l| {
                    l.shadows = Shadows::None;
                });
            }
            let fx = s.scene.spawn("Effects", None);
            let mut v = PostProcessVolume::default();
            v.tonemapping.active = true;
            v.fog.active = true;
            v.fog.anisotropy = 0.7;
            v.fog.maximum_height = 20.0;
            s.scene.add(fx, v);
        }
        _ => return false,
    }
    // Look from the camera, toward the sun.
    if let Some(cam) = s.scene.find_by_name("Main Camera") {
        let (_, rot, pos) = s.scene.world_matrix(cam).to_scale_rotation_translation();
        s.scene_view.camera.align_to(pos, rot);
    }
    true
}

/// Ground, a few simple shapes, a low sun in front of the camera.
fn outdoor(scene: &mut Scene, sun_pitch: f32) {
    if let Some(sun) = scene.find_by_name("Directional Light") {
        let t = Transform::from_position(Vec3::new(0.0, 10.0, 0.0))
            .with_euler(Vec3::new(sun_pitch, 200.0, 0.0));
        scene.set_transform(sun, t);
    }
    if let Some(cam) = scene.find_by_name("Main Camera") {
        let t = Transform::from_position(Vec3::new(0.0, 2.5, -16.0))
            .with_euler(Vec3::new(-6.0, 10.0, 0.0));
        scene.set_transform(cam, t);
    }
    let ground = mesh(scene, "Ground", PrimitiveMesh::Plane);
    let t = Transform {
        scale: Vec3::new(12.0, 1.0, 12.0),
        ..Default::default()
    };
    scene.set_transform(ground, t);
    let shapes = [
        (
            "Pillar",
            PrimitiveMesh::Cylinder,
            [-5.0, 2.0, 4.0],
            [1.0, 4.0, 1.0],
        ),
        (
            "Pillar",
            PrimitiveMesh::Cylinder,
            [4.0, 2.0, 7.0],
            [1.0, 4.0, 1.0],
        ),
        (
            "Block",
            PrimitiveMesh::Cube,
            [-1.5, 0.75, 2.0],
            [1.5, 1.5, 1.5],
        ),
        (
            "Ball",
            PrimitiveMesh::Sphere,
            [2.0, 1.0, 0.0],
            [2.0, 2.0, 2.0],
        ),
    ];
    for (name, kind, pos, scale) in shapes {
        let e = mesh(scene, name, kind);
        let mut t = Transform::from_position(Vec3::from(pos));
        t.scale = Vec3::from(scale);
        scene.set_transform(e, t);
    }
}

fn mesh(scene: &mut Scene, name: &str, kind: PrimitiveMesh) -> Entity {
    let e = scene.spawn(name, None);
    scene.add(e, MeshFilter::primitive(kind));
    scene.add(e, MeshRenderer::default());
    e
}
