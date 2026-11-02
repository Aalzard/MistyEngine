//! Asset database: material assets found under the project's `Assets` folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use std::time::SystemTime;

use misty_engine::assets::Material;
use misty_engine::render::MaterialParams;
use misty_engine::scene::ScriptValue;

/// Name shown for renderers without a material.
pub const DEFAULT_MATERIAL: &str = "Default-Material";

/// A script asset: its public fields (or the error that stopped them being read).
#[derive(Clone, Debug)]
pub struct ScriptMeta {
    pub fields: Result<Vec<(String, ScriptValue)>, String>,
    modified: Option<SystemTime>,
}

pub struct AssetDatabase {
    root: PathBuf,
    /// Project-relative path ("Assets/Materials/Red.mat") → material.
    pub materials: BTreeMap<String, Material>,
    /// Project-relative path ("Assets/Scripts/Rotator.lua") → fields.
    pub scripts: BTreeMap<String, ScriptMeta>,
    last_poll: Option<std::time::Instant>,
}

impl AssetDatabase {
    pub fn open(root: &Path) -> Self {
        let mut db = Self {
            root: root.to_path_buf(),
            materials: BTreeMap::new(),
            scripts: BTreeMap::new(),
            last_poll: None,
        };
        db.refresh();
        db
    }

    /// Re-scan the Assets folder for `.mat` and `.lua` files.
    pub fn refresh(&mut self) {
        self.materials.clear();
        let old_scripts = std::mem::take(&mut self.scripts);
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
                } else if path.extension().is_some_and(|e| e == "lua") {
                    let rel = self.relative(&path);
                    let meta = match old_scripts.get(&rel) {
                        Some(m) if m.modified == mtime(&path) => m.clone(),
                        _ => inspect(&path, &rel),
                    };
                    self.scripts.insert(rel, meta);
                }
            }
        }
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
