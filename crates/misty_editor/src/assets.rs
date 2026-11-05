//! Asset database: materials, scripts, audio clips, prefabs, textures and models found under the
//! project's `Assets` folder.

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use std::time::SystemTime;

use misty_engine::assets::Material;
use misty_engine::models;
use misty_engine::render::MaterialParams;
use misty_engine::scene::ScriptValue;
use misty_engine::scene::prefab::Prefab;

/// Name shown for renderers without a material.
pub const DEFAULT_MATERIAL: &str = "Default-Material";

/// A script asset: its public fields (or the error that stopped them being read).
#[derive(Clone, Debug)]
pub struct ScriptMeta {
    pub fields: Result<Vec<(String, ScriptValue)>, String>,
    modified: Option<SystemTime>,
}

/// A prefab read from disk (or why it couldn't be), with the file time it was read at.
type CachedPrefab = (Option<SystemTime>, Result<Rc<Prefab>, String>);

pub struct AssetDatabase {
    root: PathBuf,
    /// Project-relative path ("Assets/Materials/Red.mat") → material.
    pub materials: BTreeMap<String, Material>,
    /// Project-relative path ("Assets/Scripts/Rotator.lua") → fields.
    pub scripts: BTreeMap<String, ScriptMeta>,
    /// Project-relative paths of animation clips (.anim).
    pub anims: BTreeSet<String>,
    /// Project-relative paths of Animator Controllers (.controller).
    pub controllers: BTreeSet<String>,
    /// Project-relative paths of Timeline assets (.timeline).
    pub timelines: BTreeSet<String>,
    /// Project-relative paths of Effect Graphs (.effect).
    pub effects: BTreeSet<String>,
    /// Project-relative paths of audio clips (.wav/.ogg/.mp3).
    pub clips: BTreeSet<String>,
    /// Project-relative paths of Audio Mixers (.mixer).
    pub mixers: BTreeSet<String>,
    /// Project-relative path → preset (`.preset`).
    pub presets: BTreeMap<String, misty_engine::preset::Preset>,
    /// Prefabs read so far, re-read when the file changes.
    prefabs: BTreeMap<String, CachedPrefab>,
    /// Project-relative texture paths (`.png`, `.jpg`).
    pub textures: Vec<String>,
    /// Project-relative shader assets (`.shader`).
    pub shaders: Vec<String>,
    /// Project-relative HDR images (`.hdr`, `.exr`) usable as HDRI skies.
    pub hdris: Vec<String>,
    /// Project-relative model path → its mesh names (or why it could not be imported).
    pub models: BTreeMap<String, Result<Vec<String>, String>>,
    /// Import messages for the Console (drained by the editor).
    pub messages: Vec<String>,
    last_poll: Option<std::time::Instant>,
    /// Last check of the GUID map against the Assets folder.
    last_guid_poll: Option<std::time::Instant>,
}

impl AssetDatabase {
    pub fn open(root: &Path) -> Self {
        // Engine-side loaders (renderer textures, models, physics) resolve "Assets/…" against it.
        misty_engine::assets::set_project_root(root);
        let mut db = Self {
            root: root.to_path_buf(),
            materials: BTreeMap::new(),
            scripts: BTreeMap::new(),
            clips: BTreeSet::new(),
            mixers: BTreeSet::new(),
            anims: BTreeSet::new(),
            controllers: BTreeSet::new(),
            timelines: BTreeSet::new(),
            effects: BTreeSet::new(),
            presets: BTreeMap::new(),
            prefabs: BTreeMap::new(),
            textures: Vec::new(),
            shaders: Vec::new(),
            hdris: Vec::new(),
            models: BTreeMap::new(),
            messages: Vec::new(),
            last_poll: None,
            last_guid_poll: None,
        };
        db.refresh();
        db
    }

