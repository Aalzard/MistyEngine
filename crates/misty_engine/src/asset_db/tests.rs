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

/// A Unity asset pack copied into the project brings Unity's YAML `.meta` files: the scan
/// takes them over (a GUID, and the import settings Misty has as well).
#[test]
fn unity_meta_files_are_taken_over() {
    let root = temp("unity_meta");
    write(&root, "Assets/Pack/Tree.fbx", "fbx");
    write(
        &root,
        "Assets/Pack/Tree.fbx.meta",
        "fileFormatVersion: 2
guid: ecf79f82681e62140adeb13fd61158c5
ModelImporter:
  meshes:
    globalScale: 1.17
    useFileScale: 1
  humanDescription:
    globalScale: 0.03
",
    );
    write(&root, "Assets/Pack/Bump.png", "png");
    write(
        &root,
        "Assets/Pack/Bump.png.meta",
        "fileFormatVersion: 2
guid: 4e33ac5f608e9da4397fa9720b91a387
TextureImporter:
  mipmaps:
    sRGBTexture: 0
  maxTextureSize: 1024
  textureType: 1
",
    );
    write(&root, "Assets/Pack/Other.txt", "text");
    write(&root, "Assets/Pack/Other.txt.meta", "not json, not unity");
    let (db, report) = GuidDatabase::scan(&root);
    assert!(db.guid_of("Assets/Pack/Tree.fbx").is_some());
    assert_eq!(report.errors.len(), 1, "{:?}", report.errors);
    let model = root.join("Assets/Pack/Tree.fbx");
    assert_eq!(meta::read_guid(&model), db.guid_of("Assets/Pack/Tree.fbx"));
    let s = crate::models::ModelImporter::load(&model);
    assert_eq!(s.scale_factor, 1.17);
    assert!(s.convert_units);
    let t = crate::sprites::import::TextureImporter::load(&root.join("Assets/Pack/Bump.png"));
    assert_eq!(
        t.texture_type,
        crate::sprites::import::TextureType::NormalMap
    );
    assert!(!t.srgb);
    assert_eq!(t.max_size, 1024);
    // A second scan changes nothing.
    let before = std::fs::read_to_string(meta::meta_path(&model)).unwrap();
    let (again, _) = GuidDatabase::scan(&root);
    assert_eq!(
        again.guid_of("Assets/Pack/Tree.fbx"),
        db.guid_of("Assets/Pack/Tree.fbx")
    );
    assert_eq!(
        std::fs::read_to_string(meta::meta_path(&model)).unwrap(),
        before
    );
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
    assert!(poll_in(&db, &walk, &root).is_none());
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
        if let Some(r) = poll_in(&db, &walk, &root) {
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

/// Run `f` on a thread and fail the test if it has not returned after `limit`: a deadlock or
/// a scan of the wrong project shows up as a failure instead of a test run that never ends.
fn within<R: Send + 'static>(
    limit: std::time::Duration,
    f: impl FnOnce() -> R + Send + 'static,
) -> R {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let _ = tx.send(f());
    });
    rx.recv_timeout(limit)
        .unwrap_or_else(|_| panic!("did not finish within {limit:?}"))
}

/// A project of `files` assets without meta files (as a freshly copied folder is).
fn project_without_metas(tag: &str, files: usize) -> PathBuf {
    let root = temp(tag);
    for i in 0..files {
        write(&root, &format!("Assets/Bulk/File {i}.txt"), "");
    }
    root
}

fn metas_under(dir: &Path) -> usize {
    let mut n = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).into_iter().flatten().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "meta") {
                n += 1;
            }
        }
    }
    n
}

