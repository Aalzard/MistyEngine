//! Package tests: package.json, manifest + lock, installs from folders / zips / a registry,
//! removal, samples and dependency conflicts.

use std::io::Write;
use std::path::{Path, PathBuf};

use super::manifest::{Author, Lock, Manifest, PackageJson, Source, valid_name};
use super::registry::RegistryIndex;
use super::*;

fn temp(tag: &str) -> PathBuf {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let dir = std::env::temp_dir().join(format!("misty_pkg_{tag}_{n}"));
    std::fs::create_dir_all(dir.join("Assets")).unwrap();
    dir
}

/// A package folder named `name` at `version` depending on `deps`, with a script and a sample.
fn package(at: &Path, name: &str, version: &str, deps: &[(&str, &str)]) -> PathBuf {
    let dir = at.join(name);
    std::fs::create_dir_all(dir.join("Runtime")).unwrap();
    std::fs::create_dir_all(dir.join("Samples~/Demo")).unwrap();
    let deps: Vec<String> = deps
        .iter()
        .map(|(n, r)| format!("\"{n}\": \"{r}\""))
        .collect();
    std::fs::write(
        dir.join("package.json"),
        format!(
            r#"{{ "name": "{name}", "displayName": "Pkg {name}", "version": "{version}",
                 "description": "test", "author": {{ "name": "Me", "email": "me@x" }},
                 "license": "MIT", "dependencies": {{ {} }} }}"#,
            deps.join(", ")
        ),
    )
    .unwrap();
    std::fs::write(dir.join("Runtime/Util.lua"), "return {}").unwrap();
    std::fs::write(dir.join("Samples~/Demo/Readme.txt"), "demo").unwrap();
    dir
}

fn zip_folder(src: &Path, zip: &Path, top: &str) {
    let file = std::fs::File::create(zip).unwrap();
    let mut w = zip::ZipWriter::new(file);
    let opts = zip::write::SimpleFileOptions::default()
        .compression_method(zip::CompressionMethod::Deflated);
    let mut stack = vec![src.to_path_buf()];
    while let Some(d) = stack.pop() {
        for e in std::fs::read_dir(&d).unwrap().flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
                continue;
            }
            let rel = p
                .strip_prefix(src)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/");
            w.start_file(format!("{top}{rel}"), opts).unwrap();
            w.write_all(&std::fs::read(&p).unwrap()).unwrap();
        }
    }
    w.finish().unwrap();
}

