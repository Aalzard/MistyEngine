//! Live reload: scenes and assets following their files, the restore file and dev builds.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use misty_engine::scene::Scene;
use misty_engine::scene::prefab::Prefab;

use super::dev_build::{DevBuild, copy_over, is_newer};
use super::*;
use crate::project::ProjectBrowser;
use crate::scene_files;
use crate::state::EditorState;

fn temp(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_live_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    root
}

/// Move a file's time `secs` into the future, so the change is seen whatever the clock
/// resolution (the editor's own writes in the same test happen "now").
fn bump(path: &Path, secs: u64) {
    let f = std::fs::File::options().write(true).open(path).unwrap();
    f.set_modified(SystemTime::now() + Duration::from_secs(secs))
        .unwrap();
}

/// "Level": Props (with a child Cube) and a Light.
fn level() -> Scene {
    let mut s = Scene::new("Level");
    let props = s.spawn("Props", None);
    s.spawn("Cube", Some(props));
    s.spawn("Light", None);
    s
}

const LEVEL: &str = "Assets/Scenes/Level.scene";

/// A project with Level.scene open, files checked without waiting.
fn state(name: &str) -> EditorState {
    let root = temp(name);
    level().save(&root.join(LEVEL)).unwrap();
    let mut s = EditorState::new(ProjectBrowser::open(root));
    s.live.settle = Duration::ZERO;
    assert!(scene_files::load(&mut s, LEVEL));
    s
}

/// The scene file rewritten by someone else: Level plus a Barrel.
fn change_on_disk(s: &EditorState, rel: &str, extra: &str, secs: u64) {
    let path = s.project.root.join(rel);
    let mut scene = Scene::load(&path).unwrap().0;
    scene.spawn(extra, None);
    scene.save(&path).unwrap();
    bump(&path, secs);
}

/// Poll until nothing more happens (the first look notes a change, the next acts on it).
fn settle(s: &mut EditorState) {
    for _ in 0..3 {
        poll_now(s);
    }
}

fn harness(s: EditorState) -> Harness<'static, EditorState> {
    Harness::builder().with_size([900.0, 200.0]).build_ui_state(
        |ui, s: &mut EditorState| {
            poll_now(s);
            scenes::bar(ui, s);
        },
        s,
    )
}

#[test]
fn clean_scene_reloads_in_place_keeping_camera_selection_and_hierarchy() {
    let mut s = state("clean");
    let cube = s.scene.find_by_name("Cube").unwrap();
    let props = s.scene.find_by_name("Props").unwrap();
    s.select(cube);
    s.hierarchy.expanded.insert(props);
    s.scene_view.camera.set_view(
        glam::Vec3::new(3.0, 4.0, 5.0),
        glam::Quat::from_rotation_y(0.7),
    );
    let camera = s.scene_view.camera.state();
    change_on_disk(&s, LEVEL, "Barrel", 5);

    let mut h = harness(s);
    h.run_steps(3);
    let s = h.state();
    assert!(s.scene.find_by_name("Barrel").is_some(), "reloaded");
    assert!(
        s.live.scenes.conflicts.is_empty(),
        "no question for a clean scene"
    );
    let cube = s.scene.find_by_name("Cube").unwrap();
    assert_eq!(s.selection.active(), Some(cube), "selection kept by place");
    let props = s.scene.find_by_name("Props").unwrap();
    assert!(s.hierarchy.expanded.contains(&props), "expanded rows kept");
    assert_eq!(s.scene_view.camera.state(), camera, "camera untouched");
    assert!(!s.scene_dirty && s.files.dirty.is_empty());
    assert!(!s.play.in_play_mode());
    assert_eq!(s.live.current_toast(), Some("Reloaded Level.scene"));
}

#[test]
fn dirty_scene_shows_the_bar_and_reload_takes_the_file() {
    let mut s = state("dirty_reload");
    let light = s.scene.find_by_name("Light").unwrap();
    s.record("Rename Light");
    s.scene.game_object_mut(light).name = "Sun".into();
    change_on_disk(&s, LEVEL, "Barrel", 5);

    let mut h = harness(s);
    h.run_steps(3);
    assert_eq!(h.state().live.scenes.conflicts, [LEVEL]);
    assert!(
        h.state().scene.find_by_name("Sun").is_some(),
        "not reloaded behind my back"
    );
    h.get_by_label_contains("Level.scene changed on disk");
    h.get_by_label("Reload").click();
    h.run_steps(2);
    let s = h.state();
    assert!(s.live.scenes.conflicts.is_empty());
    assert!(s.scene.find_by_name("Barrel").is_some());
    assert!(
        s.scene.find_by_name("Sun").is_none(),
        "my change was discarded"
    );
    assert!(!s.scene_dirty);
}

