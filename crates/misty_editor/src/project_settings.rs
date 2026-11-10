//! Edit ▸ Project Settings: a floating window with a short category list and that category's
//! fields, grouped in collapsible sections. Every change is saved to
//! `ProjectSettings/ProjectSettings.json` right away.

use egui::Ui;

use misty_engine::glam::Vec3;
use misty_engine::settings::{PhysicsSettings, ProjectSettings};
use misty_engine::tags_layers::TagsAndLayers;

use crate::state::EditorState;
use crate::widgets::vec3_row;

/// Pages, in list order. Physics holds the 2D section and Tags and Layers the Sorting Layers.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Category {
    Player,
    Quality,
    Input,
    Localization,
    #[default]
    Physics,
    PresetManager,
    TagsAndLayers,
    Time,
    PackageManager,
}

impl Category {
    pub const ALL: [Category; 9] = [
        Category::Player,
        Category::Quality,
        Category::Input,
        Category::Localization,
        Category::Physics,
        Category::PresetManager,
        Category::TagsAndLayers,
        Category::Time,
        Category::PackageManager,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Category::Player => "Player",
            Category::Quality => "Quality",
            Category::Input => "Input",
            Category::Localization => "Localization",
            Category::Physics => "Physics",
            Category::PresetManager => "Preset Manager",
            Category::TagsAndLayers => "Tags and Layers",
            Category::Time => "Time",
            Category::PackageManager => "Package Manager",
        }
    }

    /// One line under the page heading.
    pub fn blurb(self) -> &'static str {
        match self {
            Category::Player => "Your game's name, icon and how its window opens.",
            Category::Quality => "Graphics presets players can pick from (Low to Ultra).",
            Category::Input => "Named axes and buttons scripts read with Input.GetAxis.",
            Category::Localization => "Languages the game is translated into.",
            Category::Physics => "Gravity and which layers collide, for 3D and 2D.",
            Category::PresetManager => "Presets new archetypes and materials start from.",
            Category::TagsAndLayers => "Names for tags, layers and 2D drawing order.",
            Category::Time => "Physics step length and the speed of time.",
            Category::PackageManager => "The package registry the Package Manager lists.",
        }
    }

    /// The page called `title`; the names of pages that were merged still work.
    pub fn from_title(title: &str) -> Option<Category> {
        match title {
            "Input Manager" => Some(Category::Input),
            "Physics 2D" => Some(Category::Physics),
            "Sorting Layers" => Some(Category::TagsAndLayers),
            t => Category::ALL.into_iter().find(|c| c.title() == t),
        }
    }
}

/// Open the window on a category (e.g. "Add Tag…" in the Inspector).
pub fn open_at(s: &mut EditorState, category: Category) {
    s.project_settings_window.open = true;
    s.project_settings_window.category = category;
}

/// Window state.
#[derive(Default)]
pub struct ProjectSettingsWindow {
    pub open: bool,
    pub category: Category,
    /// Quality level being edited on the Quality page.
    pub quality_selected: usize,
}

/// The one section open at start (screenshots); see [`section`].
pub static SCREENSHOT_SECTION: std::sync::OnceLock<String> = std::sync::OnceLock::new();

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
    s.project_settings.apply_globals();
    let q = &s.project_settings.quality;
    misty_engine::quality::install(q, q.default_level);
    install_accessibility(s);
    // PlayerPrefs in Play mode are kept per project, outside version control. (Tests keep
    // their own prefs files: opening this one would swap the store under them.)
    if !cfg!(test) {
        misty_engine::prefs::open(&s.project.root.join("Library/PlayerPrefs.json"));
    }
}

/// Player ▸ Accessibility as the settings in effect: the Game view shows the colour-blind
/// filter and text scale while editing too, and Play starts from them.
pub fn install_accessibility(s: &EditorState) {
    let a = s
        .project_settings
        .player
        .as_ref()
        .map(|p| p.accessibility)
        .unwrap_or_default();
    misty_engine::accessibility::install(&a);
}

/// Clamp, save, and apply what can change live (time scale while playing).
pub fn changed(s: &mut EditorState) {
    s.project_settings.sanitize();
    s.project_settings.apply_globals();
    // Keep previewing the same level with its new values.
    misty_engine::quality::install(&s.project_settings.quality, misty_engine::quality::level());
    install_accessibility(s);
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
        .default_size([640.0, 440.0])
        .collapsible(false)
        .show(ctx, |ui| contents(ui, s));
    s.project_settings_window.open = open;
}

