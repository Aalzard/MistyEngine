//! String Tables (`.strings`): keys × locales, JSON on disk, CSV import/export.
//!
//! ```json
//! { "locales": ["en", "fr"],
//!   "entries": [ { "key": "menu.play", "values": { "en": "Play", "fr": "Jouer" } } ] }
//! ```
//! The table's name is its file name ("UI.strings" → "UI"). An Asset Table is the same file with
//! `"kind": "assets"`: its cells hold asset paths (a sprite or sound per locale).

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};

pub const EXTENSION: &str = "strings";

/// What a table's cells hold.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TableKind {
    #[default]
    Strings,
    /// Project asset paths (localized sprites, audio clips…).
    Assets,
}

fn is_strings(k: &TableKind) -> bool {
    *k == TableKind::Strings
}

/// One key and its text per locale.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StringEntry {
    pub key: String,
    /// Note for translators.
    #[serde(skip_serializing_if = "String::is_empty")]
    pub comment: String,
    /// Locale code → text (missing or empty = not translated).
    pub values: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct StringTable {
    /// From the file name; not stored in the file.
    #[serde(skip)]
    pub name: String,
    #[serde(skip_serializing_if = "is_strings")]
    pub kind: TableKind,
    /// Locale columns, in display order.
    pub locales: Vec<String>,
    pub entries: Vec<StringEntry>,
}

impl StringTable {
    pub fn new(name: &str, locales: &[&str]) -> Self {
        Self {
            name: name.to_owned(),
            locales: locales.iter().map(|l| (*l).to_owned()).collect(),
            ..Default::default()
        }
    }

    pub fn to_json(&self) -> String {
        serde_json::to_string_pretty(self).expect("tables serialise") + "\n"
    }

    pub fn from_json(name: &str, text: &str) -> Result<Self, String> {
        let mut t: Self =
            serde_json::from_str(text).map_err(|e| format!("invalid string table: {e}"))?;
        t.name = name.to_owned();
        Ok(t)
    }

    pub fn load(path: &Path) -> Result<Self, String> {
        let text =
            crate::vfs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let name = path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_default();
        Self::from_json(&name, &text).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn save(&self, path: &Path) -> Result<(), String> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        std::fs::write(path, self.to_json()).map_err(|e| format!("{}: {e}", path.display()))
    }

    pub fn entry(&self, key: &str) -> Option<&StringEntry> {
        self.entries.iter().find(|e| e.key == key)
    }

    pub fn entry_mut(&mut self, key: &str) -> Option<&mut StringEntry> {
        self.entries.iter_mut().find(|e| e.key == key)
    }

    /// Text of `key` in exactly `locale` (empty cells count as missing).
    pub fn get(&self, key: &str, locale: &str) -> Option<&str> {
        self.entry(key)?
            .values
            .get(locale)
            .map(String::as_str)
            .filter(|s| !s.is_empty())
    }

    /// Set a cell, adding the key and the locale column when new.
    pub fn set(&mut self, key: &str, locale: &str, text: &str) {
        self.add_locale(locale);
        if self.entry(key).is_none() {
            self.add_key(key);
        }
        if let Some(e) = self.entry_mut(key) {
            if text.is_empty() {
                e.values.remove(locale);
            } else {
                e.values.insert(locale.to_owned(), text.to_owned());
            }
        }
    }

    /// Add an empty key. False when it is blank or already there.
    pub fn add_key(&mut self, key: &str) -> bool {
        let key = key.trim();
        if key.is_empty() || self.entry(key).is_some() {
            return false;
        }
        self.entries.push(StringEntry {
            key: key.to_owned(),
            ..Default::default()
        });
        true
    }

    pub fn remove_key(&mut self, key: &str) -> bool {
        let before = self.entries.len();
        self.entries.retain(|e| e.key != key);
        self.entries.len() != before
    }

    /// Rename a key (refused when the new name is blank or taken).
    pub fn rename_key(&mut self, old: &str, new: &str) -> bool {
        let new = new.trim();
        if new.is_empty() || (new != old && self.entry(new).is_some()) {
            return false;
        }
        match self.entry_mut(old) {
            Some(e) => {
                e.key = new.to_owned();
                true
            }
            None => false,
        }
    }

    pub fn add_locale(&mut self, code: &str) -> bool {
        let code = code.trim();
        if code.is_empty() || self.locales.iter().any(|l| l == code) {
            return false;
        }
        self.locales.push(code.to_owned());
        true
    }

    /// Remove a locale column and its texts.
    pub fn remove_locale(&mut self, code: &str) -> bool {
        let before = self.locales.len();
        self.locales.retain(|l| l != code);
        for e in &mut self.entries {
            e.values.remove(code);
        }
        self.locales.len() != before
    }

    /// Cells with no text: (key, locale).
    pub fn missing(&self) -> Vec<(String, String)> {
        let mut out = Vec::new();
        for e in &self.entries {
            for l in &self.locales {
                if e.values.get(l).is_none_or(|v| v.is_empty()) {
                    out.push((e.key.clone(), l.clone()));
                }
            }
        }
        out
    }

    // ---- CSV -------------------------------------------------------------------------------

