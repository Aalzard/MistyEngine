//! New Project ▸ "Include sample scenes": copies the sample project's demo scenes and
//! everything they use (materials, scripts, models, textures, effects…) into a new project.
//!
//! Only the samples as shipped are copied: the release package's `Examples/SampleProject`
//! next to the exe, or, in a development build, the committed `SampleProject` of the source
//! tree (`git archive HEAD`, so uncommitted edits and untracked files are never copied).

use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use misty_engine::build::{BuildScene, BuildSettings};
use misty_engine::settings::ProjectSettings;

use super::io_error;

/// Where the samples come from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// A plain folder (the release package's `Examples/SampleProject`).
    Folder(PathBuf),
    /// The git checkout this editor was built from; its `SampleProject` at `HEAD`.
    Git(PathBuf),
}

/// The sample project to copy from, if this install has one.
pub fn source() -> Option<Source> {
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        let shipped = dir.join("Examples").join("SampleProject");
        if shipped.join("Assets").is_dir() {
            return Some(Source::Folder(shipped));
        }
    }
    let repo = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let found = repo.join(".git").exists() && repo.join("SampleProject/Assets").is_dir();
    found.then_some(Source::Git(repo))
}

/// Copy the samples into `root` (a project the template just made). Files the template
/// wrote are kept; the samples' project settings (input axes, tags, layers, quality…) are
/// taken over with the new product name, and every sample scene is added to Build
/// Settings unticked. Returns how many files were copied.
pub fn add(root: &Path, source: &Source) -> Result<usize, String> {
    let copied = match source {
        Source::Folder(dir) => copy_folder(dir, root)?,
        Source::Git(repo) => copy_from_git(repo, root)?,
    };
    adopt_settings(root)?;
    Ok(copied)
}

/// Copy `from/Assets/**` and `from/ProjectSettings/ProjectSettings.json` into `root`.
fn copy_folder(from: &Path, root: &Path) -> Result<usize, String> {
    let mut copied = copy_tree(&from.join("Assets"), &root.join("Assets"))?;
    let settings = from.join("ProjectSettings/ProjectSettings.json");
    if settings.is_file() {
        let dest = root.join(SAMPLE_SETTINGS);
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
        }
        std::fs::copy(&settings, &dest).map_err(io_error("Could not copy", &settings))?;
        copied += 1;
    }
    Ok(copied)
}

/// Recursive copy that never overwrites a file already in `to`.
fn copy_tree(from: &Path, to: &Path) -> Result<usize, String> {
    let mut copied = 0;
    std::fs::create_dir_all(to).map_err(io_error("Could not create the folder", to))?;
    for entry in std::fs::read_dir(from).map_err(io_error("Could not read the folder", from))? {
        let entry = entry.map_err(io_error("Could not read the folder", from))?;
        let path = entry.path();
        let dest = to.join(entry.file_name());
        if path.is_dir() {
            copied += copy_tree(&path, &dest)?;
        } else if !dest.exists() {
            std::fs::copy(&path, &dest).map_err(io_error("Could not copy", &path))?;
            copied += 1;
        }
    }
    Ok(copied)
}

/// The committed sample project: `git archive HEAD` of its Assets and settings, as a zip
/// whose entries start with `SampleProject/`.
pub(super) fn git_archive(repo: &Path) -> Result<zip::ZipArchive<Cursor<Vec<u8>>>, String> {
    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(repo)
        .args([
            "archive",
            "--format=zip",
            "HEAD",
            "SampleProject/Assets",
            "SampleProject/ProjectSettings/ProjectSettings.json",
        ])
        .output()
        .map_err(|e| format!("git archive in {}: {e}", repo.display()))?;
    if !out.status.success() {
        return Err(format!(
            "git archive failed in {}: {}",
            repo.display(),
            String::from_utf8_lossy(&out.stderr).trim()
        ));
    }
    zip::ZipArchive::new(Cursor::new(out.stdout)).map_err(|e| format!("git archive: {e}"))
}

/// [`git_archive`] unpacked into `root`.
fn copy_from_git(repo: &Path, root: &Path) -> Result<usize, String> {
    let mut zip = git_archive(repo)?;
    let mut copied = 0;
    for i in 0..zip.len() {
        let mut file = zip.by_index(i).map_err(|e| e.to_string())?;
        if file.is_dir() {
            continue;
        }
        // Paths inside are "SampleProject/…": checked to stay inside the project.
        let Some(rel) = file
            .enclosed_name()
            .and_then(|p| p.strip_prefix("SampleProject").ok().map(Path::to_path_buf))
        else {
            continue;
        };
        let dest = if rel == Path::new("ProjectSettings/ProjectSettings.json") {
            root.join(SAMPLE_SETTINGS)
        } else {
            root.join(&rel)
        };
        if dest.exists() {
            continue;
        }
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
        }
        let mut bytes = Vec::with_capacity(file.size() as usize);
        file.read_to_end(&mut bytes)
            .map_err(|e| format!("Could not read {} from git: {e}", rel.display()))?;
        std::fs::write(&dest, bytes).map_err(io_error("Could not write", &dest))?;
        copied += 1;
    }
    Ok(copied)
}

/// The sample's settings are staged here, then merged by [`merge_settings`].
pub(super) const SAMPLE_SETTINGS: &str = "Library/SampleProjectSettings.json";

/// Take over the samples' project settings (keeping the new project's product name) and list
/// the sample scenes in Build Settings, unticked, after the template's own scene.
fn adopt_settings(root: &Path) -> Result<(), String> {
    merge_settings(root)?;
    let mut build = BuildSettings::load(root)?;
    let mut scenes = Vec::new();
    collect_scenes(root, &root.join("Assets"), &mut scenes);
    scenes.sort();
    for path in scenes {
        if !build.scenes.iter().any(|s| s.path == path) {
            build.scenes.push(BuildScene {
                path,
                enabled: false,
            });
        }
    }
    build.save(root)
}

/// Take over the staged sample settings (input axes, tags, layers, quality…), keeping the new
/// project's product name. Nothing to do when none are staged.
pub(super) fn merge_settings(root: &Path) -> Result<(), String> {
    let staged = root.join(SAMPLE_SETTINGS);
    if staged.is_file() {
        let ours = ProjectSettings::load(root)?;
        let text = std::fs::read_to_string(&staged).map_err(io_error("Could not read", &staged))?;
        let mut theirs: ProjectSettings = serde_json::from_str(&text)
            .map_err(|e| format!("Could not read {}: {e}", staged.display()))?;
        let name = ours.player.as_ref().map(|p| p.product_name.clone());
        let mut player = theirs.player.take().unwrap_or_default();
        if let Some(name) = name {
            player.product_name = name;
        }
        theirs.player = Some(player);
        theirs.save(root)?;
        std::fs::remove_file(&staged).map_err(io_error("Could not remove", &staged))?;
    }
    Ok(())
}

fn collect_scenes(root: &Path, dir: &Path, out: &mut Vec<String>) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir() {
            collect_scenes(root, &path, out);
        } else if path.extension().is_some_and(|e| e == "scene")
            && let Ok(rel) = path.strip_prefix(root)
        {
            out.push(rel.to_string_lossy().replace('\\', "/"));
        }
    }
}
