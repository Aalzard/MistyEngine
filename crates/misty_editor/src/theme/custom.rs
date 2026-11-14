//! Custom themes: named palettes made in the Theme editor, kept per user as `.mistytheme`
//! files (small JSON) in the `themes` folder beside `hub.json`, and the saved theme choice.
//!
//! A file names its base preset and lists colours as `#rrggbb` (or `#rrggbbaa`); tokens it
//! leaves out come from the base, so files from older or newer editors still load.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use egui::Color32;
use serde::{Deserialize, Serialize};

use super::presets::Preset;
use super::{Palette, TOKENS, contrast};

/// The extension of theme files.
pub const EXTENSION: &str = "mistytheme";

/// The current file format.
const FORMAT: u32 = 1;

/// A named custom theme.
#[derive(Clone, Debug, PartialEq)]
pub struct CustomTheme {
    pub name: String,
    /// The preset it started from ("Reset to preset" and missing tokens use it).
    pub base: Preset,
    pub palette: Palette,
}

#[derive(Serialize, Deserialize)]
struct ThemeFile {
    #[serde(default)]
    format: u32,
    #[serde(default)]
    name: String,
    #[serde(default)]
    base: String,
    #[serde(default)]
    colors: BTreeMap<String, String>,
}

/// `#rrggbb`, or `#rrggbbaa` when not opaque.
pub fn to_hex(c: Color32) -> String {
    let [r, g, b, a] = c.to_srgba_unmultiplied();
    if a == 255 {
        format!("#{r:02x}{g:02x}{b:02x}")
    } else {
        format!("#{r:02x}{g:02x}{b:02x}{a:02x}")
    }
}

/// Parse `#rgb`, `#rrggbb` or `#rrggbbaa` (the `#` is optional).
pub fn from_hex(s: &str) -> Option<Color32> {
    let h = s.trim().trim_start_matches('#');
    if !h.is_ascii() {
        return None;
    }
    let byte = |i: usize| u8::from_str_radix(h.get(i..i + 2)?, 16).ok();
    match h.len() {
        3 => {
            let d = |i: usize| {
                u8::from_str_radix(h.get(i..i + 1)?, 16)
                    .ok()
                    .map(|v| v * 17)
            };
            Some(Color32::from_rgb(d(0)?, d(1)?, d(2)?))
        }
        6 => Some(Color32::from_rgb(byte(0)?, byte(2)?, byte(4)?)),
        8 => Some(Color32::from_rgba_unmultiplied(
            byte(0)?,
            byte(2)?,
            byte(4)?,
            byte(6)?,
        )),
        _ => None,
    }
}

impl CustomTheme {
    /// A new theme with a preset's colours.
    pub fn from_preset(name: impl Into<String>, base: Preset) -> Self {
        Self {
            name: name.into(),
            base,
            palette: base.palette(),
        }
    }

    /// The `.mistytheme` text.
    pub fn to_text(&self) -> String {
        let colors = TOKENS
            .iter()
            .filter_map(|t| Some((t.key.to_owned(), to_hex(self.palette.get(t.key)?))))
            .collect();
        let file = ThemeFile {
            format: FORMAT,
            name: self.name.clone(),
            base: self.base.id().to_owned(),
            colors,
        };
        serde_json::to_string_pretty(&file).unwrap_or_default() + "\n"
    }

    /// Read a `.mistytheme` text. Unknown tokens are skipped; a colour that does not parse is
    /// an error (so a typo is not silently dropped).
    pub fn from_text(text: &str) -> Result<Self, String> {
        let file: ThemeFile =
            serde_json::from_str(text).map_err(|e| format!("not a theme file: {e}"))?;
        // A newer format still loads: unknown tokens are skipped below.
        let _ = file.format.max(FORMAT);
        let base = Preset::from_id(&file.base).unwrap_or(Preset::Dark);
        let mut palette = base.palette();
        for (key, value) in &file.colors {
            let Some(slot) = palette.get_mut(key) else {
                continue;
            };
            *slot = from_hex(value).ok_or_else(|| format!("{key}: \"{value}\" is not a colour"))?;
        }
        let name = clean_name(&file.name);
        Ok(Self {
            name: if name.is_empty() {
                "Imported".to_owned()
            } else {
                name
            },
            base,
            palette,
        })
    }

    /// Write the theme to `path`.
    pub fn export(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent()
            && !dir.as_os_str().is_empty()
        {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_text()).map_err(|e| e.to_string())
    }

