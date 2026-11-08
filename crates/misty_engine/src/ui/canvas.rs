//! Where a canvas is drawn: on top of the image (Screen Space - Overlay), on a plane in front of
//! its Render Camera (Screen Space - Camera) or as a rectangle in the world (World Space).
//!
//! Every canvas is laid out in its own *canvas pixel* space (x right, y down, origin top-left).
//! An overlay's canvas pixels are the image's pixels; the other two map canvas pixels to the
//! world ([`CanvasPlacement::to_world`]) and through the viewing camera to clip space
//! ([`CanvasPlacement::to_clip`]), which is what the depth-tested UI pass draws with and what
//! pointer rays are traced back through ([`CanvasPlacement::to_canvas`]).

use glam::{Mat3, Mat4, Vec2, Vec3, Vec4};
use hecs::Entity;

use crate::camera::{Camera, Projection};
use crate::scene::{Camera as CameraComponent, Canvas, CanvasRenderMode, RectTransform, Scene};

/// What the UI is laid out for and seen through.
#[derive(Clone, Copy, Debug)]
pub struct UiView {
    /// Pixel size of the game screen: screen-space canvases are laid out for it.
    pub screen: Vec2,
    /// Pixel size of the image the UI is drawn into (the Scene view's, in the editor).
    pub target: Vec2,
    /// The camera that sees world-placed canvases (`None` = only overlays are drawn).
    pub camera: Option<Camera>,
    /// Editor Scene view: overlay canvases stand in the world at one unit per pixel, lower-left
    /// corner at the origin, facing −Z.
    pub overlay_in_world: bool,
}

impl UiView {
    /// The game: overlays on a `screen`-sized image, the rest seen by the game camera.
    pub fn game(scene: &Scene, screen: Vec2) -> Self {
        Self {
            screen,
            target: screen,
            camera: crate::render_scene::game_camera(scene).map(|(c, _)| c),
            overlay_in_world: false,
        }
    }

    /// The editor's Scene view: everything in the world, seen by the scene camera.
    pub fn scene_view(screen: Vec2, target: Vec2, camera: Camera) -> Self {
        Self {
            screen,
            target,
            camera: Some(camera),
            overlay_in_world: true,
        }
    }

    fn aspect(&self) -> f32 {
        self.target.x / self.target.y.max(1.0)
    }
}

/// One root canvas placed for a view.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasPlacement {
    pub canvas: Entity,
    /// The mode in effect (Screen Space - Camera without a camera acts as an overlay).
    pub mode: CanvasRenderMode,
    /// Canvas pixel size.
    pub size: Vec2,
    /// Canvas pixels → world (`None` = drawn on the image).
    pub to_world: Option<Mat4>,
    /// Canvas pixels → clip space of the view camera (`None` = drawn on the image).
    pub to_clip: Option<Mat4>,
    pub sort_order: i32,
    /// Distance of the canvas centre from the view camera (world-placed canvases).
    pub distance: f32,
}

impl CanvasPlacement {
    /// Drawn in the depth-tested pass (anything not on top of the image).
    pub fn in_world(&self) -> bool {
        self.to_clip.is_some()
    }

    /// The canvas pixel under a pixel of a `target`-sized image (`None` when the ray misses the
    /// canvas plane or meets it behind the camera).
    pub fn to_canvas(&self, p: Vec2, target: Vec2) -> Option<Vec2> {
        let Some(m) = self.to_clip else {
            return Some(p);
        };
        let ndc = Vec3::new(
            p.x / target.x.max(1.0) * 2.0 - 1.0,
            1.0 - p.y / target.y.max(1.0) * 2.0,
            1.0,
        );
        let h = plane_homography(&m);
        // Seen edge-on (relative to the size of its columns), the plane has no single point
        // under the pointer.
        let size = h.x_axis.length() * h.y_axis.length() * h.z_axis.length();
        if h.determinant().abs() <= size * 1e-6 {
            return None;
        }
        let q = h.inverse() * ndc;
        if q.z.abs() < 1e-12 {
            return None;
        }
        let c = Vec2::new(q.x / q.z, q.y / q.z);
        // The point must be in front of the camera (positive w).
        (m * Vec4::new(c.x, c.y, 0.0, 1.0)).w.gt(&0.0).then_some(c)
    }

    /// The image pixel showing canvas pixel `p` (`None` behind the camera).
    pub fn to_target(&self, p: Vec2, target: Vec2) -> Option<Vec2> {
        let Some(m) = self.to_clip else {
            return Some(p);
        };
        let c = m * Vec4::new(p.x, p.y, 0.0, 1.0);
        if c.w <= 1e-6 {
            return None;
        }
        let ndc = Vec2::new(c.x / c.w, c.y / c.w);
        Some(Vec2::new(
            (ndc.x * 0.5 + 0.5) * target.x,
            (0.5 - ndc.y * 0.5) * target.y,
        ))
    }