#[test]
fn keep_mine_keeps_the_edits_until_the_file_changes_again() {
    let mut s = state("keep_mine");
    let light = s.scene.find_by_name("Light").unwrap();
    s.record("Rename Light");
    s.scene.game_object_mut(light).name = "Sun".into();
    change_on_disk(&s, LEVEL, "Barrel", 5);

    let mut h = harness(s);
    h.run_steps(3);
    h.get_by_label("Keep mine").click();
    h.run_steps(3);
    let s = h.state_mut();
    assert!(s.live.scenes.conflicts.is_empty());
    assert!(s.scene.find_by_name("Sun").is_some(), "edits kept");
    assert!(s.scene.find_by_name("Barrel").is_none());
    assert!(s.scene_dirty);
    // Saving overwrites the file with my version, without asking or reloading.
    assert!(scene_files::save(s));
    settle(s);
    assert!(s.live.scenes.conflicts.is_empty());
    let text = std::fs::read_to_string(s.project.root.join(LEVEL)).unwrap();
    assert!(text.contains("Sun") && !text.contains("Barrel"));
}

#[test]
fn own_saves_and_touches_dont_reload() {
    let mut s = state("own_save");
    let cube = s.scene.find_by_name("Cube").unwrap();
    s.select(cube);
    s.record("Rename Cube");
    s.scene.game_object_mut(cube).name = "Box".into();
    assert!(scene_files::save(&mut s));
    let selected = s.selection.active();
    settle(&mut s);
    assert_eq!(
        s.selection.active(),
        selected,
        "same entity: nothing was reloaded"
    );
    assert!(s.live.current_toast().is_none());
    assert!(s.undo.undo_label().is_some(), "history kept");
    // A newer file time with the same contents (a checkout, a copy) is no change either.
    bump(&s.project.root.join(LEVEL), 5);
    settle(&mut s);
    assert_eq!(s.selection.active(), selected);
    assert!(s.live.current_toast().is_none());
}

#[test]
fn additive_scene_reloads_alone() {
    let mut s = state("additive");
    let other = "Assets/Scenes/Other.scene";
    let mut b = Scene::new("Other");
    b.spawn("Ground", None);
    b.save(&s.project.root.join(other)).unwrap();
    let id = crate::open_scenes::open_additive(&mut s, other).unwrap();
    crate::open_scenes::set_active(&mut s, id);
    let light = s.scene.find_by_name("Light").unwrap();
    change_on_disk(&s, other, "Rock", 5);
    settle(&mut s);
    assert!(s.scene.find_by_name("Rock").is_some());
    assert_eq!(s.scene.scene_of(s.scene.find_by_name("Rock").unwrap()), id);
    assert_eq!(
        s.scene.find_by_name("Light"),
        Some(light),
        "Level untouched"
    );
    assert_eq!(s.scene.active_scene(), id, "still active");
    assert_eq!(s.scene.loaded_scenes().len(), 2);
}

#[test]
fn reload_scene_command_asks_about_unsaved_changes() {
    let mut s = state("command");
    assert_eq!(
        scene_files::guard(&mut s, scene_files::AfterPrompt::ReloadScenes),
        Some(scene_files::AfterPrompt::ReloadScenes)
    );
    s.record("Spawn");
    s.scene.spawn("Unsaved", None);
    assert_eq!(
        scene_files::guard(&mut s, scene_files::AfterPrompt::ReloadScenes),
        None
    );
    assert_eq!(s.files.prompt, Some(scene_files::AfterPrompt::ReloadScenes));
    // "Don't Save": the file comes back.
    scene_files::perform(&mut s, scene_files::AfterPrompt::ReloadScenes);
    assert!(s.scene.find_by_name("Unsaved").is_none());
    assert!(!s.scene_dirty);
}

