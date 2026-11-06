//! Motion vectors: what every temporal pass needs to know about last frame. Per render
//! target this keeps the previous unjittered view-projection (camera motion), each draw's
//! previous model matrix (object motion) and the previous grass clock (wind motion). The
//! main pass and the screen-space prepass write a per-pixel velocity (last frame's uv minus
//! this frame's, `velocity_of` in common.wgsl) that TAA, AO, the shadow mask and SSGI / SSR
//! reproject their histories through; the clouds reproject with the matrices (their image
//! is the sky, far away, so camera rotation is all that moves it).

use std::collections::HashMap;

use glam::{Mat4, Vec3};

use super::{Frame, MeshDraw};

/// Renders after which an unused target's state is dropped.
const STALE_AFTER: u64 = 600;
/// A draw whose transform jumped farther than this between frames is a different object (or
/// teleported): no object motion for it that frame.
const MAX_JUMP: f32 = 3.0;

/// Last frame of one target.
#[derive(Default)]
struct TargetMotion {
    /// Unjittered view-projection, camera position and forward.
    view: Option<(Mat4, Vec3, Vec3)>,
    /// Per draw index: mesh fingerprint and model matrix.
    draws: Vec<(u64, Mat4)>,
    grass_time: Option<f32>,
    used: u64,
}

#[derive(Default)]
pub(super) struct Motion {
    targets: HashMap<u64, TargetMotion>,
    renders: u64,
}

/// What this render reprojects with.
pub(super) struct MotionFrame {
    /// Last frame's unjittered view-projection (this frame's on a cut or first frame).
    pub prev_view_proj: Mat4,
    /// The history can be followed (no cut, not the first frame).
    pub continuous: bool,
    /// Seconds the grass clock advanced since last frame (0 on the first).
    pub grass_dt: f32,
    /// Last frame's draws (mesh fingerprint, model) by draw index.
    pub prev_draws: Vec<(u64, Mat4)>,
}

/// A cheap identity of a draw's mesh, so draw lists that shift by one entry don't pair a
/// cube with a sphere.
pub(super) fn fingerprint(d: &MeshDraw) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    d.mesh.hash(&mut h);
    h.finish()
}

/// Last frame's model matrix of draw `i` (`d` now), or its current one when last frame had
/// no such draw there (new object, a list that changed, a jump).
pub(super) fn prev_model(prev: &[(u64, Mat4)], i: usize, d: &MeshDraw, print: u64) -> Mat4 {
    match prev.get(i) {
        Some((p, m))
            if *p == print
                && m.w_axis.truncate().distance(d.model.w_axis.truncate()) < MAX_JUMP =>
        {
            *m
        }
        _ => d.model,
    }
}

/// A jump the history can't follow (teleport, switched camera).
fn cut(prev: (Mat4, Vec3, Vec3), position: Vec3, forward: Vec3) -> bool {
    prev.1.distance(position) > 5.0 || prev.2.dot(forward) < 0.85
}

impl Motion {
    /// Start a render of `target`: last frame's matrices, then remember this frame's camera
    /// (`view_proj` = unjittered) and grass clock.
    pub fn begin(
        &mut self,
        target: u64,
        frame: &Frame,
        view_proj: Mat4,
        grass_time: f32,
    ) -> MotionFrame {
        self.renders += 1;
        let renders = self.renders;
        self.targets.retain(|_, t| renders - t.used < STALE_AFTER);
        let t = self.targets.entry(target).or_default();
        t.used = renders;
        let cam = frame.camera;
        let (position, forward) = (cam.position, cam.forward());
        let prev = t.view.filter(|p| !cut(*p, position, forward));
        let grass_dt = t
            .grass_time
            .map_or(0.0, |last| (grass_time - last).clamp(0.0, 0.25));
        t.view = Some((view_proj, position, forward));
        t.grass_time = Some(grass_time);
        MotionFrame {
            prev_view_proj: prev.map_or(view_proj, |p| p.0),
            continuous: prev.is_some(),
            grass_dt,
            prev_draws: std::mem::take(&mut t.draws),
        }
    }

    /// Remember this frame's draws for the next render of `target`.
    pub fn end(&mut self, target: u64, draws: Vec<(u64, Mat4)>) {
        if let Some(t) = self.targets.get_mut(&target) {
            t.draws = draws;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::camera::{Camera, Projection};
    use crate::scene::PrimitiveMesh;

    fn draw(mesh: PrimitiveMesh, at: Vec3) -> MeshDraw {
        MeshDraw {
            mesh: mesh.into(),
            model: Mat4::from_translation(at),
            material: Default::default(),
            cast_shadows: true,
            receive_shadows: true,
            outline: false,
            sorting: Default::default(),
            lod: None,
        }
    }

    #[test]
    fn previous_models_follow_the_same_object_only() {
        let cube = draw(PrimitiveMesh::Cube, Vec3::ZERO);
        let moved = draw(PrimitiveMesh::Cube, Vec3::new(0.5, 0.0, 0.0));
        let far = draw(PrimitiveMesh::Cube, Vec3::new(9.0, 0.0, 0.0));
        let sphere = draw(PrimitiveMesh::Sphere, Vec3::new(0.5, 0.0, 0.0));
        let prev = vec![(fingerprint(&cube), cube.model)];
        assert_eq!(
            prev_model(&prev, 0, &moved, fingerprint(&moved)),
            cube.model
        );
        assert_eq!(prev_model(&prev, 0, &far, fingerprint(&far)), far.model);
        assert_eq!(
            prev_model(&prev, 0, &sphere, fingerprint(&sphere)),
            sphere.model
        );
        assert_eq!(
            prev_model(&prev, 1, &moved, fingerprint(&moved)),
            moved.model
        );
    }

    #[test]
    fn camera_cuts_and_first_frames_have_no_history() {
        let mut m = Motion::default();
        let cam = |x: f32| Camera {
            position: Vec3::new(x, 0.0, 0.0),
            rotation: glam::Quat::IDENTITY,
            projection: Projection::Perspective { fov_y: 60.0 },
            near: 0.1,
            far: 100.0,
        };
        let env = super::super::Environment::default();
        fn frame<'a>(c: &'a Camera, env: &'a super::super::Environment) -> Frame<'a> {
            Frame {
                camera: c,
                grid: None,
                skybox: false,
                background: [0.0; 3],
                environment: env,
                post: Default::default(),
                lines: &[],
                overlay: &[],
                meshes: &[],
                particles: &[],
                terrains: &[],
                shaded: true,
                lights: &[],
                water: &[],
            }
        }
        let a = cam(0.0);
        let first = m.begin(1, &frame(&a, &env), a.view_projection(1.0), 0.0);
        assert!(!first.continuous);
        assert_eq!(first.grass_dt, 0.0);
        let b = cam(0.1);
        let second = m.begin(1, &frame(&b, &env), b.view_projection(1.0), 0.016);
        assert!(second.continuous);
        assert_eq!(second.prev_view_proj, a.view_projection(1.0));
        assert!((second.grass_dt - 0.016).abs() < 1e-6);
        let c = cam(50.0);
        let third = m.begin(1, &frame(&c, &env), c.view_projection(1.0), 0.032);
        assert!(!third.continuous, "a teleport is a cut");
        assert_eq!(third.prev_view_proj, c.view_projection(1.0));
        // Another target has its own history.
        let other = m.begin(2, &frame(&c, &env), c.view_projection(1.0), 0.032);
        assert!(!other.continuous);
    }
}
