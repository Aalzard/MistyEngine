//! Installing and removing packages. Every package lives in `Packages/<name>`; installs copy a
//! folder there (from disk, an extracted zip, a git clone or a registry download) and record it
//! in `Packages/manifest.json`. [`sync`] then resolves dependencies and writes the lock file.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::Command;

use super::manifest::{
    InstalledPackage, Lock, LockEntry, Manifest, PACKAGES_DIR, PackageJson, Sample, Source, scan,
};
use super::registry::RegistryIndex;
use super::resolve::{Candidate, Origin, Resolution, resolve};
use super::{builtin, semver::Version};

fn io(p: &Path, e: std::io::Error) -> String {
    format!("{}: {e}", p.display())
}

/// Copy `src` into `dest` recursively (skipping `.git`).
pub fn copy_dir(src: &Path, dest: &Path) -> Result<(), String> {
    std::fs::create_dir_all(dest).map_err(|e| io(dest, e))?;
    for e in std::fs::read_dir(src).map_err(|e| io(src, e))?.flatten() {
        let p = e.path();
        let name = e.file_name();
        if name == ".git" {
            continue;
        }
        let to = dest.join(&name);
        if p.is_dir() {
            copy_dir(&p, &to)?;
        } else {
            std::fs::copy(&p, &to).map_err(|e| io(&p, e))?;
        }
    }
    Ok(())
}

/// `Packages/<name>` of `project`.
pub fn package_dir(project: &Path, name: &str) -> PathBuf {
    project.join(PACKAGES_DIR).join(name)
}

/// Put the package in folder `src` into `Packages/<name>` (replacing an older copy) and return
/// its `package.json`. Does not touch the manifest.
fn place(project: &Path, src: &Path) -> Result<PackageJson, String> {
    let info = PackageJson::read(src)?;
    if builtin::is_builtin(&info.name) {
        return Err(format!("{} is a built-in package", info.name));
    }
    let dest = package_dir(project, &info.name);
    let same = std::path::absolute(src).ok() == std::path::absolute(&dest).ok();
    if !same {
        // Copy beside it first, so a failed copy never leaves a half-replaced package.
        let tmp = project
            .join(PACKAGES_DIR)
            .join(format!(".{}.tmp", info.name));
        let _ = std::fs::remove_dir_all(&tmp);
        copy_dir(src, &tmp)?;
        if dest.exists() {
            std::fs::remove_dir_all(&dest).map_err(|e| io(&dest, e))?;
        }
        std::fs::rename(&tmp, &dest).map_err(|e| io(&dest, e))?;
    }
    Ok(info)
}

fn record(project: &Path, name: &str, value: String) -> Result<(), String> {
    let mut m = Manifest::load(project)?;
    m.dependencies.insert(name.to_owned(), value);
    m.save(project)
}

/// Install the package in folder `src` (a folder with a `package.json`).
pub fn install_folder(project: &Path, src: &Path) -> Result<PackageJson, String> {
    let info = place(project, src)?;
    record(project, &info.name, format!("file:{}", info.name))?;
    Ok(info)
}

/// The folder of an extracted archive holding `package.json`: the root or its only sub-folder.
fn package_root(dir: &Path) -> Result<PathBuf, String> {
    if dir.join(super::manifest::PACKAGE_FILE).is_file() {
        return Ok(dir.to_path_buf());
    }
    let subs: Vec<PathBuf> = std::fs::read_dir(dir)
        .map_err(|e| io(dir, e))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    match subs.as_slice() {
        [one] if one.join(super::manifest::PACKAGE_FILE).is_file() => Ok(one.clone()),
        _ => Err("no package.json at the top of the archive".into()),
    }
}