#[test]
fn package_json_parses_and_validates() {
    let p = PackageJson::parse(
        r#"{"name":"com.misty.tools","displayName":"Tools","version":"1.2.0",
            "description":"d","dependencies":{"com.misty.base":"^1.0.0"},
            "author":"Misty","license":"MIT",
            "samples":[{"displayName":"Demo","description":"x","path":"Samples~/Demo"}]}"#,
    )
    .unwrap();
    assert_eq!(p.title(), "Tools");
    assert_eq!(p.version().to_string(), "1.2.0");
    assert_eq!(p.author, Author::Name("Misty".into()));
    assert_eq!(p.samples[0].path, "Samples~/Demo");
    let back = PackageJson::parse(&serde_json::to_string(&p).unwrap()).unwrap();
    assert_eq!(back, p);
    assert!(PackageJson::parse(r#"{"name":"Bad Name","version":"1.0.0"}"#).is_err());
    assert!(PackageJson::parse(r#"{"name":"com.a.b","version":"1.0"}"#).is_err());
    assert!(
        PackageJson::parse(r#"{"name":"com.a.b","version":"1.0.0","dependencies":{"x.y":"^^"}}"#)
            .is_err()
    );
    assert!(valid_name("com.misty.example-tools") && !valid_name("tools") && !valid_name("a..b"));
}

#[test]
fn manifest_and_lock_round_trip() {
    let root = temp("manifest");
    assert_eq!(Manifest::load(&root).unwrap(), Manifest::default());
    let mut m = Manifest::default();
    m.dependencies.insert("com.a.b".into(), "^1.0.0".into());
    m.dependencies
        .insert("com.c.d".into(), "file:com.c.d".into());
    m.dependencies
        .insert("com.e.f".into(), "git+https://x/e.git#v2".into());
    m.registry = "https://registry.example/index.json".into();
    m.save(&root).unwrap();
    assert_eq!(Manifest::load(&root).unwrap(), m);
    assert_eq!(m.source("com.a.b"), Some(Source::Registry("^1.0.0".into())));
    assert_eq!(m.source("com.c.d"), Some(Source::Local("com.c.d".into())));
    assert_eq!(
        m.source("com.e.f"),
        Some(Source::Git("https://x/e.git".into(), Some("v2".into())))
    );
    let mut lock = Lock::default();
    lock.dependencies.insert(
        "com.a.b".into(),
        manifest::LockEntry {
            version: "1.2.0".into(),
            source: "registry".into(),
            resolved: "https://x/a.zip".into(),
            hash: "abc".into(),
            indirect: true,
            dependencies: [("com.c.d".to_owned(), "*".to_owned())].into(),
        },
    );
    lock.save(&root).unwrap();
    assert_eq!(Lock::load(&root).unwrap(), lock);
}

#[test]
fn install_from_folder_and_zip_then_remove() {
    let root = temp("install");
    let src = temp("src");
    let base = package(&src, "com.test.base", "1.0.0", &[]);
    let fx = package(&src, "com.test.fx", "0.3.0", &[("com.test.base", "^1.0.0")]);

    let mut w = PackagesWindow::default();
    w.refresh(&root);
    assert!(w.installed.is_empty());
    w.install_folder(&root, &base);
    assert_eq!(
        w.status.as_ref().unwrap(),
        &(false, "Installed Pkg com.test.base 1.0.0".into())
    );
    assert!(
        root.join("Packages/com.test.base/Runtime/Util.lua")
            .is_file()
    );
    // From a zip with a top folder, like a repository download.
    let zip = src.join("fx.zip");
    zip_folder(&fx, &zip, "fx-main/");
    w.install_zip(&root, &zip);
    assert!(!w.status.as_ref().unwrap().0, "{:?}", w.status);
    assert!(root.join("Packages/com.test.fx/package.json").is_file());
    assert_eq!(w.installed.len(), 2);
    assert!(w.resolution.conflicts.is_empty());
    let m = Manifest::load(&root).unwrap();
    assert_eq!(m.dependencies["com.test.fx"], "file:com.test.fx");
    let lock = Lock::load(&root).unwrap();
    assert_eq!(lock.dependencies["com.test.base"].source, "local");
    assert_eq!(lock.dependencies["com.test.fx"].version, "0.3.0");
    assert_eq!(
        lock.dependencies["com.test.fx"].dependencies["com.test.base"],
        "^1.0.0"
    );

    // base is still needed by fx.
    w.remove(&root, "com.test.base");
    assert!(w.status.as_ref().unwrap().0);
    assert!(root.join("Packages/com.test.base").is_dir());
    w.remove(&root, "com.test.fx");
    w.remove(&root, "com.test.base");
    assert!(!w.status.as_ref().unwrap().0, "{:?}", w.status);
    assert!(!root.join("Packages/com.test.base").exists());
    assert!(Manifest::load(&root).unwrap().dependencies.is_empty());
    assert!(Lock::load(&root).unwrap().dependencies.is_empty());
    // Built-ins can't go.
    assert!(install::remove(&root, "com.misty.modules.physics").is_err());
}

#[test]
fn conflicts_and_samples() {
    let root = temp("conflict");
    let src = temp("src2");
    let base = package(&src, "com.test.base", "1.0.0", &[]);
    let fx = package(&src, "com.test.fx", "1.0.0", &[("com.test.base", "^2.0.0")]);
    let mut w = PackagesWindow::default();
    w.refresh(&root);
    w.install_folder(&root, &base);
    w.install_folder(&root, &fx);
    assert_eq!(w.resolution.conflicts.len(), 1);
    let msg = w.resolution.conflicts[0].message();
    assert!(msg.contains("com.test.fx needs ^2.0.0"), "{msg}");
    assert!(
        w.status.as_ref().unwrap().1.contains("conflicts"),
        "{:?}",
        w.status
    );

    let pkg = w
        .installed
        .iter()
        .find(|p| p.info.name == "com.test.base")
        .unwrap()
        .clone();
    let samples = install::samples(&pkg);
    assert_eq!(samples.len(), 1);
    w.import_sample(&root, "com.test.base", &samples[0]);
    assert!(
        root.join("Assets/Samples/Pkg com.test.base/1.0.0/Demo/Readme.txt")
            .is_file(),
        "{:?}",
        w.status
    );
}

fn file_url(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

#[test]
fn registry_installs_check_hashes_and_resolve_dependencies() {
    let root = temp("registry");
    let srv = temp("server");
    let mut zips = Vec::new();
    for (name, ver, deps) in [
        ("com.reg.base", "1.0.0", vec![]),
        ("com.reg.base", "1.1.0", vec![]),
        ("com.reg.ui", "2.0.0", vec![("com.reg.base", "^1.0.0")]),
    ] {
        let tmp = temp("pkgsrc");
        let dir = package(&tmp, name, ver, &deps);
        let zip = srv.join(format!("{name}-{ver}.zip"));
        zip_folder(&dir, &zip, "");
        let hash = misty_engine::asset_groups::fetch::sha256_hex(&std::fs::read(&zip).unwrap());
        zips.push((name, ver, deps, zip, hash));
    }
    let mut index = String::from(r#"{"packages":["#);
    let names = ["com.reg.base", "com.reg.ui"];
    for (i, n) in names.iter().enumerate() {
        if i > 0 {
            index.push(',');
        }
        let vers: Vec<String> = zips
            .iter()
            .filter(|z| z.0 == *n)
            .map(|(_, v, deps, zip, hash)| {
                let deps: Vec<String> = deps
                    .iter()
                    .map(|(a, b)| format!("\"{a}\":\"{b}\""))
                    .collect();
                format!(
                    r#""{v}": {{"url":"{}","sha256":"{hash}","dependencies":{{{}}}}}"#,
                    zip.file_name().unwrap().to_string_lossy(),
                    deps.join(",")
                )
            })
            .collect();
        index.push_str(&format!(
            r#"{{"name":"{n}","displayName":"{n}","versions":{{{}}}}}"#,
            vers.join(",")
        ));
    }
    index.push_str("]}");
    let index_path = srv.join("index.json");
    std::fs::write(&index_path, &index).unwrap();
    let url = file_url(&index_path);
    let idx = registry::fetch_index(&url).unwrap();
    assert!(
        idx.version("com.reg.ui", &semver::Version::new(2, 0, 0))
            .unwrap()
            .url
            .starts_with("file://")
    );
    assert_eq!(
        idx.package("com.reg.base")
            .unwrap()
            .latest()
            .unwrap()
            .to_string(),
        "1.1.0"
    );

    let mut w = PackagesWindow::default();
    w.refresh(&root);
    w.registry = RegistryState::Loaded(idx.clone());
    w.install_registry(&root, "com.reg.ui", &semver::Version::new(2, 0, 0));
    assert!(!w.status.as_ref().unwrap().0, "{:?}", w.status);
    // The dependency came along at its newest matching version, not in the manifest.
    let lock = Lock::load(&root).unwrap();
    assert_eq!(lock.dependencies["com.reg.base"].version, "1.1.0");
    assert!(lock.dependencies["com.reg.base"].indirect);
    assert_eq!(lock.dependencies["com.reg.ui"].source, "registry");
    let m = Manifest::load(&root).unwrap();
    assert_eq!(m.dependencies.keys().collect::<Vec<_>>(), ["com.reg.ui"]);
    assert!(w.is_registry_package("com.reg.ui"));
    assert_eq!(read_only_roots(&root).len(), 2);

    // A tampered archive is refused.
    let mut bad = idx.clone();
    let e = bad.packages[0].versions.get_mut("1.0.0").unwrap();
    e.sha256 = "00".repeat(32);
    let err = install::install_registry(
        &root,
        &bad,
        "com.reg.base",
        &semver::Version::new(1, 0, 0),
        None,
    )
    .unwrap_err();
    assert!(err.contains("hash mismatch"), "{err}");

    // No registry configured.
    let empty = temp("noreg");
    let mut w = PackagesWindow::default();
    w.refresh(&empty);
    assert!(matches!(w.registry, RegistryState::NotConfigured));
    assert!(RegistryIndex::parse("{", "x").is_err());
}
