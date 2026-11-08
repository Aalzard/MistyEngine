//! Build Profiles: named sets of build options, like the reference editor's File ▸ Build
//! Profiles. Every platform has a profile (Windows, Linux, macOS, Dedicated Server) and a
//! project can add its own (a "Demo" or "Steam Deck" profile). A profile has:
//!
//! - the platform and CPU it builds for (a Dedicated Server builds a headless player),
//! - its own scene list, or the shared one of Build Settings,
//! - scripting defines (Lua `--#if NAME` blocks and `Application.HasDefine`),
//! - texture compression and max texture size overrides,
//! - a quality level override (the level the built game starts on),
//! - Development Build, Compress Data and Create .zip,
//! - an output folder.
//!
//! Profiles are JSON assets: the platform ones in `ProjectSettings/BuildProfiles/<Platform>.buildprofile`,
//! custom ones in `Assets/Settings/Build Profiles/<Name>.buildprofile`. Which one is active is
//! kept in Build Settings ([`crate::build::BuildSettings::active_profile`]).

use std::path::{Path, PathBuf};
use std::sync::RwLock;

use serde::{Deserialize, Serialize};

use crate::build::{BuildScene, BuildSettings, BuildTarget, TargetOs};
use crate::sprites::import::{PlatformOverride, TextureCompression, TextureImporter};

/// File extension of profile assets.
pub const EXTENSION: &str = "buildprofile";
/// Project-relative folder of custom profiles.
pub const CUSTOM_DIR: &str = "Assets/Settings/Build Profiles";
/// Project-relative folder of the platform profiles.
pub const PLATFORM_DIR: &str = "ProjectSettings/BuildProfiles";
/// Max Texture Size choices of the override.
pub const MAX_SIZES: [u32; 9] = [32, 64, 128, 256, 512, 1024, 2048, 4096, 8192];

/// What a profile builds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProfilePlatform {
    #[default]
    Windows,
    Linux,
    MacOs,
    /// A headless player for Windows or Linux: no window, audio or GPU; runs the network
    /// server.
    DedicatedServer,
}

impl ProfilePlatform {
    pub const ALL: [Self; 4] = [
        Self::Windows,
        Self::Linux,
        Self::MacOs,
        Self::DedicatedServer,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Windows => "Windows",
            Self::Linux => "Linux",
            Self::MacOs => "macOS",
            Self::DedicatedServer => "Dedicated Server",
        }
    }

    /// Targets the platform can build for (the first is the default).
    pub fn targets(self) -> &'static [BuildTarget] {
        match self {
            Self::Windows => &[BuildTarget::WindowsX64, BuildTarget::WindowsArm64],
            Self::Linux => &[BuildTarget::LinuxX64, BuildTarget::LinuxArm64],
            Self::MacOs => &[BuildTarget::MacArm64, BuildTarget::MacX64],
            Self::DedicatedServer => &[
                BuildTarget::LinuxX64,
                BuildTarget::WindowsX64,
                BuildTarget::LinuxArm64,
                BuildTarget::WindowsArm64,
            ],
        }
    }

    /// The default target: this machine's when the platform builds for it.
    pub fn default_target(self) -> BuildTarget {
        let host = BuildTarget::host();
        match host {
            Some(h) if self.targets().contains(&h) => h,
            _ => self.targets()[0],
        }
    }

    pub fn is_server(self) -> bool {
        self == Self::DedicatedServer
    }

    /// The platform profile's file name (without extension).
    fn file_stem(self) -> &'static str {
        match self {
            Self::DedicatedServer => "DedicatedServer",
            other => other.label(),
        }
    }
}

fn yes() -> bool {
    true
}

/// One Build Profile.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct BuildProfile {
    /// Shown in the window and matched by `misty --build … --profile <name>`. A custom
    /// profile's file is named after it; a platform profile is named after its platform.
    #[serde(skip)]
    pub name: String,
    /// Custom profile (an asset under [`CUSTOM_DIR`]) rather than a platform's.
    #[serde(skip)]
    pub custom: bool,
    pub platform: ProfilePlatform,
    /// Operating system and CPU (one of the platform's [`ProfilePlatform::targets`]).
    pub target: BuildTarget,
    /// Use [`BuildProfile::scenes`] instead of the shared Build Settings list.
    pub override_scenes: bool,
    pub scenes: Vec<BuildScene>,
    /// Scripting defines ("DEMO", "STEAM").
    pub defines: Vec<String>,
    /// Compression every compressed texture gets (`None`: the textures' own settings).
    /// "None" (uncompressed) applies to all textures.
    pub texture_compression: Option<TextureCompression>,
    /// Caps every texture's Max Size (`None`: the textures' own settings).
    pub max_texture_size: Option<u32>,
    /// Quality level the built game starts on, by name (`None`: the project default).
    pub quality_level: Option<String>,
    pub development: bool,
    #[serde(default = "yes")]
    pub compression: bool,
    pub create_zip: bool,
    /// Where Build puts the game: absolute, or relative to the project folder ("Builds/Win").
    /// Empty = ask (the window) / the command line's folder.
    pub output_folder: String,
}