/// Category list on the left, fields on the right.
pub fn contents(ui: &mut Ui, s: &mut EditorState) {
    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_width(120.0);
            for c in Category::ALL {
                let selected = s.project_settings_window.category == c;
                if ui
                    .selectable_label(selected, c.title())
                    .on_hover_text(c.blurb())
                    .clicked()
                {
                    s.project_settings_window.category = c;
                }
            }
        });
        ui.separator();
        ui.vertical(|ui| {
            let category = s.project_settings_window.category;
            ui.heading(category.title());
            ui.weak(category.blurb());
            ui.add_space(4.0);
            let selected = &mut s.project_settings_window.quality_selected;
            let p = &mut s.project_settings;
            let build = &s.build_settings;
            let root = &s.project.root;
            let presets = &s.assets.presets;
            let packages = &mut s.packages;
            let edited = egui::ScrollArea::vertical()
                .auto_shrink([false, true])
                .show(ui, |ui| match category {
                    Category::Player => crate::project_settings_player::page(ui, p, build, root),
                    Category::Quality => crate::project_settings_quality::page(ui, p, selected),
                    Category::Input => crate::project_settings_input::page(ui, &mut p.input),
                    Category::Localization => crate::saveloc::settings_page::page(ui, p),
                    Category::Physics => {
                        let mut edited = false;
                        section(ui, "3D Physics", true, |ui| edited |= physics(ui, p));
                        section(ui, "2D Physics", false, |ui| {
                            edited |= crate::project_settings_2d::physics2d(ui, p)
                        });
                        edited
                    }
                    Category::TagsAndLayers => {
                        let mut edited =
                            crate::project_settings_tags::page(ui, &mut p.tags_and_layers);
                        section(ui, "Sorting Layers (2D draw order)", false, |ui| {
                            edited |= crate::project_settings_2d::sorting_layers(ui, p)
                        });
                        edited
                    }
                    Category::PresetManager => {
                        crate::project_settings_presets::page(ui, p, presets)
                    }
                    Category::Time => time(ui, p),
                    Category::PackageManager => {
                        package_manager(ui, packages, root);
                        false
                    }
                })
                .inner;
            if edited {
                changed(s);
            }
        });
    });
}

/// Package Manager page: the registry URL (kept in `Packages/manifest.json`).
fn package_manager(ui: &mut Ui, w: &mut crate::packages::PackagesWindow, root: &std::path::Path) {
    w.ensure_loaded(root);
    w.poll();
    let current = w.manifest.registry.clone();
    let text = w.registry_edit.get_or_insert_with(|| current.clone());
    let mut apply = false;
    row(
        ui,
        "Registry URL",
        "JSON index of packages (https:// or file://); empty = none",
        |ui| {
            let r = ui.add(
                egui::TextEdit::singleline(text)
                    .hint_text("https://example.com/misty-packages/index.json")
                    .desired_width(320.0),
            );
            apply = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            false
        },
    );
    ui.horizontal(|ui| {
        apply |= ui
            .add_enabled(*text != current, egui::Button::new("Apply"))
            .clicked();
        if ui.button("Clear").clicked() {
            text.clear();
            apply = true;
        }
    });
    if apply {
        let url = text.clone();
        w.set_registry(root, &url);
        w.registry_edit = None;
    }
    use crate::packages::RegistryState;
    match &w.registry {
        RegistryState::NotConfigured => ui.weak("No registry configured."),
        RegistryState::Loading(_) => ui.weak("Loading the registry index…"),
        RegistryState::Loaded(i) => {
            ui.weak(format!("{} packages in the registry.", i.packages.len()))
        }
        RegistryState::Failed(e) => ui.colored_label(crate::theme::WARN, e),
    };
}

