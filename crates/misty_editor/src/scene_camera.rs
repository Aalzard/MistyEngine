//! Scene view camera: pivot/orbit model with fly, pan, orbit, zoom, axis snapping and 2D mode.

use egui::{Key, PointerButton};
use glam::{Quat, Vec3};
use misty_engine::camera::{Camera, Projection};

/// Interaction that started with a mouse press in the view.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drag {
    None,
    /// RMB: look around, WASD/QE fly.
    Look,
    /// MMB, or LMB with the View tool.
    Pan,
    /// Alt + LMB.
    Orbit,
    /// Alt + RMB.
    Zoom,
}

/// Camera placement: everything that is animated.
#[derive(Clone, Copy, Debug)]
struct Pose {
    pivot: Vec3,
    yaw: f32,
    pitch: f32,
    distance: f32,
}

impl Pose {
    fn lerp(a: Pose, b: Pose, t: f32) -> Pose {
        // Shortest path for yaw.
        let mut dy = (b.yaw - a.yaw) % 360.0;
        if dy > 180.0 {
            dy -= 360.0;
        } else if dy < -180.0 {
            dy += 360.0;
        }
        Pose {
            pivot: a.pivot.lerp(b.pivot, t),
            yaw: a.yaw + dy * t,
            pitch: a.pitch + (b.pitch - a.pitch) * t,
            distance: a.distance * (b.distance / a.distance).powf(t),
        }
    }
}

/// Named axis views reachable from the scene gizmo.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AxisView {
    Right,
    Left,
    Top,
    Bottom,
    Back,
    Front,
}

impl AxisView {
    /// Direction from the pivot towards the camera.
    pub fn eye_dir(self) -> Vec3 {
        match self {
            AxisView::Right => Vec3::X,
            AxisView::Left => -Vec3::X,
            AxisView::Top => Vec3::Y,
            AxisView::Bottom => -Vec3::Y,
            AxisView::Back => Vec3::Z,
            AxisView::Front => -Vec3::Z,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            AxisView::Right => "Right",
            AxisView::Left => "Left",
            AxisView::Top => "Top",
            AxisView::Bottom => "Bottom",
            AxisView::Back => "Back",
            AxisView::Front => "Front",
        }
    }
}

pub struct SceneCamera {
    pose: Pose,
    anim: Option<(Pose, Pose, f32)>,
    pub orthographic: bool,
    pub fov: f32,
    /// Fly speed multiplier (camera settings dropdown, RMB + scroll).
    pub speed: f32,
    pub dynamic_clipping: bool,
    pub near: f32,
    pub far: f32,
    pub drag: Drag,
    fly_time: f32,
    snapped: Option<AxisView>,
    ortho_before_snap: bool,
    in_2d: bool,
    saved_3d: Option<(Pose, bool)>,
}

const LOOK_SPEED: f32 = 0.25;
const ANIM_TIME: f32 = 0.25;

impl Default for SceneCamera {
    fn default() -> Self {
        // Default editor view: from above and behind-right, looking at the origin.
        Self {
            pose: Pose {
                pivot: Vec3::ZERO,
                yaw: -135.0,
                pitch: 26.0,
                distance: 17.0,
            },
            anim: None,
            orthographic: false,
            fov: 60.0,
            speed: 1.0,
            dynamic_clipping: true,
            near: 0.03,
            far: 10_000.0,
            drag: Drag::None,
            fly_time: 0.0,
            snapped: None,
            ortho_before_snap: false,
            in_2d: false,
            saved_3d: None,
        }
    }
}

impl SceneCamera {
    pub fn rotation(&self) -> Quat {
        rotation(self.pose.yaw, self.pose.pitch)
    }

    pub fn position(&self) -> Vec3 {
        self.pose.pivot - self.rotation() * Vec3::Z * self.pose.distance
    }

    pub fn pivot(&self) -> Vec3 {
        self.pose.pivot
    }

    pub fn is_2d(&self) -> bool {
        self.in_2d
    }

