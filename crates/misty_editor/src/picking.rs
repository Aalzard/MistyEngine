//! Scene-view picking: icons for cameras/lights, ray vs. mesh bounds for everything else.

use egui::{Pos2, Rect, pos2};
use glam::{Vec2, Vec3};
use misty_engine::camera::Camera;
use misty_engine::hecs::Entity;
use misty_engine::scene::{Camera as CameraComponent, Light, MeshFilter, Scene};

use crate::scene_icons::IconSettings;

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

/// Active objects drawn as icons: cameras, lights, audio sources and particle systems without a
/// mesh.
pub fn icon_objects(scene: &Scene) -> Vec<Entity> {
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.active_in_hierarchy(*e))
        .filter(|e| {
            let w = &scene.world;
            let has_mesh = w.get::<&MeshFilter>(*e).is_ok_and(|m| m.mesh.is_some())
                || w.get::<&misty_engine::scene::SpriteRenderer>(*e).is_ok();
            let iconic = w.get::<&CameraComponent>(*e).is_ok()
                || w.get::<&Light>(*e).is_ok()
                || w.get::<&misty_engine::scene::AudioSource>(*e).is_ok()
                || w.get::<&misty_engine::scene::ParticleSystem>(*e).is_ok()
                || w.get::<&misty_engine::scene::ReflectionProbe>(*e).is_ok()
                || w.get::<&misty_engine::scene::FogVolume>(*e).is_ok()
                || w.get::<&misty_engine::scene::DecalProjector>(*e).is_ok();
            !has_mesh && iconic
        })
        .collect()
}

/// Object under the cursor: icons first (closest on screen, hit area sized like the drawn
/// icon), then the nearest mesh hit.
pub fn pick(
    scene: &Scene,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    icons: &IconSettings,
) -> Option<Entity> {
    let icon = icon_objects(scene)
        .into_iter()
        .filter_map(|e| {
            let w = scene.world_position(e);
            let radius = icons.side(camera.position, w) * (2.0 / 3.0);
            to_screen(camera, rect, w).map(|p| (e, p.distance(at), radius))
        })
        .filter(|(_, d, r)| d < r)
        .map(|(e, d, _)| (e, d))
        .min_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(e, _)| e);
    if icon.is_some() {
        return icon;
    }

    mesh_hit(scene, camera, rect, at, |_| true).map(|(e, _)| e)
}

/// Nearest mesh (bounds box) hit under the cursor among objects `keep` accepts: (object, point).
fn mesh_hit(
    scene: &Scene,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    keep: impl Fn(Entity) -> bool,
) -> Option<(Entity, Vec3)> {
    let (origin, dir) = ray(camera, rect, at);
    scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| scene.active_in_hierarchy(*e) && keep(*e))
        .filter_map(|e| {
            let (min, max) = object_bounds(scene, e)?;
            let m = scene.world_matrix(e);
            let inv = m.inverse();
            // Test against the bounds box, re-centred on the origin (flat boxes get a little depth).
            let center = (min + max) * 0.5;
            let lo = inv.transform_point3(origin) - center;
            let ld = inv.transform_vector3(dir);
            let t = ray_box(lo, ld, ((max - min) * 0.5).max(Vec3::splat(0.01)))?;
            let lo = lo + center;
            let hit = m.transform_point3(lo + ld * t);
            Some((e, hit))
        })
        .chain(misty_engine::terrain::raycast(scene, origin, dir).filter(|(e, _)| keep(*e)))
        .min_by(|a, b| {
            // Sprites on one plane: the one drawn on top wins.
            let (da, db) = (a.1.distance(origin), b.1.distance(origin));
            if (da - db).abs() < 1e-3 {
                sorting(scene, b.0).cmp(&sorting(scene, a.0))
            } else {
                da.total_cmp(&db)
            }
        })
}

/// Local bounds of what an object draws: its mesh, else its sprite (as a thin box).
fn object_bounds(scene: &Scene, e: Entity) -> Option<(Vec3, Vec3)> {
    let mesh = scene
        .world
        .get::<&MeshFilter>(e)
        .ok()
        .and_then(|f| f.mesh.clone());
    match mesh {
        Some(mesh) => misty_engine::models::bounds(&mesh),
        None => {
            if let Some(b) = skinned_bounds(scene, e) {
                return Some(b);
            }
            let (min, max) = misty_engine::sprites::local_bounds(scene, e)?;
            Some((min.extend(-0.005), max.extend(0.005)))
        }
    }
}

/// Sorting Layer and Order in Layer of an object's Sprite Renderer (meshes: Default, 0).
fn sorting(scene: &Scene, e: Entity) -> (i32, i32) {
    scene
        .world
        .get::<&misty_engine::scene::SpriteRenderer>(e)
        .map(|r| {
            let layer = misty_engine::sprites::sorting::value(&r.sorting_layer);
            (layer, r.order_in_layer)
        })
        .unwrap_or((0, 0))
}

/// Surface point under the cursor (Ctrl+Shift surface snapping), ignoring objects `keep` rejects.
/// Meshes are approximated by their bounds box.
pub fn surface_point(
    scene: &Scene,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    keep: impl Fn(Entity) -> bool,
) -> Option<Vec3> {
    mesh_hit(scene, camera, rect, at, keep).map(|(_, p)| p)
}

/// World position of the mesh vertex drawn closest to `at` (within `max_px`) among objects `keep`
/// accepts (V vertex snapping).
pub fn nearest_vertex(
    scene: &Scene,
    camera: &Camera,
    rect: Rect,
    at: Pos2,
    max_px: f32,
    keep: impl Fn(Entity) -> bool,
) -> Option<Vec3> {
    let mut best: Option<(Vec3, f32)> = None;
    for (e, _) in scene.walk() {
        if !scene.active_in_hierarchy(e) || !keep(e) {
            continue;
        }
        let Some(mesh) = scene
            .world
            .get::<&MeshFilter>(e)
            .ok()
            .and_then(|f| f.mesh.clone())
        else {
            continue;
        };
        let Some(data) = misty_engine::models::mesh(&mesh) else {
            continue;
        };
        let m = scene.world_matrix(e);
        for v in &data.vertices {
            let p = m.transform_point3(Vec3::from(v.position));
            if let Some(s) = to_screen(camera, rect, p) {
                let d = s.distance(at);
                if d <= max_px && best.is_none_or(|(_, bd)| d < bd) {
                    best = Some((p, d));
                }
            }
        }
    }
    best.map(|(p, _)| p)
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

/// A Skinned Mesh Renderer's posed bounds in its object's space.
fn skinned_bounds(scene: &Scene, e: Entity) -> Option<(Vec3, Vec3)> {
    let r = scene
        .world
        .get::<&misty_engine::scene::SkinnedMeshRenderer>(e)
        .ok()?
        .clone();
    let pose = misty_engine::skinning::compute_pose(scene, e, &r, &|x| scene.world_matrix(x))?;
    Some(pose.bounds)
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
                    mesh: Some(PrimitiveMesh::Cube.into()),
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
        assert_eq!(
            pick(&scene, &camera, rect, rect.center(), &Default::default()),
            Some(near)
        );
        assert_eq!(
            pick(&scene, &camera, rect, pos2(5.0, 5.0), &Default::default()),
            None
        );
    }
}