    /// `Key,<locale>…[,Comment]` rows (UTF-8 with a byte-order mark, so spreadsheet apps read
    /// accents and non-Latin scripts correctly).
    pub fn to_csv(&self) -> String {
        let has_comments = self.entries.iter().any(|e| !e.comment.is_empty());
        let mut header = vec!["Key".to_owned()];
        header.extend(self.locales.iter().cloned());
        if has_comments {
            header.push("Comment".into());
        }
        let mut rows = vec![header];
        for e in &self.entries {
            let mut row = vec![e.key.clone()];
            row.extend(
                self.locales
                    .iter()
                    .map(|l| e.values.get(l).cloned().unwrap_or_default()),
            );
            if has_comments {
                row.push(e.comment.clone());
            }
            rows.push(row);
        }
        format!("\u{feff}{}", write_csv(&rows))
    }

    /// Merge CSV rows into the table: new keys and locale columns are added, cells in the file
    /// replace the table's. Returns (keys added, cells changed).
    pub fn merge_csv(&mut self, text: &str) -> Result<(usize, usize), String> {
        let rows = parse_csv(text)?;
        let Some((header, body)) = rows.split_first() else {
            return Err("the CSV file is empty".into());
        };
        let key_col = header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case("key"))
            .ok_or("the first row needs a \"Key\" column")?;
        let comment_col = header
            .iter()
            .position(|h| h.trim().eq_ignore_ascii_case("comment"));
        let locale_cols: Vec<(usize, String)> = header
            .iter()
            .enumerate()
            .filter(|(i, h)| *i != key_col && Some(*i) != comment_col && !h.trim().is_empty())
            .map(|(i, h)| (i, h.trim().to_owned()))
            .collect();
        for (_, l) in &locale_cols {
            self.add_locale(l);
        }
        let (mut added, mut changed) = (0, 0);
        for row in body {
            let Some(key) = row.get(key_col).map(|k| k.trim()) else {
                continue;
            };
            if key.is_empty() {
                continue;
            }
            if self.add_key(key) {
                added += 1;
            }
            let e = self.entry_mut(key).expect("added above");
            if let Some(c) = comment_col.and_then(|c| row.get(c)) {
                e.comment = c.clone();
            }
            for (i, l) in &locale_cols {
                let v = row.get(*i).cloned().unwrap_or_default();
                let old = e.values.get(l).cloned().unwrap_or_default();
                if v != old {
                    changed += 1;
                    if v.is_empty() {
                        e.values.remove(l);
                    } else {
                        e.values.insert(l.clone(), v);
                    }
                }
            }
        }
        Ok((added, changed))
    }

    /// A new table from CSV text.
    pub fn from_csv(name: &str, text: &str) -> Result<Self, String> {
        let mut t = Self::new(name, &[]);
        t.merge_csv(text)?;
        Ok(t)
    }
}

/// RFC 4180 CSV: fields with commas, quotes or line breaks are quoted, quotes doubled.
pub fn write_csv(rows: &[Vec<String>]) -> String {
    let mut out = String::new();
    for row in rows {
        let fields: Vec<String> = row
            .iter()
            .map(|f| {
                if f.contains([',', '"', '\n', '\r']) || f.starts_with(' ') || f.ends_with(' ') {
                    format!("\"{}\"", f.replace('"', "\"\""))
                } else {
                    f.clone()
                }
            })
            .collect();
        out.push_str(&fields.join(","));
        out.push_str("\r\n");
    }
    out
}

/// Parse CSV (quoted fields may hold commas, doubled quotes and line breaks; a leading byte-order
/// mark is skipped; `\r\n` and `\n` both end rows).
pub fn parse_csv(text: &str) -> Result<Vec<Vec<String>>, String> {
    let text = text.strip_prefix('\u{feff}').unwrap_or(text);
    let mut rows = Vec::new();
    let mut row = Vec::new();
    let mut field = String::new();
    let mut chars = text.chars().peekable();
    let mut quoted = false;
    let mut line = 1;
    let mut row_has_data = false;
    while let Some(c) = chars.next() {
        if quoted {
            match c {
                '"' if chars.peek() == Some(&'"') => {
                    chars.next();
                    field.push('"');
                }
                '"' => quoted = false,
                '\n' => {
                    line += 1;
                    field.push('\n');
                }
                // A quoted `\r\n` keeps just the `\n`.
                '\r' if chars.peek() == Some(&'\n') => {}
                c => field.push(c),
            }
            continue;
        }
        match c {
            '"' if field.is_empty() => {
                quoted = true;
                row_has_data = true;
            }
            ',' => {
                row.push(std::mem::take(&mut field));
                row_has_data = true;
            }
            '\r' if chars.peek() == Some(&'\n') => {}
            '\n' => {
                line += 1;
                if row_has_data || !field.is_empty() {
                    row.push(std::mem::take(&mut field));
                    rows.push(std::mem::take(&mut row));
                }
                row_has_data = false;
            }
            c => {
                field.push(c);
                row_has_data = true;
            }
        }
    }
    if quoted {
        return Err(format!("unclosed quote (line {line})"));
    }
    if row_has_data || !field.is_empty() {
        row.push(field);
        rows.push(row);
    }
    Ok(rows)
}