    /// Re-scan the Assets folder for materials, scripts, audio clips, textures and models.
    /// New models get their materials extracted (once) as `.mat` files next to them.
    /// Messages about imported or broken models go to [`Self::messages`].
    pub fn refresh(&mut self) {
        // Keep the GUID map in step: new assets get meta files, copies get new GUIDs.
        if let Some(report) = misty_engine::asset_db::rescan() {
            self.note_guids(report);
        }
        self.materials.clear();
        self.presets.clear();
        self.clips.clear();
        self.mixers.clear();
        self.anims.clear();
        self.controllers.clear();
        self.timelines.clear();
        self.effects.clear();
        self.textures.clear();
        self.shaders.clear();
        self.hdris.clear();
        self.models.clear();
        let old_scripts = std::mem::take(&mut self.scripts);
        let mut model_files = Vec::new();
        let mut stack = vec![self.root.join("Assets")];
        while let Some(dir) = stack.pop() {
            for entry in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let path = entry.path();
                if path.is_dir() {
                    stack.push(path);
                } else if path.extension().is_some_and(|e| e == "mat")
                    && let Ok(m) = Material::load(&path)
                {
                    self.materials.insert(self.relative(&path), m);
                } else if path.extension().is_some_and(|e| e == "preset") {
                    match misty_engine::preset::Preset::load(&path) {
                        Ok(p) => {
                            self.presets.insert(self.relative(&path), p);
                        }
                        Err(e) => self
                            .messages
                            .push(format!("Could not read {}: {e}", self.relative(&path))),
                    }
                } else if path.extension().is_some_and(|e| e == "lua") {
                    let rel = self.relative(&path);
                    let meta = match old_scripts.get(&rel) {
                        Some(m) if m.modified == mtime(&path) => m.clone(),
                        _ => inspect(&path, &rel),
                    };
                    self.scripts.insert(rel, meta);
                } else if misty_engine::animation::is_clip(&path.to_string_lossy()) {
                    self.anims.insert(self.relative(&path));
                } else if misty_engine::animation::is_controller(&path.to_string_lossy()) {
                    self.controllers.insert(self.relative(&path));
                } else if misty_engine::timeline::is_timeline(&path.to_string_lossy()) {
                    self.timelines.insert(self.relative(&path));
                } else if misty_engine::vfx::is_effect_path(&path.to_string_lossy()) {
                    self.effects.insert(self.relative(&path));
                } else if misty_engine::audio::is_clip(&path.to_string_lossy()) {
                    self.clips.insert(self.relative(&path));
                } else if misty_engine::audio::is_mixer(&path.to_string_lossy()) {
                    self.mixers.insert(self.relative(&path));
                } else {
                    let rel = self.relative(&path);
                    if misty_engine::assets::is_texture(&rel) {
                        self.textures.push(rel);
                    } else if misty_engine::shader_asset::is_shader_path(&rel) {
                        self.shaders.push(rel);
                    } else if misty_engine::hdri::is_hdri(&rel) {
                        self.hdris.push(rel);
                    } else if models::is_model(&rel) {
                        model_files.push(rel);
                    }
                }
            }
        }
        self.textures.sort();
        self.shaders.sort();
        self.hdris.sort();
        let messages = &mut self.messages;
        for rel in model_files {
            let entry = models::model(&rel).map(|m| {
                match models::extract_materials(&rel, &m) {
                    Ok(created) => {
                        for c in created {
                            if misty_engine::assets::is_texture(&c) {
                                self.textures.push(c.clone());
                            } else if let Ok(mat) = Material::load(&self.root.join(&c)) {
                                self.materials.insert(c.clone(), mat);
                            }
                            messages.push(format!("Imported {c} from {rel}"));
                        }
                    }
                    Err(e) => messages.push(format!("Could not extract materials of {rel}: {e}")),
                }
                match models::extract_animations(&rel, &m) {
                    Ok(created) => {
                        for c in created {
                            messages.push(format!("Imported {c} from {rel}"));
                        }
                    }
                    Err(e) => messages.push(format!("Could not extract animations of {rel}: {e}")),
                }
                m.meshes.iter().map(|mesh| mesh.name.clone()).collect()
            });
            if let Err(e) = &entry {
                messages.push(format!("Could not import model: {e}"));
            }
            self.models.insert(rel, entry);
        }
        self.textures.sort();
    }

    /// Material asset for each material of a model (extracted next to it), by model material index.
    pub fn model_materials(&self, model_rel: &str, model: &models::Model) -> Vec<Option<String>> {
        (0..model.materials.len())
            .map(|i| {
                models::material_asset_path(model_rel, model, i)
                    .filter(|p| self.materials.contains_key(p))
            })
            .collect()
    }

    /// "Assets/…" path with forward slashes.
    pub fn relative(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    pub fn absolute(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Re-read scripts whose files changed (at most twice a second). Returns new compile errors.
    pub fn poll_scripts(&mut self) -> Vec<String> {
        let now = std::time::Instant::now();
        if self
            .last_poll
            .is_some_and(|t| now.duration_since(t).as_secs_f32() < 0.5)
        {
            return Vec::new();
        }
        self.last_poll = Some(now);
        let mut errors = Vec::new();
        let paths: Vec<String> = self.scripts.keys().cloned().collect();
        for rel in paths {
            let abs = self.absolute(&rel);
            let changed = self
                .scripts
                .get(&rel)
                .is_some_and(|m| m.modified != mtime(&abs));
            if changed {
                let meta = inspect(&abs, &rel);
                if let Err(e) = &meta.fields {
                    errors.push(e.clone());
                }
                self.scripts.insert(rel, meta);
            }
        }
        errors
    }

    /// GUID file watcher (at most every 2 s): rescan when assets appeared or vanished outside
    /// the editor. Returns true when it did (the caller refreshes the Project window).
    pub fn poll_guids(&mut self) -> bool {
        let now = std::time::Instant::now();
        if self
            .last_guid_poll
            .is_some_and(|t| now.duration_since(t).as_secs_f32() < 2.0)
        {
            return false;
        }
        self.last_guid_poll = Some(now);
        match misty_engine::asset_db::poll() {
            Some(report) => {
                self.note_guids(report);
                true
            }
            None => false,
        }
    }

    /// Console messages for copies that got a new GUID and unreadable meta files.
    pub fn note_guids(&mut self, report: misty_engine::asset_db::ScanReport) {
        for rel in report.reassigned {
            self.messages.push(format!(
                "{rel} shared another asset's GUID; it was given a new one"
            ));
        }
        self.messages.extend(report.errors);
    }

    /// Drop cached prefabs (read again on next use).
    pub fn forget_prefabs(&mut self) {
        self.prefabs.clear();
    }

    /// Create a new script from the template in `folder`; returns its relative path.
    pub fn create_script(&mut self, folder: &Path) -> Result<String, String> {
        let (name, path) = (0..)
            .map(|n| {
                let name = if n == 0 {
                    "NewBehaviourScript".to_owned()
                } else {
                    format!("NewBehaviourScript{n}")
                };
                let path = folder.join(format!("{name}.lua"));
                (name, path)
            })
            .find(|(_, p)| !p.exists())
            .expect("some name is free");
        std::fs::write(&path, misty_engine::scripting::template(&name))
            .map_err(|e| e.to_string())?;
        let rel = self.relative(&path);
        self.scripts.insert(rel.clone(), inspect(&path, &rel));
        Ok(rel)
    }

    /// A prefab asset (cached; re-read when its file changes).
    pub fn prefab(&mut self, rel: &str) -> Result<Rc<Prefab>, String> {
        let abs = self.absolute(rel);
        let modified = mtime(&abs);
        match self.prefabs.get(rel) {
            Some((m, p)) if *m == modified && modified.is_some() => p.clone(),
            _ => {
                let p = Prefab::load(&abs).map(Rc::new);
                self.prefabs.insert(rel.to_owned(), (modified, p.clone()));
                p
            }
        }
    }

    /// Write a prefab asset (Apply All) and update the cache.
    pub fn save_prefab(&mut self, rel: &str, prefab: &Prefab) -> Result<(), String> {
        let abs = self.absolute(rel);
        prefab.save(&abs)?;
        self.prefabs
            .insert(rel.to_owned(), (mtime(&abs), Ok(Rc::new(prefab.clone()))));
        Ok(())
    }

    /// Display name of a material path ("Red"), or Default-Material.
    pub fn material_name(rel: Option<&str>) -> String {
        match rel {
            Some(r) => Path::new(r)
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default(),
            None => DEFAULT_MATERIAL.to_owned(),
        }
    }

    pub fn params(&self, rel: Option<&str>) -> MaterialParams {
        rel.and_then(|r| self.materials.get(r))
            .map(Material::params)
            .unwrap_or_default()
    }

    /// Create "New Material.mat" (numbered if taken) in `folder`; returns its relative path.
    pub fn create_material(&mut self, folder: &Path) -> Result<String, String> {
        let path = (0..)
            .map(|n| {
                let name = if n == 0 {
                    "New Material.mat".to_owned()
                } else {
                    format!("New Material {n}.mat")
                };
                folder.join(name)
            })
            .find(|p| !p.exists())
            .expect("some name is free");
        let m = Material::default();
        m.save(&path)?;
        let rel = self.relative(&path);
        self.materials.insert(rel.clone(), m);
        Ok(rel)
    }

    /// Presets whose target is `target` (component name or asset kind), by path.
    pub fn presets_for(&self, target: &str) -> Vec<String> {
        self.presets
            .iter()
            .filter(|(_, p)| p.target == target)
            .map(|(rel, _)| rel.clone())
            .collect()
    }

    /// Write a preset and update the cache.
    pub fn save_preset(
        &mut self,
        rel: &str,
        p: misty_engine::preset::Preset,
    ) -> Result<(), String> {
        p.save(&self.absolute(rel))?;
        self.presets.insert(rel.to_owned(), p);
        Ok(())
    }

    /// Store an edited material and write it to disk.
    pub fn update_material(&mut self, rel: &str, m: Material) -> Result<(), String> {
        m.save(&self.absolute(rel))?;
        self.materials.insert(rel.to_owned(), m);
        Ok(())
    }
}

/// Open a file with the operating system's default application.
pub fn open_external(path: &Path) -> std::io::Result<()> {
    #[cfg(target_os = "windows")]
    let mut cmd = {
        let mut c = std::process::Command::new("cmd");
        c.args(["/C", "start", ""]).arg(path);
        c
    };
    #[cfg(target_os = "macos")]
    let mut cmd = {
        let mut c = std::process::Command::new("open");
        c.arg(path);
        c
    };
    #[cfg(not(any(target_os = "windows", target_os = "macos")))]
    let mut cmd = {
        let mut c = std::process::Command::new("xdg-open");
        c.arg(path);
        c
    };
    cmd.spawn().map(|_| ())
}

fn mtime(path: &Path) -> Option<SystemTime> {
    std::fs::metadata(path).and_then(|m| m.modified()).ok()
}

fn inspect(path: &Path, rel: &str) -> ScriptMeta {
    let fields = std::fs::read_to_string(path)
        .map_err(|e| e.to_string())
        .and_then(|src| misty_engine::scripting::inspect_fields(&src, rel));
    ScriptMeta {
        fields,
        modified: mtime(path),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_find_and_update_materials() {
        let root = std::env::temp_dir().join(format!("misty_assets_test_{}", std::process::id()));
        let folder = root.join("Assets").join("Materials");
        std::fs::create_dir_all(&folder).unwrap();
        let mut db = AssetDatabase::open(&root);
        let a = db.create_material(&folder).unwrap();
        let b = db.create_material(&folder).unwrap();
        assert_eq!(a, "Assets/Materials/New Material.mat");
        assert_eq!(b, "Assets/Materials/New Material 1.mat");

        let mut m = db.materials[&a].clone();
        m.color = [1.0, 0.0, 0.0, 1.0];
        db.update_material(&a, m).unwrap();
        let reopened = AssetDatabase::open(&root);
        assert_eq!(reopened.params(Some(&a)).color, [1.0, 0.0, 0.0, 1.0]);
        assert_eq!(AssetDatabase::material_name(Some(&a)), "New Material");
        assert_eq!(reopened.params(None), MaterialParams::default());
        std::fs::remove_dir_all(root).ok();
    }
}

#[cfg(test)]
mod sample_tests {
    use super::*;

    /// The sample project's scripts must load and expose their fields.
    #[test]
    fn sample_scripts_inspect_cleanly() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let db = AssetDatabase::open(&root);
        let rotator = &db.scripts["Assets/Scripts/Rotator.lua"];
        let names: Vec<_> = rotator
            .fields
            .as_ref()
            .unwrap()
            .iter()
            .map(|(n, _)| n.clone())
            .collect();
        assert_eq!(names, ["speed", "axis"]);
        let player = &db.scripts["Assets/Scripts/PlayerController.lua"];
        assert!(player.fields.is_ok(), "{:?}", player.fields);
    }
}