/// Extract `zip` into a fresh folder under `Library/PackageCache` and return it.
fn extract(project: &Path, zip: &Path) -> Result<PathBuf, String> {
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dest = project
        .join("Library/PackageCache")
        .join(format!("extract_{n}"));
    std::fs::create_dir_all(&dest).map_err(|e| io(&dest, e))?;
    let file = std::fs::File::open(zip).map_err(|e| io(zip, e))?;
    let mut archive = zip::ZipArchive::new(file).map_err(|e| format!("{}: {e}", zip.display()))?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i).map_err(|e| e.to_string())?;
        // `enclosed_name` rejects absolute paths and `..` (nothing lands outside `dest`).
        let rel = entry
            .enclosed_name()
            .ok_or_else(|| format!("unsafe path in archive: {}", entry.name()))?;
        let out = dest.join(rel);
        if entry.is_dir() {
            std::fs::create_dir_all(&out).map_err(|e| io(&out, e))?;
            continue;
        }
        if let Some(d) = out.parent() {
            std::fs::create_dir_all(d).map_err(|e| io(d, e))?;
        }
        let mut f = std::fs::File::create(&out).map_err(|e| io(&out, e))?;
        std::io::copy(&mut entry, &mut f).map_err(|e| io(&out, e))?;
    }
    Ok(dest)
}

/// Install the package in a `.zip` file.
pub fn install_zip(project: &Path, zip: &Path) -> Result<PackageJson, String> {
    let dir = extract(project, zip)?;
    let result = package_root(&dir).and_then(|root| install_folder(project, &root));
    let _ = std::fs::remove_dir_all(&dir);
    result
}

/// Whether `git` runs from PATH.
pub fn git_available() -> bool {
    Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

/// `https://host/repo.git#v1.2` → (url, Some("v1.2")).
pub fn split_git_url(text: &str) -> (String, Option<String>) {
    let t = text.trim().trim_start_matches("git+");
    match t.rsplit_once('#') {
        Some((u, rev)) if !rev.is_empty() => (u.to_owned(), Some(rev.to_owned())),
        _ => (t.trim_end_matches('#').to_owned(), None),
    }
}

/// Clone a git repository (optionally at `#revision`) and install the package in it.
/// Returns the package and the commit.
pub fn install_git(project: &Path, url_text: &str) -> Result<(PackageJson, String), String> {
    let (url, rev) = split_git_url(url_text);
    if url.starts_with('-') {
        return Err("not a git URL".into());
    }
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos());
    let dir = project
        .join("Library/PackageCache")
        .join(format!("git_{n}"));
    let mut cmd = Command::new("git");
    cmd.args(["clone", "--depth", "1"]);
    if let Some(r) = &rev {
        cmd.args(["--branch", r]);
    }
    cmd.arg("--").arg(&url).arg(&dir);
    let out = cmd
        .output()
        .map_err(|e| format!("git could not run: {e}"))?;
    if !out.status.success() {
        let _ = std::fs::remove_dir_all(&dir);
        return Err(format!(
            "git clone failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    let commit = Command::new("git")
        .arg("-C")
        .arg(&dir)
        .args(["rev-parse", "HEAD"])
        .output()
        .ok()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_owned())
        .unwrap_or_default();
    let result = place(project, &dir).and_then(|info| {
        let value = match &rev {
            Some(r) => format!("git+{url}#{r}"),
            None => format!("git+{url}"),
        };
        record(project, &info.name, value)?;
        Ok(info)
    });
    let _ = std::fs::remove_dir_all(&dir);
    Ok((result?, commit))
}

/// Download version `version` of `name` from the registry, check its hash and install it.
pub fn install_registry(
    project: &Path,
    index: &RegistryIndex,
    name: &str,
    version: &Version,
    range: Option<&str>,
) -> Result<PackageJson, String> {
    let entry = index
        .version(name, version)
        .ok_or_else(|| format!("{name}@{version} is not in the registry"))?;
    let bytes = misty_engine::asset_groups::fetch::fetch(&entry.url)?;
    if !entry.sha256.is_empty() {
        let got = misty_engine::asset_groups::fetch::sha256_hex(&bytes);
        if !got.eq_ignore_ascii_case(&entry.sha256) {
            return Err(format!(
                "{name}@{version}: download hash mismatch; not installed"
            ));
        }
    }
    let cache = project.join("Library/PackageCache");
    std::fs::create_dir_all(&cache).map_err(|e| io(&cache, e))?;
    let zip = cache.join(format!("{name}-{version}.zip"));
    std::fs::write(&zip, &bytes).map_err(|e| io(&zip, e))?;
    let dir = extract(project, &zip)?;
    let result = package_root(&dir).and_then(|root| place(project, &root));
    let _ = std::fs::remove_dir_all(&dir);
    let info = result?;
    if info.name != name {
        return Err(format!("the registry's {name} archive holds {}", info.name));
    }
    let value = range.map_or_else(|| format!("^{version}"), str::to_owned);
    record(project, name, value)?;
    // Mark it as a registry package right away (resolution then lets it update).
    let mut lock = Lock::load(project)?;
    lock.dependencies.insert(
        name.to_owned(),
        LockEntry {
            version: version.to_string(),
            source: "registry".into(),
            resolved: entry.url.clone(),
            hash: misty_engine::asset_groups::fetch::sha256_hex(&bytes),
            indirect: false,
            dependencies: info.dependencies.clone(),
        },
    );
    lock.save(project)?;
    Ok(info)
}

/// Remove a package: its `Packages/<name>` folder and its manifest entry.
pub fn remove(project: &Path, name: &str) -> Result<(), String> {
    if builtin::is_builtin(name) {
        return Err(format!("{name} is built in and can't be removed"));
    }
    if !super::manifest::valid_name(name) {
        return Err(format!("'{name}' is not a package name"));
    }
    let dir = package_dir(project, name);
    if dir.exists() {
        std::fs::remove_dir_all(&dir).map_err(|e| io(&dir, e))?;
    }
    let mut m = Manifest::load(project)?;
    m.dependencies.remove(name);
    m.save(project)
}

/// Samples of a package: the `samples` list, or else every folder in `Samples~`.
pub fn samples(pkg: &InstalledPackage) -> Vec<Sample> {
    if !pkg.info.samples.is_empty() {
        return pkg.info.samples.clone();
    }
    let mut out: Vec<Sample> = std::fs::read_dir(pkg.dir.join("Samples~"))
        .into_iter()
        .flatten()
        .flatten()
        .filter(|e| e.path().is_dir())
        .map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            Sample {
                display_name: name.clone(),
                description: String::new(),
                path: format!("Samples~/{name}"),
            }
        })
        .collect();
    out.sort_by(|a, b| a.display_name.cmp(&b.display_name));
    out
}