#[test]
fn prefab_instances_follow_and_keep_their_overrides() {
    let mut s = state("prefab");
    let rel = "Assets/Crate.prefab";
    let abs = s.project.root.join(rel);
    let mut src = Scene::new("src");
    let root = src.spawn("Crate", None);
    Prefab::from_scene(&src, root).save(&abs).unwrap();
    let a = crate::prefabs::instantiate(&mut s, rel, None).unwrap();
    let b = crate::prefabs::instantiate(&mut s, rel, None).unwrap();
    s.scene.add(b, misty_engine::scene::BoxCollider::default());
    poll_now(&mut s);
    // Someone adds a Lid to the prefab.
    let lid = src.spawn("Lid", Some(root));
    let _ = lid;
    Prefab::from_scene(&src, root).save(&abs).unwrap();
    bump(&abs, 5);
    settle(&mut s);
    let instances = s.scene.prefab_instances(rel);
    assert_eq!(instances.len(), 2);
    let names = |e| -> Vec<String> {
        s.scene
            .children(e)
            .into_iter()
            .map(|c| s.scene.name(c))
            .collect()
    };
    let (first, second) = (instances[0], instances[1]);
    assert_eq!(names(first), ["Lid"], "plain instance took the new version");
    assert_eq!(names(second), ["Lid"], "so did the one with overrides");
    assert!(
        s.scene
            .world
            .get::<&misty_engine::scene::BoxCollider>(second)
            .is_ok(),
        "and kept its override"
    );
    assert_eq!((first, second), (a, b), "updated in place");
    assert_eq!(s.undo.undo_label(), Some("Refresh Prefab Instances"));
    assert_eq!(s.live.current_toast(), Some("Reloaded 1 asset"));
}

#[test]
fn materials_timelines_and_clips_reload_but_own_writes_dont() {
    let mut s = state("assets");
    let folder = s.project.root.join("Assets");
    let mat = s.assets.create_material(&folder).unwrap();
    let tl = "Assets/Intro.timeline";
    s.anim_clips
        .timelines
        .create(tl, Default::default())
        .unwrap();
    poll_now(&mut s);
    // The editor's own edits: written by it, so nothing to reload.
    let mut m = s.assets.materials[&mat].clone();
    m.color = [0.0, 1.0, 0.0, 1.0];
    s.assets.update_material(&mat, m).unwrap();
    settle(&mut s);
    assert!(s.live.current_toast().is_none());
    // Outside changes.
    let mut m = s.assets.materials[&mat].clone();
    m.color = [1.0, 0.0, 0.0, 1.0];
    m.save(&s.project.root.join(&mat)).unwrap();
    bump(&s.project.root.join(&mat), 5);
    let t = misty_engine::timeline::TimelineAsset {
        frame_rate: 24.0,
        ..Default::default()
    };
    t.save(&s.project.root.join(tl)).unwrap();
    bump(&s.project.root.join(tl), 5);
    settle(&mut s);
    assert_eq!(s.assets.materials[&mat].color, [1.0, 0.0, 0.0, 1.0]);
    assert_eq!(s.anim_clips.timelines.get(tl).unwrap().frame_rate, 24.0);
    assert_eq!(s.live.current_toast(), Some("Reloaded 2 assets"));
}

