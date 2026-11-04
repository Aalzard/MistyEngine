//! Provider and action-dispatch tests on a real editor state (SampleProject).

use super::dispatch::{self, After, Command};
use super::*;
use crate::actions::{Action, Create};
use crate::layout::Tab;
use crate::project::ProjectBrowser;
use crate::project_settings::Category;

fn state() -> EditorState {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    let mut s = EditorState::new(ProjectBrowser::open(root));
    s.search.index.build_now();
    open(&mut s, "");
    s
}

fn hits(s: &EditorState, text: &str) -> Vec<Hit> {
    run(s, text)
}

fn first(s: &EditorState, text: &str, provider: Provider) -> Hit {
    hits(s, text)
        .into_iter()
        .find(|h| h.provider == provider)
        .unwrap_or_else(|| panic!("no {provider:?} hit for {text:?}"))
}

#[test]
fn providers_and_grouping() {
    let s = state();
    let all = hits(&s, "camera");
    let providers: Vec<Provider> = all.iter().map(|h| h.provider).collect();
    assert!(providers.contains(&Provider::Hierarchy));
    assert!(providers.contains(&Provider::Menu));
    // Rows of one provider are contiguous (one header each).
    let mut seen = Vec::new();
    for p in &providers {
        if seen.last() != Some(p) {
            assert!(!seen.contains(p), "{p:?} split: {providers:?}");
            seen.push(*p);
        }
    }
    // Per-provider cap without a prefix.
    for p in seen {
        assert!(providers.iter().filter(|x| **x == p).count() <= CAP);
    }
}

#[test]
fn provider_prefixes_limit_results() {
    let s = state();
    for (text, provider) in [
        ("h:cam", Provider::Hierarchy),
        ("p:mat", Provider::Project),
        ("m:cube", Provider::Menu),
        ("s:phys", Provider::Settings),
    ] {
        let h = hits(&s, text);
        assert!(!h.is_empty(), "{text}");
        assert!(h.iter().all(|x| x.provider == provider), "{text}");
    }
    // A prefix alone lists that provider's entries.
    assert!(hits(&s, "s:").len() > 3);
}

#[test]
fn filters_reach_objects_and_assets_only() {
    let s = state();
    let h = hits(&s, "t:Light");
    assert!(!h.is_empty());
    assert!(h.iter().all(|x| x.provider == Provider::Hierarchy));
    let h = hits(&s, "t:Material");
    assert!(!h.is_empty());
    assert!(h.iter().all(|x| x.provider == Provider::Project));
    assert!(h.iter().all(|x| x.label.ends_with(".mat")));
    let h = hits(&s, "p:t:Scene");
    assert!(h.iter().all(|x| x.label.ends_with(".scene")));
}

#[test]
fn menu_and_settings_matching() {
    let s = state();
    let h = first(&s, "create cube", Provider::Menu);
    assert_eq!(h.label, "Create Cube");
    assert_eq!(h.detail, "GameObject/3D Object/Cube");
    assert!(!h.indices.is_empty());
    assert_eq!(first(&s, "lighting", Provider::Menu).label, "Lighting");
    assert_eq!(
        first(&s, "project settings", Provider::Menu).label,
        "Project Settings"
    );
    // Keywords find pages without highlighting the title.
    let h = first(&s, "gravity", Provider::Settings);
    assert_eq!(h.label, "Physics");
    assert!(h.indices.is_empty());
    assert!(
        hits(&s, "itch")
            .iter()
            .any(|h| h.provider == Provider::Help)
    );
}

#[test]
fn calculator_and_recent() {
    let mut s = state();
    let h = hits(&s, "=2*3");
    assert_eq!(h.len(), 1);
    assert_eq!(h[0].label, "= 6");
    assert!(hits(&s, "=2*").is_empty());
    s.search.recent.clear();
    s.search.recent.push("cube".into());
    let h = hits(&s, "");
    assert_eq!(h[0].provider, Provider::Recent);
    assert_eq!(
        dispatch::run(&mut s, &h[0], Command::UseText),
        After::Search("cube".into())
    );
}

#[test]
fn object_actions() {
    let mut s = state();
    let h = first(&s, "main camera", Provider::Hierarchy);
    let Target::Object(cam) = h.target else {
        panic!("not an object")
    };
    assert_eq!(dispatch::commands(&h)[0], Command::SelectAndFrame);
    s.actions.clear();
    assert_eq!(
        dispatch::run(&mut s, &h, Command::SelectAndFrame),
        After::Close
    );
    assert_eq!(s.selection.active(), Some(cam));
    assert!(matches!(s.actions[..], [Action::FrameSelected]));
    dispatch::run(&mut s, &h, Command::CopyName);
    assert_eq!(s.os_clipboard_out.as_deref(), Some("Main Camera"));
    // A deleted object only warns.
    s.scene.despawn(cam);
    s.selection.clear();
    dispatch::run(&mut s, &h, Command::Select);
    assert!(s.selection.is_empty());
}

#[test]
fn asset_actions() {
    let mut s = state();
    let h = first(&s, "p:samplescene", Provider::Project);
    let Target::Asset { rel, .. } = h.target.clone() else {
        panic!("not an asset")
    };
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::Ping);
    assert_eq!(s.inspector_asset.as_deref(), Some(rel.as_str()));
    assert_eq!(s.project.selected, Some(s.assets.absolute(&rel)));
    assert!(matches!(s.actions[..], [Action::FocusTab(Tab::Project)]));
    dispatch::run(&mut s, &h, Command::CopyPath);
    assert_eq!(s.os_clipboard_out.as_deref(), Some(rel.as_str()));
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::Open);
    assert!(matches!(&s.actions[..], [Action::OpenScenePath(p)] if *p == rel));
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::ShowInExplorer);
    assert!(matches!(&s.actions[..], [Action::ShowInExplorer(_)]));
}

