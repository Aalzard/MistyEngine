//! The MeadowDemo template: the sample project's MeadowDemo scene and everything it uses (the
//! terrain, its tree recipes and textures, the First Person Player prefab and its scripts), meta
//! files and GUIDs intact, plus the sample's project settings (input axes such as Run and
//! Crouch, the Player layer). The files come from the same place as New Project ▸ Include
//! sample scenes ([`super::samples::source`]): the release package's `Examples/SampleProject`,
//! or the committed `SampleProject` in a development build.
//!
//! The dependency set is the build's own walk ([`asset_deps`]); the tree and meadow texture
//! folders come along whole (README and LICENSE included: free assets to reuse). The new
//! project starts looking towards the sun, the player too (`face_the_sun`).

use std::cell::RefCell;
use std::collections::BTreeSet;
use std::io::{Cursor, Read};
use std::path::{Path, PathBuf};

use glam::{Quat, Vec2, Vec3};
use misty_engine::asset_db::{GuidDatabase, meta, refs};
use misty_engine::asset_deps::{self, ProjectFiles};
use misty_engine::terrain::TerrainData;
use serde_json::Value;

use super::io_error;
use super::samples::{self, Source};
use crate::scene_files::SavedView;

/// The starting scene (the same path as in the sample project).
pub const SCENE: &str = "Assets/Scenes/MeadowDemo.scene";
/// Copied whole on top of the scene's dependencies.
const FOLDERS: [&str; 2] = ["Assets/Trees/", "Assets/Textures/Meadow/"];
const SETTINGS: &str = "ProjectSettings/ProjectSettings.json";
/// Height of the player's eyes above its feet (the prefab's `HeadBobVertical` offset).
const EYE_HEIGHT: f32 = 1.7;
/// The Scene view looks this far down (degrees): mostly meadow, the sun's glow above.
const VIEW_PITCH: f32 = 6.0;

/// The sample project's files, by project-relative path.
pub(super) enum SampleFiles {
    /// A plain folder (the release package's `Examples/SampleProject`).
    Folder(PathBuf),
    /// `SampleProject` at `HEAD` of a git checkout.
    Git {
        repo: PathBuf,
        zip: RefCell<zip::ZipArchive<Cursor<Vec<u8>>>>,
        files: BTreeSet<String>,
    },
}

impl SampleFiles {
    pub fn open(source: &Source) -> Result<Self, String> {
        match source {
            Source::Folder(dir) => Ok(Self::Folder(dir.clone())),
            Source::Git(repo) => {
                let zip = samples::git_archive(repo)?;
                let files = zip
                    .file_names()
                    .filter(|n| !n.ends_with('/'))
                    .filter_map(|n| n.strip_prefix("SampleProject/"))
                    .map(str::to_owned)
                    .collect();
                Ok(Self::Git {
                    repo: repo.clone(),
                    zip: RefCell::new(zip),
                    files,
                })
            }
        }
    }

    /// Where the files come from, for error messages.
    fn describe(&self) -> String {
        match self {
            Self::Folder(dir) => dir.display().to_string(),
            Self::Git { repo, .. } => format!("{} (SampleProject at HEAD)", repo.display()),
        }
    }

    /// Every file under `Assets/`, meta files included.
    fn assets(&self) -> Result<BTreeSet<String>, String> {
        match self {
            Self::Folder(dir) => {
                let mut out = BTreeSet::new();
                list_folder(dir, &dir.join("Assets"), &mut out)?;
                Ok(out)
            }
            Self::Git { files, .. } => Ok(files
                .iter()
                .filter(|f| f.starts_with("Assets/"))
                .cloned()
                .collect()),
        }
    }

    pub fn read(&self, rel: &str) -> Result<Vec<u8>, String> {
        match self {
            Self::Folder(dir) => {
                let path = dir.join(rel);
                std::fs::read(&path).map_err(io_error("Could not read", &path))
            }
            Self::Git { zip, .. } => {
                let mut zip = zip.borrow_mut();
                let mut file = zip
                    .by_name(&format!("SampleProject/{rel}"))
                    .map_err(|e| format!("Could not read {rel} from {}: {e}", self.describe()))?;
                let mut bytes = Vec::with_capacity(file.size() as usize);
                file.read_to_end(&mut bytes)
                    .map_err(|e| format!("Could not read {rel} from {}: {e}", self.describe()))?;
                Ok(bytes)
            }
        }
    }

    /// GUID ↔ path map of the sample's meta files (resolves GUID references in the walk).
    fn guids(&self, assets: &BTreeSet<String>) -> GuidDatabase {
        let root = match self {
            Self::Folder(dir) => dir.clone(),
            Self::Git { repo, .. } => repo.join("SampleProject"),
        };
        let mut db = GuidDatabase::new(&root);
        for m in assets.iter().filter(|a| a.ends_with(".meta")) {
            let asset = &m[..m.len() - ".meta".len()];
            let guid = self
                .read_to_string(m)
                .and_then(|t| meta::guid_in(&t))
                .filter(|&g| db.path_of(g).is_none());
            if let Some(g) = guid {
                db.insert(asset, g);
            }
        }
        db
    }
}

