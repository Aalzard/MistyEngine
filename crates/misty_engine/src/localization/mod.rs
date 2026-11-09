//! Localization: locales, String Tables, smart strings and the Localized Text component.
//!
//! - Project Settings ▸ Localization ([`LocalizationSettings`]): the locales, the default and
//!   fallback locale and how the game picks its locale at startup.
//! - String Tables (`.strings`, [`StringTable`]): keys × locales, anywhere under `Assets/`.
//! - [`Localizer`]: a game's tables and lookups (`Localization.Get(key, args)` in Lua).
//! - [`LocalizedText`]: drives the UI Text on its object from a key and arguments, and follows
//!   locale changes. It only writes the Text's string, so it works with any text rendering.
//!
//! The current locale belongs to the running game session (it survives scene loads): see
//! [`session_locale`] / [`set_session_locale`]. Choosing one also stores it in PlayerPrefs, so
//! the "saved preference" startup mode can restore it next time.

pub mod case;
pub mod format;
pub mod table;
#[cfg(test)]
mod tests;

use std::path::Path;
use std::sync::Mutex;
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};

pub use case::{to_lower, to_title, to_upper};
pub use format::{Args, is_right_to_left, language};
pub use table::{StringEntry, StringTable, TableKind};

use crate::scene::{Scene, UiText};

/// PlayerPrefs key of the chosen locale.
pub const PREF_KEY: &str = "Misty.Locale";

/// A language (and optionally region) the game is translated into.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Locale {
    /// "en", "fr", "pt-BR"…
    pub code: String,
    /// Shown to players ("Français").
    pub name: String,
}

impl Locale {
    pub fn new(code: &str, name: &str) -> Self {
        Self {
            code: code.into(),
            name: name.into(),
        }
    }

    pub fn label(&self) -> String {
        if self.name.is_empty() {
            self.code.clone()
        } else {
            self.name.clone()
        }
    }
}

/// How the game picks its locale when it starts.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum StartupLocale {
    /// The player's last choice (PlayerPrefs), else the system language, else the default.
    #[default]
    SavedPreference,
    /// The operating system's language when the game has it, else the default.
    SystemLanguage,
    /// Always the default locale.
    Default,
}

impl StartupLocale {
    pub const ALL: [Self; 3] = [Self::SavedPreference, Self::SystemLanguage, Self::Default];

    pub fn label(self) -> &'static str {
        match self {
            Self::SavedPreference => "Saved Preference",
            Self::SystemLanguage => "System Language",
            Self::Default => "Default Locale",
        }
    }
}

/// Project Settings ▸ Localization.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalizationSettings {
    /// Empty = every locale column found in the String Tables.
    pub locales: Vec<Locale>,
    /// Used at startup when nothing better is known.
    pub default_locale: String,
    /// Shown when the current locale has no text for a key.
    pub fallback_locale: String,
    pub startup: StartupLocale,
}

impl Default for LocalizationSettings {
    fn default() -> Self {
        Self {
            locales: Vec::new(),
            default_locale: "en".into(),
            fallback_locale: "en".into(),
            startup: StartupLocale::default(),
        }
    }
}

// ---- the session's locale ------------------------------------------------------------------

static SESSION: Mutex<Option<String>> = Mutex::new(None);
static GENERATION: AtomicU64 = AtomicU64::new(1);

/// Tests that change the session locale hold this.
#[doc(hidden)]
pub static TEST_LOCK: Mutex<()> = Mutex::new(());

