//! Updater tests: versions, cache and skip rules, release parsing, asset choice, checksums,
//! extraction / swap / rollback in temp folders, end-to-end runs against a local HTTP server
//! (no real network) and the message box / button UI.

use std::collections::HashMap;
use std::io::{BufRead, BufReader, Write};
use std::net::TcpListener;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::time::{Duration, Instant};

use super::cache::{self, CHECK_INTERVAL_SECS, Cache};
use super::http::{self, Fetched, Source};
use super::install;
use super::release::{self, Asset, Update};
use super::version::{Version, is_newer};
use super::{Phase, Updater};

fn temp(name: &str) -> PathBuf {
    let d = std::env::temp_dir().join(format!("misty_updater_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

// ---- versions ---------------------------------------------------------------------------------

#[test]
fn versions_parse_and_compare() {
    let v = |s| Version::parse(s).unwrap_or_else(|| panic!("{s}"));
    assert_eq!(v("v0.0.2"), v("0.0.2"));
    assert_eq!(v("1"), v("1.0.0"));
    assert_eq!(v("1.2.3+build.7"), v("1.2.3"));
    assert!(v("0.0.10") > v("0.0.9"));
    assert!(v("0.1.0") > v("0.0.99"));
    assert!(v("1.0.0") > v("1.0.0-rc.1"));
    assert!(v("1.0.0-beta.2") < v("1.0.0-beta.11"));
    assert!(v("1.0.0-alpha") < v("1.0.0-alpha.1"));
    assert!(v("1.0.0-1") < v("1.0.0-alpha"));
    for bad in ["", "v", "x.1", "1..2", "1.2.3.4", "1.2.3-", "-1"] {
        assert!(Version::parse(bad).is_none(), "{bad:?} should not parse");
    }
    assert!(is_newer("v0.0.2", "0.0.1"));
    assert!(!is_newer("0.0.1", "0.0.1"));
    assert!(!is_newer("0.0.1", "0.0.2"));
    assert!(!is_newer("nightly", "0.0.1"));
}

// ---- cache and skip ---------------------------------------------------------------------------

fn update(v: &str) -> Update {
    Update {
        version: v.into(),
        notes: String::new(),
        page_url: "https://github.com/a/b/releases/tag/v1".into(),
        zip: None,
        checksum_url: None,
    }
}

#[test]
fn cache_freshness_and_round_trip() {
    let c = Cache {
        last_check: 1_000_000,
        ..Default::default()
    };
    assert!(c.fresh(1_000_000));
    assert!(c.fresh(1_000_000 + CHECK_INTERVAL_SECS - 1));
    assert!(!c.fresh(1_000_000 + CHECK_INTERVAL_SECS));
    assert!(!c.fresh(999_999), "clock set back = stale");
    assert!(!Cache::default().fresh(5), "never checked");

    let path = temp("cache").join("sub").join("updater.json");
    let c = Cache {
        last_check: 42,
        latest: Some(update("0.0.2")),
        skipped: Some("0.0.2".into()),
    };
    c.save(&path).unwrap();
    assert_eq!(Cache::load(&path), c);
    std::fs::write(&path, "{ broken").unwrap();
    assert_eq!(Cache::load(&path), Cache::default());
}

#[test]
fn skip_hides_only_that_version_and_only_automatically() {
    let u = update("99.0.0");
    assert!(cache::offer(&u, "0.0.1", None, false));
    assert!(!cache::offer(&u, "0.0.1", Some("99.0.0"), false));
    assert!(
        cache::offer(&u, "0.0.1", Some("99.0.0"), true),
        "manual check"
    );
    assert!(cache::offer(
        &update("99.0.1"),
        "0.0.1",
        Some("99.0.0"),
        false
    ));
    assert!(!cache::offer(&u, "99.0.0", None, true), "already current");
}

#[test]
fn skip_is_saved_and_survives_a_restart() {
    let path = temp("skip").join("updater.json");
    Cache {
        last_check: cache::now(),
        latest: Some(update("99.0.0")),
        skipped: None,
    }
    .save(&path)
    .unwrap();
    // A fresh cache answers without any request (the source is unreachable anyway).
    let src = Source::custom("http://127.0.0.1:9/none.json");
    let mut up = Updater::with(src.clone(), Some(path.clone()), None);
    up.check(false);
    assert_eq!(up.available().map(|u| u.version), Some("99.0.0".into()));
    up.skip();
    assert!(up.available().is_none());
    assert_eq!(Cache::load(&path).skipped.as_deref(), Some("99.0.0"));
    let mut again = Updater::with(src, Some(path), None);
    again.check(false);
    assert!(again.available().is_none());
}

// ---- release JSON and assets ------------------------------------------------------------------

const RELEASE_JSON: &str = r#"{
  "tag_name": "v0.0.2",
  "name": "Misty Engine 0.0.2",
  "html_url": "https://github.com/Aalzard/MistyEngine/releases/tag/v0.0.2",
  "draft": false,
  "prerelease": false,
  "body": "Fixes\r\n- one\r\n- two",
  "assets": [
    {"name": "MistyEngine-v0.0.2-windows-x64.zip.sha256.txt", "size": 100,
     "browser_download_url": "https://github.com/Aalzard/MistyEngine/releases/download/v0.0.2/MistyEngine-v0.0.2-windows-x64.zip.sha256.txt"},
    {"name": "Source.tar.gz", "size": 5, "browser_download_url": "https://github.com/x/y"},
    {"name": "MistyEngine-v0.0.2-windows-x64.zip", "size": 1234,
     "browser_download_url": "https://github.com/Aalzard/MistyEngine/releases/download/v0.0.2/MistyEngine-v0.0.2-windows-x64.zip",
     "content_type": "application/zip", "download_count": 3}
  ],
  "author": {"login": "someone"}
}"#;

#[test]
fn release_json_parses_into_an_update() {
    let r = release::parse(RELEASE_JSON).unwrap();
    let u = release::to_update(&r, "https://fallback").unwrap();
    assert_eq!(u.version, "0.0.2");
    assert_eq!(u.notes, "Fixes\n- one\n- two");
    assert_eq!(
        u.page_url,
        "https://github.com/Aalzard/MistyEngine/releases/tag/v0.0.2"
    );
    let zip = u.zip.unwrap();
    assert_eq!(zip.name, "MistyEngine-v0.0.2-windows-x64.zip");
    assert_eq!(zip.size, 1234);
    assert!(u.checksum_url.unwrap().ends_with(".zip.sha256.txt"));

    // Drafts, pre-releases and odd tags offer nothing; a minimal JSON still parses.
    let mut d = r.clone();
    d.prerelease = true;
    assert!(release::to_update(&d, "").is_none());
    let mut d = r.clone();
    d.tag_name = "latest".into();
    assert!(release::to_update(&d, "").is_none());
    let min = release::parse(r#"{"tag_name":"v1.0.0"}"#).unwrap();
    let u = release::to_update(&min, "https://fallback").unwrap();
    assert_eq!((u.page_url.as_str(), u.zip), ("https://fallback", None));
    assert!(release::parse("not json").is_err());
}

#[test]
fn asset_selection() {
    let a = |n: &str| Asset {
        name: n.into(),
        browser_download_url: format!("https://github.com/{n}"),
        size: 0,
    };
    let assets = vec![
        a("MistyEngine-v0.0.2-linux-x64.zip"),
        a("MistyEngine-0.0.2-windows-x64.zip"),
        a("MistyEngine-v0.0.2-windows-x64.zip"),
        a("mistyengine-v0.0.2-windows-x64.ZIP.sha256.txt"),
    ];
    let z = release::select_zip(&assets, "0.0.2").unwrap();
    assert_eq!(
        z.name, "MistyEngine-v0.0.2-windows-x64.zip",
        "exact name first"
    );
    let sum = release::select_checksum(&assets, &z.name).unwrap();
    assert_eq!(sum.name, "mistyengine-v0.0.2-windows-x64.ZIP.sha256.txt");
    // Pattern fallback when the tag and file name differ; none without a Windows zip.
    let z = release::select_zip(&assets[..2], "0.0.3").unwrap();
    assert_eq!(z.name, "MistyEngine-0.0.2-windows-x64.zip");
    assert!(release::select_checksum(&assets[..2], &z.name).is_none());
    assert!(release::select_zip(&assets[..1], "0.0.2").is_none());
}

// ---- source and URLs --------------------------------------------------------------------------

#[test]
fn sources_and_allowed_urls() {
    assert!(
        Source::github("OWNER/MistyEngine").is_none(),
        "placeholder = off"
    );
    assert!(Source::github("").is_none());
    assert!(Source::github("no-slash").is_none());
    assert!(Source::github("a/b/c").is_none());
    let gh = Source::github(super::REPO).unwrap();
    assert_eq!(
        gh.api_url,
        "https://api.github.com/repos/Aalzard/MistyEngine/releases/latest"
    );
    assert_eq!(
        gh.page_url,
        "https://github.com/Aalzard/MistyEngine/releases"
    );
    for ok in [
        "https://github.com/Aalzard/MistyEngine/releases/download/v1/x.zip",
        "https://objects.githubusercontent.com/abc?sig=1",
        "https://release-assets.githubusercontent.com/abc",
        "HTTPS://API.GITHUB.COM/repos",
    ] {
        assert!(gh.check_url(ok).is_ok(), "{ok}");
    }
    for bad in [
        "http://github.com/x",
        "https://evil.com/github.com",
        "https://github.com.evil.com/x",
        "https://github.com@evil.com/",
        "ftp://github.com/x",
        "file:///C:/x.zip",
        "http://127.0.0.1:1/x",
    ] {
        assert!(gh.check_url(bad).is_err(), "{bad}");
    }
    let local = Source::custom("http://127.0.0.1:5555/release.json").unwrap();
    assert!(local.check_url("http://127.0.0.1:5555/pkg.zip").is_ok());
    assert!(local.check_url("http://evil.com/pkg.zip").is_err());
    let remote = Source::custom("https://example.com/r.json").unwrap();
    assert!(
        remote.check_url("http://example.com/pkg.zip").is_err(),
        "https only"
    );
    assert_eq!(http::host("https://u:p@h.com:8080/x?y"), Some("h.com"));
    assert_eq!(http::host("http://[::1]:80/"), Some("::1"));
}

// ---- checksums, extraction, swap and rollback -------------------------------------------------

#[test]
fn checksum_parsing_and_mismatch_refusal() {
    let h = "AB".repeat(32);
    assert_eq!(install::parse_checksum(&h), Some("ab".repeat(32)));
    assert_eq!(
        install::parse_checksum(&format!("{h}  MistyEngine-v1-windows-x64.zip\n")),
        Some("ab".repeat(32))
    );
    assert_eq!(install::parse_checksum("SHA256 hash of file:\nnope"), None);
    assert!(install::verify(&"ab".repeat(32), &h).is_ok());
    let err = install::verify(&"cd".repeat(32), &h).unwrap_err();
    assert!(err.contains("checksum"), "{err}");
}

/// A deflate zip of (path, contents).
fn make_zip(files: &[(&str, &[u8])]) -> Vec<u8> {
    let mut w = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    for (name, data) in files {
        w.start_file(*name, opts).unwrap();
        w.write_all(data).unwrap();
    }
    w.finish().unwrap().into_inner()
}

fn sha256(bytes: &[u8]) -> String {
    use sha2::Digest;
    install::hex(&sha2::Sha256::digest(bytes))
}

/// An install folder with an old exe, a DLL, an edited example and a user file.
fn old_install(dir: &Path) {
    std::fs::create_dir_all(dir.join("Examples/Demo")).unwrap();
    std::fs::create_dir_all(dir.join("data")).unwrap();
    std::fs::write(dir.join("MistyEngine.exe"), b"old exe").unwrap();
    std::fs::write(dir.join("data/engine.dll"), b"old dll").unwrap();
    std::fs::write(dir.join("Examples/Demo/scene.scene"), b"user edit").unwrap();
    std::fs::write(dir.join("my_notes.txt"), b"mine").unwrap();
}

const PACKAGE: &[(&str, &[u8])] = &[
    ("MistyEngine-v0.0.2-windows-x64/MistyEngine.exe", b"new exe"),
    ("MistyEngine-v0.0.2-windows-x64/data/engine.dll", b"new dll"),
    (
        "MistyEngine-v0.0.2-windows-x64/data/new/shader.wgsl",
        b"new shader",
    ),
    (
        "MistyEngine-v0.0.2-windows-x64/Examples/Demo/scene.scene",
        b"stock",
    ),
    (
        "MistyEngine-v0.0.2-windows-x64/Examples/Demo/extra.png",
        b"added",
    ),
];

fn read(p: PathBuf) -> String {
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()))
}

/// The install after a successful update from [`PACKAGE`].
fn assert_updated(inst: &Path) {
    assert_eq!(read(inst.join("MistyEngine.exe")), "new exe");
    assert_eq!(read(inst.join("MistyEngine.old.exe")), "old exe");
    assert_eq!(read(inst.join("data/engine.dll")), "new dll");
    assert_eq!(read(inst.join("data/new/shader.wgsl")), "new shader");
    assert_eq!(
        read(inst.join("Examples/Demo/scene.scene")),
        "user edit",
        "kept"
    );
    assert_eq!(read(inst.join("Examples/Demo/extra.png")), "added");
    assert_eq!(
        read(inst.join("my_notes.txt")),
        "mine",
        "not in the package"
    );
    assert!(!inst.join(install::BACKUP_DIR).exists());
}

/// The install untouched.
fn assert_unchanged(inst: &Path) {
    assert_eq!(read(inst.join("MistyEngine.exe")), "old exe");
    assert_eq!(read(inst.join("data/engine.dll")), "old dll");
    assert!(!inst.join("data/new").exists(), "no new folders");
    assert!(!inst.join("Examples/Demo/extra.png").exists());
    assert!(!inst.join("MistyEngine.old.exe").exists());
    assert!(!inst.join(install::BACKUP_DIR).exists());
}

#[test]
fn extract_swap_and_cleanup() {
    let root = temp("swap");
    let inst = root.join("install");
    old_install(&inst);
    std::fs::write(root.join("p.zip"), make_zip(PACKAGE)).unwrap();
    let pkg = install::extract(&root.join("p.zip"), &root.join("work")).unwrap();
    assert_eq!(pkg.file_name().unwrap(), "MistyEngine-v0.0.2-windows-x64");

    let exe = install::apply(&pkg, &inst, &inst.join("MistyEngine.exe")).unwrap();
    assert_eq!(exe, inst.join("MistyEngine.exe"));
    assert_updated(&inst);

    assert!(install::cleanup_old(&inst));
    assert!(!inst.join("MistyEngine.old.exe").exists());
    assert!(inst.join("MistyEngine.exe").exists());
}

#[test]
fn failed_swap_rolls_back() {
    let root = temp("rollback");
    let inst = root.join("install");
    old_install(&inst);
    std::fs::write(root.join("p.zip"), make_zip(PACKAGE)).unwrap();
    let pkg = install::extract(&root.join("p.zip"), &root.join("work")).unwrap();
    // Fail on the last file (the exe), after everything else was swapped.
    let err =
        install::apply_inner(&pkg, &inst, &inst.join("MistyEngine.exe"), Some(4)).unwrap_err();
    assert!(err.contains("rolled back"), "{err}");
    assert_unchanged(&inst);

    // A package without the exe is refused before touching anything.
    let empty = root.join("empty");
    std::fs::create_dir_all(&empty).unwrap();
    std::fs::write(empty.join("readme.txt"), b"x").unwrap();
    let err = install::apply(&empty, &inst, &inst.join("MistyEngine.exe")).unwrap_err();
    assert!(err.contains("MistyEngine.exe"), "{err}");
    assert!(!inst.join("readme.txt").exists());
}

#[test]
fn zip_slip_is_refused() {
    let root = temp("slip");
    std::fs::write(root.join("bad.zip"), make_zip(&[("../evil.txt", b"x")])).unwrap();
    assert!(install::extract(&root.join("bad.zip"), &root.join("out")).is_err());
    assert!(!root.join("evil.txt").exists());
}

// ---- local HTTP server (no real network) ------------------------------------------------------

type Routes = HashMap<String, (u16, String, Vec<u8>)>;

struct Server {
    base: String,
    hits: Arc<AtomicUsize>,
}

/// Serve on 127.0.0.1; `routes` gets the server's base URL (path → status, headers, body).
fn serve(routes: impl FnOnce(&str) -> Routes) -> Server {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let base = format!("http://{}", listener.local_addr().unwrap());
    let routes = routes(&base);
    let hits = Arc::new(AtomicUsize::new(0));
    let counter = hits.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            counter.fetch_add(1, Ordering::SeqCst);
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let mut line = String::new();
            let _ = reader.read_line(&mut line);
            let path = line.split_whitespace().nth(1).unwrap_or("/").to_owned();
            loop {
                let mut h = String::new();
                if reader.read_line(&mut h).unwrap_or(0) == 0 || h == "\r\n" {
                    break;
                }
            }
            let (status, headers, body) = routes.get(&path).cloned().unwrap_or((
                404,
                String::new(),
                b"{\"message\":\"Not Found\"}".to_vec(),
            ));
            let head = format!(
                "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n{headers}\r\n",
                body.len()
            );
            let _ = stream.write_all(head.as_bytes());
            let _ = stream.write_all(&body);
        }
    });
    Server { base, hits }
}

