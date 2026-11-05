//! Packages: reusable bundles of scripts, prefabs and assets kept in `Packages/<name>` with a
//! `package.json`. Window ▸ Package Manager… lists built-in modules, packages in the project and
//! (when Project Settings ▸ Package Manager names one) a registry's packages.
//!
//! - [`semver`]: versions and ranges; [`resolve`]: picking versions, reporting conflicts.
//! - [`manifest`]: `package.json`, `Packages/manifest.json`, `Packages/packages-lock.json`.
//! - [`install`]: from a folder, a zip, a git URL or the registry; remove; import samples.
//! - [`registry`]: the registry index.

pub mod builtin;
pub mod install;
pub mod manifest;
pub mod registry;
pub mod resolve;
pub mod semver;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};
use std::sync::mpsc::Receiver;

use manifest::{InstalledPackage, Lock, Manifest};
use registry::RegistryIndex;
use resolve::Resolution;

/// Which packages the window lists.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    InProject,
    BuiltIn,
    Registry,
}

impl Filter {
    pub const ALL: [Filter; 3] = [Filter::InProject, Filter::BuiltIn, Filter::Registry];

    pub fn label(self) -> &'static str {
        match self {
            Filter::InProject => "In Project",
            Filter::BuiltIn => "Built-in",
            Filter::Registry => "Registry",
        }
    }
}

/// The registry index as far as the window knows.
#[derive(Default)]
pub enum RegistryState {
    #[default]
    NotConfigured,
    Loading(Receiver<Result<RegistryIndex, String>>),
    Loaded(RegistryIndex),
    Failed(String),
}

impl RegistryState {
    pub fn index(&self) -> Option<&RegistryIndex> {
        match self {
            RegistryState::Loaded(i) => Some(i),
            _ => None,
        }
    }
}

/// Package Manager window state.
#[derive(Default)]
pub struct PackagesWindow {
    pub filter: Filter,
    pub search: String,
    /// Selected package name.
    pub selected: Option<String>,
    pub installed: Vec<InstalledPackage>,
    /// Folders under `Packages/` whose package.json can't be read.
    pub broken: Vec<(PathBuf, String)>,
    pub manifest: Manifest,
    pub lock: Lock,
    pub resolution: Resolution,
    pub registry: RegistryState,
    /// Registry URL field (Project Settings ▸ Package Manager) being edited.
    pub registry_edit: Option<String>,
    /// `git` on PATH (checked once).
    pub git: Option<bool>,
    /// "Add package from git URL…" field (Some = shown).
    pub git_url: Option<String>,
    /// Last operation's message (error = true).
    pub status: Option<(bool, String)>,
    /// Project the lists were read from.
    pub loaded_for: Option<PathBuf>,
}

impl PackagesWindow {
    /// Read the project's packages again and resolve (writes the lock file when it changed).
    pub fn refresh(&mut self, project: &Path) {
        if let Err(e) = install::sync(project, None) {
            self.status = Some((true, e));
        }
        self.reload(project);
    }

    /// Re-read files without resolving.
    fn reload(&mut self, project: &Path) {
        let (installed, broken) = manifest::scan(project);
        self.installed = installed;
        self.broken = broken;
        self.manifest = Manifest::load(project).unwrap_or_default();
        self.lock = Lock::load(project).unwrap_or_default();
        let roots = self
            .manifest
            .dependencies
            .iter()
            .map(|(n, v)| {
                let req = match manifest::Source::parse(v) {
                    manifest::Source::Registry(r) => r,
                    _ => "*".into(),
                };
                (n.clone(), req)
            })
            .collect();
        self.resolution = resolve::resolve(
            &roots,
            &install::candidates(&self.installed, &self.lock, self.registry.index()),
        );
        if self.loaded_for.as_deref() != Some(project) {
            self.loaded_for = Some(project.to_path_buf());
            self.registry = if self.manifest.registry.trim().is_empty() {
                RegistryState::NotConfigured
            } else {
                RegistryState::Loading(registry::fetch_index_async(self.manifest.registry.clone()))
            };
        }
    }

    /// Load once per project.
    pub fn ensure_loaded(&mut self, project: &Path) {
        if self.loaded_for.as_deref() != Some(project) {
            self.refresh(project);
        }
    }

    /// Pick up a finished registry download.
    pub fn poll(&mut self) {
        if let RegistryState::Loading(rx) = &self.registry
            && let Ok(r) = rx.try_recv()
        {
            self.registry = match r {
                Ok(i) => RegistryState::Loaded(i),
                Err(e) => RegistryState::Failed(e),
            };
        }
    }