    /// The world point of canvas pixel `p` (an overlay's pixel stands for itself at z 0).
    pub fn world_point(&self, p: Vec2) -> Vec3 {
        match self.to_world {
            Some(m) => m.transform_point3(p.extend(0.0)),
            None => p.extend(0.0),
        }
    }
}

/// The 3×3 map from a canvas pixel (x, y, 1) to clip (x, y, w) of a canvas-to-clip matrix.
fn plane_homography(m: &Mat4) -> Mat3 {
    let col = |v: Vec4| Vec3::new(v.x, v.y, v.w);
    Mat3::from_cols(col(m.x_axis), col(m.y_axis), col(m.w_axis))
}

/// The view camera of an object with a Camera component (its world placement and lens).
pub fn camera_of(scene: &Scene, e: Entity) -> Option<Camera> {
    let c = scene.world.get::<&CameraComponent>(e).ok()?;
    let (_, rotation, position) = scene.world_matrix(e).to_scale_rotation_translation();
    let projection = if c.orthographic {
        Projection::Orthographic { size: c.ortho_size }
    } else {
        Projection::Perspective { fov_y: c.fov }
    };
    Some(Camera {
        position,
        rotation,
        projection,
        near: c.near,
        far: c.far,
    })
}

/// Canvas pixels of a `screen`-sized canvas → the plane `distance` in front of `cam` that
/// exactly fills its view (so it projects back onto the same pixels).
pub fn camera_plane(cam: &Camera, screen: Vec2, distance: f32) -> Mat4 {
    let d = distance.clamp(cam.near * 1.001, cam.far * 0.999);
    let aspect = screen.x / screen.y.max(1.0);
    let half_h = match cam.projection {
        Projection::Perspective { fov_y } => d * (fov_y.to_radians() * 0.5).tan(),
        Projection::Orthographic { size } => size,
    };
    let half_w = half_h * aspect;
    let (right, up, fwd) = (cam.right(), cam.up(), cam.forward());
    let sx = 2.0 * half_w / screen.x.max(1.0);
    let sy = 2.0 * half_h / screen.y.max(1.0);
    // p = centre + right * (x * sx - half_w) + up * (half_h - y * sy).
    let origin = cam.position + fwd * d - right * half_w + up * half_h;
    Mat4::from_cols(
        (right * sx).extend(0.0),
        (-up * sy).extend(0.0),
        fwd.extend(0.0),
        origin.extend(1.0),
    )
}

/// Canvas pixels of an overlay drawn in the Scene view: one unit per pixel, lower-left corner at
/// the origin (y flipped so the canvas reads upright from −Z).
pub fn overlay_in_world(size: Vec2) -> Mat4 {
    Mat4::from_cols(
        Vec4::X,
        Vec4::NEG_Y,
        Vec4::Z,
        Vec4::new(0.0, size.y, 0.0, 1.0),
    )
}

/// Size in canvas units of a World Space canvas (its Rect Transform's size; a stretched canvas
/// with no size of its own gets 800×600).
pub fn world_canvas_size(rt: Option<&RectTransform>) -> Vec2 {
    let s = rt.map_or(Vec2::ZERO, |r| r.size_delta);
    if s.x >= 1.0 && s.y >= 1.0 {
        s
    } else {
        Vec2::new(800.0, 600.0)
    }
}

/// Canvas pixels of a World Space canvas → world: units = pixels / `scale`, placed around the
/// Rect Transform's pivot by the object's world matrix.
pub fn world_canvas(world: Mat4, size: Vec2, pivot: Vec2, scale: f32) -> Mat4 {
    let s = 1.0 / scale.max(1e-6);
    let local = Mat4::from_cols(
        Vec4::new(s, 0.0, 0.0, 0.0),
        Vec4::new(0.0, -s, 0.0, 0.0),
        Vec4::Z,
        Vec4::new(-size.x * pivot.x, size.y * (1.0 - pivot.y), 0.0, 1.0),
    );
    world * local
}

