//! Autosave, crash detection, recovery and the panic path.

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use super::docs::{self, DocKind, DocSnapshot};
use super::recovery::{self, RecoveryDialog};
use super::session::{self, Marker};
use super::*;
use crate::project::ProjectBrowser;
use crate::state::EditorState;

/// Process ids no process has (simulated sessions of editors that are gone).
const DEAD_PID_A: u32 = 0xFFFF_FF10;
const DEAD_PID_B: u32 = 0xFFFF_FF20;

/// A fresh project folder per test.
fn project(name: &str) -> PathBuf {
    let root = std::env::temp_dir().join(format!("misty_autosave_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets/Scenes")).unwrap();
    root
}

fn state(root: &Path) -> EditorState {
    EditorState::new(ProjectBrowser::open(root.to_path_buf()))
}

/// Start a simulated editor session (process `pid`, started `ago` before now).
fn begin_session(s: &mut EditorState, pid: u32, ago: Duration) -> session::Started {
    let started_ms = session::now_ms() - ago.as_millis() as u64;
    let st = session::start_as(&s.project.root, pid, started_ms).unwrap();
    super::begin(s, Prefs::default(), Ok(st.clone()));
    st
}

/// Run one autosave now, to the end.
fn autosave_now(s: &mut EditorState) {
    request_now(s);
    // Scene snapshots finish on worker threads: wait for them (30 s at most).
    for _ in 0..3000 {
        tick(s, Instant::now(), false, Duration::from_secs(10));
        if s.autosave.run.is_none() {
            return;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    panic!("the autosave run did not finish");
}

fn files_in(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(dir) else {
        return out;
    };
    for e in entries.flatten() {
        let p = e.path();
        if p.is_dir() {
            out.extend(files_in(&p));
        } else {
            out.push(p);
        }
    }
    out.sort();
    out
}

/// A scene saved as `rel` with one object named `object`, then changed (unsaved): an object
/// `unsaved` added.
fn dirty_scene(s: &mut EditorState, rel: &str, object: &str, unsaved: &str) {
    s.scene.spawn(object, None);
    assert!(crate::scene_files::write(s, 0, rel));
    s.scene.spawn(unsaved, None);
    s.scene_dirty = true;
    crate::scene_files::refresh_dirty(s);
}

#[test]
fn autosave_backs_up_only_dirty_documents_atomically() {
    let root = project("dirty_only");
    let mut s = state(&root);
    begin_session(&mut s, DEAD_PID_A, Duration::ZERO);
    let session_dir = s.autosave.session_dir().unwrap().to_path_buf();

    // Nothing changed: nothing is written.
    autosave_now(&mut s);
    assert!(
        files_in(&session_dir).is_empty(),
        "{:?}",
        files_in(&session_dir)
    );
    assert_eq!(s.autosave.last.saved_at_ms, None);

    // A saved scene with an unsaved change: one backup, its file untouched.
    dirty_scene(
        &mut s,
        "Assets/Scenes/Level.scene",
        "Saved Object",
        "Unsaved Object",
    );
    let file = root.join("Assets/Scenes/Level.scene");
    let before = std::fs::read(&file).unwrap();
    autosave_now(&mut s);
    let backups = files_in(&session_dir);
    assert_eq!(backups.len(), 1, "{backups:?}");
    let name = backups[0]
        .file_name()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    assert_eq!(
        name,
        format!("0.{BACKUP_EXT}"),
        "no temp files left: {backups:?}"
    );
    let bytes = std::fs::read(&backups[0]).unwrap();
    let (header, body) = docs::parse_backup(&bytes).expect("a backup file");
    assert_eq!(header.kind, DocKind::Scene);
    assert_eq!(header.path.as_deref(), Some("Assets/Scenes/Level.scene"));
    assert!(body.contains("Unsaved Object"));
    assert!(!header.emergency);
    assert_eq!(
        std::fs::read(&file).unwrap(),
        before,
        "the scene file is never written"
    );
    assert!(!String::from_utf8_lossy(&before).contains("Unsaved Object"));
    assert_eq!(s.autosave.last.documents, 1);
    assert!(s.autosave.last.saved_at_ms.is_some());

    // Unchanged since that backup: not written again.
    autosave_now(&mut s);
    assert_eq!(files_in(&session_dir).len(), 1);

    // More changes: the next slot; the slots rotate, keeping the last KEEP backups.
    for i in 0..KEEP + 2 {
        s.scene.spawn(&format!("Edit {i}"), None);
        autosave_now(&mut s);
    }
    let backups = files_in(&session_dir);
    assert_eq!(backups.len(), KEEP, "{backups:?}");
    let newest = backups
        .iter()
        .map(|p| {
            docs::parse_backup(&std::fs::read(p).unwrap())
                .unwrap()
                .0
                .time_ms
        })
        .max()
        .unwrap();
    assert!(newest >= header.time_ms);

    // Saved: nothing dirty, nothing more written.
    assert!(crate::scene_files::save(&mut s));
    let count = files_in(&session_dir).len();
    autosave_now(&mut s);
    assert_eq!(files_in(&session_dir).len(), count);

    // A clean exit removes the session's backups and marks the marker.
    clean_exit(&mut s);
    assert!(!session_dir.exists());
    let marker = Marker::load(&root).unwrap();
    assert!(marker.clean_exit);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn autosave_covers_graphs_and_scripts_with_unsaved_changes() {
    let root = project("graphs");
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    std::fs::write(root.join("Assets/Scripts/Mover.lua"), "-- saved\n").unwrap();
    let mut s = state(&root);
    begin_session(&mut s, DEAD_PID_A, Duration::ZERO);
    s.code_editor
        .open("Assets/Scripts/Mover.lua", None)
        .unwrap();
    assert!(docs::dirty_docs(&s).is_empty());
    let doc = &mut s.code_editor.docs[0];
    doc.editor.reload("-- edited, not saved\n");
    assert_eq!(
        docs::dirty_docs(&s),
        [docs::DocRef::Script("Assets/Scripts/Mover.lua".into())]
    );
    autosave_now(&mut s);
    let backups = files_in(s.autosave.session_dir().unwrap());
    assert_eq!(backups.len(), 1);
    let bytes = std::fs::read(&backups[0]).unwrap();
    let (header, body) = docs::parse_backup(&bytes).unwrap();
    assert_eq!(header.kind, DocKind::Script);
    assert_eq!(body, "-- edited, not saved\n");
    assert_eq!(
        std::fs::read_to_string(root.join("Assets/Scripts/Mover.lua")).unwrap(),
        "-- saved\n"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn the_interval_waits_for_the_user_and_for_idle_on_big_snapshots() {
    let t0 = Instant::now();
    let prefs = Prefs::default();
    let interval = prefs.interval();
    assert_eq!(interval, Duration::from_secs(300));
    let mut t = Timer::new(t0);
    let idle = Duration::from_secs(30);
    // Not before the interval.
    assert!(!t.should_start(&prefs, t0 + Duration::from_secs(10), false, idle, false));
    assert!(!t.should_start(
        &prefs,
        t0 + interval - Duration::from_millis(1),
        false,
        idle,
        false
    ));
    // Due: starts.
    assert!(t.should_start(&prefs, t0 + interval, false, idle, false));
    t.done(t0 + interval);
    // The next one counts from the last autosave.
    assert!(!t.should_start(
        &prefs,
        t0 + interval * 2 - Duration::from_secs(1),
        false,
        idle,
        false
    ));
    // Mid-drag: waits, at most MAX_WAIT_FOR_INPUT.
    let due = t0 + interval * 2;
    assert!(!t.should_start(&prefs, due, true, Duration::ZERO, false));
    assert!(!t.should_start(
        &prefs,
        due + Duration::from_secs(30),
        true,
        Duration::ZERO,
        false
    ));
    assert!(t.should_start(
        &prefs,
        due + MAX_WAIT_FOR_INPUT,
        true,
        Duration::ZERO,
        false
    ));
    t.done(due + MAX_WAIT_FOR_INPUT);

    // A big snapshot waits for the user to pause (IDLE_FOR), at most one more interval.
    let mut t = Timer::new(t0);
    let due = t0 + interval;
    assert!(!t.should_start(&prefs, due, false, Duration::from_millis(500), true));
    assert!(t.should_start(&prefs, due + Duration::from_secs(5), false, IDLE_FOR, true));
    let mut t = Timer::new(t0);
    assert!(!t.should_start(&prefs, due, false, Duration::ZERO, true));
    assert!(!t.should_start(&prefs, due + interval / 2, true, Duration::ZERO, true));
    assert!(t.should_start(&prefs, due + interval, true, Duration::ZERO, true));

    // Off: never (unless asked to run now, after a restore).
    let off = Prefs {
        enabled: false,
        ..prefs
    };
    let mut t = Timer::new(t0);
    assert!(!t.should_start(&off, t0 + interval * 10, false, idle, false));
    t.now = true;
    assert!(t.should_start(&off, t0, false, idle, false));
    assert!(!t.now);

    // Another interval.
    let one = Prefs {
        interval_minutes: 1,
        ..prefs
    };
    let mut t = Timer::new(t0);
    assert!(t.should_start(&one, t0 + Duration::from_secs(60), false, idle, false));
    // Out of range values are clamped.
    let zero = Prefs {
        interval_minutes: 0,
        ..prefs
    };
    assert_eq!(zero.interval(), Duration::from_secs(60));
}

#[test]
fn an_unclean_exit_is_detected_and_its_backups_offered() {
    let root = project("unclean");
    // Session A: unsaved work backed up, then the power goes (no clean exit).
    let mut a = state(&root);
    begin_session(&mut a, DEAD_PID_A, Duration::from_secs(600));
    dirty_scene(
        &mut a,
        "Assets/Scenes/Level.scene",
        "Saved",
        "Lost In Crash",
    );
    autosave_now(&mut a);
    let a_dir = a.autosave.session_dir().unwrap().to_path_buf();
    drop(a);
    assert!(!Marker::load(&root).unwrap().clean_exit);

    // Session B opens the project.
    let mut b = state(&root);
    let started = begin_session(&mut b, DEAD_PID_B, Duration::ZERO);
    let previous = started
        .previous
        .as_ref()
        .expect("the last session's marker");
    assert!(!previous.clean_exit);
    assert_eq!(previous.pid, DEAD_PID_A);
    assert!(!started.other_editor, "the old process is gone");
    let dialog = b.autosave.recovery.as_ref().expect("the recovery dialog");
    assert!(dialog.unclean);
    assert_eq!(dialog.items.len(), 1);
    assert_eq!(
        dialog.items[0].header.path.as_deref(),
        Some("Assets/Scenes/Level.scene")
    );
    assert_eq!(dialog.items[0].dirs.len(), 1);
    assert!(dialog.items[0].dirs[0].starts_with(&a_dir));
    // B's marker replaced A's.
    let m = Marker::load(&root).unwrap();
    assert_eq!(m.pid, DEAD_PID_B);
    assert!(!m.clean_exit);

    // A clean session (that put the recovery off) leaves A's backups to the next start.
    clean_exit(&mut b);
    let mut c = state(&root);
    let started = begin_session(&mut c, DEAD_PID_A, Duration::ZERO);
    assert!(started.previous.unwrap().clean_exit);
    // (A's backups were put off, not discarded: still offered.)
    assert!(c.autosave.recovery.is_some());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_running_editors_marker_and_backups_are_left_alone() {
    let root = project("running");
    let mut a = state(&root);
    // This test process plays the running editor: its pid, started now.
    let st = session::start(&root).unwrap();
    super::begin(&mut a, Prefs::default(), Ok(st));
    dirty_scene(&mut a, "Assets/Scenes/Level.scene", "Saved", "Live Edit");
    autosave_now(&mut a);
    // A second editor (another process) opening the same project.
    let started = session::start_as(&root, DEAD_PID_B, session::now_ms()).unwrap();
    assert!(started.other_editor);
    assert_eq!(Marker::load(&root).unwrap().pid, std::process::id());
    let scan = recovery::scan(&root, Some(&started.session.id));
    // (The running session is this process: its backups are skipped as live.)
    assert!(scan.leftovers.is_empty(), "{:?}", scan.leftovers);
    clean_exit(&mut a);
    let _ = std::fs::remove_dir_all(&root);
}

/// Write a backup file for a simulated earlier session.
fn plant_backup(root: &Path, session_id: &str, snap: &DocSnapshot, time_ms: u64) -> PathBuf {
    let dir = root
        .join(AUTOSAVE_DIR)
        .join(session_id)
        .join(snap.folder_name());
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("0.{BACKUP_EXT}"));
    std::fs::write(&path, snap.file_bytes(time_ms)).unwrap();
    dir
}

fn scene_json(object: &str) -> String {
    let mut scene = misty_engine::scene::Scene::new("x");
    scene.spawn(object, None);
    scene.to_json()
}

#[test]
fn the_recovery_list_has_documents_newer_than_their_files() {
    let root = project("list");
    let now = session::now_ms();
    let old_session = format!("{}-{DEAD_PID_A}", now - 3_600_000);
    let file = |rel: &str, text: &str| {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(&p, text).unwrap();
    };
    file("Assets/Scenes/Newer.scene", &scene_json("On Disk"));
    file("Assets/Scenes/Older.scene", &scene_json("On Disk"));
    let snap = |rel: Option<&str>, name: &str| {
        DocSnapshot::new(
            DocKind::Scene,
            rel.map(str::to_owned),
            name.to_owned(),
            scene_json("Backed Up"),
        )
    };
    // Newer than its file: offered.
    plant_backup(
        &root,
        &old_session,
        &snap(Some("Assets/Scenes/Newer.scene"), "Newer"),
        now + 60_000,
    );
    // Saved after the backup: stale.
    let stale = plant_backup(
        &root,
        &old_session,
        &snap(Some("Assets/Scenes/Older.scene"), "Older"),
        now - 600_000,
    );
    // Never saved: offered.
    plant_backup(&root, &old_session, &snap(None, "Untitled"), now + 30_000);
    // File gone since: offered.
    plant_backup(
        &root,
        &old_session,
        &snap(Some("Assets/Scenes/Deleted.scene"), "Deleted"),
        now + 10_000,
    );
    // A backup cut short before its header ended: not a document.
    let broken = root
        .join(AUTOSAVE_DIR)
        .join(&old_session)
        .join("scene-Broken-00000000");
    std::fs::create_dir_all(&broken).unwrap();
    std::fs::write(
        broken.join(format!("0.{BACKUP_EXT}")),
        "{\"misty_autosave\":1,\"ki",
    )
    .unwrap();
    // The same document in a later crashed session: one row, the newest backup.
    let later_session = format!("{}-{DEAD_PID_B}", now - 1_000_000);
    plant_backup(
        &root,
        &later_session,
        &snap(Some("Assets/Scenes/Newer.scene"), "Newer"),
        now + 120_000,
    );

    let scan = recovery::scan(&root, None);
    let rows: Vec<(String, Option<String>, Option<u64>)> = scan
        .leftovers
        .iter()
        .map(|l| (l.header.name.clone(), l.header.path.clone(), l.file_ms))
        .collect();
    assert_eq!(scan.leftovers.len(), 3, "{rows:?}");
    assert_eq!(scan.leftovers[0].header.name, "Newer");
    assert_eq!(scan.leftovers[0].backup_ms, now + 120_000);
    assert_eq!(scan.leftovers[0].dirs.len(), 2, "both sessions' folders");
    assert!(scan.leftovers[0].file_ms.is_some());
    assert_eq!(scan.leftovers[1].header.name, "Untitled");
    assert_eq!(scan.leftovers[1].header.path, None);
    assert_eq!(scan.leftovers[2].header.name, "Deleted");
    assert_eq!(scan.leftovers[2].file_ms, None);
    assert!(scan.stale.contains(&stale), "{:?}", scan.stale);
    assert!(scan.stale.contains(&broken));

    // Opening the project deletes the stale folders and shows the rest.
    let mut s = state(&root);
    begin_session(&mut s, std::process::id(), Duration::ZERO);
    let dialog: &RecoveryDialog = s.autosave.recovery.as_ref().unwrap();
    assert_eq!(dialog.items.len(), 3);
    assert!(
        !dialog.unclean,
        "no marker: nothing says the last session crashed"
    );
    assert!(!stale.exists());
    assert!(!broken.exists());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn restore_opens_the_backup_as_changed_and_leaves_the_file_alone() {
    let root = project("restore");
    // Session A: an unsaved change to Level.scene, then a crash.
    let mut a = state(&root);
    begin_session(&mut a, DEAD_PID_A, Duration::from_secs(600));
    dirty_scene(
        &mut a,
        "Assets/Scenes/Level.scene",
        "Saved Object",
        "Recovered Object",
    );
    autosave_now(&mut a);
    drop(a);
    let file = root.join("Assets/Scenes/Level.scene");
    let on_disk = std::fs::read(&file).unwrap();

    // Session B reopens the scene, then restores.
    let mut b = state(&root);
    crate::scene_files::open_initial(&mut b);
    assert_eq!(
        b.scene.active_path().as_deref(),
        Some("Assets/Scenes/Level.scene")
    );
    assert!(b.scene.find_by_name("Recovered Object").is_none());
    begin_session(&mut b, DEAD_PID_B, Duration::ZERO);
    assert_eq!(b.autosave.recovery.as_ref().unwrap().items.len(), 1);
    let dirs = b.autosave.recovery.as_ref().unwrap().items[0].dirs.clone();
    recovery::restore_items(&mut b, false);
    assert!(b.autosave.recovery.is_none(), "everything restored");
    assert!(b.scene.find_by_name("Recovered Object").is_some());
    assert!(b.scene.find_by_name("Saved Object").is_some());
    assert!(b.scene_dirty);
    assert_eq!(
        crate::scene_files::dirty_now(&b).len(),
        1,
        "the scene shows as changed"
    );
    crate::scene_files::refresh_dirty(&mut b);
    assert!(crate::scene_files::window_title(&b).contains("Level*"));
    assert_eq!(
        std::fs::read(&file).unwrap(),
        on_disk,
        "the file is untouched"
    );
    // The old backups stay until this session has its own (and exits cleanly).
    assert!(dirs.iter().all(|d| d.exists()));
    assert!(
        b.autosave.timer.as_ref().unwrap().now,
        "backed up again at once"
    );
    autosave_now(&mut b);
    assert_eq!(files_in(b.autosave.session_dir().unwrap()).len(), 1);
    clean_exit(&mut b);
    assert!(dirs.iter().all(|d| !d.exists()));
    assert!(files_in(&root.join(AUTOSAVE_DIR)).is_empty());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn restoring_an_untitled_scene_and_a_prefab() {
    let root = project("restore_more");
    std::fs::create_dir_all(root.join("Assets/Prefabs")).unwrap();
    // A prefab on disk with one child.
    let mut scene = misty_engine::scene::Scene::new("p");
    let r = scene.spawn("Crate", None);
    scene.spawn("Lid", Some(r));
    let prefab = misty_engine::scene::prefab::Prefab::from_scene(&scene, r);
    prefab
        .save(&root.join("Assets/Prefabs/Crate.prefab"))
        .unwrap();
    let on_disk = std::fs::read(root.join("Assets/Prefabs/Crate.prefab")).unwrap();
    // The backup: the prefab with another child.
    scene.spawn("Handle", Some(r));
    let backup = docs::prefab_snapshot(&scene, "Assets/Prefabs/Crate.prefab", r);
    let now = session::now_ms();
    let old = format!("{}-{DEAD_PID_A}", now - 600_000);
    plant_backup(&root, &old, &backup, now + 5_000);
    let untitled = DocSnapshot::new(
        DocKind::Scene,
        None,
        "Untitled".into(),
        scene_json("Never Saved"),
    );
    plant_backup(&root, &old, &untitled, now + 4_000);

    let mut s = state(&root);
    begin_session(&mut s, DEAD_PID_B, Duration::ZERO);
    assert_eq!(s.autosave.recovery.as_ref().unwrap().items.len(), 2);
    recovery::restore_items(&mut s, true);
    assert!(s.autosave.recovery.is_none(), "{:?}", s.console.last());
    // The prefab opened in Prefab Mode, changed, over the untitled scene.
    assert_eq!(
        crate::prefab_mode::open_path(&s).as_deref(),
        Some("Assets/Prefabs/Crate.prefab")
    );
    assert!(s.scene.find_by_name("Handle").is_some());
    assert!(s.scene_dirty);
    assert_eq!(
        std::fs::read(root.join("Assets/Prefabs/Crate.prefab")).unwrap(),
        on_disk
    );
    // Leaving without saving brings back the untitled scene, changed.
    crate::prefab_mode::close(&mut s);
    assert!(s.scene.find_by_name("Never Saved").is_some());
    assert!(s.scene_dirty);
    assert_eq!(s.scene.active_path(), None);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn discard_deletes_the_backups() {
    let root = project("discard");
    let mut a = state(&root);
    begin_session(&mut a, DEAD_PID_A, Duration::from_secs(600));
    dirty_scene(&mut a, "Assets/Scenes/Level.scene", "Saved", "Unwanted");
    autosave_now(&mut a);
    let a_dir = a.autosave.session_dir().unwrap().to_path_buf();
    drop(a);
    assert!(a_dir.exists());

    let mut b = state(&root);
    begin_session(&mut b, DEAD_PID_B, Duration::ZERO);
    assert!(b.autosave.recovery.is_some());
    recovery::discard_items(&mut b);
    assert!(b.autosave.recovery.is_none());
    assert!(!a_dir.exists(), "the session folder goes once empty");
    assert!(b.scene.find_by_name("Unwanted").is_none());
    // Decide Later keeps them: nothing left here to keep, so the next start shows nothing.
    clean_exit(&mut b);
    let mut c = state(&root);
    begin_session(&mut c, DEAD_PID_A, Duration::ZERO);
    assert!(c.autosave.recovery.is_none());
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn decide_later_keeps_the_backups_for_the_next_start() {
    let root = project("later");
    let mut a = state(&root);
    begin_session(&mut a, DEAD_PID_A, Duration::from_secs(600));
    dirty_scene(&mut a, "Assets/Scenes/Level.scene", "Saved", "Kept");
    autosave_now(&mut a);
    drop(a);
    let mut b = state(&root);
    begin_session(&mut b, DEAD_PID_B, Duration::from_secs(300));
    recovery::decide_later(&mut b);
    clean_exit(&mut b);
    let mut c = state(&root);
    begin_session(&mut c, DEAD_PID_A, Duration::ZERO);
    let d = c.autosave.recovery.as_ref().expect("offered again");
    assert!(!d.unclean, "the last session closed cleanly");
    assert_eq!(d.items.len(), 1);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn a_panic_writes_an_emergency_backup_and_a_crash_log() {
    let root = project("panic");
    let mut s = state(&root);
    begin_session(&mut s, DEAD_PID_A, Duration::from_secs(60));
    dirty_scene(
        &mut s,
        "Assets/Scenes/Level.scene",
        "Saved",
        "Unsaved When It Crashed",
    );
    s.console.info("a console line before the crash");
    // The editor frame panics (the launcher catches it the same way).
    let payload = std::panic::catch_unwind(|| {
        panic!("simulated editor crash");
    })
    .unwrap_err();
    let out = crash::after_panic(&mut s, &*payload);
    assert_eq!(out.backups.len(), 1, "{out:?}");
    assert!(out.failed.is_empty());
    let bytes = std::fs::read(&out.backups[0]).unwrap();
    let (header, body) = docs::parse_backup(&bytes).unwrap();
    assert!(header.emergency);
    assert!(body.contains("Unsaved When It Crashed"));
    let log = out.log.expect("a crash log");
    assert!(log.starts_with(root.join(crash::LOG_DIR)));
    let text = std::fs::read_to_string(&log).unwrap();
    assert!(text.contains("simulated editor crash"), "{text}");
    assert!(text.contains("a console line before the crash"));
    assert!(text.contains(env!("CARGO_PKG_VERSION")));
    assert!(text.contains("never sends it anywhere"));
    let marker = Marker::load(&root).unwrap();
    assert!(marker.crashed && !marker.clean_exit);
    assert!(marker.crash_log.unwrap().starts_with("Library/Logs/crash-"));
    drop(s);

    // The next start offers the backup and the log folder.
    let mut next = state(&root);
    begin_session(&mut next, DEAD_PID_B, Duration::ZERO);
    let d = next.autosave.recovery.as_ref().unwrap();
    assert_eq!(d.items.len(), 1);
    assert!(d.items[0].header.emergency);
    assert_eq!(d.crash_logs, [log]);
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn backup_files_say_what_they_hold() {
    let snap = DocSnapshot::new(
        DocKind::ShaderGraph,
        Some("Assets/Shaders/Water Ripples.shadergraph".into()),
        "Water Ripples.shadergraph".into(),
        "{\n  \"nodes\": []\n}".into(),
    );
    let bytes = snap.file_bytes(1_700_000_000_000);
    let (h, body) = docs::parse_backup(&bytes).unwrap();
    assert_eq!(h.kind, DocKind::ShaderGraph);
    assert_eq!(h.time_ms, 1_700_000_000_000);
    assert_eq!(body, snap.body);
    let folder = snap.folder_name();
    assert!(
        folder.starts_with("shader_graph-Water_Ripples_shadergraph-"),
        "{folder}"
    );
    assert!(docs::parse_backup(b"not a backup").is_none());
    assert!(docs::parse_backup(b"{\"misty_autosave\":99,\"kind\":\"scene\",\"path\":null,\"name\":\"x\",\"time_ms\":1}\n{}").is_none());
    // Dates.
    let t = session::utc(1_700_000_000_000);
    assert_eq!(
        (t.year, t.month, t.day, t.hour, t.minute),
        (2023, 11, 14, 22, 13)
    );
    assert_eq!(session::utc(0).year, 1970);
    assert_eq!(session::local_clock(1_700_000_000_000).len(), 5);
}

/// The main-thread cost of an autosave on the largest sample scenes (release build). NoonValley
/// stays well inside one frame's worth of work; a scene past [`BIG_SNAPSHOT`] (the stress
/// scenes) marks the next autosave to wait for the user to pause.
#[test]
fn autosave_cost_on_a_big_scene_stays_within_budget() {
    let sample =
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject/Assets/Scenes");
    let root = project("cost");
    let mut s = state(&root);
    begin_session(&mut s, DEAD_PID_A, Duration::ZERO);
    let mut report = String::new();
    let mut measure = |s: &mut EditorState, name: &str| -> (Duration, Duration) {
        let (scene, _) =
            misty_engine::scene::Scene::load(&sample.join(format!("{name}.scene"))).unwrap();
        s.scene = scene;
        s.scene
            .set_scene_path(0, &format!("Assets/Scenes/{name}.scene"));
        crate::scene_files::reset_after_load(s);
        s.scene_dirty = true;
        // The best of three (the machine may be busy with other tests).
        let mut best = Duration::MAX;
        let mut bytes = 0;
        for _ in 0..3 {
            let t = Instant::now();
            let snap = docs::snapshot(s, &docs::DocRef::Scene { layer: 0, id: 0 }).unwrap();
            best = best.min(t.elapsed());
            bytes = snap.body.len();
        }
        // What an autosave costs the main thread: the scene copy (its JSON is written on a
        // worker thread).
        let mut main = Duration::MAX;
        for _ in 0..3 {
            let t = Instant::now();
            let started = docs::start_snapshot(s, &docs::DocRef::Scene { layer: 0, id: 0 });
            main = main.min(t.elapsed());
            match started {
                docs::Started::Worker(rx) => assert_eq!(rx.recv().unwrap().body.len(), bytes),
                docs::Started::Done(_) => panic!("scenes are serialised on a worker"),
            }
        }
        report.push_str(&format!(
            "{name}: JSON {:.2} ms ({} KB, on a worker), main thread {:.2} ms\n",
            best.as_secs_f64() * 1000.0,
            bytes / 1024,
            main.as_secs_f64() * 1000.0
        ));
        (best, main)
    };
    let (_, noon) = measure(&mut s, "NoonValley");
    // A whole run of NoonValley: one document, not big.
    s.autosave.backed_up.clear();
    autosave_now(&mut s);
    assert_eq!(s.autosave.last.documents, 1);
    assert!(!s.autosave.big, "{:?}", s.autosave.last);
    let (_, stress) = measure(&mut s, "StressDemo");
    println!("{report}");
    assert!(
        noon < Duration::from_millis(12),
        "NoonValley autosave took {noon:?} of main-thread time (budget 12 ms)\n{report}"
    );
    // A whole autosave run of the stress scene: written, and marked big when slow.
    s.autosave.backed_up.clear();
    autosave_now(&mut s);
    assert_eq!(s.autosave.last.documents, 1);
    assert_eq!(s.autosave.big, s.autosave.last.slowest >= BIG_SNAPSHOT);
    assert!(stress > noon, "{report}");
    let _ = std::fs::remove_dir_all(&root);
}