impl Default for BuildProfile {
    fn default() -> Self {
        Self::for_platform(ProfilePlatform::Windows)
    }
}

impl BuildProfile {
    /// A platform's profile with default options.
    pub fn for_platform(platform: ProfilePlatform) -> Self {
        Self {
            name: platform.label().to_owned(),
            custom: false,
            platform,
            target: platform.default_target(),
            override_scenes: false,
            scenes: Vec::new(),
            defines: Vec::new(),
            texture_compression: None,
            max_texture_size: None,
            quality_level: None,
            development: false,
            compression: true,
            create_zip: false,
            output_folder: String::new(),
        }
    }

    /// Keep the target on the platform, defines usable and the size one of [`MAX_SIZES`].
    pub fn sanitize(&mut self) {
        if !self.platform.targets().contains(&self.target) {
            self.target = self.platform.default_target();
        }
        for d in &mut self.defines {
            *d = clean_define(d);
        }
        self.defines.retain(|d| !d.is_empty());
        let mut seen = std::collections::HashSet::new();
        self.defines.retain(|d| seen.insert(d.clone()));
        if let Some(s) = self.max_texture_size {
            self.max_texture_size = MAX_SIZES.iter().copied().min_by_key(|m| m.abs_diff(s));
        }
    }

    pub fn is_server(&self) -> bool {
        self.platform.is_server()
    }

    /// "Windows", "Demo (Linux)".
    pub fn title(&self) -> String {
        if self.custom {
            format!("{} ({})", self.name, self.platform.label())
        } else {
            self.name.clone()
        }
    }

    /// Project-relative file of the profile.
    pub fn rel_path(&self) -> String {
        if self.custom {
            format!(
                "{CUSTOM_DIR}/{}.{EXTENSION}",
                crate::build::file_stem(&self.name)
            )
        } else {
            format!("{PLATFORM_DIR}/{}.{EXTENSION}", self.platform.file_stem())
        }
    }

    pub fn from_json(text: &str) -> Result<Self, String> {
        let mut p: Self = serde_json::from_str(&crate::asset_db::resolve_json(text))
            .map_err(|e| e.to_string())?;
        p.sanitize();
        Ok(p)
    }

    pub fn to_json(&self) -> Result<String, String> {
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        Ok(crate::asset_db::embed_json(&(text + "\n")))
    }

    /// Read a profile file; its name comes from the file (custom) or the platform.
    pub fn load(path: &Path, custom: bool) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let mut p = Self::from_json(&text).map_err(|e| format!("{}: {e}", path.display()))?;
        p.custom = custom;
        p.name = if custom {
            path.file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else {
            p.platform.label().to_owned()
        };
        Ok(p)
    }

    /// Write the profile's file under project `root`.
    pub fn save(&self, root: &Path) -> Result<PathBuf, String> {
        let path = root.join(self.rel_path());
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(&path, self.to_json()?).map_err(|e| format!("{}: {e}", path.display()))?;
        Ok(path)
    }

    /// The Build Settings a build with this profile uses: the shared ones with this profile's
    /// target, scene list (when it has its own) and options.
    pub fn apply(&self, shared: &BuildSettings) -> BuildSettings {
        let mut b = shared.clone();
        if self.override_scenes {
            b.scenes = self.scenes.clone();
        }
        b.target = self.target;
        b.development = self.development;
        b.compression = self.compression;
        b.create_zip = self.create_zip;
        b.active_profile = self.name.clone();
        b
    }

    /// The scene list builds use (its own or the shared one).
    pub fn scenes<'a>(&'a self, shared: &'a BuildSettings) -> &'a [BuildScene] {
        if self.override_scenes {
            &self.scenes
        } else {
            &shared.scenes
        }
    }

    /// Index of the quality level the profile starts on, in `quality`.
    pub fn quality_index(&self, quality: &crate::quality::QualitySettings) -> Option<usize> {
        let name = self.quality_level.as_deref()?;
        quality.levels.iter().position(|l| l.name == name)
    }

