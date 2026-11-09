//! Build Report: what a build packed, by asset type, and its largest assets. The editor shows
//! it after a build and saves it next to the game (`BuildReport.txt`) and in the project
//! (`Library/BuildReports/<Profile>.json`, read back by the Build Profiles window).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// Project-relative folder of saved reports.
pub const REPORT_DIR: &str = "Library/BuildReports";
/// Readable report written into the build folder.
pub const TEXT_FILE: &str = "BuildReport.txt";
/// Largest assets kept in a report.
pub const LARGEST: usize = 20;

/// Asset types of the report, in display order.
pub const CATEGORIES: [&str; 16] = [
    "Textures",
    "Compressed Textures",
    "Meshes",
    "Audio",
    "Video",
    "Scenes",
    "Prefabs",
    "Materials",
    "Shaders",
    "Scripts",
    "Animation",
    "Terrain",
    "Fonts",
    "Settings",
    "Import Settings",
    "Other",
];

/// The report type of a packed file.
pub fn category(rel: &str) -> &'static str {
    let lower = rel.to_ascii_lowercase();
    let ext = lower.rsplit_once('.').map_or("", |(_, e)| e);
    if lower.starts_with("library/texturecache/") {
        return "Compressed Textures";
    }
    if lower.starts_with("projectsettings/") || lower == "assetguids.json" {
        return "Settings";
    }
    match ext {
        "meta" => "Import Settings",
        "png" | "jpg" | "jpeg" | "hdr" | "exr" | "tga" | "bmp" | "psd" | "dds" | "ktx2" => {
            "Textures"
        }
        "gltf" | "glb" | "bin" | "fbx" | "obj" | "vgeo" | "mesh" => "Meshes",
        "wav" | "ogg" | "mp3" | "flac" => "Audio",
        "mp4" | "webm" | "mkv" | "mov" | "avi" | "y4m" => "Video",
        "scene" => "Scenes",
        "prefab" => "Prefabs",
        "mat" | "physicmaterial" | "physicsmaterial" => "Materials",
        "shader" | "shadergraph" | "wgsl" | "subgraph" => "Shaders",
        "lua" => "Scripts",
        "anim" | "controller" | "mask" | "timeline" | "playable" => "Animation",
        "terrain" | "terrainlayer" => "Terrain",
        "ttf" | "otf" => "Fonts",
        _ if lower.starts_with("library/") && lower.contains("virtual") => "Meshes",
        _ => "Other",
    }
}

/// Totals of one asset type.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct CategorySize {
    pub name: String,
    pub files: usize,
    /// Bytes before pak compression.
    pub raw_bytes: u64,
    /// Bytes in the pak.
    pub packed_bytes: u64,
}

/// One packed file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AssetSize {
    pub path: String,
    pub category: String,
    pub raw_bytes: u64,
    pub packed_bytes: u64,
}

/// What a build produced.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildReport {
    pub product: String,
    pub profile: String,
    /// Target label ("Windows x64").
    pub target: String,
    pub server: bool,
    pub development: bool,
    pub output: PathBuf,
    /// Unix time of the build.
    pub time: u64,
    pub seconds: f32,
    /// Executable bytes.
    pub executable_bytes: u64,
    /// Bytes of the pak file.
    pub pak_bytes: u64,
    /// Bytes of the whole build folder (executable, data, bundles).
    pub total_bytes: u64,
    pub files: usize,
    /// Files a dedicated server left out.
    pub stripped: usize,
    pub stripped_bytes: u64,
    pub warnings: usize,
    pub categories: Vec<CategorySize>,
    /// The largest packed files, biggest first.
    pub largest: Vec<AssetSize>,
}

impl BuildReport {
    /// Count a packed file.
    pub fn add(&mut self, rel: &str, raw: u64, packed: u64) {
        let cat = category(rel);
        self.files += 1;
        match self.categories.iter_mut().find(|c| c.name == cat) {
            Some(c) => {
                c.files += 1;
                c.raw_bytes += raw;
                c.packed_bytes += packed;
            }
            None => self.categories.push(CategorySize {
                name: cat.to_owned(),
                files: 1,
                raw_bytes: raw,
                packed_bytes: packed,
            }),
        }
        self.largest.push(AssetSize {
            path: rel.to_owned(),
            category: cat.to_owned(),
            raw_bytes: raw,
            packed_bytes: packed,
        });
        if self.largest.len() > LARGEST * 4 {
            self.trim_largest();
        }
    }

    /// Count a file a server build left out.
    pub fn strip(&mut self, bytes: u64) {
        self.stripped += 1;
        self.stripped_bytes += bytes;
    }

    fn trim_largest(&mut self) {
        self.largest.sort_by(|a, b| {
            b.packed_bytes
                .cmp(&a.packed_bytes)
                .then_with(|| a.path.cmp(&b.path))
        });
        self.largest.truncate(LARGEST);
    }

    /// Sort the types (biggest first) and keep the largest assets.
    pub fn finish(&mut self) {
        self.trim_largest();
        self.categories.sort_by(|a, b| {
            b.packed_bytes.cmp(&a.packed_bytes).then_with(|| {
                let i = |n: &str| CATEGORIES.iter().position(|c| *c == n);
                i(&a.name).cmp(&i(&b.name))
            })
        });
    }

