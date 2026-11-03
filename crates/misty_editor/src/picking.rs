//! Scene-view picking: icons for cameras/lights, ray vs. mesh bounds for everything else.

use egui::{Pos2, Rect, pos2};
use glam::{Vec2, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera as CameraComponent, Light, MeshFilter, Scene};

const ICON_RADIUS: f32 = 16.0;

/// World point to screen position inside `rect`, or None if behind the camera.
pub fn to_screen(camera: &Camera, rect: Rect, p: Vec3) -> Option<Pos2> {
    let aspect = rect.width() / rect.height().max(1.0);
    camera.world_to_screen(aspect, p).map(|(uv, _)| {
        pos2(
            rect.left() + uv.x * rect.width(),
            rect.top() + uv.y * rect.height(),
        )
    })
}

/// World ray under a screen position.
pub fn ray(camera: &Camera, rect: Rect, at: Pos2) -> (Vec3, Vec3) {
    let aspect = rect.width() / rect.height().max(1.0);
    let uv = Vec2::new(
        (at.x - rect.left()) / rect.width(),
        (at.y - rect.top()) / rect.height(),
    );
    camera.screen_ray(aspect, uv)
}

/// Active objects drawn as icons: cameras, lights and audio sources without a mesh.
pub fn icon_objects(scene: &Scene) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.active_in_hierarchy(*e))
        .filter(|e| {
            let w = &scene.world;
            let has_mesh = w.get::<&MeshFilter>(*e).is_ok_and(|m| m.mesh.is_some());
            let iconic = w.get::<&CameraComponent>(*e).is_ok()
                || w.get::<&Light>(*e).is_ok()
                || w.get::<&misty_engine::scene::AudioSource>(*e).is_ok();
            !has_mesh && iconic
        })
        .collect()
}

/// Object under the cursor: icons first (closest on screen), then the nearest mesh hit.
pub fn pick(scene: &Scene, camera: &Camera, rect: Rect, at: Pos2) -> Option<Entity> {
    let icon = icon_objects(scene)
        .into_iter()
        .filter_map(|e| {
            to_screen(camera, rect, scene.world_position(e)).map(|p| (e, p.distance(at)))
        })
        .filter(|(_, d)| *d < ICON_RADIUS)
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e);
    if icon.is_some() {
        return icon;
    }

    let (origin, dir) = ray(camera, rect, at);
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.active_in_hierarchy(*e))
        .filter_map(|e| {
            let mesh = scene.world.get::<&MeshFilter>(e).ok()?.mesh?;
            let m = scene.world_matrix(e);
            let inv = m.inverse();
            let lo = inv.transform_point3(origin);
            let ld = inv.transform_vector3(dir);
            let t = ray_box(lo, ld, mesh.half_extents().max(Vec3::splat(0.01)))?;
            let hit = m.transform_point3(lo + ld * t);
            Some((e, hit.distance(origin)))
        })
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e)
}

/// Slab test against an origin-centred box; returns the entry parameter.
fn ray_box(origin: Vec3, dir: Vec3, half: Vec3) -> Option<f32> {
    let inv = dir.recip();
    let t0 = (-half - origin) * inv;
    let t1 = (half - origin) * inv;
    let near = t0.min(t1).max_element();
    let far = t0.max(t1).min_element();
    (far >= near.max(0.0)).then_some(near.max(0.0))
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;
    use misty_engine::camera::Projection;
    use misty_engine::scene::{MeshRenderer, PrimitiveMesh, Transform};

    #[test]
    fn ray_hits_box_in_front_only() {
        assert_eq!(
            ray_box(Vec3::new(0.0, 0.0, -5.0), Vec3::Z, Vec3::splat(0.5)),
            Some(4.5)
        );
        assert_eq!(
            ray_box(Vec3::new(2.0, 0.0, -5.0), Vec3::Z, Vec3::splat(0.5)),
            None
        );
        assert_eq!(
            ray_box(Vec3::new(0.0, 0.0, 5.0), Vec3::Z, Vec3::splat(0.5)),
            None
        );
    }

    #[test]
    fn picks_nearest_cube_under_cursor() {
        let mut scene = Scene::new("t");
        let near = scene.spawn("Near", None);
        let far = scene.spawn("Far", None);
        for (e, z) in [(near, 0.0), (far, 5.0)] {
            scene.add(
                e,
                MeshFilter {
                    mesh: Some(PrimitiveMesh::Cube),
                },
            );
            scene.add(e, MeshRenderer::default());
            scene.set_transform(e, Transform::from_position(Vec3::new(0.0, 0.0, z)));
        }
        let camera = Camera {
            position: Vec3::new(0.0, 0.0, -10.0),
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let rect = Rect::from_min_size(pos2(0.0, 0.0), egui::vec2(400.0, 300.0));
        assert_eq!(pick(&scene, &camera, rect, rect.center()), Some(near));
        assert_eq!(pick(&scene, &camera, rect, pos2(5.0, 5.0)), None);
    }
}
