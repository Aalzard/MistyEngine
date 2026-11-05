//! Asset Groups tests: catalog planning, bundle builds, runtime loading, remote bundles.

use std::path::{Path, PathBuf};

use super::build::build_bundles;
use super::catalog::{CATALOG_FILE, REMOTE_DIR, plan, slug};
use super::fetch::sha256_hex;
use super::*;

fn temp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_groups_{tag}_{n}"));
    std::fs::create_dir_all(&root).unwrap();
    root
}

/// A, B use Shared.mat; C uses A. A and B are "enemy", C is "boss".
fn project(tag: &str) -> PathBuf {
    let root = temp(tag);
    let files = [
        (
            "Assets/Mats/Shared.mat",
            r#"{"color":[1,1,1,1]}"#.to_owned(),
        ),
        (
            "Assets/Prefabs/A.prefab",
            format!(
                r#"{{"material":"Assets/Mats/Shared.mat","pad":"{}"}}"#,
                "a".repeat(400)
            ),
        ),
        (
            "Assets/Prefabs/B.prefab",
            r#"{"material":"Assets/Mats/Shared.mat"}"#.to_owned(),
        ),
        (
            "Assets/Prefabs/C.prefab",
            r#"{"child":"Assets/Prefabs/A.prefab"}"#.to_owned(),
        ),
    ];
    for (rel, text) in files {
        let p = root.join(rel);
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, text).unwrap();
    }
    root
}

fn entry(path: &str, address: &str, labels: &[&str]) -> AssetEntry {
    AssetEntry {
        guid: String::new(),
        path: path.into(),
        address: address.into(),
        labels: labels.iter().map(|s| s.to_string()).collect(),
    }
}

fn settings() -> AssetGroupSettings {
    let mut s = AssetGroupSettings::default();
    s.groups[0].entries = vec![
        entry("Assets/Prefabs/A.prefab", "Enemy/A", &["enemy"]),
        entry("Assets/Prefabs/B.prefab", "Enemy/B", &["enemy"]),
    ];
    let mut extra = AssetGroup::new("Extra Stuff");
    extra.bundle_mode = BundleMode::PackSeparately;
    extra.compression = BundleCompression::Zlib;
    extra.entries = vec![entry("Assets/Prefabs/C.prefab", "Boss", &["boss"])];
    s.groups.push(extra);
    s
}

const DEFAULT_BUNDLE: &str = "default_local_group_all.bundle";
const BOSS_BUNDLE: &str = "extra_stuff_boss.bundle";