/// A fake latest release (v99.0.0) whose package is `zip` and whose checksum file says `sum`.
fn release_routes(base: &str, zip: Vec<u8>, sum: String) -> Routes {
    let json = format!(
        r#"{{"tag_name":"v99.0.0","body":"Big update","html_url":"{base}/page",
            "assets":[
              {{"name":"MistyEngine-v99.0.0-windows-x64.zip","size":{n},"browser_download_url":"{base}/dl/pkg.zip"}},
              {{"name":"MistyEngine-v99.0.0-windows-x64.zip.sha256.txt","size":64,"browser_download_url":"{base}/dl/pkg.sha"}}
            ]}}"#,
        n = zip.len()
    );
    let mut r = Routes::new();
    r.insert(
        "/release.json".into(),
        (200, String::new(), json.into_bytes()),
    );
    // The package redirects once, like GitHub's download link to its asset host.
    r.insert(
        "/dl/pkg.zip".into(),
        (302, "Location: /assets/pkg.zip\r\n".into(), Vec::new()),
    );
    r.insert("/assets/pkg.zip".into(), (200, String::new(), zip));
    r.insert("/dl/pkg.sha".into(), (200, String::new(), sum.into_bytes()));
    r
}

fn fetch_update(src: &Source) -> Update {
    match http::fetch_release(src).unwrap() {
        Fetched::Release(r) => release::to_update(&r, &src.page_url).unwrap(),
        Fetched::NoRelease => panic!("no release"),
    }
}

