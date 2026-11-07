//! Scene view camera: pivot/orbit model with fly, pan, orbit, zoom, axis snapping and 2D mode.
//! Input moves goals; critically damped smoothing ([`smooth`]) carries the view after them,
//! frame-rate independent.

pub mod bench;
pub mod smooth;

use egui::{Key, PointerButton};
use glam::{Quat, Vec2, Vec3};
use misty_engine::camera::{Camera, Projection};
use smooth::{Chase, Velocity};

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

/// A saved Scene view camera ([`SceneCamera::state`]).
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CameraState {
    pub pivot: [f32; 3],
    pub yaw: f32,
    pub pitch: f32,
    pub distance: f32,
    pub orthographic: bool,
    pub in_2d: bool,
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

/// Scene camera settings (the camera popover in the Scene view toolbar), saved per project.
#[derive(Clone, Copy, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
#[serde(default)]
pub struct CameraSettings {
    /// Vertical field of view in degrees (perspective).
    pub fov: f32,
    /// Near and far planes follow the distance to the pivot.
    pub dynamic_clipping: bool,
    pub near: f32,
    pub far: f32,
    /// Fly speed multiplier (right mouse + scroll changes it while flying).
    pub speed: f32,
    /// Flying speeds up while a key is held (up to 4× after 2 s).
    pub acceleration: bool,
    /// Moves ease in and out; off, the camera follows the input exactly.
    pub easing: bool,
    /// Smooth time (s) of flying and wheel zoom: how long moves ease in and glide out.
    pub damping: f32,
    /// Smooth time (s) of mouse drags: look, orbit, pan and Alt + right-drag zoom. A steady
    /// drag is followed this far behind (0.012 s is under one frame at 60 FPS).
    pub look_smoothing: f32,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            fov: 60.0,
            dynamic_clipping: true,
            near: 0.03,
            far: 10_000.0,
            speed: 1.0,
            acceleration: true,
            easing: true,
            damping: 0.08,
            look_smoothing: 0.012,
        }
    }
}

impl CameraSettings {
    pub const SPEED_RANGE: std::ops::RangeInclusive<f32> = 0.01..=50.0;
    pub const DAMPING_RANGE: std::ops::RangeInclusive<f32> = 0.0..=0.3;
    pub const LOOK_SMOOTHING_RANGE: std::ops::RangeInclusive<f32> = 0.0..=0.05;

    fn damping(&self) -> f32 {
        if self.easing { self.damping } else { 0.0 }
    }

    fn look_smoothing(&self) -> f32 {
        if self.easing {
            self.look_smoothing
        } else {
            0.0
        }
    }
}

/// One frame of navigation input, already read from the mouse and keyboard.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct NavInput {
    /// Mouse motion for look / orbit, in points (raw mouse motion when available).
    pub turn: Vec2,
    /// Cursor motion, in points (pan and drag zoom follow the cursor).
    pub cursor_delta: Vec2,
    /// Mouse wheel, in points (positive = away from the user).
    pub wheel: f32,
    /// Cursor position relative to the view centre in half view heights, +y up (where the
    /// wheel zooms to); `None` zooms to the pivot.
    pub cursor: Option<Vec2>,
    /// Fly keys: x = D − A, y = E − Q, z = W − S.
    pub fly: Vec3,
    /// Shift: fly faster.
    pub fast: bool,
}

pub struct SceneCamera {
    pose: Pose,
    anim: Option<(Pose, Pose, f32)>,
    pub orthographic: bool,
    pub settings: CameraSettings,
    pub drag: Drag,
    fly_time: f32,
    snapped: Option<AxisView>,
    ortho_before_snap: bool,
    in_2d: bool,
    saved_3d: Option<(Pose, bool)>,
    /// Look (yaw, pitch degrees) still to be applied.
    look: Chase,
    /// Orbit (yaw, pitch degrees) still to be applied.
    orbit: Chase,
    /// Pivot offset (world) still to be applied.
    pan: Chase,
    /// Wheel zoom (log distance) still to be applied, towards `zoom_anchor`.
    zoom: Chase,
    zoom_anchor: Vec2,
    /// Alt + right-drag zoom (log distance) still to be applied, towards the pivot.
    dolly: Chase,
    /// Fly velocity in view space (x right, y world up, z forward).
    fly: Velocity,
    /// Seconds left showing the fly speed in the view after it changed.
    speed_shown: f32,
    /// Where the cursor was hidden for a look drag (it comes back there).
    hidden_cursor: Option<egui::Pos2>,
}

