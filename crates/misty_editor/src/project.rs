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
            "prefab" => crate::state::PREFAB_ICON,
            "png" | "jpg" | "jpeg" | "tga" | "hdr" | "exr" => "🖼",
            "obj" | "gltf" | "glb" | "fbx" => "📦",
            "wav" | "ogg" | "mp3" => "🔊",
            "anim" => "⏵",
            "timeline" => crate::panels::timeline::ICON,
            "controller" => crate::panels::animator::CONTROLLER_ICON,
            "mask" => crate::panels::animator::MASK_ICON,
            "avatar" => crate::panels::animator::AVATAR_ICON,
            "mixer" => crate::panels::audio_mixer::MIXER_ICON,
            "inputactions" => crate::panels::input_actions::ICON,
            "terrain" => "⛰",
            "tree" => crate::terrain_trees_ui::TREE_ICON,
            "navmesh" => "🚶",
            "preset" => crate::presets::PRESET_ICON,
            "tile" => "▦",
            "palette" => "🎨",
            "buildprofile" => "🔨",
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

impl AssetEntry {
    /// Images with a thumbnail: textures and HDR images (`.hdr`, `.exr`).
    pub fn is_texture(&self) -> bool {
        let path = self.path.to_string_lossy();
        !self.is_dir
            && (misty_engine::assets::is_texture(&path) || misty_engine::hdri::is_hdri(&path))
    }

    /// Icon tint in the Project window (prefabs are blue).
    pub fn icon_color(&self) -> egui::Color32 {
        if self.path.extension().is_some_and(|e| e == "prefab") {
            crate::theme::PREFAB
        } else {
            egui::Color32::from_gray(200)
        }
    }
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
    /// Folder contents area last frame (drop target for Hierarchy rows -> new prefab).
    pub drop_rect: egui::Rect,
    /// Texture previews shown in the grid.
    pub thumbnails: crate::thumbnails::Thumbnails,
    /// Asset being renamed inline and the name typed so far (without extension).
    pub renaming: Option<(PathBuf, String)>,
    /// Asset waiting for the Delete confirmation.
    pub confirm_delete: Option<PathBuf>,
    /// Keyboard focus: the last click landed in the Project window.
    pub focused: bool,
    /// Folders open in the folder tree (Assets starts open).
    pub expanded: std::collections::HashSet<PathBuf>,
    /// The project's packages (`Packages/<name>` folders), shown under the Packages root.
    pub packages: Vec<FolderNode>,
    /// Package folders that came from a registry: no renaming, moving or deleting inside.
    pub read_only: Vec<PathBuf>,
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
            drop_rect: egui::Rect::NOTHING,
            thumbnails: Default::default(),
            renaming: None,
            confirm_delete: None,
            focused: false,
            expanded: [assets.clone()].into(),
            packages: Vec::new(),
            read_only: Vec::new(),
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
        self.packages = package_trees(&self.packages_dir());
        self.read_only = crate::packages::read_only_roots(&self.root);
    }

    pub fn packages_dir(&self) -> PathBuf {
        self.root.join("Packages")
    }

    /// Inside a registry package (read-only).
    pub fn is_read_only(&self, path: &Path) -> bool {
        self.read_only.iter().any(|r| path.starts_with(r))
    }

    pub fn open_folder(&mut self, path: PathBuf) {
        // The folder tree shows where we are: open every folder above it.
        let assets = self.assets_dir();
        let packages = self.packages_dir();
        for dir in path.ancestors().skip(1) {
            if !dir.starts_with(&assets) && !dir.starts_with(&packages) {
                break;
            }
            self.expanded.insert(dir.to_path_buf());
        }
        self.current = path;
        self.selected = None;
        self.entries = list_dir(&self.current);
    }

    /// Breadcrumb segments from `Assets` to the current folder.
    pub fn breadcrumb(&self) -> Vec<(String, PathBuf)> {
        let packages = self.packages_dir();
        if let Ok(rel) = self.current.strip_prefix(&packages) {
            let mut out = vec![("Packages".to_owned(), packages.clone())];
            let mut acc = packages;
            for part in rel.components() {
                acc = acc.join(part);
                out.push((part.as_os_str().to_string_lossy().into_owned(), acc.clone()));
            }
            return out;
        }
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

    /// Start inline renaming `path` (the field shows the name without extension).
    pub fn start_rename(&mut self, path: PathBuf) {
        let stem = if path.is_dir() {
            path.file_name()
        } else {
            path.file_stem()
        };
        let name = stem
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        self.renaming = Some((path, name));
    }

    /// Recursive search under Assets: name words plus `t:Type` filters (see [`SearchFilter`]).
    pub fn search_results(&self) -> Vec<AssetEntry> {
        let filter = SearchFilter::parse(&self.search);
        let mut out = Vec::new();
        collect_matches(&self.assets_dir(), &filter, &mut out);
        out
    }
}