#[test]
fn menu_and_settings_actions() {
    let mut s = state();
    s.actions.clear();
    let h = first(&s, "create cube", Provider::Menu);
    dispatch::run(&mut s, &h, Command::Run);
    assert!(matches!(
        s.actions[..],
        [Action::Create {
            what: Create::Primitive(_),
            parent: None
        }]
    ));
    let h = first(&s, "s:tags", Provider::Settings);
    dispatch::run(&mut s, &h, Command::OpenPage);
    assert!(s.project_settings_window.open);
    assert_eq!(s.project_settings_window.category, Category::TagsAndLayers);
    let h = first(&s, "s:preferences", Provider::Settings);
    dispatch::run(&mut s, &h, Command::OpenPage);
    assert!(s.code_editor.ui.prefs_open);
    let h = first(&s, "=7*6", Provider::Calculator);
    dispatch::run(&mut s, &h, Command::CopyResult);
    assert_eq!(s.os_clipboard_out.as_deref(), Some("42"));
}

#[test]
fn every_menu_command_has_title_and_path() {
    let s = state();
    let menu = menu_items::all(&s);
    assert!(menu.len() > 80, "{}", menu.len());
    for c in &menu {
        assert!(!c.title.is_empty() && c.path.contains('/'), "{c:?}");
    }
    assert!(menu.iter().any(|c| c.title == "Add Light"));
}

#[test]
fn update_caches_until_text_changes() {
    let mut s = state();
    s.search.set_query("cube");
    update(&mut s);
    let n = s.search.hits.len();
    assert!(n > 0);
    s.search.hits.clear();
    update(&mut s);
    assert!(s.search.hits.is_empty(), "same text: cached");
    s.search.set_query("cub");
    update(&mut s);
    assert!(!s.search.hits.is_empty());
}

#[test]
fn large_scene_stays_fast() {
    let mut s = state();
    for i in 0..5000 {
        s.scene.spawn(&format!("Rock {i}"), None);
    }
    let start = std::time::Instant::now();
    let h = run(&s, "rock 42");
    let took = start.elapsed();
    assert!(h.iter().any(|x| x.label == "Rock 42"));
    // Generous for debug builds; release is a few milliseconds.
    assert!(took.as_millis() < 500, "query took {took:?}");
}

#[test]
fn other_round_features_are_searchable() {
    let mut s = state();
    // Objects of every loaded scene, named with their scene when several are open.
    let mut extra = misty_engine::scene::Scene::new("Extra");
    extra.spawn("Zeppelin", None);
    let (id, _, _) = s
        .scene
        .load_additive("Assets/Scenes/Extra.scene", &extra.to_json())
        .unwrap();
    let h = first(&s, "h:zeppelin", Provider::Hierarchy);
    assert_eq!(h.detail, "Extra");
    let menu = menu_items::all(&s);
    let find = |title: &str| {
        menu.iter()
            .find(|c| c.title == title)
            .unwrap_or_else(|| panic!("no menu command {title:?}"))
    };
    assert!(matches!(
        find("Set Active Scene Extra").run,
        menu_items::MenuRun::Action(Action::Scene(i, crate::actions::SceneOp::SetActive)) if i == id
    ));
    assert!(menu.iter().any(|c| c.title.starts_with("Unload Scene ")));
    // Undo History is an Edit command, not a Window one.
    assert_eq!(find("Undo History").path, "Edit/Undo History");
    assert_eq!(find("Undo History").shortcut, "Ctrl+H");
    // Presets apply to the selection.
    let lamp = s.scene.spawn("Lamp", None);
    s.scene.add(lamp, misty_engine::scene::Light::default());
    s.select(lamp);
    let menu = menu_items::all(&s);
    let apply = menu
        .iter()
        .find(|c| c.title == "Apply Preset Warm Street Lamp")
        .expect("component preset command");
    assert!(matches!(
        &apply.run,
        menu_items::MenuRun::Action(Action::ApplyPreset { entities, .. }) if entities == &[lamp]
    ));
    // Scene assets open additively, preset assets apply to the selection.
    let h = first(&s, "p:multiscenelighting", Provider::Project);
    assert!(dispatch::commands(&h).contains(&Command::OpenAdditive));
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::OpenAdditive);
    assert!(
        matches!(&s.actions[..], [Action::OpenSceneAdditive(p)] if p.ends_with("MultiSceneLighting.scene"))
    );
    let h = first(&s, "p:warm street lamp", Provider::Project);
    assert_eq!(dispatch::commands(&h)[1], Command::ApplyPreset);
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::ApplyPreset);
    assert!(matches!(&s.actions[..], [Action::ApplyPreset { .. }]));
    // GUID tools on files: Reimport and Find References In Scene.
    let h = first(&s, "p:warm street lamp", Provider::Project);
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::Reimport);
    assert!(matches!(&s.actions[..], [Action::ReimportAsset(_)]));
    let h = first(&s, "p:t:material", Provider::Project);
    assert!(dispatch::commands(&h).contains(&Command::FindReferences));
    s.actions.clear();
    dispatch::run(&mut s, &h, Command::FindReferences);
    assert!(matches!(&s.actions[..], [Action::FindReferencesInScene(_)]));
}