    /// Read a theme from `path`; a nameless file is named after the file.
    pub fn import(path: &Path) -> Result<Self, String> {
        let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
        let mut t = Self::from_text(&text)?;
        if t.name == "Imported"
            && let Some(stem) = path.file_stem()
        {
            let stem = clean_name(&stem.to_string_lossy());
            if !stem.is_empty() {
                t.name = stem;
            }
        }
        Ok(t)
    }
}

/// A theme name without characters that can't go in a file name; trimmed, at most 60 chars.
pub fn clean_name(name: &str) -> String {
    let cleaned: String = name
        .chars()
        .filter(|c| {
            !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control()
        })
        .take(60)
        .collect();
    cleaned.trim().trim_end_matches('.').trim().to_owned()
}

/// Text / background pairs that must stay readable, with their contrast ratio, lowest first:
/// those under `min` (4.5 is WCAG AA for body text; weak text gets 3).
pub fn low_contrast(p: &Palette) -> Vec<(&'static str, &'static str, f32)> {
    let pairs: [(&str, Color32, &str, Color32, f32); 8] = [
        ("Text", p.text, "Panel", p.panel_bg, 4.5),
        ("Text", p.text, "Field", p.field_bg, 4.5),
        ("Text", p.text, "Card", p.card_bg, 4.5),
        ("Text", p.text, "Button", p.button_bg, 3.0),
        ("Text", p.text, "Popup", p.popup_bg, 4.5),
        ("Weak text", p.text_dim, "Panel", p.panel_bg, 3.0),
        (
            "Selected text",
            p.selection_text,
            "Selection",
            p.selection,
            3.0,
        ),
        ("Code text", p.code_text, "Code background", p.code_bg, 4.5),
    ];
    let mut out: Vec<_> = pairs
        .into_iter()
        .filter_map(|(a, ca, b, cb, min)| {
            let r = contrast(ca, cb);
            (r < min).then_some((a, b, r))
        })
        .collect();
    out.sort_by(|a, b| a.2.total_cmp(&b.2));
    out
}

/// The saved theme choice.
#[derive(Clone, Debug, PartialEq)]
pub enum Choice {
    Preset(Preset),
    Custom(String),
}

impl Default for Choice {
    fn default() -> Self {
        Choice::Preset(Preset::Dark)
    }
}

impl Choice {
    /// The text stored in `hub.json` (`light`, `custom:My Theme`).
    pub fn to_setting(&self) -> String {
        match self {
            Choice::Preset(p) => p.id().to_owned(),
            Choice::Custom(name) => format!("custom:{name}"),
        }
    }

    pub fn from_setting(s: &str) -> Self {
        if let Some(name) = s.strip_prefix("custom:") {
            Choice::Custom(name.to_owned())
        } else {
            Choice::Preset(Preset::from_id(s).unwrap_or(Preset::Dark))
        }
    }
}

/// The custom themes in a folder.
#[derive(Clone, Debug, Default)]
pub struct ThemeStore {
    /// `None` = nowhere to save (no config folder): themes live for this session only.
    pub dir: Option<PathBuf>,
    pub themes: Vec<CustomTheme>,
}

/// The per-user themes folder (`%APPDATA%\MistyEngine\themes`).
pub fn themes_dir() -> Option<PathBuf> {
    crate::hub::config::config_dir().map(|d| d.join("themes"))
}

impl ThemeStore {
    /// Every readable `.mistytheme` in `dir`, by name (unreadable files are skipped).
    pub fn load(dir: Option<PathBuf>) -> Self {
        let mut themes = Vec::new();
        if let Some(d) = &dir
            && let Ok(entries) = std::fs::read_dir(d)
        {
            for e in entries.flatten() {
                let path = e.path();
                if path
                    .extension()
                    .is_some_and(|x| x.eq_ignore_ascii_case(EXTENSION))
                    && let Ok(t) = CustomTheme::import(&path)
                    && !themes
                        .iter()
                        .any(|o: &CustomTheme| o.name.eq_ignore_ascii_case(&t.name))
                {
                    themes.push(t);
                }
            }
        }
        themes.sort_by_key(|t| t.name.to_lowercase());
        Self { dir, themes }
    }

    pub fn get(&self, name: &str) -> Option<&CustomTheme> {
        self.themes.iter().find(|t| t.name == name)
    }