    /// Output folder as an absolute path (`None` when the profile has none).
    pub fn output_path(&self, root: &Path) -> Option<PathBuf> {
        let o = self.output_folder.trim();
        if o.is_empty() {
            return None;
        }
        let p = Path::new(o);
        Some(if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        })
    }

    /// The defines a build with this profile runs with: the profile's own plus the built-in
    /// ones (`MISTY_STANDALONE_WIN`, `MISTY_SERVER`, `DEVELOPMENT_BUILD`).
    pub fn build_defines(&self) -> Vec<String> {
        let mut d = vec![
            match self.target.os() {
                TargetOs::Windows => "MISTY_STANDALONE_WIN",
                TargetOs::Linux => "MISTY_STANDALONE_LINUX",
                TargetOs::MacOs => "MISTY_STANDALONE_OSX",
            }
            .to_owned(),
        ];
        if self.is_server() {
            d.push("MISTY_SERVER".into());
        }
        if self.development {
            d.push("DEVELOPMENT_BUILD".into());
        }
        for x in &self.defines {
            if !d.contains(x) {
                d.push(x.clone());
            }
        }
        d
    }

    /// The texture overrides (none when the profile keeps the textures' settings).
    pub fn texture_override(&self) -> Option<TextureOverride> {
        let o = TextureOverride {
            compression: self.texture_compression,
            max_size: self.max_texture_size,
        };
        (o.compression.is_some() || o.max_size.is_some()).then_some(o)
    }
}

/// A define as typed: letters, digits and `_`, upper-cased like the familiar convention is
/// not forced (defines are case-sensitive).
pub fn clean_define(d: &str) -> String {
    d.trim()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
        .collect()
}

/// A profile's texture compression and Max Size overrides.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TextureOverride {
    pub compression: Option<TextureCompression>,
    pub max_size: Option<u32>,
}

impl TextureOverride {
    /// `imp` as a build with these overrides imports it on `platform` (written as the
    /// platform's override, so players find the same compressed blocks the build packed).
    pub fn apply(&self, imp: &TextureImporter, platform: &str) -> TextureImporter {
        let mut out = imp.clone();
        let current = imp.compression_for(platform);
        let compression = match self.compression {
            Some(TextureCompression::None) => TextureCompression::None,
            // Textures kept uncompressed (sprites, pixel art) stay so.
            Some(c) if current != TextureCompression::None => c,
            _ => current,
        };
        let max = imp.max_size_for(platform);
        let max_size = self.max_size.map_or(max, |m| m.min(max));
        out.overrides.retain(|o| o.platform != platform);
        out.overrides.push(PlatformOverride {
            platform: platform.to_owned(),
            max_size,
            compression,
        });
        out
    }

    /// The `.meta` text of a texture with these overrides (`meta`: its current text, if
    /// any; the GUID and other keys are kept).
    pub fn apply_meta(&self, meta: Option<&str>, platform: &str) -> Result<String, String> {
        let mut map = match meta.map(serde_json::from_str::<serde_json::Value>) {
            Some(Ok(serde_json::Value::Object(m))) => m,
            Some(Err(e)) => return Err(e.to_string()),
            _ => serde_json::Map::new(),
        };
        let imp: TextureImporter =
            serde_json::from_value(serde_json::Value::Object(map.clone())).unwrap_or_default();
        let new = self.apply(&imp, platform);
        if let serde_json::Value::Object(fields) =
            serde_json::to_value(&new).map_err(|e| e.to_string())?
        {
            for (k, v) in fields {
                map.insert(k, v);
            }
        }
        serde_json::to_string_pretty(&serde_json::Value::Object(map))
            .map(|t| t + "\n")
            .map_err(|e| e.to_string())
    }
}

/// The project's profiles: one per platform, then the custom ones (by name).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct BuildProfiles {
    pub profiles: Vec<BuildProfile>,
    /// Problems reading profile files (shown in the Console).
    pub warnings: Vec<String>,
}