/// The locale chosen in this game session (`None` until a game picks one).
pub fn session_locale() -> Option<String> {
    SESSION.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

/// Change the session's locale (and remember it in PlayerPrefs). Returns whether it changed.
pub fn set_session_locale(code: &str) -> bool {
    let changed = {
        let mut s = SESSION.lock().unwrap_or_else(|e| e.into_inner());
        if s.as_deref() == Some(code) {
            false
        } else {
            *s = Some(code.to_owned());
            true
        }
    };
    if changed {
        GENERATION.fetch_add(1, Ordering::Relaxed);
        crate::prefs::set(PREF_KEY, crate::prefs::PrefValue::String(code.to_owned()));
    }
    changed
}

/// Set the startup locale without counting it as a change.
fn init_session_locale(code: &str) {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = Some(code.to_owned());
}

/// Forget the session's locale: the next game picks one with its startup mode (editor Play).
pub fn reset_session() {
    *SESSION.lock().unwrap_or_else(|e| e.into_inner()) = None;
    GENERATION.fetch_add(1, Ordering::Relaxed);
}

/// Bumped on every locale change (cheap "did it change?" checks).
pub fn generation() -> u64 {
    GENERATION.load(Ordering::Relaxed)
}

/// The operating system's language ("fr-FR"), when it can be found.
pub fn system_language() -> Option<String> {
    #[cfg(windows)]
    {
        #[link(name = "kernel32")]
        unsafe extern "system" {
            fn GetUserDefaultLocaleName(name: *mut u16, len: i32) -> i32;
        }
        let mut buf = [0u16; 85];
        // SAFETY: the buffer holds LOCALE_NAME_MAX_LENGTH (85) UTF-16 units, as documented.
        let n = unsafe { GetUserDefaultLocaleName(buf.as_mut_ptr(), buf.len() as i32) };
        if n > 1 {
            return Some(String::from_utf16_lossy(&buf[..n as usize - 1]));
        }
        None
    }
    #[cfg(not(windows))]
    {
        ["LC_ALL", "LC_MESSAGES", "LANG", "LANGUAGE"]
            .iter()
            .filter_map(|k| std::env::var(k).ok())
            .map(|v| v.split(['.', '@', ':']).next().unwrap_or("").to_owned())
            .find(|v| !v.is_empty() && v != "C" && v != "POSIX")
            .map(|v| v.replace('_', "-"))
    }
}

// ---- a game's tables -------------------------------------------------------------------------

/// The String Tables and settings of a running game (or the editor's preview).
#[derive(Clone, Debug, Default)]
pub struct Localizer {
    pub settings: LocalizationSettings,
    pub tables: Vec<StringTable>,
    seen: u64,
}

impl Localizer {
    pub fn new(settings: LocalizationSettings, tables: Vec<StringTable>) -> Self {
        Self {
            settings,
            tables,
            seen: generation(),
        }
    }

    /// Every `.strings` file under the project's `Assets/` (from the pak in built games).
    pub fn load(root: &Path, settings: &LocalizationSettings) -> (Self, Vec<String>) {
        let (tables, warnings) = load_tables(root);
        (Self::new(settings.clone(), tables), warnings)
    }

    /// Locales players can choose: the settings' list, else every table column.
    pub fn locales(&self) -> Vec<Locale> {
        if !self.settings.locales.is_empty() {
            return self.settings.locales.clone();
        }
        let mut out: Vec<Locale> = Vec::new();
        for t in &self.tables {
            for l in &t.locales {
                if !out.iter().any(|o| &o.code == l) {
                    out.push(Locale::new(l, ""));
                }
            }
        }
        out
    }

    /// The available locale `code` means: an exact match, else the same language.
    pub fn resolve_code(&self, code: &str) -> Option<String> {
        let code = code.trim();
        let locales = self.locales();
        if let Some(l) = locales.iter().find(|l| l.code.eq_ignore_ascii_case(code)) {
            return Some(l.code.clone());
        }
        let lang = language(code);
        locales
            .iter()
            .find(|l| language(&l.code) == lang)
            .map(|l| l.code.clone())
    }

    fn default_code(&self) -> String {
        self.resolve_code(&self.settings.default_locale)
            .or_else(|| self.locales().first().map(|l| l.code.clone()))
            .unwrap_or_else(|| self.settings.default_locale.clone())
    }

    /// The locale the startup mode picks.
    pub fn startup_locale(&self) -> String {
        let saved = || {
            let s = crate::prefs::get_string(PREF_KEY, "");
            (!s.is_empty()).then(|| self.resolve_code(&s)).flatten()
        };
        let system = || system_language().and_then(|s| self.resolve_code(&s));
        match self.settings.startup {
            StartupLocale::SavedPreference => saved().or_else(system),
            StartupLocale::SystemLanguage => system(),
            StartupLocale::Default => None,
        }
        .unwrap_or_else(|| self.default_code())
    }

    /// The current locale (picked with the startup mode the first time).
    pub fn locale(&self) -> String {
        if let Some(c) = session_locale().and_then(|c| self.resolve_code(&c)) {
            return c;
        }
        let code = self.startup_locale();
        init_session_locale(&code);
        code
    }

    /// Switch locale. Returns the locale used, or an error when the game doesn't have it.
    pub fn set_locale(&self, code: &str) -> Result<String, String> {
        let resolved = self
            .resolve_code(code)
            .ok_or_else(|| format!("locale '{code}' is not in Localization Settings"))?;
        set_session_locale(&resolved);
        Ok(resolved)
    }

    /// The locale changed since the last call.
    pub fn take_changed(&mut self) -> bool {
        let g = generation();
        std::mem::replace(&mut self.seen, g) != g
    }

    /// Raw text of `key` in `locale` (that column, else a column of the same language).
    pub fn lookup(&self, table: Option<&str>, key: &str, locale: &str) -> Option<&str> {
        let lang = language(locale);
        self.tables
            .iter()
            .filter(|t| table.is_none_or(|n| n.is_empty() || t.name == n))
            .find_map(|t| {
                t.get(key, locale).or_else(|| {
                    t.locales
                        .iter()
                        .find(|l| language(l) == lang && l.as_str() != locale)
                        .and_then(|l| t.get(key, l))
                })
            })
    }

    /// Text of `key` in `locale`, then the fallback and default locales; formatted with `args`.
    /// A key with no text anywhere comes back as the key itself.
    pub fn get_in(&self, table: Option<&str>, key: &str, args: &Args, locale: &str) -> String {
        let chain = [
            locale.to_owned(),
            self.settings.fallback_locale.clone(),
            self.default_code(),
        ];
        for l in &chain {
            if let Some(text) = self.lookup(table, key, l) {
                return format::format(text, args, l);
            }
        }
        key.to_owned()
    }

    /// Text of `key` in the current locale.
    pub fn get(&self, table: Option<&str>, key: &str, args: &Args) -> String {
        self.get_in(table, key, args, &self.locale())
    }

    /// Whether any table has `key`.
    pub fn has_key(&self, table: Option<&str>, key: &str) -> bool {
        self.tables
            .iter()
            .filter(|t| table.is_none_or(|n| n.is_empty() || t.name == n))
            .any(|t| t.entry(key).is_some())
    }
}

/// Every String Table under `root/Assets`, sorted by path.
pub fn load_tables(root: &Path) -> (Vec<StringTable>, Vec<String>) {
    let mut tables = Vec::new();
    let mut warnings = Vec::new();
    for p in crate::asset_db::assets_with_extension(root, table::EXTENSION) {
        match StringTable::load(&p) {
            Ok(t) => tables.push(t),
            Err(e) => warnings.push(e),
        }
    }
    (tables, warnings)
}

// ---- Localized Text --------------------------------------------------------------------------

/// Sets the UI Text on its object to a localized string, and keeps it up to date.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LocalizedText {
    pub enabled: bool,
    /// String Table name ("" = search every table).
    pub table: String,
    pub key: String,
    /// Smart-string arguments: `{0}`, `{1}`… in order; `name=value` entries fill `{name}`.
    pub args: Vec<String>,
}