#[test]
fn terrain_data_reloads_unless_it_has_unsaved_edits() {
    use misty_engine::terrain::TerrainData;
    use misty_engine::terrain::store;
    let root = temp("terrain");
    let path = root.join("Assets/T.terrain");
    let key = path.to_string_lossy().into_owned();
    let flat = TerrainData::new(33, glam::Vec3::splat(10.0), 32);
    flat.save(&path).unwrap();
    let (_, v0) = store::get(&key).unwrap();
    let mut hills = flat.clone();
    hills.heights[0] = 0.5;
    hills.details.density = 3.0;
    hills.save(&path).unwrap();
    bump(&path, 5);
    // The store is shared with other tests (any of them may be the one whose poll reloads it)
    // and checks at most twice a second: give it time.
    for _ in 0..40 {
        store::poll_changes();
        if store::get(&key).unwrap().1 > v0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    let (data, v1) = store::get(&key).unwrap();
    assert!(v1 > v0, "renderers re-upload");
    assert!(
        (data.heights[0] - 0.5).abs() < 1e-3,
        "heights are stored as 16 bits"
    );
    assert_eq!(data.details.density, 3.0, "grass / detail settings too");
    // Unsaved brush edits win over the file.
    store::modify(&key, |d| d.heights[1] = 0.25);
    flat.save(&path).unwrap();
    bump(&path, 10);
    std::thread::sleep(Duration::from_millis(600));
    store::poll_changes();
    assert_eq!(
        store::get(&key).unwrap().0.heights[1],
        0.25,
        "still in memory"
    );
    // Our own save counts as the file's version.
    store::save(&key).unwrap();
    let saved = store::get(&key).unwrap().1;
    std::thread::sleep(Duration::from_millis(600));
    store::poll_changes();
    assert_eq!(store::get(&key).unwrap().1, saved, "not reloaded");
    store::unload(&key);
}

#[test]
fn restore_file_round_trip() {
    let mut s = state("restore");
    let other = "Assets/Scenes/Other.scene";
    let mut b = Scene::new("Other");
    let ground = b.spawn("Ground", None);
    b.spawn("Rock", Some(ground));
    // Two siblings with the same name: matched by their order.
    let rock2 = b.spawn("Rock", Some(ground));
    b.game_object_mut(rock2).name = "Rock".into();
    b.save(&s.project.root.join(other)).unwrap();
    let id = crate::open_scenes::open_additive(&mut s, other).unwrap();
    crate::open_scenes::set_active(&mut s, id);
    let ground = s.scene.find_by_name("Ground").unwrap();
    let second_rock = s.scene.children(ground)[1];
    let cube = s.scene.find_by_name("Cube").unwrap();
    s.selection.set_many(vec![cube, second_rock]);
    s.hierarchy.expanded.insert(ground);
    s.hierarchy.collapsed_scenes.insert(0);
    s.scene_view.camera.set_view(
        glam::Vec3::new(-2.0, 7.0, 1.0),
        glam::Quat::from_rotation_x(-0.4),
    );
    let mut dock = crate::layout::default_layout();
    crate::layout::focus_or_open(&mut dock, crate::layout::Tab::Profiler);
    let file = restore::write(&s, &dock).unwrap();
    let back = restore::read(&file).unwrap();
    assert_eq!(back, restore::capture(&s, &dock), "file round trip");

    // A fresh editor whose remembered open set differs (only Level).
    let mut again = EditorState::new(ProjectBrowser::open(s.project.root.clone()));
    assert!(scene_files::load(&mut again, LEVEL));
    let mut again_dock = crate::layout::default_layout();
    restore::apply(&mut again, &mut again_dock, back);
    let names: Vec<String> = again
        .scene
        .scenes()
        .iter()
        .map(|e| again.scene.scene_name(e.id))
        .collect();
    assert_eq!(names, ["Level", "Other"]);
    assert_eq!(again.scene.name, "Other", "active scene");
    assert_eq!(again.scene_view.camera.state(), s.scene_view.camera.state());
    let sel: Vec<_> = again
        .selection
        .list()
        .iter()
        .map(|e| paths::object_ref(&again.scene, *e))
        .collect();
    let want: Vec<_> = s
        .selection
        .list()
        .iter()
        .map(|e| paths::object_ref(&s.scene, *e))
        .collect();
    assert_eq!(sel, want, "selection by place (the second Rock)");
    assert_eq!(
        again
            .selection
            .active()
            .map(|e| paths::steps(&again.scene, e)),
        Some(vec![("Ground".to_owned(), 0), ("Rock".to_owned(), 1)])
    );
    let g = again.scene.find_by_name("Ground").unwrap();
    assert!(again.hierarchy.expanded.contains(&g));
    let level = again.scene.find_by_path(LEVEL).unwrap();
    assert!(again.hierarchy.collapsed_scenes.contains(&level));
    let tabs = |d: &egui_dock::DockState<crate::layout::Tab>| -> Vec<String> {
        d.iter_all_tabs()
            .map(|(_, t)| t.title().to_owned())
            .collect()
    };
    assert_eq!(tabs(&again_dock), tabs(&dock), "dock layout");
}

#[test]
fn pending_restore_is_taken_once_for_its_project() {
    let s = state("pending");
    let file = restore::write(&s, &crate::layout::default_layout()).unwrap();
    assert_eq!(restore::set_pending(&file).unwrap(), s.project.root);
    assert!(restore::take_pending(Path::new("elsewhere")).is_none());
    assert!(
        restore::take_pending(&s.project.root).is_none(),
        "taken once"
    );
    assert!(!file.exists(), "the file is removed");
}

#[test]
fn newer_build_detection() {
    let stamp = |secs: u64, len: u64| {
        Some(Stamp {
            modified: Some(SystemTime::UNIX_EPOCH + Duration::from_secs(secs)),
            len,
        })
    };
    assert!(is_newer(stamp(100, 10), stamp(200, 10)), "built later");
    assert!(
        !is_newer(stamp(100, 10), stamp(100, 10)),
        "a copy of the same build"
    );
    assert!(
        is_newer(stamp(100, 10), stamp(100, 11)),
        "same time, other contents"
    );
    assert!(!is_newer(stamp(200, 10), stamp(100, 10)), "older");
    assert!(!is_newer(stamp(100, 10), None), "no build");

    // A copy outside target/ and the checkout's build.
    let dir = temp("devbuild");
    let repo = dir.join("repo");
    let exe = format!("misty{}", std::env::consts::EXE_SUFFIX);
    let built = repo.join("target/debug").join(&exe);
    std::fs::create_dir_all(built.parent().unwrap()).unwrap();
    std::fs::write(&built, b"build 1").unwrap();
    let copy = dir.join("test/misty-grass.exe");
    std::fs::create_dir_all(copy.parent().unwrap()).unwrap();
    std::fs::copy(&built, &copy).unwrap();
    let t = SystemTime::now() - Duration::from_secs(60);
    for p in [&built, &copy] {
        std::fs::File::options()
            .write(true)
            .open(p)
            .unwrap()
            .set_modified(t)
            .unwrap();
    }
    let mut dev = DevBuild::with(repo.clone(), copy.clone(), "debug");
    let poll = |d: &mut DevBuild| d.poll_with(Duration::ZERO, Duration::ZERO);
    assert!(!poll(&mut dev) && !poll(&mut dev), "same build");
    assert!(!dev.newer);
    // cargo writes a new build.
    std::fs::write(&built, b"build 2!").unwrap();
    assert!(!poll(&mut dev), "waits for the file to settle");
    assert!(poll(&mut dev), "newer build ready");
    assert!(dev.newer);

    // Restart from a copy: the build is copied over it (rename trick) and the copy started.
    let (start, note) = dev.prepare_exe().unwrap();
    assert_eq!(start, copy);
    assert!(note.is_some());
    assert_eq!(std::fs::read(&copy).unwrap(), b"build 2!");
    let old = crate::updater::install::old_exe_path(&copy);
    assert_eq!(
        std::fs::read(&old).unwrap(),
        b"build 1",
        "the running copy moved aside"
    );
    // Running the build in target/ itself: start the new build there.
    let in_target = DevBuild::with(repo, built.clone(), "debug");
    assert_eq!(in_target.prepare_exe().unwrap().0, built);
    // copy_over again while an older .old.exe exists.
    std::fs::write(&built, b"build 3").unwrap();
    copy_over(&built, &copy).unwrap();
    assert_eq!(std::fs::read(&copy).unwrap(), b"build 3");
}

#[test]
fn settle_waits_for_a_stable_stamp() {
    let mut w = Settle::default();
    let a = Some(Stamp {
        modified: None,
        len: 1,
    });
    let b = Some(Stamp {
        modified: None,
        len: 2,
    });
    let wait = Duration::from_millis(50);
    assert!(!w.ready("f", a, wait));
    assert!(!w.ready("f", b, wait), "still changing");
    assert!(!w.ready("f", b, wait), "not long enough");
    std::thread::sleep(Duration::from_millis(60));
    assert!(w.ready("f", b, wait));
    assert_eq!(reloaded_text(1), "Reloaded 1 asset");
    assert_eq!(reloaded_text(3), "Reloaded 3 assets");
}

#[test]
fn changed_stamp_reports_only_real_changes() {
    let root = temp("changed_stamp");
    let path = root.join("Assets/a.txt");
    std::fs::write(&path, "one").unwrap();
    let seen = stamp(&path);
    assert!(seen.is_some());
    assert_eq!(changed_stamp(&path, seen), None, "unchanged file");
    std::fs::write(&path, "three").unwrap();
    bump(&path, 5);
    let now = changed_stamp(&path, seen).expect("rewritten file");
    assert_eq!(now, stamp(&path));
    assert_eq!(changed_stamp(&path, now), None, "the new stamp is the one seen");
    std::fs::remove_file(&path).unwrap();
    assert_eq!(changed_stamp(&path, now), Some(None), "deleted file");
    let _ = std::fs::remove_dir_all(&root);
}