/// r40: the editor's tests ran forever. One test installed the database of its own project of
/// 1500 files; every other test's asset refresh then rescanned *that* project, writing (and
/// flushing to the device) a meta file per asset, again and again, on a busy hard disk. A
/// rescan or watcher poll for one project must leave another project's database alone.
#[test]
fn rescanning_a_project_leaves_another_projects_database_alone() {
    let other = project_without_metas("rescan_other", 200);
    let mine = temp("rescan_mine");
    write(&mine, "Assets/Red.mat", "{}");
    let db = std::sync::Arc::new(RwLock::new(Some(GuidDatabase::new(&other))));
    let walk = std::sync::Arc::new(Mutex::new(None));
    let (db2, walk2, mine2) = (db.clone(), walk.clone(), mine.clone());
    let (rescanned, polled) = within(std::time::Duration::from_secs(60), move || {
        let rescanned = rescan_in(&db2, &mine2);
        let polled = (0..20)
            .map(|_| poll_in(&db2, &walk2, &mine2))
            .find(Option::is_some);
        (rescanned, polled.flatten())
    });
    assert!(rescanned.is_none() && polled.is_none());
    assert_eq!(metas_under(&other), 0, "the other project was scanned");
    assert_eq!(
        metas_under(&mine),
        0,
        "not the open project: nothing to scan"
    );
    assert_eq!(db.read().unwrap().as_ref().unwrap().root(), other.as_path());
    // Its own project is rescanned as before.
    *db.write().unwrap() = Some(GuidDatabase::new(&mine));
    let report = rescan_in(&db, &mine).expect("the open project is rescanned");
    assert_eq!(report.created, ["Assets/Red.mat"]);
    let _ = std::fs::remove_dir_all(other);
    let _ = std::fs::remove_dir_all(mine);
}

/// A rescan runs without a lock: a project installed while it ran stays installed (the scan's
/// result for the old project is dropped), and readers are never blocked by the scan.
#[test]
fn a_database_installed_during_a_rescan_is_kept() {
    let a = project_without_metas("rescan_race_a", 50);
    let b = temp("rescan_race_b");
    let db = RwLock::new(Some(GuidDatabase::new(&a)));
    let (scanned, _) = GuidDatabase::scan(&a);
    // What the rescan finds when it is done: another project was installed meanwhile.
    *db.write().unwrap() = Some(GuidDatabase::new(&b));
    assert!(!replace_if_same(&db, scanned));
    assert_eq!(db.read().unwrap().as_ref().unwrap().root(), b.as_path());
    let _ = std::fs::remove_dir_all(a);
    let _ = std::fs::remove_dir_all(b);
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

/// A project opened read-only refuses saves of its assets and settings (the other editor's
/// files stay as it wrote them) but not of its Library caches, nor of other projects.
#[test]
fn read_only_projects_refuse_asset_saves() {
    let root = temp("read_only");
    let other = temp("read_only_other");
    std::fs::create_dir_all(root.join("ProjectSettings")).unwrap();
    std::fs::create_dir_all(root.join("Library")).unwrap();
    let scene = root.join("Assets/A.scene");
    write_atomic(&scene, "before").unwrap();
    set_read_only(&root, true);
    assert!(is_read_only(&root));
    let refused = write_atomic(&scene, "after").unwrap_err();
    assert_eq!(refused.kind(), std::io::ErrorKind::PermissionDenied);
    assert!(refused.to_string().contains(READ_ONLY_MESSAGE));
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), "before");
    assert!(write_atomic(&root.join("Assets/New.mat"), "{}").is_err());
    assert!(write_atomic(&root.join("ProjectSettings/X.json"), "{}").is_err());
    // Spelled another way (a `.` segment; another letter case on Windows).
    assert!(write_atomic(&root.join("Assets/./B.scene"), "x").is_err());
    write_atomic(&root.join("Library/Cache.bin"), "cache").unwrap();
    write_atomic(&other.join("Assets/A.scene"), "other").unwrap();
    set_read_only(&root, false);
    write_atomic(&scene, "after").unwrap();
    assert_eq!(std::fs::read_to_string(&scene).unwrap(), "after");
    let _ = std::fs::remove_dir_all(&root);
    let _ = std::fs::remove_dir_all(&other);
}