    /// Label under the scene gizmo.
    pub fn view_label(&self) -> &'static str {
        match self.snapped {
            Some(axis) => axis.label(),
            None if self.orthographic => "Iso",
            None => "Persp",
        }
    }

    /// Half view height at the pivot, used for ortho size and pan speed.
    fn half_height(&self) -> f32 {
        self.pose.distance * (self.fov.to_radians() * 0.5).tan()
    }

    pub fn camera(&self) -> Camera {
        let (near, far) = if self.dynamic_clipping {
            let d = self.pose.distance;
            ((d * 0.002).clamp(0.01, 1.0), (d * 1000.0).max(1000.0))
        } else {
            (self.near, self.far)
        };
        if self.orthographic {
            // Pull the eye far back so nothing in front of the pivot is clipped.
            let back = far * 0.5;
            Camera {
                position: self.pose.pivot - self.rotation() * Vec3::Z * back,
                rotation: self.rotation(),
                projection: Projection::Orthographic {
                    size: self.half_height(),
                },
                near: 0.0,
                far: back + far,
            }
        } else {
            Camera {
                position: self.position(),
                rotation: self.rotation(),
                projection: Projection::Perspective { fov_y: self.fov },
                near,
                far,
            }
        }
    }

    /// Process mouse/keyboard for this frame. Returns true while the camera is moving.
    pub fn handle_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        view_height: f32,
        view_tool: bool,
    ) -> bool {
        let (pressed, alt, delta, scroll, dt) = ctx.input(|i| {
            let pressed = [
                PointerButton::Primary,
                PointerButton::Secondary,
                PointerButton::Middle,
            ]
            .into_iter()
            .find(|b| i.pointer.button_pressed(*b));
            (
                pressed,
                i.modifiers.alt,
                i.pointer.delta(),
                i.smooth_scroll_delta.y,
                i.stable_dt.min(0.1),
            )
        });

        // Start a drag only when the press lands on the view itself.
        if let Some(button) = pressed
            && response.hovered()
            && self.drag == Drag::None
        {
            self.drag = match (button, alt) {
                (PointerButton::Secondary, true) => Drag::Zoom,
                (PointerButton::Secondary, false) if self.in_2d => Drag::Pan,
                (PointerButton::Secondary, false) => Drag::Look,
                (PointerButton::Middle, _) => Drag::Pan,
                (PointerButton::Primary, true) if !self.in_2d => Drag::Orbit,
                (PointerButton::Primary, _) if view_tool => Drag::Pan,
                _ => Drag::None,
            };
            if self.drag != Drag::None {
                self.anim = None;
                ctx.memory_mut(|m| m.stop_text_input());
            }
        }
        let still_down = ctx.input(|i| i.pointer.any_down());
        if !still_down {
            self.drag = Drag::None;
            self.fly_time = 0.0;
        }

        match self.drag {
            Drag::Look => {
                self.look(delta.x, delta.y);
                self.fly(ctx, dt);
                if response.hovered() && scroll != 0.0 {
                    // RMB + wheel changes fly speed, like the camera speed setting.
                    self.speed = (self.speed * (1.0 + scroll * 0.002)).clamp(0.01, 100.0);
                }
            }
            Drag::Pan => self.pan(delta.x, delta.y, view_height),
            Drag::Orbit => self.orbit(delta.x, delta.y),
            Drag::Zoom => self.zoom((delta.x + delta.y) * 2.0),
            Drag::None => {
                if response.hovered() && scroll != 0.0 {
                    self.zoom(scroll);
                }
            }
        }

        if let Some(icon) = match self.drag {
            Drag::Look => Some(egui::CursorIcon::Crosshair),
            Drag::Pan => Some(egui::CursorIcon::Grabbing),
            Drag::Orbit => Some(egui::CursorIcon::AllScroll),
            Drag::Zoom => Some(egui::CursorIcon::ZoomIn),
            Drag::None if view_tool && response.hovered() => Some(egui::CursorIcon::Grab),
            Drag::None => None,
        } {
            ctx.set_cursor_icon(icon);
        }

        self.step_animation(dt);
        self.drag != Drag::None || self.anim.is_some()
    }

    /// Rotate around the eye (first-person look).
    fn look(&mut self, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        self.leave_snapped_view();
        let eye = self.position();
        self.pose.yaw += dx * LOOK_SPEED;
        self.pose.pitch = (self.pose.pitch + dy * LOOK_SPEED).clamp(-89.9, 89.9);
        self.pose.pivot = eye + self.rotation() * Vec3::Z * self.pose.distance;
    }

    /// Rotate around the pivot.
    fn orbit(&mut self, dx: f32, dy: f32) {
        if dx == 0.0 && dy == 0.0 {
            return;
        }
        self.leave_snapped_view();
        self.pose.yaw += dx * LOOK_SPEED;
        self.pose.pitch = (self.pose.pitch + dy * LOOK_SPEED).clamp(-89.9, 89.9);
    }

    /// Drag the world with the mouse.
    fn pan(&mut self, dx: f32, dy: f32, view_height: f32) {
        let per_point = 2.0 * self.half_height() / view_height.max(1.0);
        let r = self.rotation();
        self.pose.pivot += (r * Vec3::X * -dx + r * Vec3::Y * dy) * per_point;
    }

    /// Dolly towards the pivot. `amount` > 0 zooms in.
    fn zoom(&mut self, amount: f32) {
        let factor = (1.0 - amount * 0.002).clamp(0.5, 1.5);
        self.pose.distance = (self.pose.distance * factor).clamp(0.01, 50_000.0);
    }

    /// WASD + QE while looking; Shift for 4x, speed ramps up while held.
    fn fly(&mut self, ctx: &egui::Context, dt: f32) {
        let (dir, fast) = ctx.input(|i| {
            let axis =
                |pos: Key, neg: Key| i.key_down(pos) as i32 as f32 - i.key_down(neg) as i32 as f32;
            (
                Vec3::new(
                    axis(Key::D, Key::A),
                    axis(Key::E, Key::Q),
                    axis(Key::W, Key::S),
                ),
                i.modifiers.shift,
            )
        });
        if dir == Vec3::ZERO {
            self.fly_time = 0.0;
            return;
        }
        self.fly_time += dt;
        let ramp = 1.0 + (self.fly_time * 1.5).min(3.0);
        let speed = 5.0 * self.speed * ramp * if fast { 4.0 } else { 1.0 };
        let r = self.rotation();
        let world = r * Vec3::X * dir.x + Vec3::Y * dir.y + r * Vec3::Z * dir.z;
        self.pose.pivot += world.normalize_or_zero() * speed * dt;
    }

    /// Smoothly move the pivot onto a point (Frame Selected, F).
    pub fn frame(&mut self, center: Vec3, radius: f32) {
        let mut target = self.pose;
        target.pivot = center;
        target.distance = (radius.max(0.25) * 2.5) / (self.fov.to_radians() * 0.5).tan().max(0.1);
        self.animate_to(target);
    }

    /// Keep the pivot on `center` without animating (Lock View to Selected).
    pub fn follow(&mut self, center: Vec3) {
        if let Some((_, to, _)) = &mut self.anim {
            to.pivot = center;
        } else {
            self.pose.pivot = center;
        }
    }

    /// Put the eye at `position` looking along `rotation` (Align View to Selected).
    pub fn align_to(&mut self, position: Vec3, rotation: Quat) {
        let (yaw, pitch) = yaw_pitch_from_forward(rotation * Vec3::Z);
        let mut target = Pose {
            yaw,
            pitch,
            ..self.pose
        };
        target.pivot = position + rotation * Vec3::Z * self.pose.distance;
        self.leave_snapped_view();
        self.animate_to(target);
    }

    /// Jump straight to an eye position and rotation (restoring a saved view).
    pub fn set_view(&mut self, position: Vec3, rotation: Quat) {
        self.align_to(position, rotation);
        if let Some((_, to, _)) = self.anim.take() {
            self.pose = to;
        }
    }

    /// Snap to an axis view (scene gizmo cone): orthographic, looking at the pivot.
    pub fn snap_to_axis(&mut self, axis: AxisView) {
        if self.in_2d {
            return;
        }
        if self.snapped.is_none() {
            self.ortho_before_snap = self.orthographic;
        }
        let forward = -axis.eye_dir();
        let (yaw, pitch) = yaw_pitch_from_forward(forward);
        let mut target = self.pose;
        target.yaw = yaw;
        target.pitch = pitch;
        self.snapped = Some(axis);
        self.orthographic = true;
        self.animate_to(target);
    }

    /// Centre cube of the scene gizmo: perspective <-> isometric.
    pub fn toggle_projection(&mut self) {
        if self.in_2d {
            return;
        }
        self.orthographic = !self.orthographic;
        self.snapped = None;
    }

    /// 2D mode: orthographic, looking down +Z, no rotation.
    pub fn set_2d(&mut self, on: bool) {
        if on == self.in_2d {
            return;
        }
        self.in_2d = on;
        if on {
            self.saved_3d = Some((self.pose, self.orthographic));
            let mut target = self.pose;
            target.yaw = 0.0;
            target.pitch = 0.0;
            self.orthographic = true;
            self.snapped = None;
            self.animate_to(target);
        } else if let Some((pose, ortho)) = self.saved_3d.take() {
            self.orthographic = ortho;
            self.animate_to(pose);
        }
    }

    /// Rotating away from an axis view restores the previous projection.
    fn leave_snapped_view(&mut self) {
        if self.snapped.take().is_some() {
            self.orthographic = self.ortho_before_snap;
        }
    }

    fn animate_to(&mut self, target: Pose) {
        self.anim = Some((self.pose, target, 0.0));
    }

    fn step_animation(&mut self, dt: f32) {
        if let Some((from, to, t)) = &mut self.anim {
            *t = (*t + dt / ANIM_TIME).min(1.0);
            let e = 1.0 - (1.0 - *t).powi(3);
            self.pose = Pose::lerp(*from, *to, e);
            if *t >= 1.0 {
                self.pose = *to;
                self.anim = None;
            }
        }
    }
}

