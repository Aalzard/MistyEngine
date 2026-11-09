//! Editor UI languages: English (the source), Türkçe and العربية.
//!
//! Text is keyed by its English source: `tr!("Save")` is "Save" in English, the translation from
//! `locales/<code>.toml` in another language, and English again when that file has no entry, so
//! untranslated text falls back by itself. Named placeholders are filled after translating:
//! `tr!("Save changes to {scene}?", scene = name)`. When one English word needs two
//! translations, the key gets a `#context` suffix that is never shown: `tr!("Light#theme")`.
//!
//! Arabic text comes back shaped for egui (letters joined, right-to-left runs in visual order,
//! see [`shape`]); [`display`] does the same for user content (object names, console lines).
//! [`is_rtl`] tells layouts to mirror (the menu bar, Inspector labels, the default docking).
//!
//! The language is Edit ▸ Preferences ▸ General ▸ Language (also on the Hub's Settings page),
//! saved per user in `hub.json` and applied on the next frame. `MISTY_EDITOR_LANGUAGE=tr`
//! (`en`, `tr`, `ar`) overrides it for one run (screenshots). How to add a language and the
//! term glossary: `docs/localization.md`.

pub mod catalog;
pub mod shape;
#[cfg(test)]
mod tests;

use std::borrow::Cow;
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::fmt::Display;

/// A language the editor UI is translated into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum Language {
    #[default]
    English,
    Turkish,
    Arabic,
}

impl Language {
    pub const ALL: [Language; 3] = [Language::English, Language::Turkish, Language::Arabic];

    /// ISO 639-1 code ("en", "tr", "ar"), also the translation file's name.
    pub fn code(self) -> &'static str {
        match self {
            Language::English => "en",
            Language::Turkish => "tr",
            Language::Arabic => "ar",
        }
    }

    /// The language's own name, as the language picker shows it.
    pub fn native_name(self) -> &'static str {
        match self {
            Language::English => "English",
            Language::Turkish => "Türkçe",
            Language::Arabic => "العربية",
        }
    }

    /// "tr", "tr-TR", "Turkish" or "Türkçe" → Turkish.
    pub fn from_code(code: &str) -> Option<Language> {
        let c = code.trim();
        let lang = c
            .split(['-', '_'])
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        Language::ALL.into_iter().find(|l| {
            l.code() == lang
                || l.native_name().eq_ignore_ascii_case(c)
                || format!("{l:?}").eq_ignore_ascii_case(c)
        })
    }

    /// Written right to left.
    pub fn is_rtl(self) -> bool {
        self == Language::Arabic
    }
}

thread_local! {
    // The UI runs on one thread; tests (one thread each) keep their own language.
    static CURRENT: Cell<Language> = const { Cell::new(Language::English) };
    // Shaped right-to-left text by source (cleared on language changes; capped).
    static SHAPED: RefCell<HashMap<String, String>> = RefCell::new(HashMap::new());
}

/// Longest cache of shaped text kept.
const SHAPED_CAP: usize = 8192;
/// Longer right-to-left texts are broken into lines of about this many characters (tooltips
/// and wrapped labels would otherwise wrap visual text from the wrong end).
const LINE_CHARS: usize = 64;

/// The editor UI's language.
pub fn language() -> Language {
    CURRENT.with(Cell::get)
}

/// Switch the editor UI language (this thread; takes effect on the next frame).
pub fn set_language(lang: Language) {
    if language() != lang {
        CURRENT.with(|c| c.set(lang));
        SHAPED.with(|s| s.borrow_mut().clear());
    }
}

/// The UI is right to left (mirror layouts).
pub fn is_rtl() -> bool {
    language().is_rtl()
}

/// The language this run starts in: `MISTY_EDITOR_LANGUAGE`, else the saved preference, else
/// English.
pub fn startup_language() -> Language {
    std::env::var("MISTY_EDITOR_LANGUAGE")
        .ok()
        .and_then(|v| Language::from_code(&v))
        .or_else(|| crate::hub::config::editor_language().and_then(|c| Language::from_code(&c)))
        .unwrap_or_default()
}

/// Set the startup language (main thread, before the first frame).
pub fn init() {
    set_language(startup_language());
}

/// Change the language and save it as the user's preference.
pub fn choose(lang: Language) -> Result<(), String> {
    set_language(lang);
    crate::hub::config::set_editor_language(lang.code())
}

/// The translation of `english` in the current language, unshaped (logical order); `None` in
/// English or when the translation file has no entry.
pub fn lookup(english: &str) -> Option<&'static str> {
    catalog::get(language(), english)
}

/// `english` in the current language, ready to draw (see the module docs). Use [`tr!`].
pub fn tr(english: &str) -> String {
    match lookup(english) {
        Some(t) => display_ui(t),
        // Names and paths pass through (shaped when they are Arabic, so menus of user content
        // read right).
        None => display(source_text(english)).into_owned(),
    }
}

/// The English text of a key: a key may end in `#context` ("Light#theme") when one English
/// word needs two translations; the context is never shown. A stored name the editor shows
/// under another name ("World Partition", see [`crate::renames::shown_name`]) gives that name.
pub fn source_text(key: &str) -> &str {
    let shown = crate::renames::shown_name(key);
    if shown != key {
        return shown;
    }
    match key.rsplit_once('#') {
        Some((text, ctx))
            if !text.is_empty()
                && !ctx.is_empty()
                && ctx.chars().all(|c| c.is_ascii_lowercase() || c == '_') =>
        {
            text
        }
        _ => key,
    }
}