/// Round 70: asset saves that wrote with `std::fs::write` (Animator controllers, which save
/// after every edit, Script / Shader / Effect Graphs, Behaviour Trees, mixers, physics
/// materials, localization tables…) went straight past the read-only check, so a second editor
/// on a project overwrote the first one's files. They go through [`write_asset`] now.
#[test]
fn read_only_projects_refuse_saves_of_every_asset_kind() {
    let root = temp("read_only_kinds");
    let path = |name: &str| root.join("Assets").join(name);
    type Save = Box<dyn Fn(&Path) -> Result<(), String>>;
    let saves: Vec<(&str, Save)> = vec![
        (
            "A.controller",
            Box::new(|p| crate::animation::controller::AnimatorController::default().save(p)),
        ),
        (
            "A.vgraph",
            Box::new(|p| crate::visual_script::ScriptGraph::default().save(p)),
        ),
        (
            "A.shadergraph",
            Box::new(|p| crate::shader_graph::ShaderGraph::default().save(p)),
        ),
        (
            "A.vfx",
            Box::new(|p| crate::vfx::graph::EffectGraph::default().save(p)),
        ),
        (
            "A.btree",
            Box::new(|p| crate::behaviour_tree::BehaviourTree::new_default().save(p)),
        ),
        (
            "A.mixer",
            Box::new(|p| crate::audio::mixer::AudioMixer::default().save(p)),
        ),
        (
            "A.physicmaterial",
            Box::new(|p| crate::physics::material::PhysicsMaterial::default().save(p)),
        ),
        (
            "A.loctable",
            Box::new(|p| crate::localization::table::StringTable::new("A", &["en"]).save(p)),
        ),
    ];
    for (name, _) in &saves {
        std::fs::write(path(name), "before").unwrap();
    }
    set_read_only(&root, true);
    for (name, save) in &saves {
        assert!(
            save(&path(name)).is_err(),
            "{name} saved in a read-only project"
        );
        assert_eq!(
            std::fs::read_to_string(path(name)).unwrap(),
            "before",
            "{name}"
        );
    }
    set_read_only(&root, false);
    for (name, save) in &saves {
        save(&path(name)).unwrap();
        assert_ne!(
            std::fs::read_to_string(path(name)).unwrap(),
            "before",
            "{name}"
        );
    }
    let _ = std::fs::remove_dir_all(&root);
}

/// Round 71: saves Round 70 missed still wrote with `std::fs::write` past the read-only check:
/// `ProjectSettings/ProjectSettings.json` (Edit ▸ Project Settings), Timelines, tiles and tile
/// palettes (painting a palette saves it), trees, biomes and camera blend lists. A second
/// editor on a project overwrote the first one's files with them.
#[test]
fn read_only_projects_refuse_the_saves_round_70_missed() {
    let root = temp("read_only_kinds_r71");
    let path = |name: &str| root.join("Assets").join(name);
    type Save = Box<dyn Fn(&Path) -> Result<(), String>>;
    let saves: Vec<(&str, Save)> = vec![
        (
            "A.timeline",
            Box::new(|p| crate::timeline::asset::TimelineAsset::default().save(p)),
        ),
        (
            "A.tile",
            Box::new(|p| crate::tilemap::tile::TileAsset::default().save(p)),
        ),
        (
            "A.palette",
            Box::new(|p| crate::tilemap::palette::TilePalette::default().save(p)),
        ),
        (
            "A.tree",
            Box::new(|p| crate::trees::TreeAsset::oak().save(p)),
        ),
        (
            "A.biome",
            Box::new(|p| crate::world_gen::biome::Biome::default().save(p)),
        ),
        (
            "A.blends",
            Box::new(|p| crate::vcam::BlendList::default().save(p)),
        ),
    ];
    for (name, _) in &saves {
        std::fs::write(path(name), "before").unwrap();
    }
    let settings = crate::settings::ProjectSettings::path(&root);
    std::fs::create_dir_all(settings.parent().unwrap()).unwrap();
    std::fs::write(&settings, "before").unwrap();
    set_read_only(&root, true);
    for (name, save) in &saves {
        assert!(
            save(&path(name)).is_err(),
            "{name} saved in a read-only project"
        );
        assert_eq!(
            std::fs::read_to_string(path(name)).unwrap(),
            "before",
            "{name}"
        );
    }
    assert!(
        crate::settings::ProjectSettings::default()
            .save(&root)
            .is_err(),
        "Project Settings saved in a read-only project"
    );
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), "before");
    set_read_only(&root, false);
    for (name, save) in &saves {
        save(&path(name)).unwrap();
        assert_ne!(
            std::fs::read_to_string(path(name)).unwrap(),
            "before",
            "{name}"
        );
    }
    crate::settings::ProjectSettings::default()
        .save(&root)
        .unwrap();
    assert_ne!(std::fs::read_to_string(&settings).unwrap(), "before");
    let _ = std::fs::remove_dir_all(&root);
}

