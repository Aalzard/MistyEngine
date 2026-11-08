//! Scene view ▸ Physics Debug draw mode: colliders coloured by what their body is doing,
//! contacts, joints, sleeping bodies and cloth particles (from the engine's `physics::debug`).
//!
//! While playing the lines come from the running physics world. In Edit mode a physics world is
//! built from the scene (at most twice a second while the mode is on), so the view shows the
//! colliders and joints exactly as Play will create them.

use std::cell::RefCell;
use std::time::Instant;

use egui::{Color32, Rect, Ui};
use glam::Vec3;
use misty_engine::physics::Physics;
use misty_engine::physics::debug::{self, Counts};

use crate::gizmos::Lines;
use crate::state::EditorState;

thread_local! {
    /// The Edit-mode world and when it was built.
    static EDIT_WORLD: RefCell<Option<(Instant, Physics)>> = const { RefCell::new(None) };
}

/// Add the Physics Debug lines to `lines`; returns what was drawn.
pub fn lines(state: &EditorState, lines: &mut Lines) -> Counts {
    let (segments, counts) = match state.runtime.as_ref().filter(|_| state.play.in_play_mode()) {
        Some(rt) => rt.with_physics(|p| p.debug_lines()),
        None => EDIT_WORLD.with(|w| {
            let mut w = w.borrow_mut();
            let stale = w
                .as_ref()
                .is_none_or(|(t, _)| t.elapsed().as_secs_f32() > 0.5);
            if stale {
                let p = Physics::build_with(&state.scene, &state.project_settings.physics);
                *w = Some((Instant::now(), p));
            }
            w.as_ref().map(|(_, p)| p.debug_lines()).unwrap_or_default()
        }),
    };
    for s in segments {
        lines.line(s.a, s.b, s.color);
    }
    // Cloth particles while playing.
    for pose in state
        .scene
        .world
        .query::<&misty_engine::physics::cloth::ClothPose>()
        .iter()
    {
        for p in pose.particles.iter() {
            for a in [Vec3::X, Vec3::Y, Vec3::Z] {
                lines.line(*p - a * 0.01, *p + a * 0.01, debug::CLOTH);
            }
        }
    }
    counts
}

/// Forget the Edit-mode world.
#[cfg(test)]
pub fn reset() {
    EDIT_WORLD.with(|w| *w.borrow_mut() = None);
}

fn swatch(c: [f32; 4]) -> Color32 {
    Color32::from_rgba_unmultiplied(
        (c[0] * 255.0) as u8,
        (c[1] * 255.0) as u8,
        (c[2] * 255.0) as u8,
        255,
    )
}

/// The legend in the Scene view's corner.
pub fn legend(ui: &Ui, rect: Rect, counts: Counts) {
    let painter = ui.painter().with_clip_rect(rect);
    let rows = [
        (debug::STATIC, "Static".to_owned()),
        (debug::DYNAMIC, "Dynamic".to_owned()),
        (debug::SLEEPING, format!("Sleeping ({})", counts.sleeping)),
        (debug::KINEMATIC, "Kinematic".to_owned()),
        (debug::TRIGGER, "Trigger".to_owned()),
        (debug::CONTACT, format!("Contacts ({})", counts.contacts)),
        (debug::JOINT, format!("Joints ({})", counts.joints)),
    ];
    // Bottom right, clear of the view gizmo and the toolbar overlays.
    let x = rect.right() - 160.0;
    let mut y = rect.bottom() - (18.0 * 8.0 + 14.0);
    painter.rect_filled(
        Rect::from_min_size(
            egui::pos2(x - 8.0, y - 6.0),
            egui::vec2(150.0, 18.0 * 8.0 + 8.0),
        ),
        4.0,
        Color32::from_black_alpha(150),
    );
    painter.text(
        egui::pos2(x, y),
        egui::Align2::LEFT_TOP,
        format!("Physics Debug: {} colliders", counts.colliders),
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );
    y += 18.0;
    for (c, label) in rows {
        painter.rect_filled(
            Rect::from_min_size(egui::pos2(x, y + 3.0), egui::vec2(10.0, 10.0)),
            2.0,
            swatch(c),
        );
        painter.text(
            egui::pos2(x + 16.0, y),
            egui::Align2::LEFT_TOP,
            label,
            egui::FontId::proportional(12.0),
            Color32::WHITE,
        );
        y += 18.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::ProjectBrowser;
    use misty_engine::scene::{BoxCollider, FixedJoint, Rigidbody, Transform};

    #[test]
    fn edit_mode_draws_colliders_and_joints() {
        let mut s = EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_physics_debug_view"),
        ));
        reset();
        let floor = s.scene.spawn("Floor", None);
        s.scene.add(floor, BoxCollider::default());
        let b = s.scene.spawn("Crate", None);
        s.scene
            .set_transform(b, Transform::from_position(Vec3::new(0.0, 2.0, 0.0)));
        s.scene.add(b, BoxCollider::default());
        s.scene.add(b, Rigidbody::default());
        s.scene.add(b, FixedJoint::default());
        let mut l = Lines::default();
        let counts = lines(&s, &mut l);
        assert_eq!(counts.colliders, 2);
        assert_eq!(counts.joints, 1);
        // Two wire boxes (12 edges each) plus the joint line and frame axes.
        assert!(l.0.len() >= 2 * 24 + 2, "{}", l.0.len());
        let statics = l.0.iter().filter(|v| v.color == debug::STATIC).count();
        let dynamics = l.0.iter().filter(|v| v.color == debug::DYNAMIC).count();
        assert_eq!((statics, dynamics), (24, 24));
    }
}
