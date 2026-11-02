//! Asset database: material assets found under the project's `Assets` folder.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use misty_engine::assets::Material;
use misty_engine::render::MaterialParams;

/// Name shown for renderers without a material.
pub const DEFAULT_MATERIAL: &str = "Default-Material";

pub struct AssetDatabase {
    root: PathBuf,
    /// Project-relative path ("Assets/Materials/Red.mat") → material.
    pub materials: BTreeMap<String, Material>,
}

impl AssetDatabase {
    pub fn open(root: &Path) -> Self {
        let mut db = Self {
            root: root.to_path_buf(),
            materials: BTreeMap::new(),
        };
        db.refresh();
        db
    }

    /// Re-scan the Assets folder for `.mat` files.
    pub fn refresh(&mut self) {
        self.materials.clear();
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