impl Default for LocalizedText {
    fn default() -> Self {
        Self {
            enabled: true,
            table: String::new(),
            key: String::new(),
            args: Vec::new(),
        }
    }
}

impl crate::scene::SceneComponent for LocalizedText {
    const NAME: &'static str = "Localized Text";
    const CATEGORY: &'static str = "UI";
}

impl LocalizedText {
    /// The string this component shows in `locale`.
    pub fn resolve(&self, loc: &Localizer, locale: &str) -> String {
        let table = Some(self.table.as_str()).filter(|t| !t.is_empty());
        loc.get_in(table, &self.key, &Args::from_entries(&self.args), locale)
    }
}

/// Write every enabled Localized Text's string into its UI Text (only when it differs).
/// Returns how many Texts changed.
pub fn refresh_texts(scene: &mut Scene, loc: &Localizer) -> usize {
    let targets: Vec<(hecs::Entity, LocalizedText)> = scene
        .world
        .query::<(hecs::Entity, &LocalizedText)>()
        .iter()
        .filter(|(_, l)| l.enabled && !l.key.is_empty())
        .map(|(e, l)| (e, l.clone()))
        .collect();
    if targets.is_empty() {
        return 0;
    }
    let locale = loc.locale();
    let mut changed = 0;
    for (e, l) in targets {
        let text = l.resolve(loc, &locale);
        if let Ok(mut t) = scene.world.get::<&mut UiText>(e)
            && t.text != text
        {
            t.text = text;
            changed += 1;
        }
    }
    changed
}