/// A collapsible group of fields on a page.
pub fn section(ui: &mut Ui, title: &str, open: bool, add: impl FnOnce(&mut Ui)) {
    // Screenshots (`MISTY_PROJECT_SETTINGS=Player:Accessibility`): only that section starts open.
    let open = SCREENSHOT_SECTION.get().map_or(open, |s| s == title);
    egui::CollapsingHeader::new(egui::RichText::new(title).strong())
        .id_salt(("ps_section", title))
        .default_open(open)
        .show(ui, add);
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

fn physics(ui: &mut Ui, settings: &mut ProjectSettings) -> bool {
    let names = &settings.tags_and_layers;
    let p = &mut settings.physics;
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
    edited |= layer_matrix(ui, p, names);
    edited
}

/// Layer Collision Matrix of the named layers: rows top to bottom, columns in reverse order,
/// one box per pair.
fn layer_matrix(ui: &mut Ui, p: &mut PhysicsSettings, names: &TagsAndLayers) -> bool {
    let mut edited = false;
    let layers = names.named_layers();
    egui::CollapsingHeader::new("Layer Collision Matrix")
        .id_salt("layer_matrix")
        .default_open(true)
        .show(ui, |ui| {
            let n = layers.len();
            egui::Grid::new("layer_matrix_grid")
                .spacing([2.0, 2.0])
                .show(ui, |ui| {
                    ui.label("");
                    for j in (0..n).rev() {
                        let (index, name) = layers[j];
                        ui.label(index.to_string()).on_hover_text(name);
                    }
                    ui.end_row();
                    for (i, (a, name)) in layers.iter().enumerate() {
                        ui.label(format!("{a} {name}"));
                        for j in (0..n).rev() {
                            if j < i {
                                ui.label("");
                                continue;
                            }
                            let (a, b) = (*a, layers[j].0);
                            let mut on = p.layers_collide(a, b);
                            let tip = format!("{name} × {}", layers[j].1);
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
        // Merged pages: 2D physics is a section of Physics.
        h.get_by_label("Physics").click();
        h.run();
        h.get_by_label("2D Physics").click();
        h.run();
        h.get_by_label("Queries Start In Colliders");
        assert_eq!(Category::from_title("Physics 2D"), Some(Category::Physics));
        assert_eq!(
            Category::from_title("Sorting Layers"),
            Some(Category::TagsAndLayers)
        );
        assert_eq!(Category::from_title("Input Manager"), Some(Category::Input));
    }

    #[test]
    fn player_and_quality_pages_edit_the_settings() {
        let mut s = state("misty_ps_player_quality_test");
        s.build_settings.product_name = "Legacy Name".into();
        open_at(&mut s, Category::Player);
        let mut h = Harness::builder()
            .with_size([640.0, 700.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Product Name");
        h.get_by_label("Screen Mode");
        // The checkbox is named after its row's label (screen readers).
        h.get_by_role_and_label(egui::accesskit::Role::CheckBox, "Resizable Window")
            .click();
        h.run();
        let player = h.state().project_settings.player.clone().unwrap();
        assert_eq!(
            player.product_name, "Legacy Name",
            "migrated from Build Settings"
        );
        assert!(!player.resizable);
        h.get_by_label("Quality").click();
        h.run();
        h.get_by_label("Shadow Resolution");
        h.get_by_label("Render Scale");
        h.get_by_label("Effects");
        h.get_by_label("Add Level").click();
        h.run();
        let q = &h.state().project_settings.quality;
        assert_eq!(q.levels.len(), 5);
        let saved = ProjectSettings::load(&h.state().project.root).unwrap();
        assert_eq!(saved.quality.levels.len(), 5);
        assert!(saved.player.is_some());
    }

    #[test]
    fn input_manager_and_tags_pages() {
        let mut s = state("misty_ps_input_tags_test");
        s.project_settings.tags_and_layers.layers[8] = "Hidden".into();
        open_at(&mut s, Category::Input);
        let mut h = Harness::builder()
            .with_size([620.0, 600.0])
            .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s);
        h.run();
        h.get_by_label("Axes");
        h.get_by_label("Size");
        assert!(
            h.query_all_by_label("Horizontal").count() >= 2,
            "keys + stick"
        );
        h.get_by_label("Mouse ScrollWheel").click();
        h.run();
        h.get_by_label("Sensitivity");
        h.get_by_label("Joy Num");
        h.get_by_label("Tags and Layers").click();
        h.run();
        h.get_by_label("Builtin Layer 0");
        h.get_by_label("User Layer 31");
        h.get_by_label("+").click();
        h.run();
        assert_eq!(h.state().project_settings.tags_and_layers.tags, ["New Tag"]);
        h.get_by_label("Physics").click();
        h.run();
        h.get_by_label("8 Hidden");
        // The tag was saved with the project.
        let root = h.state().project.root.clone();
        let saved = ProjectSettings::load(&root).unwrap();
        assert!(saved.tags_and_layers.has_tag("New Tag"));
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