#[test]
fn end_to_end_download_verify_install() {
    let zip = make_zip(PACKAGE);
    let sum = format!("{}  MistyEngine-v99.0.0-windows-x64.zip\n", sha256(&zip));
    let server = serve(|b| release_routes(b, zip, sum));
    let src = Source::custom(&format!("{}/release.json", server.base)).unwrap();
    let update = fetch_update(&src);
    assert_eq!(update.version, "99.0.0");
    assert_eq!(update.notes, "Big update");

    let root = temp("e2e");
    let inst = root.join("install");
    old_install(&inst);
    let mut phases = Vec::new();
    let out = install::run(
        &src,
        &update,
        &inst,
        &inst.join("MistyEngine.exe"),
        &root.join("work"),
        |p| phases.push(p),
    )
    .unwrap();
    let install::Outcome::Installed(exe) = out else {
        panic!("expected an install")
    };
    assert_eq!(exe, inst.join("MistyEngine.exe"));
    assert_updated(&inst);
    assert!(!root.join("work").exists(), "staging removed");
    assert!(matches!(phases.first(), Some(Phase::Downloading { .. })));
    assert!(phases.contains(&Phase::Verifying) && phases.ends_with(&[Phase::Installing]));
    let Some(Phase::Downloading { received, total }) = phases
        .iter()
        .rev()
        .find(|p| matches!(p, Phase::Downloading { .. }))
    else {
        unreachable!()
    };
    assert_eq!(Some(*received), *total);
}

