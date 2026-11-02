//! Project window model: browses the real `Assets` folder on disk.

use std::path::{Path, PathBuf};

#[derive(Clone, Debug)]
pub struct AssetEntry {
    pub path: PathBuf,
    pub name: String,
    pub is_dir: bool,
}

impl AssetEntry {
    pub fn icon(&self) -> &'static str {
        if self.is_dir {
            return "🗁";
        }
        match self.path.extension().and_then(|e| e.to_str()).unwrap_or("") {
            "scene" | "mscene" => "🎬",
            "rs" | "lua" | "wasm" => "📜",
            "mat" => "⚪",
            "png" | "jpg" | "jpeg" | "tga" => "🖼",
            "obj" | "gltf" | "glb" | "fbx" => "📦",
            "wav" | "ogg" | "mp3" => "🔊",
            _ => "📄",
        }
    }
}

/// Folder tree node for the left column.
#[derive(Clone, Debug)]
pub struct FolderNode {
    pub path: PathBuf,
    pub name: String,
    pub children: Vec<FolderNode>,
}

pub struct ProjectBrowser {
    pub root: PathBuf,
    pub current: PathBuf,
    pub selected: Option<PathBuf>,
    pub search: String,
    /// Icon size slider; at the minimum the grid becomes a list.
    pub icon_size: f32,
    pub tree: FolderNode,
    pub entries: Vec<AssetEntry>,
}

impl ProjectBrowser {
    pub fn open(root: PathBuf) -> Self {
        let assets = root.join("Assets");
        let _ = std::fs::create_dir_all(&assets);
        let mut browser = Self {
            root,
            current: assets.clone(),
            selected: None,
            search: String::new(),
            icon_size: 0.5,
            tree: FolderNode {
                path: assets.clone(),
                name: "Assets".into(),
                children: Vec::new(),
            },
            entries: Vec::new(),
        };
        browser.refresh();
        browser
    }

    pub fn assets_dir(&self) -> PathBuf {
        self.root.join("Assets")
    }

    /// Re-read the folder tree and the current folder's contents.
    pub fn refresh(&mut self) {
        self.tree = build_tree(&self.assets_dir(), "Assets");
        self.entries = list_dir(&self.current);
    }

    pub fn open_folder(&mut self, path: PathBuf) {
        self.current = path;
        self.selected = None;
        self.entries = list_dir(&self.current);
    }

    /// Breadcrumb segments from `Assets` to the current folder.
    pub fn breadcrumb(&self) -> Vec<(String, PathBuf)> {
        let assets = self.assets_dir();
        let mut out = vec![("Assets".to_owned(), assets.clone())];
        if let Ok(rel) = self.current.strip_prefix(&assets) {
            let mut acc = assets;
            for part in rel.components() {
                acc = acc.join(part);
                out.push((part.as_os_str().to_string_lossy().into_owned(), acc.clone()));
            }
        }
        out
    }

    /// Path shown in the bottom bar, relative to the project root.
    pub fn display_path(&self, path: &Path) -> String {
        path.strip_prefix(&self.root)
            .unwrap_or(path)
            .to_string_lossy()
            .replace('\\', "/")
    }

    /// Recursive name search under Assets.
    pub fn search_results(&self) -> Vec<AssetEntry> {
        let needle = self.search.to_lowercase();
        let mut out = Vec::new();
        collect_matches(&self.assets_dir(), &needle, &mut out);
        out
    }
}

fn list_dir(dir: &Path) -> Vec<AssetEntry> {
    let mut out: Vec<AssetEntry> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let name = e.file_name().to_string_lossy().into_owned();
            // Hidden files and engine metadata are not shown.
            if name.starts_with('.') || name.ends_with(".meta") {
                return None;
            }
            Some(AssetEntry {
                is_dir: e.path().is_dir(),
                path: e.path(),
                name,
            })
        })
        .collect();
    // Folders first, then alphabetical.
    out.sort_by(|a, b| {
        b.is_dir
            .cmp(&a.is_dir)
            .then(a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

fn build_tree(dir: &Path, name: &str) -> FolderNode {
    let children = list_dir(dir)
        .into_iter()
        .filter(|e| e.is_dir)
        .map(|e| build_tree(&e.path, &e.name))
        .collect();
    FolderNode {
        path: dir.to_path_buf(),
        name: name.to_owned(),
        children,
    }
}

fn collect_matches(dir: &Path, needle: &str, out: &mut Vec<AssetEntry>) {
    for entry in list_dir(dir) {
        if entry.name.to_lowercase().contains(needle) {
            out.push(entry.clone());
        }
        if entry.is_dir {
            collect_matches(&entry.path, needle, out);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lists_sample_assets() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let browser = ProjectBrowser::open(root);
        let names: Vec<_> = browser.entries.iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"Scenes"), "got {names:?}");
        assert!(browser.tree.children.iter().any(|c| c.name == "Scripts"));
    }
}