fn rotation(yaw: f32, pitch: f32) -> Quat {
    Quat::from_rotation_y(yaw.to_radians()) * Quat::from_rotation_x(pitch.to_radians())
}

/// Inverse of [`rotation`] applied to +Z.
fn yaw_pitch_from_forward(f: Vec3) -> (f32, f32) {
    let f = f.normalize();
    let pitch = (-f.y).clamp(-1.0, 1.0).asin().to_degrees();
    let yaw = if f.x.abs() < 1e-6 && f.z.abs() < 1e-6 {
        0.0
    } else {
        f.x.atan2(f.z).to_degrees()
    };
    (yaw, pitch)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn yaw_pitch_round_trip() {
        for f in [Vec3::X, -Vec3::Z, Vec3::new(-1.0, -0.7, -1.0)] {
            let (yaw, pitch) = yaw_pitch_from_forward(f);
            let back = rotation(yaw, pitch) * Vec3::Z;
            assert!((back - f.normalize()).length() < 1e-4, "{f:?} -> {back:?}");
        }
    }

    #[test]
    fn top_view_has_z_up_on_screen() {
        let (yaw, pitch) = yaw_pitch_from_forward(-Vec3::Y);
        let up = rotation(yaw, pitch) * Vec3::Y;
        assert!((up - Vec3::Z).length() < 1e-4, "{up:?}");
    }

    #[test]
    fn look_keeps_eye_fixed() {
        let mut cam = SceneCamera::default();
        let eye = cam.position();
        cam.look(40.0, -20.0);
        assert!((cam.position() - eye).length() < 1e-3);
    }

    #[test]
    fn orbit_keeps_pivot_fixed() {
        let mut cam = SceneCamera::default();
        let pivot = cam.pivot();
        cam.orbit(40.0, 10.0);
        assert!((cam.pivot() - pivot).length() < 1e-5);
    }

    #[test]
    fn snap_then_rotate_restores_perspective() {
        let mut cam = SceneCamera::default();
        cam.snap_to_axis(AxisView::Top);
        assert!(cam.orthographic);
        assert_eq!(cam.view_label(), "Top");
        cam.orbit(10.0, 0.0);
        assert!(!cam.orthographic);
        assert_eq!(cam.view_label(), "Persp");
    }
}