#[test]
fn end_to_end_checksum_mismatch_changes_nothing() {
    let zip = make_zip(PACKAGE);
    let server = serve(|b| release_routes(b, zip, "00".repeat(32)));
    let src = Source::custom(&format!("{}/release.json", server.base)).unwrap();
    let update = fetch_update(&src);
    let root = temp("e2e_bad");
    let inst = root.join("install");
    old_install(&inst);
    let err = install::run(
        &src,
        &update,
        &inst,
        &inst.join("MistyEngine.exe"),
        &root.join("work"),
        |_| {},
    )
    .err()
    .unwrap();
    assert!(err.contains("checksum"), "{err}");
    assert_unchanged(&inst);
}

#[test]
fn redirect_to_another_host_is_refused() {
    let server = serve(|_| {
        let mut r = Routes::new();
        r.insert(
            "/release.json".into(),
            (
                302,
                "Location: https://evil.example/x.json\r\n".into(),
                Vec::new(),
            ),
        );
        r
    });
    let src = Source::custom(&format!("{}/release.json", server.base)).unwrap();
    let err = http::fetch_release(&src).unwrap_err();
    assert!(err.contains("refusing"), "{err}");
}

/// Wait for a background check to finish.
fn wait_idle(up: &Updater) {
    let start = Instant::now();
    while up.phase() == Phase::Checking {
        assert!(start.elapsed() < Duration::from_secs(20), "check timed out");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn background_check_uses_the_cache_and_treats_404_as_no_update() {
    let zip = make_zip(PACKAGE);
    let server = serve(|b| release_routes(b, zip, String::new()));
    let src = Source::custom(&format!("{}/release.json", server.base));
    let cache_path = temp("check").join("updater.json");

    let mut up = Updater::with(src.clone(), Some(cache_path.clone()), None);
    up.check(false);
    wait_idle(&up);
    assert_eq!(up.available().map(|u| u.version), Some("99.0.0".into()));
    assert!(up.take_announce(), "a new update opens the message box");
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    let saved = Cache::load(&cache_path);
    assert!(saved.fresh(cache::now()));

    // Within 6 hours a restart reuses the result without a request; a manual check asks again.
    let mut again = Updater::with(src.clone(), Some(cache_path.clone()), None);
    again.check(false);
    assert!(again.available().is_some());
    assert_eq!(server.hits.load(Ordering::SeqCst), 1);
    again.check(true);
    wait_idle(&again);
    assert_eq!(server.hits.load(Ordering::SeqCst), 2);

    // No release yet (404): no update, no error, and a manual check says "latest".
    let none = Source::custom(&format!("{}/missing.json", server.base));
    let path = temp("check404").join("updater.json");
    let mut up = Updater::with(none, Some(path), None);
    up.check(true);
    wait_idle(&up);
    assert!(up.available().is_none());
    assert!(up.notice().unwrap().contains("latest version"));
}

#[test]
fn offline_check_is_silent() {
    // Nothing listens on the port: the automatic check fails quietly.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let src = Source::custom(&format!("http://127.0.0.1:{port}/r.json"));
    let mut up = Updater::with(src, Some(temp("offline").join("updater.json")), None);
    up.check(false);
    wait_idle(&up);
    assert!(up.available().is_none());
    assert!(up.notice().is_none());
    assert_eq!(up.phase(), Phase::Idle);
}

#[test]
fn read_only_install_offers_the_release_page() {
    let src = Source::github(super::REPO).unwrap();
    let missing = temp("ro").join("does-not-exist");
    let out = install::run(
        &src,
        &update("99.0.0"),
        &missing,
        &missing.join("MistyEngine.exe"),
        &temp("ro_work"),
        |_| {},
    )
    .ok()
    .unwrap();
    assert!(matches!(out, install::Outcome::Manual(why) if why.contains("can't change")));
}

// ---- UI ----------------------------------------------------------------------------------------

#[test]
fn message_box_and_green_button() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    let mut up = Updater::disabled();
    up.set_fake("0.0.2");
    let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
        |ui, up: &mut Updater| {
            ui.horizontal(|ui| {
                ui.label("toolbar");
                if super::ui::update_button(ui, up) {
                    up.show_box = true;
                }
            });
            super::ui::windows(ui.ctx(), up);
        },
        up,
    );
    h.run();
    assert!(h.query_by_label("⬆ Update").is_some(), "green button");
    assert!(h.query_by_label("New version 0.0.2 is available").is_some());
    assert!(h.query_by_label("Update now").is_some());

    // Later: the box closes, the button stays.
    h.get_by_label("Later").click();
    h.run();
    assert!(h.query_by_label("New version 0.0.2 is available").is_none());
    assert!(h.query_by_label("⬆ Update").is_some());

    // Skip this version: both go.
    h.get_by_label("⬆ Update").click();
    h.run();
    h.get_by_label("Skip this version").click();
    h.run();
    assert!(h.query_by_label("New version 0.0.2 is available").is_none());
    assert!(h.query_by_label("⬆ Update").is_none());
}

#[test]
fn update_now_reports_install() {
    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    let mut up = Updater::disabled();
    up.set_fake("0.0.2");
    let mut h = Harness::builder().with_size([900.0, 600.0]).build_ui_state(
        |ui, up: &mut Updater| {
            if super::ui::windows(ui.ctx(), up) {
                up.install();
            }
        },
        up,
    );
    h.run();
    h.get_by_label("Update now").click();
    h.run();
    // A simulated update can't install: the fallback offers the release page.
    assert!(matches!(h.state().phase(), Phase::Manual(_)));
    assert!(h.query_by_label("Open release page").is_some());
    h.get_by_label("Close").click();
    h.run();
    assert_eq!(h.state().phase(), Phase::Idle);
}
