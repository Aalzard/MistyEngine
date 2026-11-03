//! Edit ▸ Project Settings: a floating window with a category list and that category's fields.
//! Every change is saved to `ProjectSettings/ProjectSettings.json` right away.

use egui::Ui;

use misty_engine::glam::Vec3;
use misty_engine::physics::LAYERS;
use misty_engine::settings::{PhysicsSettings, ProjectSettings};

use crate::state::EditorState;
use crate::widgets::vec3_row;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    #[default]
    Physics,
    Time,
}

impl Category {
    pub const ALL: [Category; 2] = [Category::Physics, Category::Time];

    pub fn title(self) -> &'static str {
        match self {
            Category::Physics => "Physics",
            Category::Time => "Time",
        }
    }
}

/// Window state.
#[derive(Default)]
pub struct ProjectSettingsWindow {
    pub open: bool,
    pub category: Category,
}

/// Settings of the project at `s.project.root`; problems go to the Console.
pub fn load(s: &mut EditorState) {
    match ProjectSettings::load(&s.project.root) {
        Ok(settings) => s.project_settings = settings,
        Err(e) => {
            s.project_settings = ProjectSettings::default();
            s.console
                .warn(format!("ProjectSettings.json: {e}. Using defaults."));
        }
    }
}

/// Clamp, save, and apply what can change live (time scale while playing).
pub fn changed(s: &mut EditorState) {
    s.project_settings.sanitize();
    if let Err(e) = s.project_settings.save(&s.project.root) {
        s.console.log(
            crate::console::LogLevel::Error,
            format!("Could not save Project Settings: {e}"),
        );
    }
    if s.play.in_play_mode() {
        s.time.time_scale = s.project_settings.time.time_scale;
        s.time.maximum_delta = s.project_settings.time.maximum_allowed_timestep;
    }
}

/// Draw the window if it is open.
pub fn show(ctx: &egui::Context, s: &mut EditorState) {
    let mut open = s.project_settings_window.open;
    egui::Window::new("Project Settings")
        .open(&mut open)
        .default_size([620.0, 380.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.project_settings_window.open = open;
}

/// Category list on the left, fields on the right.
pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(130.0);
            for c in Category::ALL {
                let selected = s.project_settings_window.category == c;
                if ui.selectable_label(selected, c.title()).clicked() {
                    s.project_settings_window.category = c;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let category = s.project_settings_window.category;
            ui.heading(category.title());
            ui.add_space(4.0);
            let edited = match category {
                Category::Physics => physics(ui, &mut s.project_settings),
                Category::Time => time(ui, &mut s.project_settings),
            };
            if edited {
                changed(s);
            }
        });
    });
}

/// Label + field row with a fixed label column.
fn row(ui: &mut Ui, label: &str, tip: &str, add: impl FnOnce(&mut Ui) -> bool) -> bool {
    ui.horizontal(|ui| {
        ui.add_sized([160.0, 18.0], egui::Label::new(label).truncate())
            .on_hover_text(tip);
        add(ui)
    })
    .inner
}

fn physics(ui: &mut Ui, p: &mut ProjectSettings) -> bool {
    let p = &mut p.physics;
    let mut gravity = p.gravity.to_array();
    let mut edited = vec3_row(ui, "Gravity", &mut gravity, 0.05);
    p.gravity = Vec3::from_array(gravity);
    edited |= row(
        ui,
        "Max Steps Per Frame",
        "Upper bound of fixed physics steps in one frame, so a slow frame can't spiral.",
        |ui| {
            ui.add(egui::DragValue::new(&mut p.max_steps_per_frame).range(1..=100))
                .changed()
        },
    );
    edited |= row(
        ui,
        "Queries Hit Triggers",
        "Raycasts, sphere casts and overlap queries also return trigger colliders.",
        |ui| ui.checkbox(&mut p.queries_hit_triggers, "").changed(),
    );
    ui.add_space(6.0);
    ui.weak("Friction and Bounciness are set on each collider.");
    ui.add_space(8.0);
    edited |= layer_matrix(ui, p);
    edited
}