/// Degrees per point of mouse motion.
const LOOK_SPEED: f32 = 0.25;
const ANIM_TIME: f32 = 0.3;
/// Fly speed at Camera Speed 1, metres per second.
const FLY_SPEED: f32 = 5.0;
/// Shift multiplies the fly speed.
const FAST: f32 = 4.0;
/// Log distance per point of wheel (a 40-point notch zooms about 10 %).
const WHEEL_ZOOM: f32 = 0.0025;
/// Log distance per point of Alt + right-drag.
const DRAG_ZOOM: f32 = 0.004;
/// Log speed per point of wheel while flying (a notch is about ×1.2).
const WHEEL_SPEED: f32 = 0.005;
const MIN_DISTANCE: f32 = 0.01;
const MAX_DISTANCE: f32 = 50_000.0;

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
            settings: CameraSettings::default(),
            drag: Drag::None,
            fly_time: 0.0,
            snapped: None,
            ortho_before_snap: false,
            in_2d: false,
            saved_3d: None,
            look: Chase::default(),
            orbit: Chase::default(),
            pan: Chase::default(),
            zoom: Chase::default(),
            zoom_anchor: Vec2::ZERO,
            dolly: Chase::default(),
            fly: Velocity::default(),
            speed_shown: 0.0,
            hidden_cursor: None,
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

    #[cfg(test)]
    pub fn distance(&self) -> f32 {
        self.pose.distance
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
        self.pose.distance * (self.settings.fov.to_radians() * 0.5).tan()
    }

    pub fn camera(&self) -> Camera {
        let s = &self.settings;
        let (near, far) = if s.dynamic_clipping {
            let d = self.pose.distance;
            ((d * 0.002).clamp(0.01, 1.0), (d * 1000.0).max(1000.0))
        } else {
            (s.near, s.far)
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
                projection: Projection::Perspective { fov_y: s.fov },
                near,
                far,
            }
        }
    }

    /// The fly speed while it is shown in the view after a change, with its opacity.
    pub fn speed_overlay(&self) -> Option<(f32, f32)> {
        (self.speed_shown > 0.0).then(|| (self.settings.speed, (self.speed_shown / 0.3).min(1.0)))
    }

    /// Anything still moving: a drag, a fly glide, smoothing catching up or a transition.
    pub fn is_moving(&self) -> bool {
        self.drag != Drag::None
            || self.anim.is_some()
            || !self.look.is_settled()
            || !self.orbit.is_settled()
            || !self.pan.is_settled()
            || !self.zoom.is_settled()
            || !self.dolly.is_settled()
            || !self.fly.is_settled()
            || self.speed_shown > 0.0
    }

    /// Process mouse/keyboard for this frame. Returns true while the camera is moving (the
    /// view should keep repainting).
    pub fn handle_input(
        &mut self,
        ctx: &egui::Context,
        response: &egui::Response,
        view_height: f32,
        view_tool: bool,
    ) -> bool {
        let rect = response.rect;
        let (pressed, alt, dt) = ctx.input(|i| {
            let pressed = [
                PointerButton::Primary,
                PointerButton::Secondary,
                PointerButton::Middle,
            ]
            .into_iter()
            .find(|b| i.pointer.button_pressed(*b));
            (pressed, i.modifiers.alt, i.stable_dt.min(0.1))
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
            // Flying hides the cursor (raw mouse motion keeps turning at the screen edges)
            // and puts it back where the drag started.
            if self.drag == Drag::Look {
                self.hidden_cursor = ctx.input(|i| i.pointer.interact_pos());
                ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                    egui::viewport::CursorGrab::Confined,
                ));
            }
        }
        let still_down = ctx.input(|i| i.pointer.any_down());
        if !still_down {
            self.drag = Drag::None;
        }
        if self.drag != Drag::Look
            && let Some(at) = self.hidden_cursor.take()
        {
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorGrab(
                egui::viewport::CursorGrab::None,
            ));
            ctx.send_viewport_cmd(egui::ViewportCommand::CursorPosition(at));
        }

        let line = ctx.options(|o| o.input_options.line_scroll_speed);
        let v2 = |v: egui::Vec2| Vec2::new(v.x, v.y);
        let input = ctx.input(|i| {
            let mut input = NavInput {
                cursor_delta: v2(i.pointer.delta()),
                ..Default::default()
            };
            // Raw mouse motion (no pointer acceleration, not stopped by the screen edge) when
            // the platform sends it, in about the same units as points.
            input.turn = match i.pointer.motion() {
                Some(m) => v2(m) / i.pixels_per_point.max(0.1),
                None => v2(i.pointer.delta()),
            };
            // The wheel unsmoothed (egui smooths it for scroll areas; the camera has its own).
            for e in &i.events {
                if let egui::Event::MouseWheel { unit, delta, .. } = e {
                    let points = match unit {
                        egui::MouseWheelUnit::Point => *delta,
                        egui::MouseWheelUnit::Line => *delta * line,
                        egui::MouseWheelUnit::Page => *delta * rect.height(),
                    };
                    // Shift turns the wheel horizontal on some platforms.
                    input.wheel += points.x + points.y;
                }
            }
            if !response.hovered() && self.drag == Drag::None {
                input.wheel = 0.0;
            }
            input.cursor = i
                .pointer
                .hover_pos()
                .filter(|p| rect.contains(*p))
                .map(|p| {
                    let half = (rect.height() * 0.5).max(1.0);
                    Vec2::new(
                        (p.x - rect.center().x) / half,
                        (rect.center().y - p.y) / half,
                    )
                });
            if self.drag == Drag::Look {
                let axis = |pos: Key, neg: Key| {
                    i.key_down(pos) as i32 as f32 - i.key_down(neg) as i32 as f32
                };
                input.fly = Vec3::new(
                    axis(Key::D, Key::A),
                    axis(Key::E, Key::Q),
                    axis(Key::W, Key::S),
                );
                input.fast = i.modifiers.shift;
            }
            input
        });
        let moving = self.update(&input, view_height, dt);

        if let Some(icon) = match self.drag {
            Drag::Look => Some(egui::CursorIcon::None),
            Drag::Pan => Some(egui::CursorIcon::Grabbing),
            Drag::Orbit => Some(egui::CursorIcon::AllScroll),
            Drag::Zoom => Some(egui::CursorIcon::ZoomIn),
            Drag::None if view_tool && response.hovered() => Some(egui::CursorIcon::Grab),
            Drag::None => None,
        } {
            ctx.set_cursor_icon(icon);
        }
        moving
    }

    /// Apply one frame of input (`dt` seconds) for the current drag and advance the
    /// smoothing. Returns true while anything is still moving.
    pub fn update(&mut self, input: &NavInput, view_height: f32, dt: f32) -> bool {
        match self.drag {
            Drag::Look => {
                self.look
                    .push(Vec3::new(input.turn.x, input.turn.y, 0.0) * LOOK_SPEED);
                if input.wheel != 0.0 {
                    // Right mouse + wheel changes the fly speed, like the camera setting.
                    let range = CameraSettings::SPEED_RANGE;
                    self.settings.speed = (self.settings.speed * (input.wheel * WHEEL_SPEED).exp())
                        .clamp(*range.start(), *range.end());
                    self.speed_shown = 1.2;
                }
            }
            Drag::Pan => {
                let per_point = 2.0 * self.half_height() / view_height.max(1.0);
                let r = self.rotation();
                let d = input.cursor_delta;
                self.pan
                    .push((r * Vec3::X * -d.x + r * Vec3::Y * d.y) * per_point);
            }
            Drag::Orbit => self
                .orbit
                .push(Vec3::new(input.turn.x, input.turn.y, 0.0) * LOOK_SPEED),
            Drag::Zoom => {
                let d = input.cursor_delta;
                self.dolly.push(Vec3::X * -(d.x + d.y) * DRAG_ZOOM);
            }
            Drag::None => {
                if input.wheel != 0.0 {
                    self.zoom.push(Vec3::X * -input.wheel * WHEEL_ZOOM);
                    self.zoom_anchor = input.cursor.unwrap_or(Vec2::ZERO);
                }
            }
        }
        let fly_target = self.fly_target(input, dt);
        self.step(fly_target, dt);
        self.speed_shown = (self.speed_shown - dt).max(0.0);
        self.is_moving()
    }

    /// Target fly velocity (view space) for the keys held.
    fn fly_target(&mut self, input: &NavInput, dt: f32) -> Vec3 {
        if self.drag != Drag::Look || input.fly == Vec3::ZERO {
            self.fly_time = 0.0;
            return Vec3::ZERO;
        }
        self.fly_time += dt;
        // Speed at the middle of the frame, so long and short frames ramp alike.
        let held = self.fly_time - dt * 0.5;
        let ramp = if self.settings.acceleration {
            1.0 + (held * 1.5).min(3.0)
        } else {
            1.0
        };
        let fast = if input.fast { FAST } else { 1.0 };
        input.fly.normalize_or_zero() * FLY_SPEED * self.settings.speed * ramp * fast
    }

    /// Advance the smoothing by `dt` and apply what it lets through.
    fn step(&mut self, fly_target: Vec3, dt: f32) {
        let direct = self.settings.look_smoothing();
        let damping = self.settings.damping();

        let (yaw, pitch) = (self.pose.yaw, self.pose.pitch);
        let turn = self.look.step(direct, dt, 1e-4);
        if self.look_by(turn.x, turn.y) {
            self.look.cancel_axis(1);
        }
        let turn = self.orbit.step(direct, dt, 1e-4);
        if self.orbit_by(turn.x, turn.y) {
            self.orbit.cancel_axis(1);
        }
        let eps = self.pose.distance * 1e-5;
        self.pose.pivot += self.pan.step(direct, dt, eps);
        let dolly = self.dolly.step(direct, dt, 1e-5).x;
        self.zoom_by(dolly, Vec2::ZERO);
        let zoom = self.zoom.step(damping, dt, 1e-5).x;
        self.zoom_by(zoom, self.zoom_anchor);

        let local = self.fly.step(fly_target, damping, dt);
        if local != Vec3::ZERO {
            // The view turned during the frame: fly along its direction halfway through, so a
            // turning flight takes the same path at any frame rate.
            let r = rotation((yaw + self.pose.yaw) * 0.5, (pitch + self.pose.pitch) * 0.5);
            self.pose.pivot += r * Vec3::X * local.x + Vec3::Y * local.y + r * Vec3::Z * local.z;
        }
        self.step_animation(dt);
    }

    /// Rotate around the eye (first-person look). True when the pitch hit its limit.
    fn look_by(&mut self, dx: f32, dy: f32) -> bool {
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.leave_snapped_view();
        let eye = self.position();
        let limited = self.turn_by(dx, dy);
        self.pose.pivot = eye + self.rotation() * Vec3::Z * self.pose.distance;
        limited
    }

    /// Rotate around the pivot. True when the pitch hit its limit.
    fn orbit_by(&mut self, dx: f32, dy: f32) -> bool {
        if dx == 0.0 && dy == 0.0 {
            return false;
        }
        self.leave_snapped_view();
        self.turn_by(dx, dy)
    }

    fn turn_by(&mut self, dx: f32, dy: f32) -> bool {
        self.pose.yaw += dx;
        let pitch = self.pose.pitch + dy;
        self.pose.pitch = pitch.clamp(-89.9, 89.9);
        self.pose.pitch != pitch
    }

    /// Change the distance by `log` (natural log, < 0 = closer), keeping the point under
    /// `anchor` (half view heights from the centre, +y up) where it is on screen.
    fn zoom_by(&mut self, log: f32, anchor: Vec2) {
        if log == 0.0 {
            return;
        }
        let d = self.pose.distance;
        let f = ((d * log.exp()).clamp(MIN_DISTANCE, MAX_DISTANCE)) / d;
        let r = self.rotation();
        let hh = self.half_height();
        let point = self.pose.pivot + (r * Vec3::X * anchor.x + r * Vec3::Y * anchor.y) * hh;
        self.pose.pivot = point + (self.pose.pivot - point) * f;
        self.pose.distance = d * f;
    }

    /// Stop every move in progress (smoothing, glide) where it is.
    fn stop_motion(&mut self) {
        for c in [
            &mut self.look,
            &mut self.orbit,
            &mut self.pan,
            &mut self.zoom,
            &mut self.dolly,
        ] {
            *c = Chase::default();
        }
        self.fly.stop();
    }

    /// Smoothly move the pivot onto a point (Frame Selected, F).
    pub fn frame(&mut self, center: Vec3, radius: f32) {
        let mut target = self.pose;
        target.pivot = center;
        target.distance =
            (radius.max(0.25) * 2.5) / (self.settings.fov.to_radians() * 0.5).tan().max(0.1);
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

    /// Everything needed to put the view back exactly (the restore file of a restart).
    pub fn state(&self) -> CameraState {
        // Mid-animation, the pose being animated to is where the view is going.
        let p = self.anim.map_or(self.pose, |(_, to, _)| to);
        CameraState {
            pivot: p.pivot.to_array(),
            yaw: p.yaw,
            pitch: p.pitch,
            distance: p.distance,
            orthographic: self.orthographic,
            in_2d: self.in_2d,
        }
    }

    /// Put the view back as [`Self::state`] gave it (non-finite values are ignored).
    pub fn set_state(&mut self, st: &CameraState) {
        let pivot = Vec3::from(st.pivot);
        let ok = pivot.is_finite()
            && st.yaw.is_finite()
            && st.pitch.is_finite()
            && st.distance.is_finite()
            && st.distance > 0.0;
        if !ok {
            return;
        }
        self.pose = Pose {
            pivot,
            yaw: st.yaw,
            pitch: st.pitch,
            distance: st.distance,
        };
        self.anim = None;
        self.orthographic = st.orthographic;
        self.in_2d = st.in_2d;
        self.snapped = None;
        self.saved_3d = None;
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
        self.stop_motion();
        self.anim = Some((self.pose, target, 0.0));
    }

    fn step_animation(&mut self, dt: f32) {
        if let Some((from, to, t)) = &mut self.anim {
            *t = (*t + dt / ANIM_TIME).min(1.0);
            // Ease in and out (smoothstep of an ease-out): starts gently, lands softly.
            let out = 1.0 - (1.0 - *t).powi(3);
            let e = out * out * (3.0 - 2.0 * out);
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
        cam.look_by(10.0, -5.0);
        assert!((cam.position() - eye).length() < 1e-3);
    }

    #[test]
    fn orbit_keeps_pivot_fixed() {
        let mut cam = SceneCamera::default();
        let pivot = cam.pivot();
        cam.orbit_by(10.0, 2.5);
        assert!((cam.pivot() - pivot).length() < 1e-5);
    }

    #[test]
    fn snap_then_rotate_restores_perspective() {
        let mut cam = SceneCamera::default();
        cam.snap_to_axis(AxisView::Top);
        assert!(cam.orthographic);
        assert_eq!(cam.view_label(), "Top");
        cam.orbit_by(2.5, 0.0);
        assert!(!cam.orthographic);
        assert_eq!(cam.view_label(), "Persp");
    }

    /// Fly `seconds` at `fps` with `input(t)` per frame (right mouse held); the last frame is
    /// shortened to end on time.
    fn fly(fps: f32, seconds: f32, input: impl Fn(f32, f32) -> NavInput) -> SceneCamera {
        let mut cam = SceneCamera {
            drag: Drag::Look,
            ..Default::default()
        };
        let mut t = 0.0;
        while t < seconds - 1e-6 {
            let dt = (1.0 / fps).min(seconds - t);
            cam.update(&input(t, dt), 600.0, dt);
            t += dt;
        }
        cam
    }

    /// Hold W while turning at 60°/s for a second, then let go: the same flight at 30, 60
    /// and 144 FPS.
    #[test]
    fn flythrough_is_frame_rate_independent() {
        let cams: Vec<SceneCamera> = [30.0, 60.0, 144.0]
            .into_iter()
            .map(|fps| {
                fly(fps, 1.5, |t, dt| {
                    let held = (t + dt).min(1.0) - t.min(1.0);
                    NavInput {
                        // The mouse moved during this frame.
                        turn: Vec2::new(60.0 * held.max(0.0) / LOOK_SPEED, 0.0),
                        fly: if t + dt * 0.5 < 1.0 {
                            Vec3::Z
                        } else {
                            Vec3::ZERO
                        },
                        ..Default::default()
                    }
                })
            })
            .collect();
        let start = SceneCamera::default().position();
        let flown = (cams[1].position() - start).length();
        assert!(flown > 5.0, "flew {flown} m");
        for c in &cams[1..] {
            let d = (c.position() - cams[0].position()).length();
            // Measured: 2.4 cm (60 vs 30 FPS) and 5.4 cm (144 vs 30) of 8.5 m.
            assert!(d < 0.01 * flown, "eye differs by {d} m of {flown}");
            assert!((c.pose.yaw - cams[0].pose.yaw).abs() < 0.01, "yaw");
            assert!(!c.fly.vel.is_nan());
        }
    }

    /// Default look smoothing follows a steady mouse less than one 60 FPS frame behind.
    #[test]
    fn look_smoothing_lags_less_than_a_frame() {
        let rate = 90.0; // degrees per second
        let cam = fly(240.0, 0.5, |_, dt| NavInput {
            turn: Vec2::new(rate * dt / LOOK_SPEED, 0.0),
            ..Default::default()
        });
        let lag = cam.look.remaining().x / rate;
        assert!(lag > 0.0 && lag < 1.0 / 60.0, "lag {lag} s");
    }

    #[test]
    fn easing_off_follows_input_exactly() {
        let mut cam = SceneCamera::default();
        cam.settings.easing = false;
        cam.drag = Drag::Look;
        let yaw = cam.pose.yaw;
        let input = NavInput {
            turn: Vec2::new(40.0, 0.0),
            fly: Vec3::Z,
            ..Default::default()
        };
        cam.update(&input, 600.0, 0.1);
        assert!((cam.pose.yaw - yaw - 40.0 * LOOK_SPEED).abs() < 1e-4);
        cam.drag = Drag::None;
        cam.update(&NavInput::default(), 600.0, 0.1);
        assert!(!cam.is_moving(), "stops at once without easing");
    }

    /// Letting go of W glides a short way (speed × damping), then the camera rests and stops
    /// asking for repaints.
    #[test]
    fn fly_glides_to_rest_after_release() {
        let mut cam = SceneCamera::default();
        cam.settings.acceleration = false;
        cam.drag = Drag::Look;
        let w = NavInput {
            fly: Vec3::Z,
            ..Default::default()
        };
        for _ in 0..60 {
            cam.update(&w, 600.0, 1.0 / 60.0);
        }
        let released = cam.position();
        cam.drag = Drag::None;
        let mut frames = 0;
        while cam.update(&NavInput::default(), 600.0, 1.0 / 60.0) {
            frames += 1;
            assert!(frames < 120, "never came to rest");
        }
        let glide = (cam.position() - released).length();
        let expected = FLY_SPEED * cam.settings.damping;
        assert!(
            (glide - expected).abs() < 0.05,
            "glide {glide} m, expected {expected}"
        );
    }

    /// The wheel zooms towards the point under the cursor: it stays put on screen.
    #[test]
    fn wheel_zoom_keeps_the_point_under_the_cursor() {
        let mut cam = SceneCamera::default();
        let aspect = 16.0 / 9.0;
        let anchor = Vec2::new(0.6, 0.3);
        let r = cam.rotation();
        let point =
            cam.pivot() + (r * Vec3::X * anchor.x + r * Vec3::Y * anchor.y) * cam.half_height();
        let (before, _) = cam.camera().world_to_screen(aspect, point).unwrap();
        let expected = glam::Vec2::new(0.5 + anchor.x / (2.0 * aspect), 0.5 - anchor.y / 2.0);
        assert!(
            (before - expected).length() < 1e-3,
            "{before} vs {expected}"
        );
        let distance = cam.distance();
        let wheel = NavInput {
            wheel: 120.0,
            cursor: Some(anchor),
            ..Default::default()
        };
        cam.update(&wheel, 600.0, 1.0 / 60.0);
        while cam.update(&NavInput::default(), 600.0, 1.0 / 60.0) {}
        let (after, _) = cam.camera().world_to_screen(aspect, point).unwrap();
        assert!(cam.distance() < distance * 0.8, "zoomed in");
        assert!((after - before).length() < 1e-3, "{before} -> {after}");
    }

    /// F: the view eases onto the selection and lands exactly.
    #[test]
    fn frame_selected_animates_then_lands() {
        let mut cam = SceneCamera::default();
        let center = Vec3::new(10.0, 2.0, -4.0);
        cam.frame(center, 1.0);
        cam.update(&NavInput::default(), 600.0, 1.0 / 60.0);
        let early = cam.pivot();
        assert!(
            early != Vec3::ZERO && (early - center).length() > 1.0,
            "in transit"
        );
        let mut frames = 0;
        while cam.update(&NavInput::default(), 600.0, 1.0 / 60.0) {
            frames += 1;
        }
        assert!((cam.pivot() - center).length() < 1e-5);
        assert!((16..=20).contains(&frames), "{frames} frames");
    }

    #[test]
    fn right_mouse_wheel_changes_speed_not_zoom() {
        let mut cam = SceneCamera {
            drag: Drag::Look,
            ..Default::default()
        };
        let distance = cam.distance();
        let wheel = NavInput {
            wheel: 40.0,
            ..Default::default()
        };
        cam.update(&wheel, 600.0, 1.0 / 60.0);
        assert!(cam.settings.speed > 1.1);
        assert!(cam.speed_overlay().is_some());
        assert_eq!(cam.distance(), distance);
    }
}