/// r72: a meta file cut short by a crash (empty) gets a GUID like a missing one. It used to be
/// refused ("not a JSON object") on every scan.
#[test]
fn an_empty_meta_file_gets_a_guid() {
    let root = temp("empty_meta");
    write(&root, "Assets/Scenes/Level.scene", "{}");
    write(&root, "Assets/Scenes/Level.scene.meta", "");
    let (db, report) = GuidDatabase::scan(&root);
    assert!(report.errors.is_empty(), "{report:?}");
    // (The folder gets its first meta file too.)
    assert!(
        report
            .created
            .iter()
            .any(|p| p == "Assets/Scenes/Level.scene"),
        "{report:?}"
    );
    let guid = db.guid_of("Assets/Scenes/Level.scene").expect("a GUID");
    let text = std::fs::read_to_string(root.join("Assets/Scenes/Level.scene.meta")).unwrap();
    assert_eq!(meta::guid_in(&text), Some(guid));
    let _ = std::fs::remove_dir_all(root);
}

/// r72: an asset whose meta file can't be given a GUID (not a JSON object) is not "new" to
/// every later walk: the watcher rescanned the whole project (and the editor refreshed every
/// asset list, 100-650 ms on the main thread) every few seconds, failing on it each time.
#[test]
fn an_unfixable_meta_file_does_not_rescan_the_project_again_and_again() {
    let root = temp("unfixable_meta");
    write(&root, "Assets/Mats/Red.mat", "{}");
    write(&root, "Assets/Mats/Broken.mat", "{}");
    write(&root, "Assets/Mats/Broken.mat.meta", "[1, 2, 3]");
    let (scanned, report) = GuidDatabase::scan(&root);
    assert_eq!(report.errors.len(), 1, "{report:?}");
    assert!(scanned.guid_of("Assets/Mats/Broken.mat").is_none());
    assert!(scanned.unresolved().any(|p| p == "Assets/Mats/Broken.mat"));
    let db = RwLock::new(Some(scanned));
    let walk = Mutex::new(None);
    for _ in 0..3 {
        assert!(
            poll_now_in(&db, &walk).is_none(),
            "nothing changed on disk: no rescan"
        );
    }
    // A real change still rescans (and the broken one stays unresolved).
    write(&root, "Assets/Mats/Blue.mat", "{}");
    let report = poll_now_in(&db, &walk).expect("a rescan for the new file");
    assert!(
        report.created.iter().any(|p| p.contains("Blue.mat")),
        "{report:?}"
    );
    assert!(poll_now_in(&db, &walk).is_none());
    let _ = std::fs::remove_dir_all(root);
}