#[test]
fn settings_round_trip_and_edits() {
    let mut s = settings();
    let back = AssetGroupSettings::from_json(&s.to_json()).unwrap();
    assert_eq!(back, s);
    assert!(s.set_addressable("Assets/x.png", true, None));
    assert!(!s.set_addressable("Assets/x.png", true, None), "already on");
    assert_eq!(s.entry("Assets/x.png").unwrap().0, DEFAULT_GROUP);
    assert_eq!(s.entry("Assets/x.png").unwrap().1.address, "Assets/x.png");
    assert!(s.move_entry("Assets/x.png", "Extra Stuff"));
    assert_eq!(s.entry("Assets/x.png").unwrap().0, "Extra Stuff");
    let g = s.add_group();
    assert_eq!(g, "New Group");
    assert_eq!(s.add_group(), "New Group 2");
    assert!(s.remove_group("Extra Stuff"));
    assert_eq!(
        s.entry("Assets/x.png").unwrap().0,
        DEFAULT_GROUP,
        "entries move home"
    );
    assert!(!s.remove_group(DEFAULT_GROUP));
    assert_eq!(
        s.labels().into_iter().collect::<Vec<_>>(),
        ["boss", "enemy"]
    );
    assert_eq!(s.resolve("enemy").len(), 2);
    assert_eq!(s.resolve("Boss"), ["Assets/Prefabs/C.prefab"]);
    assert!(s.set_addressable("Assets/x.png", false, None));
    assert!(s.entry("Assets/x.png").is_none());
    // A file without the default group gets it back.
    let s = AssetGroupSettings::from_json(r#"{"groups":[{"name":"Mine"}]}"#).unwrap();
    assert_eq!(s.groups[0].name, DEFAULT_GROUP);
}

#[test]
fn catalog_has_addresses_labels_and_dependencies() {
    let root = project("catalog");
    let p = plan(&root, &settings(), "windows-x64");
    let c = &p.catalog;
    assert!(p.missing.is_empty(), "{:?}", p.missing);
    assert_eq!(c.platform, "windows-x64");
    let names: Vec<&str> = c.bundles.iter().map(|b| b.name.as_str()).collect();
    assert_eq!(names, [DEFAULT_BUNDLE, BOSS_BUNDLE]);
    let default = c.bundle(DEFAULT_BUNDLE).unwrap();
    assert_eq!(
        default.assets,
        [
            "Assets/Mats/Shared.mat",
            "Assets/Prefabs/A.prefab",
            "Assets/Prefabs/B.prefab"
        ]
    );
    // C references A, which lives in the default bundle: a bundle dependency, not a copy.
    let boss = c.bundle(BOSS_BUNDLE).unwrap();
    assert_eq!(boss.assets, ["Assets/Prefabs/C.prefab"]);
    assert_eq!(boss.dependencies, [DEFAULT_BUNDLE]);
    assert_eq!(boss.compression, BundleCompression::Zlib);
    let a = &c.resolve("Enemy/A")[0];
    assert_eq!(a.bundle, DEFAULT_BUNDLE);
    assert_eq!(a.dependencies, ["Assets/Mats/Shared.mat"]);
    let cc = &c.resolve("boss")[0];
    assert_eq!(
        cc.dependencies,
        ["Assets/Mats/Shared.mat", "Assets/Prefabs/A.prefab"]
    );
    assert_eq!(c.resolve("enemy").len(), 2);
    assert!(c.resolve("nothing").is_empty());
    assert_eq!(c.load_order(BOSS_BUNDLE), [DEFAULT_BUNDLE, BOSS_BUNDLE]);
    assert_eq!(slug("Default Local Group"), "default_local_group");
    assert_eq!(slug("  !!"), "bundle");

    // By label: one bundle per first label.
    let mut s = settings();
    s.groups[0].bundle_mode = BundleMode::PackByLabel;
    s.groups[0].entries[1].labels.clear();
    let c = plan(&root, &s, "x").catalog;
    let names: Vec<&str> = c.bundles.iter().map(|b| b.name.as_str()).collect();
    assert!(
        names.contains(&"default_local_group_enemy.bundle"),
        "{names:?}"
    );
    assert!(
        names.contains(&"default_local_group_unlabeled.bundle"),
        "{names:?}"
    );

    // Excluded groups are left out.
    let mut s = settings();
    s.groups[1].include_in_build = false;
    assert_eq!(plan(&root, &s, "x").catalog.bundles.len(), 1);
}

#[test]
fn analyze_finds_files_shared_by_bundles() {
    let root = project("analyze");
    let s = settings();
    assert!(analyze(&root, &s).is_empty(), "packed together: no copies");
    let mut s = settings();
    s.groups[0].bundle_mode = BundleMode::PackSeparately;
    let dups = analyze(&root, &s);
    assert_eq!(dups.len(), 1, "{dups:?}");
    assert_eq!(dups[0].path, "Assets/Mats/Shared.mat");
    assert_eq!(
        dups[0].bundles,
        [
            "default_local_group_enemy_a.bundle",
            "default_local_group_enemy_b.bundle"
        ]
    );
}

fn build(root: &Path, s: &AssetGroupSettings) -> (PathBuf, build::BundleBuild) {
    let out = build::build_dir(root, "windows-x64");
    let b = build_bundles(root, s, "windows-x64", &out).unwrap();
    (out, b)
}

#[test]
fn bundle_build_round_trips() {
    let root = project("build");
    let (out, b) = build(&root, &settings());
    assert!(b.warnings.is_empty(), "{:?}", b.warnings);
    let on_disk = Catalog::load(&out.join(CATALOG_FILE)).unwrap();
    assert_eq!(on_disk, b.catalog);
    for bundle in &on_disk.bundles {
        let path = out.join(&bundle.name);
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(sha256_hex(&bytes), bundle.hash);
        assert_eq!(bytes.len() as u64, bundle.size);
        let pak = crate::pak::Pak::open(&path).unwrap();
        for a in &bundle.assets {
            assert_eq!(pak.read(a).unwrap(), std::fs::read(root.join(a)).unwrap());
        }
    }
    let pak = crate::pak::Pak::open(&out.join(DEFAULT_BUNDLE)).unwrap();
    assert_eq!(
        pak.entry("Assets/Prefabs/A.prefab").unwrap().codec,
        crate::pak::Codec::Lz4
    );
    // A second build replaces the old bundles.
    let mut s = settings();
    s.groups[1].include_in_build = false;
    build(&root, &s);
    assert!(!out.join(BOSS_BUNDLE).exists());
    // Player copy: the catalog and the local bundles.
    let dest = temp("player").join("Bundles");
    assert_eq!(build::copy_local(&out, &dest).unwrap(), 2);
    assert!(dest.join(CATALOG_FILE).is_file() && dest.join(DEFAULT_BUNDLE).is_file());
}

fn built_system(root: &Path, s: &AssetGroupSettings) -> AssetSystem {
    let (out, b) = build(root, s);
    AssetSystem::new(
        root,
        Provider::Bundles {
            catalog: b.catalog,
            local_dir: out,
            cache_dir: root.join("Library/BundleCache"),
        },
    )
}

fn pump(sys: &mut AssetSystem, id: u64) -> Status {
    for _ in 0..500 {
        sys.update();
        let st = sys.handle(id).unwrap().status;
        if st != Status::Loading {
            return st;
        }
        std::thread::sleep(std::time::Duration::from_millis(5));
    }
    Status::Loading
}

#[test]
fn load_and_release_count_references_and_unload_bundles() {
    let root = project("refs");
    let mut sys = built_system(&root, &settings());
    // Only the bundle has B now: reads succeed only while it is mounted.
    let b_path = root.join("Assets/Prefabs/B.prefab");
    let original = std::fs::read(&b_path).unwrap();
    std::fs::remove_file(&b_path).unwrap();
    assert!(crate::vfs::read(&b_path).is_err());

    let h1 = sys.load("Enemy/B", HandleKind::Load);
    assert_eq!(sys.handle(h1).unwrap().status, Status::Loading);
    assert_eq!(pump(&mut sys, h1), Status::Succeeded);
    assert_eq!(sys.handle(h1).unwrap().paths, ["Assets/Prefabs/B.prefab"]);
    assert_eq!(crate::vfs::read(&b_path).unwrap(), original);
    let h2 = sys.load("enemy", HandleKind::Load);
    assert_eq!(pump(&mut sys, h2), Status::Succeeded);
    assert_eq!(sys.handle(h2).unwrap().paths.len(), 2);
    assert_eq!(sys.bundle_refs(DEFAULT_BUNDLE), 2);
    assert_eq!(sys.stats().bundles_loaded, 1, "loaded once, shared");

    sys.release(h1);
    assert_eq!(sys.bundle_refs(DEFAULT_BUNDLE), 1);
    assert!(sys.bundle_loaded(DEFAULT_BUNDLE));
    assert!(crate::vfs::read(&b_path).is_ok());
    sys.release(h2);
    assert_eq!(sys.bundle_refs(DEFAULT_BUNDLE), 0);
    assert!(!sys.bundle_loaded(DEFAULT_BUNDLE));
    assert!(crate::vfs::read(&b_path).is_err(), "unmounted");
    let st = sys.stats();
    assert_eq!((st.loaded, st.released, st.live_handles), (2, 2, 0));
    assert_eq!((st.bundles_loaded, st.bundles_unloaded), (1, 1));
    assert!(sys.release(h2).is_none(), "released once");

    // Unknown keys fail on the next update.
    let bad = sys.load("Nope", HandleKind::Load);
    assert_eq!(pump(&mut sys, bad), Status::Failed);
    assert!(
        sys.handle(bad)
            .unwrap()
            .error
            .as_ref()
            .unwrap()
            .contains("Nope")
    );
}

#[test]
fn dependency_bundles_load_first() {
    let root = project("order");
    let mut sys = built_system(&root, &settings());
    let h = sys.load("Boss", HandleKind::Load);
    assert_eq!(
        sys.handle(h).unwrap().bundles,
        [DEFAULT_BUNDLE, BOSS_BUNDLE]
    );
    assert_eq!(pump(&mut sys, h), Status::Succeeded);
    assert_eq!(sys.load_log, [DEFAULT_BUNDLE, BOSS_BUNDLE]);
    assert_eq!(sys.bundle_refs(DEFAULT_BUNDLE), 1);
    sys.release(h);
    assert_eq!(sys.stats().live_bundles, 0);
}

#[test]
fn database_and_simulated_providers() {
    let root = project("modes");
    let s = settings();
    let mut db = AssetSystem::new(&root, Provider::Database(s.clone()));
    let h = db.load("enemy", HandleKind::Load);
    assert_eq!(pump(&mut db, h), Status::Succeeded);
    assert_eq!(db.handle(h).unwrap().paths.len(), 2);
    assert!(db.handle(h).unwrap().bundles.is_empty());

    let mut sim = AssetSystem::new(
        &root,
        Provider::Simulated(plan(&root, &s, "windows-x64").catalog),
    );
    let h = sim.load("Boss", HandleKind::Load);
    assert_eq!(pump(&mut sim, h), Status::Succeeded);
    assert_eq!(sim.load_log, [DEFAULT_BUNDLE, BOSS_BUNDLE]);
    assert_eq!(sim.stats().live_bundles, 2);
    sim.release(h);
    assert_eq!(sim.stats().live_bundles, 0);

    let mut none = AssetSystem::new(&root, Provider::Unavailable("no bundles".into()));
    let h = none.load("Boss", HandleKind::Load);
    assert_eq!(pump(&mut none, h), Status::Failed);

    // Editor modes come from the settings file.
    let mut s2 = s.clone();
    s2.play_mode = PlayModeScript::SimulateGroups;
    s2.save(&root).unwrap();
    assert!(matches!(
        AssetSystem::for_editor(&root, "windows-x64").provider(),
        Provider::Simulated(_)
    ));
    s2.play_mode = PlayModeScript::UseBuiltBundles;
    s2.save(&root).unwrap();
    assert!(matches!(
        AssetSystem::for_editor(&root, "linux-x64").provider(),
        Provider::Unavailable(_)
    ));
    assert!(matches!(
        AssetSystem::auto(&root).provider(),
        Provider::Database(_)
    ));
}

fn file_url(dir: &Path) -> String {
    let p = dir.to_string_lossy().replace('\\', "/");
    if p.starts_with('/') {
        format!("file://{p}")
    } else {
        format!("file:///{p}")
    }
}

fn remote_settings(url: &str) -> AssetGroupSettings {
    let mut s = settings();
    s.groups[1].location = BundleLocation::Remote;
    s.groups[1].remote_url = url.into();
    s
}

#[test]
fn remote_bundles_download_verify_and_cache() {
    let root = project("remote");
    let out = build::build_dir(&root, "windows-x64");
    let url = file_url(&out.join(REMOTE_DIR)).replace("windows-x64", "{Platform}");
    let s = remote_settings(&url);
    let b = build_bundles(&root, &s, "windows-x64", &out).unwrap();
    let boss = b.catalog.bundle(BOSS_BUNDLE).unwrap().clone();
    assert!(
        boss.remote && boss.url.ends_with("windows-x64/Remote"),
        "{}",
        boss.url
    );
    assert!(out.join(REMOTE_DIR).join(BOSS_BUNDLE).is_file());
    assert!(
        !out.join(BOSS_BUNDLE).exists(),
        "remote bundles go to Remote/"
    );

    let cache = temp("cache");
    let mut sys = AssetSystem::new(
        &root,
        Provider::Bundles {
            catalog: b.catalog.clone(),
            local_dir: out.clone(),
            cache_dir: cache.clone(),
        },
    );
    let h = sys.load("Boss", HandleKind::Load);
    assert_eq!(pump(&mut sys, h), Status::Succeeded, "{:?}", sys.handle(h));
    assert_eq!(sys.load_log, [DEFAULT_BUNDLE, BOSS_BUNDLE]);
    let cached = fetch::cache_path(&cache, BOSS_BUNDLE, &boss.hash);
    assert!(cached.is_file());
    sys.release(h);

    // Tampered on the server: the cached copy still matches and is used...
    let served = out.join(REMOTE_DIR).join(BOSS_BUNDLE);
    let mut bytes = std::fs::read(&served).unwrap();
    bytes.extend_from_slice(b"tampered");
    std::fs::write(&served, &bytes).unwrap();
    let h = sys.load("Boss", HandleKind::Load);
    assert_eq!(pump(&mut sys, h), Status::Succeeded);
    sys.release(h);
    // ...but without it the download is rejected.
    std::fs::remove_file(&cached).unwrap();
    let h = sys.load("Boss", HandleKind::Load);
    assert_eq!(pump(&mut sys, h), Status::Failed);
    let err = sys.handle(h).unwrap().error.clone().unwrap();
    assert!(err.contains("hash mismatch"), "{err}");
    assert!(!cached.exists(), "rejected bundles are not cached");
    sys.release(h);
    assert_eq!(sys.stats().live_bundles, 0);
}

/// Serve `body` once per connection over plain HTTP on 127.0.0.1; returns the base URL.
fn serve(body: Vec<u8>, requests: usize) -> String {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming().take(requests) {
            let Ok(mut s) = stream else { continue };
            let mut buf = [0u8; 4096];
            let _ = s.read(&mut buf);
            let head = format!(
                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                body.len()
            );
            let _ = s.write_all(head.as_bytes());
            let _ = s.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}")
}

#[test]
fn http_downloads_are_hash_checked() {
    let dir = temp("http");
    let good = b"bundle bytes".to_vec();
    let hash = sha256_hex(&good);
    let url = serve(good.clone(), 1);
    let path =
        fetch::download_verified(&format!("{url}/x.bundle"), "x.bundle", &hash, &dir).unwrap();
    assert_eq!(std::fs::read(path).unwrap(), good);

    let url = serve(b"evil bytes".to_vec(), 1);
    let other = temp("http2");
    let err = fetch::download_verified(&format!("{url}/x.bundle"), "x.bundle", &hash, &other)
        .unwrap_err();
    assert!(err.contains("hash mismatch"), "{err}");
    assert!(fetch::fetch("ftp://nope").is_err());
}
