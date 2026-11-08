use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;

use super::*;
use crate::actions::Action;
use crate::project::ProjectBrowser;

fn state(name: &str) -> EditorState {
    let root = std::env::temp_dir().join(format!("{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    EditorState::new(ProjectBrowser::open(root))
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    Harness::builder()
        .with_size([820.0, 900.0])
        .build_ui_state(|ui, s: &mut EditorState| contents(ui, s), s)
}

#[test]
fn menu_action_opens_the_window_and_scenes_are_added_and_reordered() {
    let mut s = state("misty_build_window");
    crate::commands::apply(&mut s, Action::OpenBuildSettings);
    assert!(s.build_window.open);
    s.build_window.page = Page::SceneList;
    s.scene.set_scene_path(0, "Assets/Scenes/A.scene");
    let mut h = harness(s);
    h.run();
    h.get_by_label("Add Open Scenes").click();
    h.run();
    h.state_mut()
        .scene
        .set_scene_path(0, "Assets/Scenes/B.scene");
    h.get_by_label("Add Open Scenes").click();
    h.run();
    h.get_by_label("Add Open Scenes").click();
    h.run();
    let paths = |s: &EditorState| -> Vec<String> {
        s.build_settings
            .scenes
            .iter()
            .map(|b| b.path.clone())
            .collect()
    };
    assert_eq!(
        paths(h.state()),
        ["Assets/Scenes/A.scene", "Assets/Scenes/B.scene"]
    );

    h.get_by_label("Assets/Scenes/B.scene").click();
    h.run();
    h.get_by_label("⏶").click();
    h.run();
    assert_eq!(
        paths(h.state()),
        ["Assets/Scenes/B.scene", "Assets/Scenes/A.scene"]
    );
    h.get_by_label("Remove Selected").click();
    h.run();
    assert_eq!(paths(h.state()), ["Assets/Scenes/A.scene"]);

    // Saved to the project and loaded with it.
    let reopened = EditorState::new(ProjectBrowser::open(h.state().project.root.clone()));
    assert_eq!(paths(&reopened), ["Assets/Scenes/A.scene"]);
}

#[test]
fn platform_profiles_are_listed_and_switching_is_saved() {
    let s = state("misty_build_profiles_list");
    let mut h = harness(s);
    h.run();
    for name in ["Windows", "Linux", "macOS", "Dedicated Server"] {
        h.get_by_label_contains(&format!("  {name}"));
    }
    h.get_by_label("Scripting Defines");
    h.get_by_label("Texture Overrides");
    h.get_by_label("Max Texture Size");
    h.get_by_label("Quality Level");
    h.get_by_label("Development Build");
    h.get_by_label("Output Folder");
    h.get_by_label("Steam App ID");
    h.get_by_label("＋ Add Build Profile");

    // Show the Linux profile and make it active.
    h.get_by_label("🐧  Linux").click();
    h.run();
    h.get_by_label("Switch Profile").click();
    h.run();
    let root = h.state().project.root.clone();
    let saved = BuildSettings::load(&root).unwrap();
    assert_eq!(saved.active_profile, "Linux");
    assert_eq!(saved.target.os(), misty_engine::build::TargetOs::Linux);
    assert_eq!(active_profile(h.state()).name, "Linux");
    h.get_by_label("Active");
    // Play mode sees the active profile's platform define and the editor's.
    let d = play_defines(h.state());
    assert!(d.contains(&"MISTY_STANDALONE_LINUX".to_owned()), "{d:?}");
    assert!(d.contains(&"MISTY_EDITOR".to_owned()));
    assert!(!d.contains(&"DEVELOPMENT_BUILD".to_owned()));
}

#[test]
fn custom_profiles_are_added_edited_duplicated_renamed_and_deleted() {
    let mut s = state("misty_build_profiles_custom");
    let root = s.project.root.clone();
    add_profile(&mut s, None, ProfilePlatform::Windows);
    let i = s.build_profiles.index_of("New Windows Profile").unwrap();
    assert!(
        root.join("Assets/Settings/Build Profiles/New Windows Profile.buildprofile")
            .is_file()
    );
    assert_eq!(s.build_window.page, Page::Profile(i));
    rename(&mut s, i, "Demo");
    let i = s.build_profiles.index_of("Demo").unwrap();
    switch_profile(&mut s, i);
    s.build_window.renaming = None;

    // Edit through the window: add a define.
    let mut h = harness(s);
    h.run();
    h.get_by_label("Demo");
    h.state_mut().build_window.new_define = "DEMO".into();
    h.run();
    h.get_by_label("Add").click();
    h.run();
    let back = BuildProfiles::load(&root, &BuildSettings::load(&root).unwrap());
    assert_eq!(back.find("Demo").unwrap().defines, ["DEMO"]);
    assert!(play_defines(h.state()).contains(&"DEMO".to_owned()));

    let mut s = h.into_state();
    let demo = s.build_profiles.find("Demo").cloned().unwrap();
    add_profile(&mut s, Some(&demo), demo.platform);
    let copy = s.build_profiles.index_of("Demo 1").unwrap();
    assert_eq!(s.build_profiles.profiles[copy].defines, ["DEMO"]);
    // Renaming the active profile keeps it active.
    let i = s.build_profiles.index_of("Demo").unwrap();
    rename(&mut s, i, "Full Demo");
    assert_eq!(s.build_settings.active_profile, "Full Demo");
    assert_eq!(
        BuildSettings::load(&root).unwrap().active_profile,
        "Full Demo"
    );
    let i = s.build_profiles.index_of("Full Demo").unwrap();
    delete(&mut s, i);
    assert!(s.build_settings.active_profile.is_empty());
    assert!(s.build_profiles.find("Full Demo").is_none());
    let names: Vec<String> = BuildProfiles::load(&root, &s.build_settings)
        .profiles
        .into_iter()
        .filter(|p| p.custom)
        .map(|p| p.name)
        .collect();
    assert_eq!(names, ["Demo 1"]);
}

#[test]
fn builds_start_in_the_background_and_report_to_the_console() {
    let mut s = state("misty_build_job");
    let root = s.project.root.clone();
    std::fs::write(root.join("Assets/Scenes/A.scene"), "[]").unwrap();
    s.scene.set_scene_path(0, "Assets/Scenes/A.scene");
    // An empty scene list builds the open scene.
    let active = active_index(&s);
    assert!(ensure_scenes(&mut s, active));
    assert_eq!(s.build_settings.scenes.len(), 1);
    let req = request(&s, &root.join("Out"));
    assert_eq!(req.build.scenes.len(), 1);
    assert_eq!(req.product(), root.file_name().unwrap().to_string_lossy());
    assert!(req.profile.is_some());

    // Building into the project's own Assets is refused (by the build thread).
    build_into(&mut s, &root.join("Assets/Build"), false);
    assert_eq!(
        s.files.settings.last_build_folder.as_deref(),
        Some(root.join("Assets/Build").as_path())
    );
    let ctx = egui::Context::default();
    while s.build_window.job.as_ref().is_some_and(BuildJob::running) {
        poll(&ctx, &mut s);
        std::thread::yield_now();
    }
    let last = s.console.last().unwrap();
    assert_eq!(last.level, LogLevel::Error);
    assert!(
        last.message.contains("outside the project"),
        "{}",
        last.message
    );
}

#[test]
fn a_profile_with_its_own_scene_list_builds_those_scenes() {
    let mut s = state("misty_build_own_scenes");
    let root = s.project.root.clone();
    s.build_settings.scenes = vec![BuildScene::new("Assets/Scenes/Main.scene")];
    let i = s.build_profiles.index_of("Dedicated Server").unwrap();
    s.build_profiles.profiles[i].override_scenes = true;
    s.build_profiles.profiles[i].scenes = vec![BuildScene::new("Assets/Scenes/Server.scene")];
    profile_changed(&mut s, i);
    let req = request_for(&s, i, &root.join("../Out"));
    assert_eq!(req.build.scenes[0].path, "Assets/Scenes/Server.scene");
    assert!(req.server());
    // The shared list is untouched.
    assert_eq!(s.build_settings.scenes[0].path, "Assets/Scenes/Main.scene");
    assert!(
        root.join("ProjectSettings/BuildProfiles/DedicatedServer.buildprofile")
            .is_file()
    );
}
