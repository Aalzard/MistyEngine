//! The editor's UI fonts: the user's UI and code fonts picked from the fonts installed on the
//! PC (Preferences ▸ Appearance), a text size scale, and the fallback chain.
//!
//! Each family lists fonts in order and egui takes every glyph from the first font that has
//! it, so a chosen font without Arabic, Turkish or symbol glyphs falls back glyph by glyph to
//! the built-in fonts. The built-in Arabic font stays first in the text families (see
//! `i18n::shape`). A chosen font that is gone or unreadable at startup is skipped (the default
//! font is used) and a note is left for the editor to show once ([`take_fallback_note`]).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::Mutex;

use egui::{FontData, FontDefinitions, FontFamily};
use serde::{Deserialize, Serialize};

use super::{ARABIC_FONT, BOLD_FAMILY};

/// The smallest and largest text size scale.
pub const MIN_SCALE: f32 = 0.8;
pub const MAX_SCALE: f32 = 1.5;

/// A font face in a file (`index` picks a face in a `.ttc` collection).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FaceRef {
    pub path: PathBuf,
    #[serde(default, skip_serializing_if = "is_zero")]
    pub index: u32,
}

fn is_zero(v: &u32) -> bool {
    *v == 0
}

/// The saved font choice.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct FontPrefs {
    /// The UI font; `None` = the OS UI font (Segoe UI on Windows) or egui's built-in one.
    pub ui: Option<FaceRef>,
    /// The Code Editor and Console font; `None` = the built-in monospace font.
    pub mono: Option<FaceRef>,
    /// Text size scale (1 = 100 %).
    pub scale: f32,
}

impl FontPrefs {
    pub fn new() -> Self {
        Self {
            scale: 1.0,
            ..Default::default()
        }
    }

    /// The saved choice (`hub.json`).
    pub fn load() -> Self {
        let Some(path) = crate::hub::config::config_path() else {
            return Self::new();
        };
        let c = crate::hub::config::HubConfig::load(&path);
        Self {
            ui: c.editor_font.map(face_from_setting),
            mono: c.editor_mono_font.map(face_from_setting),
            scale: clamp_scale(c.editor_font_scale.unwrap_or(1.0)),
        }
    }

    /// Save the choice in `hub.json` at `config` (`None` = nowhere: tests).
    pub fn save(&self, config: Option<&Path>) -> Result<(), String> {
        let scale = clamp_scale(self.scale);
        let Some(config) = config else {
            return Ok(());
        };
        crate::hub::config::update_at(config, |c| {
            c.editor_font = self.ui.as_ref().map(face_to_setting);
            c.editor_mono_font = self.mono.as_ref().map(face_to_setting);
            c.editor_font_scale = ((scale - 1.0).abs() > 1e-3).then_some(scale);
        })
    }
}

/// Faces after the first in a collection are saved as `file.ttc#2`.
fn face_to_setting(f: &FaceRef) -> PathBuf {
    if f.index == 0 {
        f.path.clone()
    } else {
        PathBuf::from(format!("{}#{}", f.path.display(), f.index))
    }
}

fn face_from_setting(p: PathBuf) -> FaceRef {
    let s = p.to_string_lossy();
    if let Some((file, idx)) = s.rsplit_once('#')
        && let Ok(index) = idx.parse::<u32>()
    {
        return FaceRef {
            path: PathBuf::from(file),
            index,
        };
    }
    FaceRef { path: p, index: 0 }
}

pub fn clamp_scale(v: f32) -> f32 {
    if v.is_finite() {
        v.clamp(MIN_SCALE, MAX_SCALE)
    } else {
        1.0
    }
}

/// An installed font face.
#[derive(Clone, Debug, PartialEq)]
pub struct FontEntry {
    pub family: String,
    /// "Regular", "Bold Italic" …
    pub style: String,
    pub face: FaceRef,
    pub monospace: bool,
}

impl FontEntry {
    /// "Family" for the regular face, "Family Style" otherwise.
    pub fn label(&self) -> String {
        if self.style.is_empty() || self.style.eq_ignore_ascii_case("regular") {
            self.family.clone()
        } else {
            format!("{} {}", self.family, self.style)
        }
    }
}

