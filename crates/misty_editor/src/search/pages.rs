//! Settings pages (Project Settings categories, Preferences) and Help headings from `docs/`.

use crate::project_settings::Category;
use crate::state::EditorState;

/// A page the palette can jump to.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SettingsPage {
    Project(Category),
    Preferences,
}

impl SettingsPage {
    pub fn all() -> impl Iterator<Item = SettingsPage> {
        Category::ALL
            .into_iter()
            .map(SettingsPage::Project)
            .chain([SettingsPage::Preferences])
    }

    pub fn title(self) -> &'static str {
        match self {
            SettingsPage::Project(c) => c.title(),
            SettingsPage::Preferences => "Preferences",
        }
    }

    /// Window the page lives in (shown dimmed next to the title).
    pub fn window(self) -> &'static str {
        match self {
            SettingsPage::Project(_) => "Project Settings",
            SettingsPage::Preferences => "Preferences",
        }
    }

    /// Extra words that find the page ("gravity" → Physics).
    pub fn keywords(self) -> &'static str {
        match self {
            SettingsPage::Project(c) => c.blurb(),
            SettingsPage::Preferences => "External Tools script editor Code Editor font tabs",
        }
    }

    pub fn open(self, s: &mut EditorState) {
        match self {
            SettingsPage::Project(c) => crate::project_settings::open_at(s, c),
            SettingsPage::Preferences => s.code_editor.ui.prefs_open = true,
        }
    }
}

/// Guides in `docs/` (embedded so headings are searchable in any build).
const DOCS: [(&str, &str); 4] = [
    ("building.md", include_str!("../../../../docs/building.md")),
    (
        "scripting.md",
        include_str!("../../../../docs/scripting.md"),
    ),
    ("shaders.md", include_str!("../../../../docs/shaders.md")),
    (
        "world_builder.md",
        include_str!("../../../../docs/world_builder.md"),
    ),
];

/// A heading in one of the guides.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DocHeading {
    pub file: &'static str,
    pub title: String,
    /// The guide's top heading (shown dimmed).
    pub guide: String,
}

/// Every `#`/`##`/`###` heading of the guides.
pub fn doc_headings() -> Vec<DocHeading> {
    let mut out = Vec::new();
    for (file, text) in DOCS {
        let mut guide = String::new();
        let mut in_code = false;
        for line in text.lines() {
            if line.trim_start().starts_with("```") {
                in_code = !in_code;
            }
            let Some(title) = line.strip_prefix('#').filter(|_| !in_code) else {
                continue;
            };
            let title = title.trim_start_matches('#').trim().to_owned();
            if guide.is_empty() {
                guide = title.clone();
            }
            out.push(DocHeading {
                file,
                title,
                guide: guide.clone(),
            });
        }
    }
    out
}

/// Open a guide in the system's viewer for Markdown files. The embedded copy is written to the
/// temp folder first, so this works in a packaged editor without the source tree's `docs/`.
pub fn open_doc(s: &mut EditorState, file: &str) {
    let Some((_, text)) = DOCS.iter().find(|(f, _)| *f == file) else {
        s.console.warn(format!("No guide named {file}."));
        return;
    };
    let dir = std::env::temp_dir().join("MistyEngine-guides");
    let path = dir.join(file);
    let written = std::fs::create_dir_all(&dir).and_then(|_| std::fs::write(&path, text));
    if let Err(e) = written
        .map_err(|e| e.to_string())
        .and_then(|_| crate::assets::open_external(&path).map_err(|e| e.to_string()))
    {
        s.console.warn(format!("Could not open {file}: {e}"));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn headings_found() {
        let h = doc_headings();
        assert!(
            h.iter()
                .any(|d| d.file == "building.md" && d.title.contains("itch"))
        );
        assert!(
            h.iter()
                .all(|d| !d.title.is_empty() && !d.title.starts_with('#'))
        );
    }

    #[test]
    fn pages_cover_every_category() {
        assert_eq!(SettingsPage::all().count(), Category::ALL.len() + 1);
        assert!(
            SettingsPage::Project(Category::Physics)
                .keywords()
                .contains("Gravity")
        );
    }
}