/// Where a sample is imported: `Assets/Samples/<package title>/<version>/<sample>`.
pub fn sample_dest(project: &Path, pkg: &PackageJson, sample: &Sample) -> PathBuf {
    project
        .join("Assets/Samples")
        .join(pkg.title())
        .join(&pkg.version)
        .join(&sample.display_name)
}

/// Copy a sample into the project's Assets (replacing an older import). Returns the folder.
pub fn import_sample(
    project: &Path,
    pkg: &InstalledPackage,
    sample: &Sample,
) -> Result<PathBuf, String> {
    let src = pkg.dir.join(&sample.path);
    if !src.is_dir() || sample.path.contains("..") {
        return Err(format!("{}: sample folder not found", src.display()));
    }
    let dest = sample_dest(project, &pkg.info, sample);
    if dest.exists() {
        std::fs::remove_dir_all(&dest).map_err(|e| io(&dest, e))?;
    }
    copy_dir(&src, &dest)?;
    Ok(dest)
}

/// Every version that could satisfy a requirement: built-ins, project folders, the registry.
pub fn candidates(
    installed: &[InstalledPackage],
    lock: &Lock,
    index: Option<&RegistryIndex>,
) -> BTreeMap<String, Vec<Candidate>> {
    let mut out: BTreeMap<String, Vec<Candidate>> = BTreeMap::new();
    for b in builtin::all() {
        out.entry(b.name.clone()).or_default().push(Candidate {
            version: b.version(),
            origin: Origin::BuiltIn,
            dependencies: BTreeMap::new(),
        });
    }
    for p in installed {
        // Registry downloads compete with the registry's other versions (updates).
        let from_registry = lock
            .dependencies
            .get(&p.info.name)
            .is_some_and(|l| l.source == "registry");
        out.entry(p.info.name.clone()).or_default().push(Candidate {
            version: p.info.version(),
            origin: if from_registry {
                Origin::Registry
            } else {
                Origin::Installed
            },
            dependencies: p.info.dependencies.clone(),
        });
    }
    if let Some(index) = index {
        for p in &index.packages {
            let list = out.entry(p.name.clone()).or_default();
            for (v, entry) in &p.versions {
                let Ok(version) = Version::parse(v) else {
                    continue;
                };
                if list.iter().any(|c| c.version == version) {
                    continue;
                }
                list.push(Candidate {
                    version,
                    origin: Origin::Registry,
                    dependencies: entry.dependencies.clone(),
                });
            }
        }
    }
    out
}