impl BuildProfiles {
    /// Load the profiles of project `root`. Platform profiles without a file start from the
    /// old Build Settings options (`legacy`).
    pub fn load(root: &Path, legacy: &BuildSettings) -> Self {
        let mut out = Self::default();
        for platform in ProfilePlatform::ALL {
            let mut def = BuildProfile::for_platform(platform);
            if platform.targets().contains(&legacy.target) {
                def.target = legacy.target;
            }
            def.development = legacy.development;
            def.compression = legacy.compression;
            def.create_zip = legacy.create_zip;
            let path = root.join(def.rel_path());
            let p = match crate::vfs::read_to_string(&path) {
                Ok(_) => match BuildProfile::load(&path, false) {
                    Ok(mut p) => {
                        // The file decides the platform by its name.
                        p.platform = platform;
                        p.name = platform.label().to_owned();
                        p.sanitize();
                        p
                    }
                    Err(e) => {
                        out.warnings.push(e);
                        def
                    }
                },
                Err(_) => def,
            };
            out.profiles.push(p);
        }
        let dir = root.join(CUSTOM_DIR);
        let mut custom: Vec<PathBuf> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.flatten()
                    .map(|e| e.path())
                    .filter(|p| p.extension().is_some_and(|e| e == EXTENSION))
                    .collect()
            })
            .unwrap_or_default();
        custom.sort();
        for path in custom {
            match BuildProfile::load(&path, true) {
                Ok(p) => out.profiles.push(p),
                Err(e) => out.warnings.push(e),
            }
        }
        out
    }

    /// The profile called `name` (case-insensitive; custom profiles first). Platform profiles
    /// also answer to "Server" / "DedicatedServer" / "mac".
    pub fn find(&self, name: &str) -> Option<&BuildProfile> {
        let n = name.trim();
        let squash = |s: &str| s.replace([' ', '_', '-'], "").to_ascii_lowercase();
        self.profiles
            .iter()
            .filter(|p| p.custom)
            .find(|p| p.name.eq_ignore_ascii_case(n))
            .or_else(|| {
                self.profiles.iter().filter(|p| !p.custom).find(|p| {
                    let s = squash(n);
                    squash(&p.name) == s
                        || (p.platform == ProfilePlatform::DedicatedServer && s == "server")
                        || (p.platform == ProfilePlatform::MacOs && s == "mac")
                })
            })
    }

    pub fn index_of(&self, name: &str) -> Option<usize> {
        let p = self.find(name)?;
        self.profiles.iter().position(|q| std::ptr::eq(q, p))
    }

    /// The active profile: Build Settings' choice, else the one of its legacy target's OS.
    pub fn active<'a>(&'a self, shared: &BuildSettings) -> &'a BuildProfile {
        self.find(&shared.active_profile)
            .or_else(|| {
                self.profiles
                    .iter()
                    .find(|p| !p.custom && p.platform.targets()[0].os() == shared.target.os())
            })
            .unwrap_or(&self.profiles[0])
    }

    /// A free custom profile name starting from `base` ("Demo", "Demo 1", "Demo 2"…).
    pub fn unique_name(&self, base: &str) -> String {
        let base = crate::build::file_stem(base.trim());
        let taken = |n: &str| self.profiles.iter().any(|p| p.name.eq_ignore_ascii_case(n));
        if !taken(&base) {
            return base;
        }
        (1..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .unwrap_or(base)
    }

    /// Add a custom profile copied from `from` (or a new one for `platform`), saved now.
    /// Returns its index.
    pub fn add_custom(
        &mut self,
        root: &Path,
        name: &str,
        from: Option<&BuildProfile>,
        platform: ProfilePlatform,
    ) -> Result<usize, String> {
        let mut p = from
            .cloned()
            .unwrap_or_else(|| BuildProfile::for_platform(platform));
        p.custom = true;
        p.name = self.unique_name(name);
        p.save(root)?;
        self.profiles.push(p);
        Ok(self.profiles.len() - 1)
    }

    /// Rename custom profile `index` (its file and `.meta` move). Returns the new name.
    pub fn rename(&mut self, root: &Path, index: usize, name: &str) -> Result<String, String> {
        let p = self
            .profiles
            .get(index)
            .filter(|p| p.custom)
            .ok_or("only custom profiles can be renamed")?;
        let clean = crate::build::file_stem(name.trim());
        if clean.eq_ignore_ascii_case(&p.name) {
            return Ok(p.name.clone());
        }
        let old = root.join(p.rel_path());
        let new_name = self.unique_name(&clean);
        let p = &mut self.profiles[index];
        p.name = new_name.clone();
        let new = root.join(p.rel_path());
        if old.is_file() {
            std::fs::rename(&old, &new).map_err(|e| e.to_string())?;
            let (om, nm) = (
                crate::asset_db::meta::meta_path(&old),
                crate::asset_db::meta::meta_path(&new),
            );
            if om.is_file() {
                std::fs::rename(&om, &nm).map_err(|e| e.to_string())?;
            }
        } else {
            p.save(root)?;
        }
        Ok(new_name)
    }

    /// Remove custom profile `index` and its file.
    pub fn delete(&mut self, root: &Path, index: usize) -> Result<BuildProfile, String> {
        if !self.profiles.get(index).is_some_and(|p| p.custom) {
            return Err("only custom profiles can be deleted".into());
        }
        let p = self.profiles.remove(index);
        let path = root.join(p.rel_path());
        for f in [crate::asset_db::meta::meta_path(&path), path] {
            if f.is_file() {
                std::fs::remove_file(&f).map_err(|e| e.to_string())?;
            }
        }
        Ok(p)
    }
}