/// Font data egui can load: the same check egui does when it loads the font (so a broken or
/// unsupported file never reaches it and panics).
pub fn usable(bytes: &[u8], index: u32) -> bool {
    skrifa::FontRef::from_index(bytes, index).is_ok()
}

/// Read a chosen face: `None` when the file is gone, unreadable or not a font egui can use.
pub fn read_face(face: &FaceRef) -> Option<Vec<u8>> {
    let bytes = std::fs::read(&face.path).ok()?;
    usable(&bytes, face.index).then_some(bytes)
}

/// The faces in one font file's data.
pub fn faces_in(path: &Path, bytes: &[u8]) -> Vec<FontEntry> {
    use skrifa::MetadataProvider as _;
    use skrifa::raw::{FileRef, TableProvider as _};
    use skrifa::string::StringId;
    let Ok(file) = FileRef::new(bytes) else {
        return Vec::new();
    };
    let name = |font: &skrifa::FontRef, ids: [StringId; 2]| {
        ids.into_iter().find_map(|id| {
            let s: String = font
                .localized_strings(id)
                .english_or_first()?
                .chars()
                .collect();
            let s = s.trim().to_owned();
            (!s.is_empty()).then_some(s)
        })
    };
    let mut out = Vec::new();
    for (index, font) in file.fonts().enumerate() {
        let Ok(font) = font else { continue };
        let Some(family) = name(
            &font,
            [StringId::TYPOGRAPHIC_FAMILY_NAME, StringId::FAMILY_NAME],
        ) else {
            continue;
        };
        // Skip faces egui can't draw (no outlines: bitmap-only fonts).
        if font.glyf().is_err() && font.cff().is_err() && font.cff2().is_err() {
            continue;
        }
        let style = name(
            &font,
            [
                StringId::TYPOGRAPHIC_SUBFAMILY_NAME,
                StringId::SUBFAMILY_NAME,
            ],
        )
        .unwrap_or_default();
        let monospace = font.post().is_ok_and(|p| p.is_fixed_pitch() != 0);
        out.push(FontEntry {
            family,
            style,
            face: FaceRef {
                path: path.to_path_buf(),
                index: index as u32,
            },
            monospace,
        });
    }
    out
}

/// The folders fonts are installed in on this OS (system and per-user).
pub fn system_font_dirs() -> Vec<PathBuf> {
    let env = |k: &str| std::env::var_os(k).map(PathBuf::from);
    let mut dirs = Vec::new();
    if cfg!(windows) {
        dirs.push(
            env("WINDIR")
                .or_else(|| env("SystemRoot"))
                .unwrap_or_else(|| PathBuf::from("C:/Windows"))
                .join("Fonts"),
        );
        if let Some(local) = env("LOCALAPPDATA") {
            dirs.push(local.join("Microsoft/Windows/Fonts"));
        }
    } else if cfg!(target_os = "macos") {
        dirs.push("/System/Library/Fonts".into());
        dirs.push("/Library/Fonts".into());
        if let Some(home) = env("HOME") {
            dirs.push(home.join("Library/Fonts"));
        }
    } else {
        dirs.push("/usr/share/fonts".into());
        dirs.push("/usr/local/share/fonts".into());
        if let Some(data) = env("XDG_DATA_HOME") {
            dirs.push(data.join("fonts"));
        }
        if let Some(home) = env("HOME") {
            dirs.push(home.join(".local/share/fonts"));
            dirs.push(home.join(".fonts"));
        }
    }
    dirs
}

/// Every usable face in `dirs` (searched recursively), sorted by name, without duplicates.
pub fn enumerate(dirs: &[PathBuf]) -> Vec<FontEntry> {
    let mut files = Vec::new();
    for d in dirs {
        collect_font_files(d, 0, &mut files);
    }
    let mut out: Vec<FontEntry> = Vec::new();
    for f in files {
        let Ok(bytes) = std::fs::read(&f) else {
            continue;
        };
        for e in faces_in(&f, &bytes) {
            if !out
                .iter()
                .any(|o| o.family == e.family && o.style == e.style)
            {
                out.push(e);
            }
        }
    }
    out.sort_by(|a, b| {
        a.family
            .to_lowercase()
            .cmp(&b.family.to_lowercase())
            .then_with(|| style_rank(&a.style).cmp(&style_rank(&b.style)))
            .then_with(|| a.style.cmp(&b.style))
    });
    out
}

