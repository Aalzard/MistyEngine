//! hub.json, the recent list, name validation and the Hub pages.

use std::path::{Path, PathBuf};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::config::{self, HubConfig, HubTheme, MAX_RECENT};
use super::{Hub, validate};

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_hub_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn config_round_trips_and_tolerates_missing_or_broken_files() {
    let dir = temp("config");
    let path = dir.join("nested/hub.json");
    assert_eq!(HubConfig::load(&path), HubConfig::default());
    let mut c = HubConfig {
        projects_folder: Some(dir.join("Projects")),
        theme: HubTheme::Light,
        ..Default::default()
    };
    c.touch(Path::new("C:/Games/Alpha"), 100);
    c.save(&path).unwrap();
    assert_eq!(HubConfig::load(&path), c);
    std::fs::write(&path, "{ not json").unwrap();
    assert_eq!(HubConfig::load(&path), HubConfig::default());
    // Unknown and missing fields are fine.
    std::fs::write(&path, r#"{"theme":"Light","future":1}"#).unwrap();
    assert_eq!(HubConfig::load(&path).theme, HubTheme::Light);
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn recent_list_moves_to_front_removes_relocates_and_caps() {
    let mut c = HubConfig::default();
    c.touch(Path::new("C:/P/A"), 1);
    c.touch(Path::new("C:/P/B"), 2);
    c.touch(Path::new("C:/P/A/"), 3);
    let names: Vec<String> = c.recent.iter().map(|r| r.name()).collect();
    assert_eq!(names, ["A", "B"]);
    assert_eq!(c.recent[0].last_opened, 3);
    c.relocate(Path::new("C:/P/B"), Path::new("D:/Moved/B2"));
    assert_eq!(c.recent[1].path, PathBuf::from("D:/Moved/B2"));
    assert_eq!(c.recent[1].last_opened, 2);
    assert_eq!(c.search("moved").len(), 1);
    assert_eq!(c.search("  ").len(), 2);
    c.remove(Path::new("C:/P/A"));
    assert_eq!(c.recent.len(), 1);
    for i in 0..MAX_RECENT + 5 {
        c.touch(Path::new(&format!("C:/Many/{i}")), i as u64);
    }
    assert_eq!(c.recent.len(), MAX_RECENT);
    assert_eq!(c.recent[0].name(), format!("{}", MAX_RECENT + 4));
}

#[test]
fn paths_compare_loosely_on_windows() {
    assert!(config::same_path(Path::new("C:/a/b/"), Path::new("C:/a/b")));
    if cfg!(windows) {
        assert!(config::same_path(
            Path::new("C:\\A\\B"),
            Path::new("c:/a/b")
        ));
    }
    assert!(!config::same_path(
        Path::new("C:/a/b"),
        Path::new("C:/a/bc")
    ));
}

#[test]
fn ago_reads_naturally() {
    assert_eq!(config::ago(5), "just now");
    assert_eq!(config::ago(60), "1 minute ago");
    assert_eq!(config::ago(3 * 3600 + 5), "3 hours ago");
    assert_eq!(config::ago(86400 * 2), "2 days ago");
    assert_eq!(config::ago(86400 * 400), "1 year ago");
}

#[test]
fn project_names_are_validated() {
    for bad in [
        "", "   ", "a/b", "a\\b", "what?", "dots.", "CON", "nul.txt", "x<y",
    ] {
        assert!(
            validate::name_error(bad).is_some(),
            "{bad:?} should be rejected"
        );
    }
    assert!(validate::name_error(&"x".repeat(validate::MAX_NAME + 1)).is_some());
    for good in ["My Game", "game-2", "Ünïcødé", "v1.0 test", "Console"] {
        assert_eq!(validate::name_error(good), None, "{good:?}");
    }
    let dir = temp("validate");
    std::fs::create_dir_all(dir.join("Taken")).unwrap();
    assert!(validate::project_folder("Taken", &dir).is_err());
    assert!(validate::project_folder("Free", Path::new("")).is_err());
    assert_eq!(
        validate::project_folder("  Free ", &dir).unwrap(),
        dir.join("Free")
    );
    std::fs::write(dir.join("file"), "").unwrap();
    assert!(validate::project_folder("Free", &dir.join("file")).is_err());
    assert_eq!(validate::unique_name("Taken", &dir), "Taken 2");
    assert_eq!(validate::unique_name("Other", &dir), "Other");
    let _ = std::fs::remove_dir_all(dir);
}

/// Hub plus the project it asked to open (and the tutorial to start there).
struct Shown {
    hub: Hub,
    opened: Option<PathBuf>,
    tutorial: Option<String>,
}

fn harness(config: &Path) -> Harness<'static, Shown> {
    let config = config.to_path_buf();
    let ctx = egui::Context::default();
    let hub = Hub::with_config(&ctx, Some(config));
    Harness::builder()
        .with_size([1100.0, 700.0])
        .build_ui_state(
            |ui, s: &mut Shown| {
                if let Some(p) = s.hub.ui(ui) {
                    s.opened = Some(p);
                    s.tutorial = s.hub.take_tutorial();
                }
            },
            Shown {
                hub,
                opened: None,
                tutorial: None,
            },
        )
}

#[test]
fn projects_page_lists_recent_and_missing_projects_and_opens_one() {
    let dir = temp("page");
    let real = dir.join("Real Game");
    crate::templates::create(&real, crate::templates::Template::FloatingPlatform).unwrap();
    let mut c = HubConfig::default();
    c.touch(&dir.join("Gone Game"), config::now() - 7200);
    c.touch(&real, config::now() - 60);
    let path = dir.join("hub.json");
    c.save(&path).unwrap();
    let mut h = harness(&path);
    h.run();
    assert!(h.query_all_by_label("Projects").count() >= 1);
    h.get_by_label("Real Game");
    h.get_by_label("Gone Game");
    assert!(h.query_by_label_contains("folder not found").is_some());
    assert!(h.query_by_label_contains("1 minute ago").is_some());
    h.get_by_label("Real Game").click();
    h.run();
    assert_eq!(h.state().opened.as_deref(), Some(real.as_path()));
    // Opening re-saves the list with the project first.
    let saved = HubConfig::load(&path);
    assert!(config::same_path(&saved.recent[0].path, &real));
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_project_page_validates_the_name() {
    let dir = temp("newpage");
    std::fs::create_dir_all(dir.join("Projects/Taken")).unwrap();
    let path = dir.join("hub.json");
    HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut h = harness(&path);
    h.run();
    // Empty list: the empty state offers New project.
    h.get_by_label("No projects yet");
    h.get_all_by_label("New project").next().unwrap().click();
    h.run();
    for t in crate::templates::Template::ALL {
        assert!(h.query_all_by_label(t.name()).count() > 0, "{}", t.name());
    }
    h.state_mut().hub.new_project.as_mut().unwrap().name = "Taken".into();
    h.run();
    assert!(h.query_by_label_contains("already exists").is_some());
    h.state_mut().hub.new_project.as_mut().unwrap().name = String::new();
    h.run();
    h.get_by_label("Enter a project name.");
    h.get_by_label("Cancel").click();
    h.run();
    assert!(h.state().hub.new_project.is_none());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn new_project_page_creates_and_opens_the_project() {
    let dir = temp("create");
    let path = dir.join("hub.json");
    HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut h = harness(&path);
    h.run();
    h.state_mut().hub.start_new_project();
    {
        let np = h.state_mut().hub.new_project.as_mut().unwrap();
        np.template = crate::templates::Template::FloatingPlatform;
        np.name = "Quick".into();
    }
    h.run();
    h.get_by_label("Create project").click();
    for _ in 0..200 {
        // The spinner keeps repainting: step instead of run.
        h.step();
        if h.state().opened.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let np = h
        .state()
        .hub
        .new_project
        .as_ref()
        .map(|n| (n.target(), n.busy()));
    let opened = h
        .state()
        .opened
        .clone()
        .unwrap_or_else(|| panic!("not opened: {np:?}"));
    assert_eq!(opened, dir.join("Projects/Quick"));
    assert!(opened.join(crate::templates::SCENE).is_file());
    let _ = std::fs::remove_dir_all(dir);
}

/// New Project lists the 2D Platformer (always available: it is generated); clicking its card
/// picks it, and Create makes a project that opens on its scene with the Scene view in 2D.
#[test]
fn new_project_page_creates_a_2d_platformer() {
    use crate::templates::Template;
    let dir = temp("platformer_card");
    let path = dir.join("hub.json");
    HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut h = harness(&path);
    h.run();
    h.state_mut().hub.start_new_project();
    h.run();
    let t = Template::Platformer2D;
    assert!(t.available());
    assert_eq!(h.query_all_by_label(t.description()).count(), 1, "the card");
    h.get_by_label(t.name()).click();
    h.run();
    assert_eq!(h.state().hub.new_project.as_ref().unwrap().template, t);
    // The card and the details.
    assert_eq!(h.query_all_by_label(t.description()).count(), 2);
    h.state_mut().hub.new_project.as_mut().unwrap().name = "Jumper".into();
    h.run();
    h.get_by_label("Create project").click();
    for _ in 0..500 {
        // The spinner keeps repainting: step instead of run.
        h.step();
        if h.state().opened.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let error = h
        .state()
        .hub
        .new_project
        .as_ref()
        .and_then(|n| n.target().err());
    let opened = h
        .state()
        .opened
        .clone()
        .unwrap_or_else(|| panic!("not opened: {error:?}"));
    assert_eq!(opened, dir.join("Projects/Jumper"));
    assert!(opened.join(crate::templates::SCENE).is_file());
    let editor = crate::scene_files::EditorSettings::load(&opened);
    assert!(
        editor.scene_view_2d.is_some(),
        "the Scene view starts in 2D"
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// New Project lists every template with its one-line description (MeadowDemo's says it has a
/// first-person player); clicking the MeadowDemo card picks it, and Create makes a project
/// that opens on MeadowDemo.
#[test]
fn new_project_page_shows_the_meadow_demo_card() {
    use crate::templates::{Template, meadow};
    let dir = temp("meadow_card");
    let path = dir.join("hub.json");
    HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let mut h = harness(&path);
    h.run();
    h.state_mut().hub.start_new_project();
    h.run();
    for t in Template::ALL {
        assert!(h.query_all_by_label(t.name()).count() > 0, "{}", t.name());
        assert!(
            h.query_all_by_label(t.description()).count() > 0,
            "{}",
            t.description()
        );
    }
    assert!(
        Template::MeadowDemo
            .description()
            .contains("first-person player")
    );
    if !Template::MeadowDemo.available() {
        eprintln!("no sample project here: card disabled, not created");
        let _ = std::fs::remove_dir_all(dir);
        return;
    }
    // Only the card shows the name until it is picked; then the details show it too.
    h.get_by_label(Template::MeadowDemo.name()).click();
    h.run();
    assert_eq!(
        h.state().hub.new_project.as_ref().unwrap().template,
        Template::MeadowDemo
    );
    assert_eq!(
        h.query_all_by_label(Template::MeadowDemo.description())
            .count(),
        2
    );
    h.state_mut().hub.new_project.as_mut().unwrap().name = "Meadow".into();
    h.run();
    h.get_by_label("Create project").click();
    // Copying the meadow project takes a while under load: wait by the clock, not a count
    // of frames.
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(300);
    while std::time::Instant::now() < deadline {
        // The spinner keeps repainting: step instead of run.
        h.step();
        if h.state().opened.is_some() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(20));
    }
    let error = h
        .state()
        .hub
        .new_project
        .as_ref()
        .and_then(|n| n.target().err());
    let opened = h
        .state()
        .opened
        .clone()
        .unwrap_or_else(|| panic!("not opened: {error:?}"));
    assert_eq!(opened, dir.join("Projects/Meadow"));
    assert!(opened.join(meadow::SCENE).is_file());
    assert!(!opened.join(crate::templates::SCENE).exists());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn learn_and_settings_pages_show() {
    let dir = temp("pages");
    let path = dir.join("hub.json");
    let mut h = harness(&path);
    h.run();
    h.get_by_label("Learn").click();
    h.run();
    for t in crate::tutorials::list() {
        h.get_by_label(t.title);
    }
    h.get_by_label("Settings").click();
    h.run();
    h.get_by_label("Light").click();
    h.run();
    assert_eq!(HubConfig::load(&path).theme, HubTheme::Light);
    h.get_by_label(&format!("v{}", super::VERSION));
    let _ = std::fs::remove_dir_all(dir);
}

/// The Learn page lists the editor's tutorials with a ✓ for those in `tutorials.json` beside
/// `hub.json`; Start with no recent project makes the Tutorial Playground and opens it.
#[test]
fn learn_starts_a_tutorial_in_the_playground() {
    let dir = temp("learn_playground");
    let path = dir.join("hub.json");
    HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    }
    .save(&path)
    .unwrap();
    let list = crate::tutorials::list();
    std::fs::write(
        dir.join("tutorials.json"),
        format!(r#"{{"done":["{}"]}}"#, list[1].id),
    )
    .unwrap();
    let mut h = harness(&path);
    h.run();
    h.get_by_label("Learn").click();
    h.run();
    // One finished: one "Again", the others "Start".
    h.get_by_label("✓ Done");
    assert_eq!(h.get_all_by_label("Again").count(), 1);
    assert_eq!(h.get_all_by_label("Start").count(), list.len() - 1);
    h.get_all_by_label("Start").next().unwrap().click();
    h.run();
    let s = h.state();
    let opened = s.opened.clone().expect("opened");
    assert_eq!(opened, dir.join("Projects").join(super::learn::PLAYGROUND));
    assert!(
        opened.join(crate::templates::SCENE).is_file(),
        "Floating Platform template"
    );
    assert_eq!(s.tutorial.as_deref(), Some(list[0].id));
    assert!(config::same_path(
        &HubConfig::load(&path).recent[0].path,
        &opened
    ));
    // A second time it reuses the folder (it is not empty, so creating it again would fail).
    assert_eq!(
        super::learn::playground(&dir.join("Projects")).unwrap(),
        opened
    );
    let _ = std::fs::remove_dir_all(dir);
}

/// With recent projects, Start asks which one to open the tutorial in.
#[test]
fn learn_asks_which_recent_project() {
    let dir = temp("learn_pick");
    let path = dir.join("hub.json");
    let game = dir.join("My Game");
    std::fs::create_dir_all(game.join("Assets")).unwrap();
    let mut c = HubConfig {
        projects_folder: Some(dir.join("Projects")),
        ..Default::default()
    };
    c.touch(&game, 1);
    c.save(&path).unwrap();
    let mut h = harness(&path);
    h.run();
    h.get_by_label("Learn").click();
    h.run();
    h.get_all_by_label("Start").nth(2).unwrap().click();
    h.run();
    assert!(h.state().opened.is_none(), "asks first");
    h.get_by_label("New Tutorial Playground (Floating Platform template)");
    h.get_by_label("My Game").click();
    h.run();
    let s = h.state();
    assert_eq!(s.opened.as_deref(), Some(game.as_path()));
    assert_eq!(s.tutorial.as_deref(), Some(crate::tutorials::list()[2].id));
    assert!(!dir.join("Projects").exists(), "no playground made");
    let _ = std::fs::remove_dir_all(dir);
}
