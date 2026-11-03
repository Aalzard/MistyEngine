//! Edit-mode preview of the selected Particle Systems (and their child systems), with the
//! Scene view's "Particle Effect" overlay: Play/Pause, Restart, Stop, speed, time and count.

use egui::{Color32, Rect, RichText, Ui, vec2};
use misty_engine::hecs::Entity;
use misty_engine::particles::{self, ParticleEmitter, ParticleSystem};

use crate::state::EditorState;

/// Overlay settings that outlive a selection.
pub struct ParticlePreview {
    pub speed: f32,
}

impl Default for ParticlePreview {
    fn default() -> Self {
        Self { speed: 1.0 }
    }
}

/// Systems previewed now: selected objects and their descendants that have a Particle System.
pub fn targets(s: &EditorState) -> Vec<Entity> {
    if s.play.in_play_mode() {
        return Vec::new();
    }
    s.scene
        .walk()
        .into_iter()
        .map(|(e, _)| e)
        .filter(|e| s.scene.world.get::<&ParticleSystem>(*e).is_ok())
        .filter(|e| s.scene.active_in_hierarchy(*e))
        .filter(|e| {
            s.selection
                .list()
                .iter()
                .any(|sel| s.scene.is_ancestor(*sel, *e))
        })
        .collect()
}

/// Advance the preview by `dt`; emitters of deselected systems are dropped.
/// Returns true while something is moving (the view needs repainting).
pub fn tick(s: &mut EditorState, dt: f32) -> bool {
    if s.play.in_play_mode() {
        return false;
    }
    let targets = targets(s);
    let stale: Vec<Entity> = s
        .scene
        .world
        .query::<(Entity, &ParticleEmitter)>()
        .iter()
        .map(|(e, _)| e)
        .filter(|e| !targets.contains(e))
        .collect();
    for e in stale {
        let _ = s.scene.world.remove_one::<ParticleEmitter>(e);
    }
    let gravity = s.project_settings.physics.gravity;
    // Long gaps (the window was idle) count as one short step.
    let dt = dt.min(0.1) * s.particle_preview.speed.max(0.0);
    let mut moving = false;
    for e in targets {
        // Selecting a system starts it, whatever Play On Awake says.
        particles::ensure_emitter(&mut s.scene, e, true, gravity);
        let world = s.scene.world_matrix(e);
        particles::step_one(&mut s.scene, e, &world, dt, gravity);
        moving |= s
            .scene
            .world
            .get::<&ParticleEmitter>(e)
            .is_ok_and(|em| em.is_playing() || (!em.is_paused() && em.count() > 0));
    }
    moving
}

/// Overlay buttons.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Command {
    PlayPause,
    Restart,
    Stop,
}

/// Apply an overlay command to every previewed system.
pub fn command(s: &mut EditorState, cmd: Command) {
    let gravity = s.project_settings.physics.gravity;
    let targets = targets(s);
    let any_playing = targets.iter().any(|e| {
        s.scene
            .world
            .get::<&ParticleEmitter>(*e)
            .is_ok_and(|em| em.is_playing())
    });
    for e in targets {
        particles::with_emitter(&mut s.scene, e, gravity, |em, sys, m| match cmd {
            Command::PlayPause if any_playing => em.pause(),
            Command::PlayPause => em.play(sys, m, gravity),
            Command::Restart => em.restart(sys, m, gravity),
            // The overlay's Stop also clears, so the view is empty.
            Command::Stop => {
                em.stop();
                em.clear();
            }
        });
    }
}