    /// Bytes of one type in the pak.
    pub fn packed_of(&self, category: &str) -> u64 {
        self.categories
            .iter()
            .find(|c| c.name == category)
            .map_or(0, |c| c.packed_bytes)
    }

    /// Plain-text report (the file written next to the game).
    pub fn to_text(&self) -> String {
        use std::fmt::Write;
        let mut t = String::new();
        let _ = writeln!(t, "Build Report: {}", self.product);
        let _ = writeln!(
            t,
            "Profile {} · {}{}{}",
            self.profile,
            self.target,
            if self.server {
                " · Dedicated Server"
            } else {
                ""
            },
            if self.development {
                " · Development"
            } else {
                ""
            }
        );
        let _ = writeln!(t, "Output {}", self.output.display());
        let _ = writeln!(
            t,
            "Total {}  (executable {}, data {}) · {} files · {:.1} s",
            size_text(self.total_bytes),
            size_text(self.executable_bytes),
            size_text(self.pak_bytes),
            self.files,
            self.seconds
        );
        if self.stripped > 0 {
            let _ = writeln!(
                t,
                "Stripped for the server: {} files ({})",
                self.stripped,
                size_text(self.stripped_bytes)
            );
        }
        let _ = writeln!(t, "\nBy type{:>34}{:>12}{:>8}", "packed", "raw", "%");
        let total: u64 = self.categories.iter().map(|c| c.packed_bytes).sum();
        for c in &self.categories {
            let _ = writeln!(
                t,
                "  {:<22}{:>6} files{:>12}{:>12}{:>7.1}%",
                c.name,
                c.files,
                size_text(c.packed_bytes),
                size_text(c.raw_bytes),
                percent(c.packed_bytes, total)
            );
        }
        let _ = writeln!(t, "\nLargest assets");
        for a in &self.largest {
            let _ = writeln!(
                t,
                "  {:>10}  {:<20} {}",
                size_text(a.packed_bytes),
                a.category,
                a.path
            );
        }
        t
    }

    /// Where the report of `profile` is kept in project `root`.
    pub fn library_path(root: &Path, profile: &str) -> PathBuf {
        root.join(REPORT_DIR)
            .join(format!("{}.json", crate::build::file_stem(profile)))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text + "\n").map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        serde_json::from_str(&text).map_err(|e| e.to_string())
    }
}

/// Share of `part` in `total`, percent.
pub fn percent(part: u64, total: u64) -> f64 {
    if total == 0 {
        0.0
    } else {
        part as f64 * 100.0 / total as f64
    }
}

/// "812 B", "12.4 KB", "3.1 MB".
pub fn size_text(bytes: u64) -> String {
    let b = bytes as f64;
    if b < 1024.0 {
        format!("{bytes} B")
    } else if b < 1024.0 * 1024.0 {
        format!("{:.1} KB", b / 1024.0)
    } else if b < 1024.0 * 1024.0 * 1024.0 {
        format!("{:.1} MB", b / (1024.0 * 1024.0))
    } else {
        format!("{:.2} GB", b / (1024.0 * 1024.0 * 1024.0))
    }
}

/// Bytes of every file under `dir`.
pub fn folder_bytes(dir: &Path) -> u64 {
    let mut total = 0;
    let mut stack = vec![dir.to_path_buf()];
    while let Some(d) = stack.pop() {
        let Ok(rd) = std::fs::read_dir(&d) else {
            continue;
        };
        for e in rd.flatten() {
            match e.metadata() {
                Ok(m) if m.is_dir() => stack.push(e.path()),
                Ok(m) => total += m.len(),
                Err(_) => {}
            }
        }
    }
    total
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sorts_types_and_keeps_the_largest_assets() {
        let mut r = BuildReport::default();
        r.add("Assets/Big.png", 4000, 3000);
        r.add("Assets/Big.png.meta", 40, 40);
        r.add("Assets/Level.scene", 900, 300);
        r.add("Library/TextureCache/ab.mtc", 1000, 1000);
        r.add("Assets/Scripts/Player.lua", 200, 100);
        for i in 0..100 {
            r.add(&format!("Assets/Small{i}.mat"), 10, 5);
        }
        r.finish();
        assert_eq!(r.files, 105);
        assert_eq!(r.categories[0].name, "Textures");
        assert_eq!(r.packed_of("Materials"), 500);
        assert_eq!(r.categories[1].name, "Compressed Textures");
        assert_eq!(r.largest.len(), LARGEST);
        assert_eq!(r.largest[0].path, "Assets/Big.png");
        assert_eq!(category("ProjectSettings/BuildInfo.json"), "Settings");
        assert_eq!(category("Assets/a.ogg"), "Audio");
        let text = r.to_text();
        assert!(text.contains("Textures"), "{text}");
        assert!(text.contains("Assets/Big.png"), "{text}");
        assert_eq!(size_text(1536), "1.5 KB");

        let path = std::env::temp_dir().join(format!("misty_report_{}.json", std::process::id()));
        r.save(&path).unwrap();
        assert_eq!(BuildReport::load(&path).unwrap(), r);
        std::fs::remove_file(path).ok();
    }
}