/// Regular faces first within a family.
fn style_rank(style: &str) -> u8 {
    match style.to_ascii_lowercase().as_str() {
        "regular" | "" | "normal" | "book" | "roman" => 0,
        _ => 1,
    }
}

fn collect_font_files(dir: &Path, depth: u32, out: &mut Vec<PathBuf>) {
    if depth > 6 {
        return;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for e in entries.flatten() {
        let path = e.path();
        if path.is_dir() {
            collect_font_files(&path, depth + 1, out);
        } else if path.extension().is_some_and(|x| {
            let x = x.to_string_lossy().to_ascii_lowercase();
            matches!(x.as_str(), "ttf" | "otf" | "ttc" | "otc")
        }) {
            out.push(path);
        }
    }
}

/// The installed fonts, read on a background thread (the first open of the picker).
#[derive(Default)]
pub struct FontList {
    rx: Option<std::sync::mpsc::Receiver<Vec<FontEntry>>>,
    pub fonts: Option<Vec<FontEntry>>,
}

impl FontList {
    /// Start reading the installed fonts (once).
    pub fn request(&mut self) {
        if self.fonts.is_some() || self.rx.is_some() {
            return;
        }
        let (tx, rx) = std::sync::mpsc::channel();
        self.rx = Some(rx);
        let spawned = std::thread::Builder::new()
            .name("font list".into())
            .spawn(move || {
                let _ = tx.send(enumerate(&system_font_dirs()));
            });
        if spawned.is_err() {
            self.rx = None;
            self.fonts = Some(Vec::new());
        }
    }

    /// The list once read (polls the thread).
    pub fn get(&mut self) -> Option<&[FontEntry]> {
        if let Some(rx) = &self.rx
            && let Ok(list) = rx.try_recv()
        {
            self.fonts = Some(list);
            self.rx = None;
        }
        self.fonts.as_deref()
    }
}

/// A note for the user when a chosen font could not be loaded (taken once).
static FALLBACK_NOTE: Mutex<Option<String>> = Mutex::new(None);

/// The note left when a chosen font was missing (once; later calls get `None`).
pub fn take_fallback_note() -> Option<String> {
    FALLBACK_NOTE.lock().ok()?.take()
}

/// What [`definitions`] could load.
#[derive(Debug, Default, PartialEq)]
pub struct Loaded {
    /// Chosen fonts that are missing or unreadable (and replaced by the defaults).
    pub missing: Vec<PathBuf>,
}

/// The OS fonts used when nothing is chosen: (name, path, primary UI font).
const OS_FONTS: [(&str, &str, bool); 3] = [
    ("ui", "C:/Windows/Fonts/segoeui.ttf", true),
    ("symbols", "C:/Windows/Fonts/seguisym.ttf", false),
    (
        "symbols_linux",
        "/usr/share/fonts/truetype/dejavu/DejaVuSans.ttf",
        false,
    ),
];

/// The font definitions for a choice.
pub fn definitions(prefs: &FontPrefs) -> (FontDefinitions, Loaded) {
    let mut fonts = FontDefinitions::default();
    let mut loaded = Loaded::default();
    let user_ui = prefs.ui.as_ref().and_then(|f| {
        let bytes = read_face(f);
        if bytes.is_none() {
            loaded.missing.push(f.path.clone());
        }
        bytes.map(|b| (b, f.index))
    });
    let user_mono = prefs.mono.as_ref().and_then(|f| {
        let bytes = read_face(f);
        if bytes.is_none() {
            loaded.missing.push(f.path.clone());
        }
        bytes.map(|b| (b, f.index))
    });
    for (name, path, primary) in OS_FONTS {
        let Ok(bytes) = std::fs::read(path) else {
            continue;
        };
        if !usable(&bytes, 0) {
            continue;
        }
        fonts
            .font_data
            .insert(name.to_owned(), Arc::new(FontData::from_owned(bytes)));
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let list = fonts.families.entry(family.clone()).or_default();
            if primary && family == FontFamily::Proportional {
                list.insert(0, name.to_owned());
            } else {
                list.push(name.to_owned());
            }
        }
    }
    // The chosen fonts go first; everything above stays behind them as glyph fallback.
    if let Some((bytes, index)) = user_ui {
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        fonts.font_data.insert("user_ui".to_owned(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Proportional)
            .or_default()
            .insert(0, "user_ui".to_owned());
    }
    if let Some((bytes, index)) = user_mono {
        let mut data = FontData::from_owned(bytes);
        data.index = index;
        fonts
            .font_data
            .insert("user_mono".to_owned(), Arc::new(data));
        fonts
            .families
            .entry(FontFamily::Monospace)
            .or_default()
            .insert(0, "user_mono".to_owned());
    }
    // Arabic: the built-in font comes first in the text families, so every Arabic character
    // (and only those: it has no Latin, digits or spaces) is drawn with it on any system. egui
    // shapes each run of one font and lays a run of Arabic out right to left by itself;
    // `i18n::shape` relies on Arabic runs being exactly the runs of this font. Its line metrics
    // are Segoe UI's (see assets/fonts/README.md), so rows keep their height. Code keeps its
    // monospace font first.
    fonts.font_data.insert(
        ARABIC_FONT.to_owned(),
        Arc::new(FontData::from_static(
            misty_engine::ui::rich::fonts::ARABIC_FONT_DATA,
        )),
    );
    fonts
        .families
        .entry(FontFamily::Proportional)
        .or_default()
        .insert(0, ARABIC_FONT.to_owned());
    let mono = fonts.families.entry(FontFamily::Monospace).or_default();
    mono.insert(1.min(mono.len()), ARABIC_FONT.to_owned());
    // Bold titles: the bold face first, then the regular fallbacks for symbols. A chosen UI
    // font has no bold file of its own here, so its regular face is used for titles too.
    let bold = if prefs.ui.is_some() && fonts.font_data.contains_key("user_ui") {
        Some("user_ui".to_owned())
    } else {
        std::fs::read("C:/Windows/Fonts/segoeuib.ttf")
            .ok()
            .filter(|b| usable(b, 0))
            .map(|bytes| {
                fonts
                    .font_data
                    .insert("ui_bold".to_owned(), Arc::new(FontData::from_owned(bytes)));
                "ui_bold".to_owned()
            })
    };
    if let Some(bold) = bold {
        // Arabic first here too (regular weight: the built-in font has no bold face).
        let mut list = vec![ARABIC_FONT.to_owned(), bold.clone()];
        list.extend(
            fonts.families[&FontFamily::Proportional]
                .iter()
                .filter(|n| *n != ARABIC_FONT && **n != bold)
                .cloned(),
        );
        fonts
            .families
            .insert(FontFamily::Name(BOLD_FAMILY.into()), list);
    }
    (fonts, loaded)
}

/// Load a font choice into egui (live) with the text size scale. A missing chosen font leaves
/// a note ([`take_fallback_note`]) instead of failing.
pub fn apply(ctx: &egui::Context, prefs: &FontPrefs) -> Loaded {
    let (defs, loaded) = definitions(prefs);
    ctx.set_fonts(defs);
    apply_scale(ctx, prefs.scale);
    if !loaded.missing.is_empty()
        && let Ok(mut note) = FALLBACK_NOTE.lock()
    {
        let names: Vec<String> = loaded
            .missing
            .iter()
            .map(|p| {
                p.file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| p.display().to_string())
            })
            .collect();
        *note = Some(names.join(", "));
    }
    loaded
}

/// Scale egui's text styles from their defaults.
pub fn apply_scale(ctx: &egui::Context, scale: f32) {
    let scale = clamp_scale(scale);
    let defaults = egui::Style::default().text_styles;
    ctx.global_style_mut(|s| {
        for (style, font) in s.text_styles.iter_mut() {
            if let Some(d) = defaults.get(style) {
                font.size = d.size * scale;
            }
        }
    });
}