/// [`tr`] with `{name}` placeholders filled in after translating.
pub fn tr_args(english: &str, args: &[(&str, &dyn Display)]) -> String {
    let template = lookup(english).unwrap_or_else(|| source_text(english));
    let filled = fill(template, args);
    if language() == Language::English {
        filled
    } else {
        display_ui(&filled)
    }
}

/// Replace each `{name}` in `template` with its argument (unknown ones stay as written).
pub fn fill(template: &str, args: &[(&str, &dyn Display)]) -> String {
    let mut out = String::with_capacity(template.len() + 16);
    let mut rest = template;
    while let Some(open) = rest.find('{') {
        out.push_str(&rest[..open]);
        let after = &rest[open + 1..];
        match after.find('}') {
            Some(close) => {
                let name = &after[..close];
                match args.iter().find(|(n, _)| *n == name) {
                    Some((_, v)) => out.push_str(&v.to_string()),
                    None => {
                        out.push('{');
                        out.push_str(name);
                        out.push('}');
                    }
                }
                rest = &after[close + 1..];
            }
            None => {
                out.push_str(&rest[open..]);
                rest = "";
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `{name}` placeholders of a text, sorted (placeholder checks).
#[cfg(test)]
pub fn placeholders(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text;
    while let Some(open) = rest.find('{') {
        let after = &rest[open + 1..];
        let Some(close) = after.find('}') else { break };
        out.push(after[..close].to_owned());
        rest = &after[close + 1..];
    }
    out.sort();
    out
}

/// Editor text in the current language's direction, cached.
fn display_ui(text: &str) -> String {
    if !shape::needs_shaping(text) {
        return text.to_owned();
    }
    cached(text, || {
        if text.chars().count() > LINE_CHARS && !text.contains('\n') {
            shape::visual_wrapped_chars(text, shape::Base::Rtl, LINE_CHARS)
        } else {
            shape::visual(text, shape::Base::Rtl).into_owned()
        }
    })
}

fn cached(text: &str, make: impl FnOnce() -> String) -> String {
    if let Some(hit) = SHAPED.with(|s| s.borrow().get(text).cloned()) {
        return hit;
    }
    let v = make();
    SHAPED.with(|s| {
        let mut s = s.borrow_mut();
        if s.len() >= SHAPED_CAP {
            s.clear();
        }
        s.insert(text.to_owned(), v.clone());
    });
    v
}

/// User content (an object or file name, a console line) ready to draw: right-to-left text is
/// shaped and put in visual order; anything else comes back as is. Only for drawing, never for
/// text fields.
pub fn display(text: &str) -> Cow<'_, str> {
    if !shape::needs_shaping(text) {
        return Cow::Borrowed(text);
    }
    Cow::Owned(cached(text, || {
        shape::visual(text, shape::Base::Auto).into_owned()
    }))
}

/// The translation of `english` broken into lines that fit `width` points in `ui`'s body font
/// (Arabic paragraphs in narrow places, such as the Hub's cards); other languages come back
/// unbroken for egui to wrap.
pub fn tr_wrapped(ui: &egui::Ui, english: &str, width: f32) -> String {
    let Some(t) = lookup(english).filter(|t| shape::needs_shaping(t)) else {
        return tr(english);
    };
    wrap_rtl(ui, t, width)
}

/// Right-to-left `text` (logical order) broken to fit `width` in `ui`'s body font, shaped.
pub fn wrap_rtl(ui: &egui::Ui, text: &str, width: f32) -> String {
    let font = egui::TextStyle::Body.resolve(ui.style());
    let color = ui.visuals().text_color();
    shape::visual_wrapped(text, shape::Base::Rtl, width, &mut |line: &str| {
        ui.ctx().fonts_mut(|f| {
            f.layout_no_wrap(line.to_owned(), font.clone(), color)
                .size()
                .x
        })
    })
}

/// `english` translated: `tr!("Save")`, or with named placeholders filled after translating:
/// `tr!("Delete {count} objects?", count = n)`.
macro_rules! tr_macro {
    ($english:expr $(,)?) => {
        $crate::i18n::tr($english)
    };
    ($english:expr, $($name:ident = $value:expr),+ $(,)?) => {
        $crate::i18n::tr_args(
            $english,
            &[$((stringify!($name), &$value as &dyn ::std::fmt::Display)),+],
        )
    };
}
pub(crate) use tr_macro as tr;

/// "Language" and its dropdown (each language in its own name); the language picked this
/// frame (Preferences ▸ General and the Hub's Settings).
pub fn picker_row(ui: &mut egui::Ui, id: &str) -> Option<Language> {
    let current = language();
    let mut picked = None;
    ui.horizontal(|ui| {
        let label = ui.label(tr!("Language"));
        let r = egui::ComboBox::from_id_salt(id)
            .selected_text(display(current.native_name()))
            .show_ui(ui, |ui| {
                for lang in Language::ALL {
                    let name = display(lang.native_name());
                    if ui.selectable_label(lang == current, name).clicked() && lang != current {
                        picked = Some(lang);
                    }
                }
            });
        r.response.labelled_by(label.id);
    });
    picked
}