    /// Change the registry URL (saved in the manifest) and fetch its index.
    pub fn set_registry(&mut self, project: &Path, url: &str) {
        let mut m = Manifest::load(project).unwrap_or_default();
        m.registry = url.trim().to_owned();
        if let Err(e) = m.save(project) {
            self.status = Some((true, e));
            return;
        }
        self.manifest.registry = m.registry.clone();
        self.registry = if m.registry.is_empty() {
            RegistryState::NotConfigured
        } else {
            RegistryState::Loading(registry::fetch_index_async(m.registry))
        };
    }

    fn done(&mut self, project: &Path, result: Result<String, String>) {
        self.status = Some(match result {
            Ok(m) => (false, m),
            Err(e) => (true, e),
        });
        let index = match &self.registry {
            RegistryState::Loaded(i) => Some(i.clone()),
            _ => None,
        };
        match install::sync(project, index.as_ref()) {
            Ok(r) if !r.conflicts.is_empty() => {
                self.status = Some((
                    true,
                    format!("Dependency conflicts: {}", r.conflicts[0].message()),
                ))
            }
            Ok(_) => {}
            Err(e) => self.status = Some((true, e)),
        }
        self.reload(project);
    }

    pub fn install_folder(&mut self, project: &Path, src: &Path) {
        let r = install::install_folder(project, src).map(|p| {
            self.selected = Some(p.name.clone());
            format!("Installed {} {}", p.title(), p.version)
        });
        self.done(project, r);
    }

    pub fn install_zip(&mut self, project: &Path, zip: &Path) {
        let r = install::install_zip(project, zip).map(|p| {
            self.selected = Some(p.name.clone());
            format!("Installed {} {}", p.title(), p.version)
        });
        self.done(project, r);
    }

    pub fn install_git(&mut self, project: &Path, url: &str) {
        let r = install::install_git(project, url);
        let commit = r.as_ref().ok().map(|(p, c)| (p.name.clone(), c.clone()));
        let r = r.map(|(p, _)| {
            self.selected = Some(p.name.clone());
            format!("Installed {} {} from git", p.title(), p.version)
        });
        self.done(project, r);
        if let Some((name, c)) = commit {
            let _ = install::set_lock_hash(project, &name, &c);
            self.lock = Lock::load(project).unwrap_or_default();
        }
    }

    pub fn install_registry(&mut self, project: &Path, name: &str, version: &semver::Version) {
        let r = match self.registry.index() {
            Some(index) => install::install_registry(project, &index.clone(), name, version, None)
                .map(|p| format!("Installed {} {}", p.title(), p.version)),
            None => Err("the registry index is not loaded".into()),
        };
        self.selected = Some(name.to_owned());
        self.done(project, r);
    }

    pub fn remove(&mut self, project: &Path, name: &str) {
        // Refuse when another package still needs it.
        let users: Vec<String> = self
            .installed
            .iter()
            .filter(|p| p.info.name != name && p.info.dependencies.contains_key(name))
            .map(|p| p.info.title().to_owned())
            .collect();
        let r = if users.is_empty() {
            install::remove(project, name).map(|_| format!("Removed {name}"))
        } else {
            Err(format!("{name} is needed by {}", users.join(", ")))
        };
        if r.is_ok() {
            self.selected = None;
        }
        self.done(project, r);
    }

    pub fn import_sample(&mut self, project: &Path, name: &str, sample: &manifest::Sample) {
        let Some(pkg) = self.installed.iter().find(|p| p.info.name == name).cloned() else {
            return;
        };
        self.status = Some(match install::import_sample(project, &pkg, sample) {
            Ok(dest) => (
                false,
                format!(
                    "Imported sample to {}",
                    dest.strip_prefix(project)
                        .unwrap_or(&dest)
                        .to_string_lossy()
                        .replace('\\', "/")
                ),
            ),
            Err(e) => (true, e),
        });
    }

    /// Packages in the project come from the registry (read-only in the Project window).
    pub fn is_registry_package(&self, name: &str) -> bool {
        self.lock
            .dependencies
            .get(name)
            .is_some_and(|l| l.source == "registry")
    }

    pub fn git_available(&mut self) -> bool {
        *self.git.get_or_insert_with(install::git_available)
    }
}

/// `Packages/<folder>` paths under `root` that came from a registry (read-only).
pub fn read_only_roots(project: &Path) -> Vec<PathBuf> {
    let lock = Lock::load(project).unwrap_or_default();
    lock.dependencies
        .iter()
        .filter(|(_, l)| l.source == "registry")
        .map(|(n, _)| install::package_dir(project, n))
        .collect()
}