    fn file(&self, name: &str) -> Option<PathBuf> {
        self.dir
            .as_ref()
            .map(|d| d.join(format!("{}.{EXTENSION}", clean_name(name))))
    }

    /// A name not used by another theme or preset: `base`, `base 2`, `base 3` …
    pub fn unique_name(&self, base: &str) -> String {
        let base = clean_name(base);
        let base = if base.is_empty() {
            "Custom".to_owned()
        } else {
            base
        };
        let taken = |n: &str| {
            self.themes.iter().any(|t| t.name.eq_ignore_ascii_case(n))
                || Preset::ALL.iter().any(|p| p.id().eq_ignore_ascii_case(n))
        };
        if !taken(&base) {
            return base;
        }
        (2..)
            .map(|i| format!("{base} {i}"))
            .find(|n| !taken(n))
            .unwrap_or(base)
    }

    /// Add or replace (by name) a theme and write its file.
    pub fn save(&mut self, theme: CustomTheme) -> Result<(), String> {
        let result = match self.file(&theme.name) {
            Some(path) => theme.export(&path),
            None => Ok(()),
        };
        match self.themes.iter_mut().find(|t| t.name == theme.name) {
            Some(t) => *t = theme,
            None => {
                self.themes.push(theme);
                self.themes.sort_by_key(|t| t.name.to_lowercase());
            }
        }
        result
    }

    /// Remove a theme and its file.
    pub fn delete(&mut self, name: &str) -> Result<(), String> {
        self.themes.retain(|t| t.name != name);
        match self.file(name) {
            Some(path) if path.exists() => std::fs::remove_file(path).map_err(|e| e.to_string()),
            _ => Ok(()),
        }
    }

    /// Rename a theme (and its file); the new name is made unique. Returns it.
    pub fn rename(&mut self, from: &str, to: &str) -> Result<String, String> {
        let Some(mut theme) = self.get(from).cloned() else {
            return Err(format!("no theme named {from}"));
        };
        let to = clean_name(to);
        if to.is_empty() || to == from {
            return Ok(from.to_owned());
        }
        self.themes.retain(|t| t.name != from);
        let name = self.unique_name(&to);
        if let Some(old) = self.file(from)
            && old.exists()
        {
            std::fs::remove_file(old).map_err(|e| e.to_string())?;
        }
        theme.name = name.clone();
        self.save(theme)?;
        Ok(name)
    }

    /// The palette for a choice (`None` when a custom theme is gone).
    pub fn palette(&self, choice: &Choice) -> Option<Palette> {
        match choice {
            Choice::Preset(p) => Some(p.palette()),
            Choice::Custom(name) => self.get(name).map(|t| t.palette),
        }
    }
}

/// The saved choice; `MISTY_THEME=<preset id or custom theme name>` overrides it.
pub fn saved_choice() -> Choice {
    if let Ok(v) = std::env::var("MISTY_THEME")
        && !v.trim().is_empty()
    {
        return match Preset::from_id(&v) {
            Some(p) => Choice::Preset(p),
            None => Choice::Custom(v.trim().trim_start_matches("custom:").to_owned()),
        };
    }
    crate::hub::config::config_path()
        .and_then(|p| crate::hub::config::HubConfig::load(&p).editor_theme)
        .map(|s| Choice::from_setting(&s))
        .unwrap_or_default()
}

/// The palette the editor starts with (Dark when a saved custom theme is gone).
pub fn startup_palette() -> Palette {
    let choice = saved_choice();
    match &choice {
        Choice::Preset(p) => p.palette(),
        Choice::Custom(_) => ThemeStore::load(themes_dir())
            .palette(&choice)
            .unwrap_or_else(|| Preset::Dark.palette()),
    }
}

/// Save the theme choice in `hub.json` at `config` (`None` = nowhere: tests), and keep the
/// Hub's light / dark theme in step with it.
pub fn save_choice(
    config: Option<&Path>,
    choice: &Choice,
    palette: &Palette,
) -> Result<(), String> {
    let dark = palette.is_dark();
    let Some(config) = config else {
        return Ok(());
    };
    crate::hub::config::update_at(config, |c| {
        c.editor_theme = (*choice != Choice::default()).then(|| choice.to_setting());
        c.theme = if dark {
            crate::hub::config::HubTheme::Dark
        } else {
            crate::hub::config::HubTheme::Light
        };
    })
}