/// Layer Collision Matrix: rows top to bottom, columns in reverse order, one box per pair.
fn layer_matrix(ui: &mut Ui, p: &mut PhysicsSettings) -> bool {
    let mut edited = false;
    egui::CollapsingHeader::new("Layer Collision Matrix")
        .id_salt("layer_matrix")
        .default_open(true)
        .show(ui, |ui| {
            let n = LAYERS.len();
            egui::Grid::new("layer_matrix_grid")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    ui.label("");
                    for j in (0..n).rev() {
                        ui.label(j.to_string()).on_hover_text(LAYERS[j]);
                    }
                    ui.end_row();
                    for (i, name) in LAYERS.iter().enumerate() {
                        ui.label(format!("{i} {name}"));
                        for j in (0..n).rev() {
                            if j < i {
                                ui.label("");
                                continue;
                            }
                            let (a, b) = (i as u8, j as u8);
                            let mut on = p.layers_collide(a, b);
                            let tip = format!("{name} × {}", LAYERS[j]);
                            if ui.checkbox(&mut on, "").on_hover_text(tip).changed() {
                                p.set_layers_collide(a, b, on);
                                edited = true;
                            }
                        }
                        ui.end_row();
                    }
                });
        });
    edited
}

fn time(ui: &mut Ui, p: &mut ProjectSettings) -> bool {
    let mut edited = row(
        ui,
        "Fixed Timestep",
        "Seconds between physics steps and FixedUpdate calls.",
        |ui| seconds(ui, &mut p.physics.fixed_timestep),
    );
    edited |= row(
        ui,
        "Maximum Allowed Timestep",
        "Longest frame delta the game sees; long hitches are capped to this.",
        |ui| seconds(ui, &mut p.time.maximum_allowed_timestep),
    );
    edited |= row(
        ui,
        "Time Scale",
        "Speed at which time passes (1 = real time, 0.5 = slow motion).",
        |ui| {
            ui.add(
                egui::DragValue::new(&mut p.time.time_scale)
                    .speed(0.01)
                    .range(0.0..=100.0)
                    .max_decimals(3),
            )
            .changed()
        },
    );
    edited
}

fn seconds(ui: &mut Ui, v: &mut f32) -> bool {
    ui.add(
        egui::DragValue::new(v)
            .speed(0.001)
            .range(0.0001..=10.0)
            .max_decimals(4),
    )
    .changed()
}

#[cfg(test)]
mod tests {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::actions::Action;
    use crate::project::ProjectBrowser;

    fn state(name: &str) -> EditorState {
        let root = std::env::temp_dir().join(name);
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        EditorState::new(ProjectBrowser::open(root))
    }

    #[test]
    fn menu_action_opens_the_window_and_categories_switch() {
        let mut s = state("misty_ps_window_test");
        crate::commands::apply(&mut s, Action::OpenProjectSettings);
        assert!(s.project_settings_window.open);
        let mut h = Harness::builder()
            .with_size([620.0, 380.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Gravity");
        h.get_by_label("Queries Hit Triggers");
        h.get_by_label("Layer Collision Matrix");
        h.get_by_label("5 Player");
        h.get_by_label("Time").click();
        h.run();
        assert_eq!(h.state().project_settings_window.category, Category::Time);
        h.get_by_label("Fixed Timestep");
        h.get_by_label("Time Scale");
    }

    #[test]
    fn changes_save_to_the_project_and_load_at_startup() {
        let mut s = state("misty_ps_save_test");
        s.project_settings.physics.gravity = Vec3::new(0.0, -1.62, 0.0);
        changed(&mut s);
        assert!(
            s.project
                .root
                .join("ProjectSettings/ProjectSettings.json")
                .is_file()
        );
        let reopened = EditorState::new(ProjectBrowser::open(s.project.root.clone()));
        assert_eq!(reopened.project_settings.physics.gravity.y, -1.62);
    }

    #[test]
    fn play_mode_uses_the_project_settings() {
        let mut s = state("misty_ps_play_test");
        s.project_settings.physics.gravity = Vec3::new(0.0, -3.0, 0.0);
        s.project_settings.physics.fixed_timestep = 0.01;
        s.project_settings.time.time_scale = 0.25;
        s.project_settings.physics.set_layers_collide(0, 5, false);
        crate::play::start(&mut s);
        let (gravity, fixed_dt) = s.runtime.as_ref().unwrap().physics_config();
        assert_eq!(gravity, Vec3::new(0.0, -3.0, 0.0));
        assert_eq!(fixed_dt, 0.01);
        assert_eq!(s.time.time_scale, 0.25);
        let physics = s.runtime.as_ref().unwrap().physics_settings();
        assert!(!physics.layers_collide(5, 0));
    }
}