/// The requirement the resolver uses for a manifest value: a range for registry packages,
/// any version for project folders (they are pinned to what they hold).
fn requirement(value: &str) -> String {
    match Source::parse(value) {
        Source::Registry(range) => range,
        Source::Local(_) | Source::Git(..) => "*".into(),
    }
}

/// Resolve the manifest, download registry packages that need it (when `index` is given),
/// and write the lock file. Returns the resolution (conflicts included).
pub fn sync(project: &Path, index: Option<&RegistryIndex>) -> Result<Resolution, String> {
    let manifest = Manifest::load(project)?;
    let old_lock = Lock::load(project)?;
    let (installed, _) = scan(project);
    let roots: BTreeMap<String, String> = manifest
        .dependencies
        .iter()
        .map(|(n, v)| (n.clone(), requirement(v)))
        .collect();
    let mut res = resolve(&roots, &candidates(&installed, &old_lock, index));
    // Registry versions not on disk yet.
    if let Some(index) = index {
        for (name, r) in &res.packages {
            if r.origin != Origin::Registry {
                continue;
            }
            let have = installed
                .iter()
                .find(|p| &p.info.name == name)
                .map(|p| p.info.version());
            if have.as_ref() != Some(&r.version) {
                let range = manifest.dependencies.get(name).map(String::as_str);
                install_registry(project, index, name, &r.version, range)?;
            }
        }
        // Indirect registry installs are not manifest entries.
        let mut m = Manifest::load(project)?;
        m.dependencies
            .retain(|n, _| manifest.dependencies.contains_key(n));
        m.save(project)?;
    }
    let (installed, _) = scan(project);
    let mut lock = Lock::default();
    for (name, r) in &res.packages {
        let src = manifest.dependencies.get(name).map(|v| Source::parse(v));
        let (source, resolved) = match (&r.origin, &src) {
            (Origin::BuiltIn, _) => ("builtin", String::new()),
            (Origin::Registry, _) => (
                "registry",
                index
                    .and_then(|i| i.version(name, &r.version))
                    .map(|e| e.url.clone())
                    .or_else(|| old_lock.dependencies.get(name).map(|l| l.resolved.clone()))
                    .unwrap_or_default(),
            ),
            (Origin::Installed, Some(Source::Git(url, _))) => ("git", url.clone()),
            (Origin::Installed, _) => (
                "local",
                installed
                    .iter()
                    .find(|p| &p.info.name == name)
                    .map(|p| {
                        format!(
                            "{PACKAGES_DIR}/{}",
                            p.dir.file_name().unwrap_or_default().to_string_lossy()
                        )
                    })
                    .unwrap_or_default(),
            ),
        };
        let hash = old_lock
            .dependencies
            .get(name)
            .filter(|l| l.version == r.version.to_string())
            .map(|l| l.hash.clone())
            .unwrap_or_default();
        lock.dependencies.insert(
            name.clone(),
            LockEntry {
                version: r.version.to_string(),
                source: source.into(),
                resolved,
                hash,
                indirect: r.indirect,
                dependencies: r.dependencies.clone(),
            },
        );
    }
    let lock_missing = !project.join(super::manifest::LOCK_FILE).is_file();
    if lock != old_lock || (lock_missing && project.join(super::manifest::MANIFEST_FILE).is_file())
    {
        lock.save(project)?;
    }
    res.conflicts.sort_by(|a, b| a.name.cmp(&b.name));
    Ok(res)
}

/// Record the git commit of a git install in the lock file.
pub fn set_lock_hash(project: &Path, name: &str, hash: &str) -> Result<(), String> {
    let mut lock = Lock::load(project)?;
    if let Some(e) = lock.dependencies.get_mut(name) {
        e.hash = hash.to_owned();
    }
    lock.save(project)
}
