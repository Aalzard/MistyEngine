//! Sprite Renderers as renderer draws: a unit quad scaled to the sprite and offset by its pivot,
//! with the Sprites-Default material (unlit, alpha blended, both faces).

use glam::{Mat4, Vec2, Vec3};
use hecs::Entity;

use super::{Sprite, resolve, sorting};
use crate::render::surface::SortingKey;
use crate::render::{MaterialParams, MeshDraw, RenderingMode};
use crate::scene::{PrimitiveMesh, Scene, SpriteRenderer};

/// Local bounds (min, max) of an object's Sprite Renderer, in the object's space.
pub fn local_bounds(scene: &Scene, e: Entity) -> Option<(Vec2, Vec2)> {
    let r = scene.world.get::<&SpriteRenderer>(e).ok()?;
    resolve(r.sprite.as_deref()?).map(|s| s.bounds())
}

/// Quad placement inside the object: centred on the sprite's bounds.
fn quad_matrix(s: &Sprite) -> Mat4 {
    let (min, max) = s.bounds();
    let center = (min + max) * 0.5;
    Mat4::from_translation(center.extend(0.0)) * Mat4::from_scale((max - min).extend(1.0))
}

/// The Sprites-Default material for a renderer.
fn material(r: &SpriteRenderer, s: &Sprite) -> MaterialParams {
    let (tiling, offset) = s.uv_transform(r.flip_x, r.flip_y);
    MaterialParams {
        color: r.color,
        albedo_map: Some(s.texture.clone()),
        tiling,
        offset,
        unlit: true,
        mode: RenderingMode::Fade,
        double_sided: true,
        lit_2d: true,
        ..Default::default()
    }
}

/// Draws of the enabled Sprite Renderers on active objects; `outline(e)` = selected.
pub fn draws(scene: &Scene, outline: &dyn Fn(Entity) -> bool, out: &mut Vec<MeshDraw>) {
    for (e, _) in scene.walk() {
        let Ok(r) = scene.world.get::<&SpriteRenderer>(e) else {
            continue;
        };
        if !r.enabled || !scene.active_in_hierarchy(e) {
            continue;
        }
        out.extend(draw_of(&r, scene.world_matrix(e), outline(e)));
    }
}

/// The draw of one enabled Sprite Renderer on an active object at `world`.
pub fn draw_of(r: &SpriteRenderer, world: Mat4, outline: bool) -> Option<MeshDraw> {
    let sprite = r.sprite.as_deref().and_then(resolve)?;
    Some(MeshDraw {
        mesh: PrimitiveMesh::Quad.into(),
        model: world * quad_matrix(&sprite),
        material: material(r, &sprite),
        cast_shadows: false,
        receive_shadows: false,
        outline,
        sorting: SortingKey {
            layer: sorting::value(&r.sorting_layer),
            order: r.order_in_layer,
        },
        lod: None,
    })
}

/// World-space corners of a sprite quad (for gizmos): bottom-left, bottom-right, top-right,
/// top-left.
pub fn world_corners(scene: &Scene, e: Entity) -> Option<[Vec3; 4]> {
    let (min, max) = local_bounds(scene, e)?;
    let m = scene.world_matrix(e);
    Some(
        [min, Vec2::new(max.x, min.y), max, Vec2::new(min.x, max.y)]
            .map(|p| m.transform_point3(p.extend(0.0))),
    )
}