impl ProjectFiles for SampleFiles {
    fn is_file(&self, rel: &str) -> bool {
        match self {
            Self::Folder(dir) => dir.join(rel).is_file(),
            Self::Git { files, .. } => files.contains(rel),
        }
    }

    fn read_to_string(&self, rel: &str) -> Option<String> {
        String::from_utf8(self.read(rel).ok()?).ok()
    }
}

fn list_folder(root: &Path, dir: &Path, out: &mut BTreeSet<String>) -> Result<(), String> {
    for entry in std::fs::read_dir(dir).map_err(io_error("Could not read the folder", dir))? {
        let path = entry
            .map_err(io_error("Could not read the folder", dir))?
            .path();
        if path.is_dir() {
            list_folder(root, &path, out)?;
        } else if let Ok(rel) = path.strip_prefix(root) {
            out.insert(rel.to_string_lossy().replace('\\', "/"));
        }
    }
    Ok(())
}

/// Every file the template copies: the scene's dependencies with their meta files, the
/// [`FOLDERS`], and the meta files of the folders all of these sit in (folders keep their
/// GUIDs too). A reference to a file the sample doesn't have is an error.
pub(super) fn files(src: &SampleFiles) -> Result<BTreeSet<String>, String> {
    dependencies(src, &[SCENE], &FOLDERS)
}

/// The sample files `roots` use (themselves included, with their meta files), every file under
/// `folders`, and the meta files of the folders all of these sit in. A reference to a file the
/// sample doesn't have is an error.
pub(super) fn dependencies(
    src: &SampleFiles,
    roots: &[&str],
    folders: &[&str],
) -> Result<BTreeSet<String>, String> {
    let assets = src.assets()?;
    let roots: Vec<String> = roots.iter().map(|r| (*r).to_owned()).collect();
    let deps = asset_deps::collect_in(src, &roots, &src.guids(&assets));
    if let Some((rel, from)) = deps.missing.iter().next() {
        let by = if from.is_empty() {
            String::new()
        } else {
            format!(" (used by {from})")
        };
        return Err(format!("{rel}{by} is missing from {}", src.describe()));
    }
    let mut files = deps.files;
    files.extend(
        assets
            .iter()
            .filter(|a| folders.iter().any(|f| a.starts_with(f)))
            .cloned(),
    );
    let parents: BTreeSet<String> = files
        .iter()
        .flat_map(|f| {
            Path::new(f)
                .ancestors()
                .skip(1)
                .map(|d| d.to_string_lossy().replace('\\', "/"))
                .filter(|d| d.starts_with("Assets/"))
                .collect::<Vec<_>>()
        })
        .collect();
    for d in parents {
        let m = format!("{d}.meta");
        if assets.contains(&m) {
            files.insert(m);
        }
    }
    Ok(files)
}

/// Copy [`files`] into `root` (a file already there is kept: copying twice is a no-op) and
/// take over the sample's project settings. Returns how many files were written.
pub(super) fn copy(src: &SampleFiles, root: &Path) -> Result<usize, String> {
    copy_files(src, root, files(src)?)
}

/// Copy `files` from the sample into `root` (keeping files already there) and take over the
/// sample's project settings (input axes, tags and layers…). Returns how many files were
/// written.
pub(super) fn copy_files(
    src: &SampleFiles,
    root: &Path,
    files: BTreeSet<String>,
) -> Result<usize, String> {
    let mut copied = 0;
    for rel in files {
        let dest = root.join(&rel);
        if dest.exists() {
            continue;
        }
        if let Some(dir) = dest.parent() {
            std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
        }
        std::fs::write(&dest, src.read(&rel)?).map_err(io_error("Could not write", &dest))?;
        copied += 1;
    }
    if src.is_file(SETTINGS) {
        let staged = root.join(samples::SAMPLE_SETTINGS);
        if let Some(dir) = staged.parent() {
            std::fs::create_dir_all(dir).map_err(io_error("Could not create the folder", dir))?;
        }
        std::fs::write(&staged, src.read(SETTINGS)?)
            .map_err(io_error("Could not write", &staged))?;
        samples::merge_settings(root)?;
    }
    Ok(copied)
}

/// Write the MeadowDemo template into `root` (a project folder with its settings), turned
/// towards the sun. Returns the Scene view to start with.
pub(super) fn create(root: &Path) -> Result<SavedView, String> {
    let source = samples::source().ok_or(
        "this install has no sample project to copy MeadowDemo from \
         (Examples/SampleProject next to the editor)",
    )?;
    create_from(root, &source)
}

pub(super) fn create_from(root: &Path, source: &Source) -> Result<SavedView, String> {
    copy(&SampleFiles::open(source)?, root)?;
    face_the_sun(root)
}

