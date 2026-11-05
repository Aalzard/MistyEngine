//! Package files:
//!
//! - `package.json` at the root of every package (name, displayName, version, description,
//!   dependencies, author, license, samples).
//! - `Packages/manifest.json`: the packages the project asked for and the registry URL.
//!   A dependency value is a version range (registry), `file:<folder>` (a package copied into
//!   `Packages/<folder>`) or `git+<url>[#rev]`.
//! - `Packages/packages-lock.json`: what was resolved and installed, written after every change.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use super::semver::Version;

pub const PACKAGES_DIR: &str = "Packages";
pub const MANIFEST_FILE: &str = "Packages/manifest.json";
pub const LOCK_FILE: &str = "Packages/packages-lock.json";
pub const PACKAGE_FILE: &str = "package.json";

/// `author` may be a string or `{ "name": …, "email": …, "url": … }`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Author {
    #[default]
    None,
    Name(String),
    Full {
        name: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        email: String,
        #[serde(default, skip_serializing_if = "String::is_empty")]
        url: String,
    },
}

impl Author {
    pub fn name(&self) -> &str {
        match self {
            Author::None => "",
            Author::Name(n) | Author::Full { name: n, .. } => n,
        }
    }
}

/// A sample the package offers (copied into `Assets/Samples` by "Import Sample").
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct Sample {
    pub display_name: String,
    pub description: String,
    /// Folder inside the package ("Samples~/Demo").
    pub path: String,
}

/// `package.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct PackageJson {
    /// Reverse-domain id ("com.misty.example-tools").
    pub name: String,
    pub display_name: String,
    pub version: String,
    pub description: String,
    /// Package name → version range.
    pub dependencies: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "is_none_author")]
    pub author: Author,
    #[serde(skip_serializing_if = "String::is_empty")]
    pub license: String,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub samples: Vec<Sample>,
}

fn is_none_author(a: &Author) -> bool {
    *a == Author::None
}

/// Package names: lower-case reverse-domain words (`com.company.name`), digits, `-`, `_`, `.`.
pub fn valid_name(name: &str) -> bool {
    !name.is_empty()
        && name.len() <= 214
        && name.contains('.')
        && !name.starts_with('.')
        && !name.ends_with('.')
        && !name.contains("..")
        && name
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || "-_.".contains(c))
}

impl PackageJson {
    pub fn parse(text: &str) -> Result<Self, String> {
        let p: Self = serde_json::from_str(text).map_err(|e| format!("package.json: {e}"))?;
        if !valid_name(&p.name) {
            return Err(format!(
                "package.json: '{}' is not a valid package name (like com.company.tool)",
                p.name
            ));
        }
        Version::parse(&p.version).map_err(|e| format!("package.json: version {e}"))?;
        for (dep, range) in &p.dependencies {
            super::semver::Range::parse(range)
                .map_err(|e| format!("package.json: dependency {dep}: {e}"))?;
        }
        Ok(p)
    }

    pub fn read(dir: &Path) -> Result<Self, String> {
        let path = dir.join(PACKAGE_FILE);
        let text =
            std::fs::read_to_string(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        Self::parse(&text)
    }

    pub fn version(&self) -> Version {
        Version::parse(&self.version).unwrap_or(Version::new(0, 0, 0))
    }

    pub fn title(&self) -> &str {
        if self.display_name.is_empty() {
            &self.name
        } else {
            &self.display_name
        }
    }
}

/// `Packages/manifest.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Manifest {
    pub dependencies: BTreeMap<String, String>,
    /// Registry index URL (Project Settings ▸ Package Manager); empty = none.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub registry: String,
}

/// Where a manifest dependency comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A version range resolved against the registry.
    Registry(String),
    /// A folder under `Packages/`.
    Local(String),
    /// A git repository (URL, optional revision).
    Git(String, Option<String>),
}

impl Source {
    pub fn parse(value: &str) -> Source {
        if let Some(folder) = value.strip_prefix("file:") {
            Source::Local(folder.trim_start_matches("./").to_owned())
        } else if let Some(url) = value.strip_prefix("git+") {
            match url.rsplit_once('#') {
                Some((u, rev)) => Source::Git(u.to_owned(), Some(rev.to_owned())),
                None => Source::Git(url.to_owned(), None),
            }
        } else {
            Source::Registry(value.to_owned())
        }
    }
}

fn read_json<T: for<'a> Deserialize<'a> + Default>(path: &Path) -> Result<T, String> {
    match std::fs::read_to_string(path) {
        Ok(t) => serde_json::from_str(&t).map_err(|e| format!("{}: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(T::default()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

fn write_json<T: Serialize>(path: &Path, v: &T) -> Result<(), String> {
    if let Some(d) = path.parent() {
        std::fs::create_dir_all(d).map_err(|e| format!("{}: {e}", d.display()))?;
    }
    let text = serde_json::to_string_pretty(v).map_err(|e| e.to_string())? + "\n";
    std::fs::write(path, text).map_err(|e| format!("{}: {e}", path.display()))
}

impl Manifest {
    pub fn load(project: &Path) -> Result<Self, String> {
        read_json(&project.join(MANIFEST_FILE))
    }

    pub fn save(&self, project: &Path) -> Result<(), String> {
        write_json(&project.join(MANIFEST_FILE), self)
    }

    pub fn source(&self, name: &str) -> Option<Source> {
        self.dependencies.get(name).map(|v| Source::parse(v))
    }
}

/// One resolved package in the lock file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LockEntry {
    pub version: String,
    /// "local", "registry", "git" or "builtin".
    pub source: String,
    /// Folder, URL or zip the package was installed from.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub resolved: String,
    /// SHA-256 of a downloaded zip / the git commit.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub hash: String,
    /// Installed only because another package needs it.
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub indirect: bool,
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dependencies: BTreeMap<String, String>,
}

/// `Packages/packages-lock.json`.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Lock {
    pub dependencies: BTreeMap<String, LockEntry>,
}

impl Lock {
    pub fn load(project: &Path) -> Result<Self, String> {
        read_json(&project.join(LOCK_FILE))
    }

    pub fn save(&self, project: &Path) -> Result<(), String> {
        write_json(&project.join(LOCK_FILE), self)
    }
}

/// A package folder found under `Packages/`.
#[derive(Clone, Debug, PartialEq)]
pub struct InstalledPackage {
    pub dir: PathBuf,
    pub info: PackageJson,
}

/// Every `Packages/<folder>` with a valid `package.json` (sorted by name), plus the folders
/// whose `package.json` is broken (folder, error).
pub fn scan(project: &Path) -> (Vec<InstalledPackage>, Vec<(PathBuf, String)>) {
    let mut ok = Vec::new();
    let mut bad = Vec::new();
    let dir = project.join(PACKAGES_DIR);
    for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
        let p = e.path();
        if !p.is_dir() || !p.join(PACKAGE_FILE).is_file() {
            continue;
        }
        match PackageJson::read(&p) {
            Ok(info) => ok.push(InstalledPackage { dir: p, info }),
            Err(err) => bad.push((p, err)),
        }
    }
    ok.sort_by(|a, b| a.info.name.cmp(&b.info.name));
    bad.sort();
    (ok, bad)
}