/// Place canvas `e` for `view`: `size_units` is its rect in canvas units and `scale` its pixels
/// per unit. `None` when it can't be shown (a world canvas with no camera to see it).
pub fn place(
    scene: &Scene,
    view: &UiView,
    e: Entity,
    canvas: &Canvas,
    size_px: Vec2,
    scale: f32,
) -> Option<CanvasPlacement> {
    let render_camera = canvas.world_camera.and_then(|c| camera_of(scene, c));
    let mode = match canvas.render_mode {
        CanvasRenderMode::ScreenSpaceCamera if render_camera.is_none() => {
            CanvasRenderMode::ScreenSpaceOverlay
        }
        m => m,
    };
    let to_world = match mode {
        CanvasRenderMode::ScreenSpaceOverlay => {
            view.overlay_in_world.then(|| overlay_in_world(size_px))
        }
        CanvasRenderMode::ScreenSpaceCamera => {
            let cam = render_camera?;
            Some(camera_plane(&cam, size_px, canvas.plane_distance))
        }
        CanvasRenderMode::WorldSpace => {
            let rt = scene.world.get::<&RectTransform>(e).ok().map(|r| *r);
            let pivot = rt.map_or(Vec2::splat(0.5), |r| r.pivot);
            Some(world_canvas(
                scene.world_matrix(e),
                size_px / scale.max(1e-6),
                pivot,
                scale,
            ))
        }
    };
    let (to_clip, distance) = match (to_world, view.camera) {
        (Some(w), Some(cam)) => {
            let centre = w.transform_point3((size_px * 0.5).extend(0.0));
            (
                Some(cam.view_projection(view.aspect()) * w),
                centre.distance(cam.position),
            )
        }
        // A world-placed canvas needs a camera to be seen.
        (Some(_), None) => return None,
        (None, _) => (None, 0.0),
    };
    Some(CanvasPlacement {
        canvas: e,
        mode,
        size: size_px,
        to_world,
        to_clip,
        sort_order: canvas.sort_order,
        distance,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use glam::Quat;

    fn cam() -> Camera {
        Camera {
            position: Vec3::new(0.0, 1.0, -10.0),
            rotation: Quat::from_rotation_y(0.3),
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 500.0,
        }
    }

    #[test]
    fn camera_plane_projects_back_onto_the_same_pixels() {
        let c = cam();
        let screen = Vec2::new(1600.0, 900.0);
        let m = camera_plane(&c, screen, 25.0);
        let vp = c.view_projection(screen.x / screen.y) * m;
        let p = CanvasPlacement {
            canvas: Entity::DANGLING,
            mode: CanvasRenderMode::ScreenSpaceCamera,
            size: screen,
            to_world: Some(m),
            to_clip: Some(vp),
            sort_order: 0,
            distance: 25.0,
        };
        for q in [Vec2::ZERO, Vec2::new(800.0, 450.0), Vec2::new(1234.0, 77.0)] {
            let t = p.to_target(q, screen).unwrap();
            assert!(t.distance(q) < 0.05, "{q} -> {t}");
            let back = p.to_canvas(t, screen).unwrap();
            assert!(back.distance(q) < 0.05, "{t} -> {back}");
        }
        // The plane sits 25 units in front.
        let centre = m.transform_point3(Vec3::new(800.0, 450.0, 0.0));
        assert!(((centre - c.position).dot(c.forward()) - 25.0).abs() < 1e-3);
    }

    #[test]
    fn world_canvas_is_centred_on_its_pivot_and_upright() {
        let m = world_canvas(
            Mat4::IDENTITY,
            Vec2::new(200.0, 100.0),
            Vec2::splat(0.5),
            2.0,
        );
        // Canvas pixels are 400×200 at two per unit; the centre pixel is the object's origin.
        assert!(m.transform_point3(Vec3::new(200.0, 100.0, 0.0)).length() < 1e-5);
        // Top-left pixel: left and up.
        let tl = m.transform_point3(Vec3::ZERO);
        assert!((tl - Vec3::new(-100.0, 50.0, 0.0)).length() < 1e-4, "{tl}");
    }

    #[test]
    fn rays_behind_the_camera_miss() {
        let c = Camera {
            position: Vec3::new(0.0, 0.0, 5.0),
            rotation: Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        // A canvas behind the camera (z = 0 while the camera at z 5 looks to +z).
        let w = world_canvas(Mat4::IDENTITY, Vec2::splat(10.0), Vec2::splat(0.5), 1.0);
        let p = CanvasPlacement {
            canvas: Entity::DANGLING,
            mode: CanvasRenderMode::WorldSpace,
            size: Vec2::splat(10.0),
            to_world: Some(w),
            to_clip: Some(c.view_projection(1.0) * w),
            sort_order: 0,
            distance: 5.0,
        };
        assert_eq!(p.to_canvas(Vec2::splat(50.0), Vec2::splat(100.0)), None);
    }
}