/// The overlay in the Scene view's lower-right corner (nothing when no system is previewed).
pub fn overlay(ui: &mut Ui, rect: Rect, s: &mut EditorState) {
    let targets = targets(s);
    let Some(&first) = targets.first() else {
        return;
    };
    let (time, count, playing) = {
        let w = &s.scene.world;
        let time = w
            .get::<&ParticleEmitter>(first)
            .map(|em| em.total_time)
            .unwrap_or(0.0);
        let count: usize = targets
            .iter()
            .filter_map(|e| w.get::<&ParticleEmitter>(*e).ok().map(|em| em.count()))
            .sum();
        let playing = targets.iter().any(|e| {
            w.get::<&ParticleEmitter>(*e)
                .is_ok_and(|em| em.is_playing())
        });
        (time, count, playing)
    };
    let size = vec2(220.0, 112.0);
    let pos = rect.right_bottom() - size - vec2(10.0, 10.0);
    let mut cmd = None;
    egui::Area::new(egui::Id::new("particle_effect_overlay"))
        .fixed_pos(pos)
        .order(egui::Order::Middle)
        .constrain_to(rect)
        .show(ui.ctx(), |ui| {
            egui::Frame::new()
                .fill(Color32::from_rgba_unmultiplied(40, 40, 40, 235))
                .corner_radius(4.0)
                .inner_margin(8.0)
                .show(ui, |ui| {
                    ui.set_width(size.x - 16.0);
                    ui.label(RichText::new("Particle Effect").strong());
                    ui.horizontal(|ui| {
                        let label = if playing { "Pause" } else { "Play" };
                        if ui.button(label).clicked() {
                            cmd = Some(Command::PlayPause);
                        }
                        if ui.button("Restart").clicked() {
                            cmd = Some(Command::Restart);
                        }
                        if ui.button("Stop").clicked() {
                            cmd = Some(Command::Stop);
                        }
                    });
                    egui::Grid::new("particle_effect_grid")
                        .num_columns(2)
                        .show(ui, |ui| {
                            ui.label("Playback Speed");
                            ui.add(
                                egui::DragValue::new(&mut s.particle_preview.speed)
                                    .speed(0.01)
                                    .range(0.0..=10.0),
                            );
                            ui.end_row();
                            ui.label("Playback Time");
                            ui.label(format!("{time:.2}"));
                            ui.end_row();
                            ui.label("Particles");
                            ui.label(count.to_string());
                            ui.end_row();
                        });
                });
        });
    if let Some(c) = cmd {
        command(s, c);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::actions::{Action, Create};
    use crate::project::ProjectBrowser;

    fn state() -> EditorState {
        EditorState::new(ProjectBrowser::open(
            std::env::temp_dir().join("misty_particle_preview_test"),
        ))
    }

    fn count(s: &EditorState, e: Entity) -> usize {
        s.scene
            .world
            .get::<&ParticleEmitter>(e)
            .map(|em| em.count())
            .unwrap_or(0)
    }

    #[test]
    fn menu_creates_a_system_facing_up_and_selection_previews_it() {
        let mut s = state();
        let what = Create::ParticleSystem;
        crate::commands::apply(&mut s, Action::Create { what, parent: None });
        let e = s.scene.find_by_name("Particle System").unwrap();
        assert_eq!(s.scene.component_names(e), ["Transform", "Particle System"]);
        let up = s.scene.world_rotation(e) * misty_engine::glam::Vec3::Z;
        assert!(up.y > 0.999, "emits upwards like the default: {up}");

        for _ in 0..30 {
            tick(&mut s, 1.0 / 60.0);
        }
        assert!(count(&s, e) > 0, "selected system previews in edit mode");
        command(&mut s, Command::PlayPause);
        let n = count(&s, e);
        tick(&mut s, 0.5);
        assert_eq!(count(&s, e), n, "paused");
        command(&mut s, Command::Stop);
        assert_eq!(count(&s, e), 0);
        command(&mut s, Command::Restart);
        tick(&mut s, 0.5);
        assert!(count(&s, e) > 0);

        s.selection.clear();
        tick(&mut s, 1.0 / 60.0);
        assert!(
            s.scene.world.get::<&ParticleEmitter>(e).is_err(),
            "deselected: preview ends"
        );
    }

    #[test]
    fn preview_particles_are_not_saved_or_played() {
        let mut s = state();
        let what = Create::ParticleSystem;
        crate::commands::apply(&mut s, Action::Create { what, parent: None });
        let e = s.scene.find_by_name("Particle System").unwrap();
        tick(&mut s, 0.5);
        assert!(count(&s, e) > 0);
        assert!(
            s.scene.clone().world.get::<&ParticleEmitter>(e).is_err(),
            "copies (undo, Play) don't carry particles"
        );
        crate::play::start(&mut s);
        assert_eq!(count(&s, e), 0, "the game starts fresh");
        assert!(targets(&s).is_empty(), "no preview while playing");
        for _ in 0..30 {
            crate::play::tick(&mut s, 1.0 / 60.0);
        }
        assert!(count(&s, e) > 0, "Play On Awake");
        crate::play::stop(&mut s);
    }
}
