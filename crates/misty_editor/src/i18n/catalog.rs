//! The translation files: `crates/misty_editor/locales/<code>.toml`, embedded in the binary.
//!
//! Each file is TOML: tables group the editor's areas (`[menu]`, `[hub]`, `[inspector]`, …) and
//! every entry maps the English text, exactly as the code writes it, to its translation:
//!
//! ```toml
//! [menu]
//! "File" = "Dosya"
//! "Save changes to {scene}?" = "{scene} sahnesindeki değişiklikler kaydedilsin mi?"
//! ```
//!
//! Tables only organise the file: lookups use the English text alone. An empty translation
//! (`""`) counts as not translated yet.

use std::collections::HashMap;
use std::sync::OnceLock;

use super::Language;

/// The file of `lang` (`None` for English, the source language).
pub fn source(lang: Language) -> Option<&'static str> {
    match lang {
        Language::English => None,
        Language::Turkish => Some(include_str!("../../locales/tr.toml")),
        Language::Arabic => Some(include_str!("../../locales/ar.toml")),
    }
}

/// One entry of a translation file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    pub table: String,
    pub english: String,
    pub translation: String,
}

/// Every entry of a translation file, in file order within each table.
pub fn parse(text: &str) -> Result<Vec<Entry>, String> {
    let doc: toml::Table = toml::from_str(text).map_err(|e| e.to_string())?;
    let mut out = Vec::new();
    for (table, value) in doc {
        let toml::Value::Table(entries) = value else {
            return Err(format!(
                "`{table}` is not a table: entries go inside [tables]"
            ));
        };
        for (english, v) in entries {
            let toml::Value::String(translation) = v else {
                return Err(format!("[{table}] \"{english}\" is not a string"));
            };
            out.push(Entry {
                table: table.clone(),
                english,
                translation,
            });
        }
    }
    Ok(out)
}

/// English → translation for `lang` (parsed once; empty for English or a broken file, which
/// the tests catch).
pub fn table(lang: Language) -> &'static HashMap<String, String> {
    static TABLES: [OnceLock<HashMap<String, String>>; 3] =
        [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    TABLES[lang as usize].get_or_init(|| {
        let Some(text) = source(lang) else {
            return HashMap::new();
        };
        parse(text)
            .unwrap_or_default()
            .into_iter()
            .filter(|e| !e.translation.is_empty())
            .map(|e| (e.english, e.translation))
            .collect()
    })
}

/// The translation of `english` in `lang`, if the file has one.
pub fn get(lang: Language, english: &str) -> Option<&'static str> {
    table(lang).get(english).map(String::as_str)
}