/// Round 73: an asset the last scan could not give a GUID (its meta file could not be written)
/// was remembered as known so the watcher stopped rescanning the project for it (round 72),
/// but nothing ever tried it again: once its meta file could be written (the read-only lock
/// gone, the file's permissions fixed) or another editor wrote it a GUID, the asset still had
/// none until something else changed under `Assets/`. A read-only project still writes nothing.
#[test]
fn an_unresolved_asset_is_picked_up_once_its_meta_file_can_be_written() {
    let root = temp("unresolved_later");
    write(&root, "Assets/Mats/Blue.mat", "{}");
    write(&root, "Assets/Mats/Green.mat", "{}");
    let blue_meta = root.join("Assets/Mats/Blue.mat.meta");
    set_read_only(&root, true);
    let (scanned, _) = GuidDatabase::scan(&root);
    assert!(
        !blue_meta.exists(),
        "a read-only project writes no meta file"
    );
    assert!(scanned.unresolved().any(|p| p == "Assets/Mats/Blue.mat"));
    assert!(scanned.guid_of("Assets/Mats/Blue.mat").is_none());
    let db = RwLock::new(Some(scanned));
    let walk = Mutex::new(None);
    let guid = |rel: &str| db.read().unwrap().as_ref().unwrap().guid_of(rel);
    // Still read-only: nothing written, nothing new.
    assert!(poll_now_in(&db, &walk).is_none());
    assert!(
        !blue_meta.exists(),
        "a read-only project writes no meta file"
    );

    // The editor holding the lock gives Green a GUID: picked up, still without writing.
    let green = Guid::new();
    write(
        &root,
        "Assets/Mats/Green.mat.meta",
        &meta::compose(green, &Default::default()),
    );
    assert!(poll_now_in(&db, &walk).is_some(), "Green's GUID is read");
    assert_eq!(guid("Assets/Mats/Green.mat"), Some(green));
    assert!(
        !blue_meta.exists(),
        "a read-only project writes no meta file"
    );
    assert!(guid("Assets/Mats/Blue.mat").is_none());

    // The project stops being read-only: Blue gets its meta file and GUID.
    set_read_only(&root, false);
    let report = poll_now_in(&db, &walk).expect("Blue is given a GUID");
    assert!(
        report.created.iter().any(|p| p == "Assets/Mats/Blue.mat"),
        "{report:?}"
    );
    let blue = guid("Assets/Mats/Blue.mat").expect("Blue's GUID");
    let text = std::fs::read_to_string(&blue_meta).unwrap();
    assert_eq!(meta::guid_in(&text), Some(blue));
    assert_eq!(guid("Assets/Mats/Green.mat"), Some(green));
    // Then nothing more to do.
    assert!(poll_now_in(&db, &walk).is_none());
    let _ = std::fs::remove_dir_all(root);
}

/// Round 73: the same for a meta file whose permissions are fixed later (read-only file).
#[cfg(windows)]
#[test]
fn an_unresolved_asset_is_picked_up_once_its_meta_file_is_writable_again() {
    let root = temp("unresolved_perms");
    write(&root, "Assets/Level.scene", "{}");
    let m = root.join("Assets/Level.scene.meta");
    write(&root, "Assets/Level.scene.meta", "");
    let set_ro = |ro: bool| {
        let mut p = std::fs::metadata(&m).unwrap().permissions();
        p.set_readonly(ro);
        std::fs::set_permissions(&m, p).unwrap();
    };
    set_ro(true);
    let (scanned, report) = GuidDatabase::scan(&root);
    assert_eq!(report.errors.len(), 1, "{report:?}");
    let db = RwLock::new(Some(scanned));
    let walk = Mutex::new(None);
    assert!(poll_now_in(&db, &walk).is_none(), "still read-only");
    set_ro(false);
    let report = poll_now_in(&db, &walk).expect("the meta file is written now");
    assert_eq!(report.created, ["Assets/Level.scene"]);
    let g = db
        .read()
        .unwrap()
        .as_ref()
        .unwrap()
        .guid_of("Assets/Level.scene");
    assert!(g.is_some());
    assert_eq!(meta::guid_in(&std::fs::read_to_string(&m).unwrap()), g);
    let _ = std::fs::remove_dir_all(root);
}