/// Project search: words must all appear in the name; `t:Material t:Prefab` keeps those types.
/// Parsing and word matching are shared with the search palette (see [`crate::search`]).
#[derive(Debug, Default, PartialEq)]
pub struct SearchFilter {
    pub words: Vec<String>,
    /// Lower-case type names after `t:`.
    pub types: Vec<String>,
}

impl SearchFilter {
    pub fn parse(text: &str) -> Self {
        let q = crate::search::query::Query::parse(text);
        Self {
            words: q.words,
            types: q.types,
        }
    }

    pub fn matches(&self, entry: &AssetEntry) -> bool {
        use crate::search::fuzzy::{Mode, match_words};
        match_words(&self.words, &entry.name, Mode::Contains).is_some()
            && (self.types.is_empty() || self.types.iter().any(|t| asset_type(entry) == *t))
    }
}

/// Type name used by `t:` filters (see [`type_for_path`]).
pub fn asset_type(entry: &AssetEntry) -> &'static str {
    type_for_path(&entry.path, entry.is_dir)
}

/// Type name used by `t:` filters ("material", "script", "scene", "prefab", "texture",
/// "model", "audioclip", "animationclip", "folder", …), "" for other files.
pub fn type_for_path(path: &Path, is_dir: bool) -> &'static str {
    if is_dir {
        return "folder";
    }
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
        .as_str()
    {
        "mat" => "material",
        "lua" | "rs" | "wasm" => "script",
        "scene" | "mscene" => "scene",
        "prefab" => "prefab",
        "png" | "jpg" | "jpeg" | "tga" | "hdr" | "exr" => "texture",
        "obj" | "gltf" | "glb" | "fbx" => "model",
        "wav" | "ogg" | "mp3" => "audioclip",
        "anim" => "animationclip",
        "timeline" => "timelineasset",
        "controller" => "animatorcontroller",
        "mask" => "avatarmask",
        "avatar" => "avatar",
        "mixer" => "audiomixer",
        "inputactions" => "inputactionasset",
        "shader" => "shader",
        "terrain" => "terraindata",
        "preset" => "preset",
        _ => "",
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

/// One node per `Packages/<folder>` with a package.json, named by its display name.
fn package_trees(dir: &Path) -> Vec<FolderNode> {
    let mut out: Vec<FolderNode> = list_dir(dir)
        .into_iter()
        .filter(|e| e.is_dir && e.path.join("package.json").is_file())
        .map(|e| {
            let title = crate::packages::manifest::PackageJson::read(&e.path)
                .map(|p| p.title().to_owned())
                .unwrap_or(e.name.clone());
            build_tree(&e.path, &title)
        })
        .collect();
    out.sort_by_key(|n| n.name.to_lowercase());
    out
}

fn collect_matches(dir: &Path, filter: &SearchFilter, out: &mut Vec<AssetEntry>) {
    for entry in list_dir(dir) {
        if filter.matches(&entry) {
            out.push(entry.clone());
        }
        if entry.is_dir {
            collect_matches(&entry.path, filter, out);
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

    #[test]
    fn type_filters_narrow_the_search() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
        let mut browser = ProjectBrowser::open(root);
        let kinds = |b: &ProjectBrowser| {
            b.search_results()
                .iter()
                .map(|e| asset_type(e))
                .collect::<std::collections::BTreeSet<_>>()
        };
        for (query, ty) in [
            ("t:Material", "material"),
            ("t:Script", "script"),
            ("t:scene", "scene"),
            ("t:Texture", "texture"),
            ("t:Model", "model"),
            ("t:AudioClip", "audioclip"),
        ] {
            browser.search = query.into();
            assert_eq!(
                kinds(&browser).into_iter().collect::<Vec<_>>(),
                [ty],
                "{query}"
            );
        }
        browser.search = "t:Material t:Scene".into();
        assert_eq!(kinds(&browser).len(), 2);
        browser.search = "sample t:Scene".into();
        let names: Vec<_> = browser
            .search_results()
            .into_iter()
            .map(|e| e.name)
            .collect();
        assert!(names.contains(&"SampleScene.scene".to_owned()), "{names:?}");
        assert!(names.iter().all(|n| n.to_lowercase().contains("sample")));
        assert_eq!(
            SearchFilter::parse("Red  T:Prefab"),
            SearchFilter {
                words: vec!["red".into()],
                types: vec!["prefab".into()],
            }
        );
    }
}