/// The Player of the copied scene turned to face the sun (horizontally: the controller keeps
/// the head level), and a Scene view just behind and above its eyes looking the same way,
/// slightly down. The sample's own scene is left alone.
fn face_the_sun(root: &Path) -> Result<SavedView, String> {
    let path = root.join(SCENE);
    let bad = |what: &str| format!("{}: {what}", path.display());
    let text = std::fs::read_to_string(&path).map_err(io_error("Could not read", &path))?;
    let mut doc: Value = serde_json::from_str(&text).map_err(|e| bad(&e.to_string()))?;
    let objects = doc["objects"]
        .as_array_mut()
        .ok_or_else(|| bad("no objects"))?;
    let find = |objects: &[Value], name: &str| {
        objects.iter().position(|o| {
            o["parent"].is_null() && o["components"]["GameObject"]["name"].as_str() == Some(name)
        })
    };
    let sun = find(objects, "Directional Light").ok_or_else(|| bad("no Directional Light"))?;
    let player = find(objects, "Player").ok_or_else(|| bad("no Player"))?;
    let sun_rotation = quat(&objects[sun]["components"]["Transform"]["rotation"]);
    let feet = vec3(&objects[player]["components"]["Transform"]["position"]);
    let eye = feet + Vec3::Y * EYE_HEIGHT;
    let trees = find(objects, "Terrain")
        .map(|t| tree_positions(root, &objects[t]))
        .unwrap_or_default();
    let yaw = clear_yaw(toward_sun_yaw(sun_rotation), eye, &trees);

    let rotation = Quat::from_rotation_y(yaw.to_radians());
    let transform = &mut objects[player]["components"]["Transform"];
    transform["rotation"] = serde_json::json!(rotation.to_array());
    transform["euler_hint"] = serde_json::json!([0.0, yaw, 0.0]);
    // Re-saved with the GUID table rebuilt from the copied meta files (same GUIDs).
    if let Some(o) = doc.as_object_mut() {
        o.remove(refs::KEY);
    }
    let text = serde_json::to_string_pretty(&doc).map_err(|e| bad(&e.to_string()))?;
    let text = refs::embed(&GuidDatabase::read(root), &text);
    std::fs::write(&path, text).map_err(io_error("Could not write", &path))?;

    let forward = rotation * Vec3::Z;
    let view = misty_engine::camera::euler_to_quat(Vec3::new(VIEW_PITCH, yaw, 0.0));
    Ok(SavedView {
        position: (eye - forward * 3.0 + Vec3::Y * 0.6).to_array(),
        rotation: view.to_array(),
    })
}

fn quat(v: &Value) -> Quat {
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    let q = Quat::from_xyzw(f(0), f(1), f(2), f(3));
    if q.length_squared() > 1e-6 {
        q.normalize()
    } else {
        Quat::IDENTITY
    }
}

fn vec3(v: &Value) -> Vec3 {
    let f = |i: usize| v[i].as_f64().unwrap_or(0.0) as f32;
    Vec3::new(f(0), f(1), f(2))
}

/// Yaw (degrees, 0 = +Z) of the horizontal direction towards the sun of a directional light
/// rotated by `sun` (its light travels along its +Z).
pub(super) fn toward_sun_yaw(sun: Quat) -> f32 {
    let to_sun = -(sun * Vec3::Z);
    to_sun.x.atan2(to_sun.z).to_degrees().rem_euclid(360.0)
}

/// World positions (on the ground plane, y = 0) of the trees of a scene's terrain object.
fn tree_positions(root: &Path, terrain: &Value) -> Vec<Vec3> {
    let c = &terrain["components"];
    let Some(rel) = c["Terrain"]["terrain_data"].as_str() else {
        return Vec::new();
    };
    let Ok(data) = TerrainData::load(&root.join(rel)) else {
        return Vec::new();
    };
    let corner = vec3(&c["Transform"]["position"]);
    data.trees
        .instances
        .iter()
        .map(|t| corner + Vec3::new(t.position.x * data.size.x, 0.0, t.position.y * data.size.z))
        .collect()
}

/// How far ahead a tree spoils the first view.
const CLEAR_AHEAD: f32 = 80.0;

/// Whether a tree stands in front of `eye` looking along `yaw`: within [`CLEAR_AHEAD`] and
/// close to the line of sight (2 m either side, widening with distance for the crowns).
pub(super) fn blocked(yaw: f32, eye: Vec3, trees: &[Vec3]) -> bool {
    let (s, c) = yaw.to_radians().sin_cos();
    let forward = Vec2::new(s, c);
    trees.iter().any(|t| {
        let d = Vec2::new(t.x - eye.x, t.z - eye.z);
        let along = d.dot(forward);
        let side = d.perp_dot(forward).abs();
        along > 0.0 && along < CLEAR_AHEAD && side < 2.0 + along * 0.08
    })
}

/// `toward` when the view that way is clear of trees, else the nearest clear yaw within 30°
/// (3° steps, alternating sides).
pub(super) fn clear_yaw(toward: f32, eye: Vec3, trees: &[Vec3]) -> f32 {
    (0..=10)
        .flat_map(|i| [i as f32 * 3.0, i as f32 * -3.0])
        .map(|d| (toward + d).rem_euclid(360.0))
        .find(|&yaw| !blocked(yaw, eye, trees))
        .unwrap_or(toward)
}
