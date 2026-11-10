use std::path::{Path, PathBuf};
use std::sync::{Mutex, RwLock};

use super::*;

fn temp(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir().join(format!("misty_guids_{tag}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(dir.join("Assets")).unwrap();
    dir
}

fn write(root: &Path, rel: &str, text: &str) {
    let p = root.join(rel);
    std::fs::create_dir_all(p.parent().unwrap()).unwrap();
    std::fs::write(p, text).unwrap();
}

#[test]
fn guids_are_random_hex_and_parse_back() {
    let (a, b) = (Guid::new(), Guid::new());
    assert_ne!(a, b);
    let s = a.to_string();
    assert_eq!(s.len(), 32);
    assert_eq!(s.parse::<Guid>().unwrap(), a);
    assert_eq!(s.to_uppercase().parse::<Guid>().unwrap(), a);
    assert!("xyz".parse::<Guid>().is_err());
    assert!("0".repeat(32).parse::<Guid>().is_err());
}

#[test]
fn guid_goes_first_and_other_settings_keep_their_order() {
    let g: Guid = "0123456789abcdef0123456789abcdef".parse().unwrap();
    let old = "{\n  \"texture_type\": \"Sprite\",\n  \"filter_mode\": \"Point\"\n}\n";
    let text = meta::with_guid(old, g).unwrap();
    assert_eq!(
        text,
        "{\n  \"guid\": \"0123456789abcdef0123456789abcdef\",\n  \"texture_type\": \"Sprite\",\n  \"filter_mode\": \"Point\"\n}\n"
    );
    assert_eq!(meta::guid_in(&text), Some(g));
    // Replacing keeps everything else byte for byte.
    let h = Guid::new();
    let again = meta::with_guid(&text, h).unwrap();
    assert_eq!(again, text.replace(&g.to_string(), &h.to_string()));
    assert_eq!(
        meta::with_guid("{}", g).unwrap(),
        meta::compose(g, &Default::default())
    );
    assert!(meta::with_guid("not json", g).is_none());
}

#[test]
fn scan_creates_metas_for_files_and_folders_and_splits_copies() {
    let root = temp("scan");
    write(&root, "Assets/Mats/Red.mat", "{}");
    write(&root, "Assets/Tex/A.png", "png");
    write(
        &root,
        "Assets/Tex/A.png.meta",
        "{\n  \"texture_type\": \"Sprite\"\n}\n",
    );
    let (db, report) = GuidDatabase::scan(&root);
    assert_eq!(
        report.created,
        [
            "Assets/Mats",
            "Assets/Mats/Red.mat",
            "Assets/Tex",
            "Assets/Tex/A.png"
        ]
    );
    assert!(root.join("Assets/Mats.meta").is_file(), "folders get metas");
    let tex = std::fs::read_to_string(root.join("Assets/Tex/A.png.meta")).unwrap();
    assert!(
        tex.contains("\"texture_type\": \"Sprite\""),
        "settings kept: {tex}"
    );
    let red = db.guid_of("Assets/Mats/Red.mat").unwrap();
    assert_eq!(db.path_of(red), Some("Assets/Mats/Red.mat"));

    // A second scan changes nothing; a read-only load sees the same map.
    let (again, report) = GuidDatabase::scan(&root);
    assert_eq!(report, ScanReport::default());
    assert_eq!(again.to_map(), db.to_map());
    assert_eq!(GuidDatabase::read(&root).to_map(), db.to_map());

    // A copy made outside the editor (file and meta) gets a new GUID; the original keeps its own.
    std::thread::sleep(std::time::Duration::from_millis(20));
    std::fs::copy(
        root.join("Assets/Mats/Red.mat"),
        root.join("Assets/Mats/Red 1.mat"),
    )
    .unwrap();
    std::fs::copy(
        root.join("Assets/Mats/Red.mat.meta"),
        root.join("Assets/Mats/Red 1.mat.meta"),
    )
    .unwrap();
    let (db, report) = GuidDatabase::scan(&root);
    assert_eq!(report.reassigned, ["Assets/Mats/Red 1.mat"]);
    assert_eq!(db.guid_of("Assets/Mats/Red.mat"), Some(red));
    assert_ne!(db.guid_of("Assets/Mats/Red 1.mat"), Some(red));
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn moves_and_removals_follow_folders() {
    let mut db = GuidDatabase::new(Path::new("p"));
    let (a, b, c) = (Guid::new(), Guid::new(), Guid::new());
    db.insert("Assets/M", a);
    db.insert("Assets/M/Red.mat", b);
    db.insert("Assets/M2.mat", c);
    db.moved("Assets/M", "Assets/Materials");
    assert_eq!(db.path_of(b), Some("Assets/Materials/Red.mat"));
    assert_eq!(db.path_of(a), Some("Assets/Materials"));
    assert_eq!(
        db.path_of(c),
        Some("Assets/M2.mat"),
        "a sibling with the same prefix"
    );
    db.remove("Assets/Materials");
    assert_eq!(db.len(), 1);
    assert_eq!(db.guid_of("Assets/Materials/Red.mat"), None);
}

#[test]
fn references_embed_and_resolve_after_a_rename() {
    let mut db = GuidDatabase::new(Path::new("p"));
    let (red, ramp) = (Guid::new(), Guid::new());
    db.insert("Assets/Red.mat", red);
    db.insert("Assets/Ramp.obj", ramp);
    let text = "{\n  \"material\": \"Assets/Red.mat\",\n  \"mesh\": \"Assets/Ramp.obj#0\",\n  \"gone\": \"Assets/Missing.png\"\n}\n";
    let saved = refs::embed(&db, text);
    assert!(saved.starts_with(text.trim_end().trim_end_matches('}').trim_end()));
    assert!(saved.ends_with("\n}\n"), "{saved}");
    let v: serde_json::Value = serde_json::from_str(&saved).unwrap();
    assert_eq!(refs::table(&v).len(), 2, "unknown paths get no GUID");
    assert_eq!(
        refs::embed(&db, &saved),
        saved,
        "embedding twice is a no-op"
    );
    assert_eq!(refs::embed(&GuidDatabase::default(), text), text);

    // Not moved: same values, table dropped.
    let same: serde_json::Value = serde_json::from_str(&refs::resolve(&db, &saved)).unwrap();
    assert_eq!(same["material"], "Assets/Red.mat");
    assert!(same.get(refs::KEY).is_none());

    // Renamed and moved: the GUID wins over the stale path, sub-assets keep their suffix.
    db.moved("Assets/Red.mat", "Assets/Materials/Crimson.mat");
    db.moved("Assets/Ramp.obj", "Assets/Models/Ramp.obj");
    let v: serde_json::Value = serde_json::from_str(&refs::resolve(&db, &saved)).unwrap();
    assert_eq!(v["material"], "Assets/Materials/Crimson.mat");
    assert_eq!(v["mesh"], "Assets/Models/Ramp.obj#0");
    assert_eq!(v["gone"], "Assets/Missing.png");

    // Unknown GUID (other project): the path is the fallback.
    let v: serde_json::Value =
        serde_json::from_str(&refs::resolve(&GuidDatabase::default(), &saved)).unwrap();
    assert_eq!(v["material"], "Assets/Red.mat");
    // Old files without a table are untouched.
    assert!(matches!(refs::resolve(&db, text), Cow::Borrowed(_)));
}

#[test]
fn settings_writes_keep_the_guid() {
    let root = temp("settings");
    write(&root, "Assets/A.png", "png");
    let (db, _) = GuidDatabase::scan(&root);
    let g = db.guid_of("Assets/A.png").unwrap();
    let image = root.join("Assets/A.png");
    meta::write_settings(&image, &serde_json::json!({ "srgb": false })).unwrap();
    assert_eq!(meta::read_guid(&image), Some(g));
    let text = std::fs::read_to_string(meta::meta_path(&image)).unwrap();
    assert!(text.contains("\"srgb\": false"), "{text}");
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn the_watcher_walks_on_a_thread_and_reports_new_files() {
    // The watcher's own database and walk, not the process-wide ones: other tests running at
    // the same time read the global database (GUID tables in saved scenes, asset listings).
    let root = temp("watch");
    write(&root, "Assets/Mats/Red.mat", "{}");
    let db = RwLock::new(Some(GuidDatabase::scan(&root).0));
    let walk = Mutex::new(None);
    let guid = |rel: &str| db.read().unwrap().as_ref().unwrap().guid_of(rel);
    assert!(guid("Assets/Mats/Red.mat").is_some());
    assert!(guid("Assets/Mats/Blue.mat").is_none());
    // Nothing changed.
    assert!(poll_now_in(&db, &walk).is_none());
    // A walk under way when the file appears (the editor's next poll reads it) is not the
    // one that reports it: `poll_now_in` waits for it and walks again.
    assert!(poll_in(&db, &walk).is_none());
    write(&root, "Assets/Mats/Blue.mat", "{}");
    let report = poll_now_in(&db, &walk).expect("a rescan for the new file");
    assert!(
        report.created.iter().any(|p| p.contains("Blue.mat")),
        "{report:?}"
    );
    assert!(guid("Assets/Mats/Blue.mat").is_some());
    // And through the editor's call, which reads a finished walk one poll later.
    write(&root, "Assets/Mats/Green.mat", "{}");
    let started = std::time::Instant::now();
    let report = loop {
        if let Some(r) = poll_in(&db, &walk) {
            break r;
        }
        assert!(
            started.elapsed() < std::time::Duration::from_secs(60),
            "the watcher never reported Green.mat"
        );
        std::thread::sleep(std::time::Duration::from_millis(1));
    };
    assert!(
        report.created.iter().any(|p| p.contains("Green.mat")),
        "{report:?}"
    );
    assert!(guid("Assets/Mats/Green.mat").is_some());
    let _ = std::fs::remove_dir_all(root);
}

/// How long one walk of the sample project's Assets folder takes (prints only): the
/// watcher used to do it on the main thread every two seconds.
#[test]
#[ignore = "timing, prints only"]
fn sample_project_walk_time() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    for _ in 0..3 {
        let started = std::time::Instant::now();
        let mut n = 0;
        database::walk(&root.join("Assets"), &mut |_| n += 1);
        eprintln!(
            "walked {n} entries in {:.1} ms",
            started.elapsed().as_secs_f32() * 1000.0
        );
    }
    // And one stat per file, what the live-reload polls do twice a second.
    let mut files = Vec::new();
    database::walk(&root.join("Assets"), &mut |p| files.push(p.to_path_buf()));
    let started = std::time::Instant::now();
    let mut n = 0;
    for f in files.iter().take(200) {
        n += usize::from(std::fs::metadata(f).is_ok());
    }
    eprintln!(
        "{n} stats in {:.2} ms",
        started.elapsed().as_secs_f32() * 1000.0
    );
}

/// Asset saves go through a temp file and a rename: the file is replaced whole, no temp file
/// is left behind, and a save that cannot write keeps the previous file intact (a crash or a
/// full disk mid-save used to leave a truncated scene/prefab/terrain).
#[test]
fn atomic_writes_replace_whole_files_and_keep_the_old_one_on_failure() {
    let dir = temp("atomic");
    let path = dir.join("Assets/Level.scene");
    write_atomic(&path, "first").unwrap();
    write_atomic(&path, "second, longer").unwrap();
    assert_eq!(std::fs::read_to_string(&path).unwrap(), "second, longer");
    let names = |d: &Path| -> Vec<String> {
        std::fs::read_dir(d)
            .unwrap()
            .flatten()
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .collect()
    };
    assert_eq!(names(&dir.join("Assets")), ["Level.scene"]);
    // The scene, prefab and terrain savers use it too.
    let scene = crate::scene::Scene::new("Level");
    scene.save(&path).unwrap();
    assert!(
        std::fs::read_to_string(&path)
            .unwrap()
            .contains("\"objects\"")
    );
    assert_eq!(names(&dir.join("Assets")), ["Level.scene"]);
    // A save that fails (here: the folder is gone) reports it and leaves nothing behind.
    assert!(write_atomic(&dir.join("Missing/X.scene"), "x").is_err());
    // A path that is a folder: the rename and the in-place write both fail; the temp file
    // is cleaned up.
    std::fs::create_dir_all(dir.join("Assets/Folder.scene")).unwrap();
    assert!(write_atomic(&dir.join("Assets/Folder.scene"), "x").is_err());
    let mut left = names(&dir.join("Assets"));
    left.sort();
    assert_eq!(left, ["Folder.scene", "Level.scene"]);
    let _ = std::fs::remove_dir_all(dir);
}