// ---- scripting defines ------------------------------------------------------------------------

static DEFINES: RwLock<Vec<String>> = RwLock::new(Vec::new());

/// Defines scripts see (`Application.HasDefine`, `--#if` blocks); the player sets the built
/// ones at startup, the editor the active profile's at Play.
pub fn set_active_defines(defines: Vec<String>) {
    *DEFINES.write().unwrap_or_else(|e| e.into_inner()) = defines;
}

pub fn active_defines() -> Vec<String> {
    DEFINES.read().unwrap_or_else(|e| e.into_inner()).clone()
}

pub fn has_define(name: &str) -> bool {
    DEFINES
        .read()
        .unwrap_or_else(|e| e.into_inner())
        .iter()
        .any(|d| d == name)
}

/// Lua with conditional blocks resolved for `defines`:
///
/// ```lua
/// --#if DEMO
/// local maxLevel = 3
/// --#elif !STEAM
/// local maxLevel = 10
/// --#else
/// local maxLevel = 99
/// --#endif
/// ```
///
/// A condition is a define, `!DEFINE`, or several joined by `&&` / `||` (no brackets; `&&`
/// binds tighter). Lines left out become empty, so error line numbers stay right. Sources
/// without `--#` come back unchanged.
pub fn preprocess_lua(source: &str, defines: &[String]) -> String {
    if !source.contains("--#") {
        return source.to_owned();
    }
    // Per open block: (this branch is taken, some branch was taken, the parent is taken).
    let mut stack: Vec<(bool, bool, bool)> = Vec::new();
    let active = |stack: &[(bool, bool, bool)]| stack.last().is_none_or(|b| b.0);
    let mut out = String::with_capacity(source.len());
    for line in source.split_inclusive('\n') {
        let t = line.trim();
        let directive = t.strip_prefix("--#").map(str::trim_start);
        let ending = if line.ends_with("\r\n") {
            "\r\n"
        } else if line.ends_with('\n') {
            "\n"
        } else {
            ""
        };
        match directive {
            Some(d) if d.starts_with("if ") || d == "if" => {
                let parent = active(&stack);
                let take = parent && eval_condition(&d[2..], defines);
                stack.push((take, take, parent));
                out.push_str(ending);
            }
            Some(d) if d.starts_with("elif ") || d == "elif" => {
                if let Some(top) = stack.last_mut() {
                    let take = top.2 && !top.1 && eval_condition(&d[4..], defines);
                    top.0 = take;
                    top.1 |= take;
                }
                out.push_str(ending);
            }
            Some(d) if d.starts_with("else") => {
                if let Some(top) = stack.last_mut() {
                    top.0 = top.2 && !top.1;
                    top.1 = true;
                }
                out.push_str(ending);
            }
            Some(d) if d.starts_with("endif") => {
                stack.pop();
                out.push_str(ending);
            }
            _ if active(&stack) => out.push_str(line),
            _ => out.push_str(ending),
        }
    }
    out
}

/// `A`, `!A`, `A && !B || C`.
pub fn eval_condition(cond: &str, defines: &[String]) -> bool {
    cond.split("||").any(|any| {
        any.split("&&").all(|term| {
            let term = term.trim();
            match term.strip_prefix('!') {
                Some(name) => !defines.iter().any(|d| d == name.trim()),
                None if term.eq_ignore_ascii_case("true") => true,
                None => !term.is_empty() && defines.iter().any(|d| d == term),
            }
        })
    })
}

// ---- dedicated server stripping ---------------------------------------------------------------

/// Whether a dedicated server build leaves `rel` out: files only seen or heard (textures,
/// audio, video) and their `.meta` files. Meshes stay (colliders use them).
pub fn server_strips(rel: &str) -> bool {
    let base = rel.strip_suffix(".meta").unwrap_or(rel);
    crate::assets::is_texture(base)
        || crate::audio::is_clip(base)
        || crate::video::source::is_video_like(base)
        || crate::assets::has_extension(base, &["hdr", "exr", "tga", "bmp", "psd", "ktx2", "dds"])
}

#[cfg(test)]
mod tests;
