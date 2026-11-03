//! Code Editor preferences, saved per user in the project's `Library/CodeEditor.json`.

use std::path::Path;

use serde::{Deserialize, Serialize};

const FILE: &str = "Library/CodeEditor.json";

/// Where double-clicked scripts open (Edit ▸ Preferences ▸ External Tools).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum ScriptEditor {
    /// The built-in Code Editor tab.
    #[default]
    Builtin,
    /// The operating system's default app for `.lua` files.
    External,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub script_editor: ScriptEditor,
    pub font_size: f32,
    pub word_wrap: bool,
    pub minimap: bool,
    pub tab_size: usize,
    /// Auto-close brackets and quotes.
    pub auto_close: bool,
    pub line_numbers: bool,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            script_editor: ScriptEditor::Builtin,
            font_size: 14.0,
            word_wrap: false,
            minimap: true,
            tab_size: 4,
            auto_close: true,
            line_numbers: true,
        }
    }
}

pub const MIN_FONT: f32 = 8.0;
pub const MAX_FONT: f32 = 40.0;

impl Settings {
    pub fn load(root: &Path) -> Self {
        let mut s: Settings = std::fs::read_to_string(root.join(FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        s.font_size = s.font_size.clamp(MIN_FONT, MAX_FONT);
        s.tab_size = s.tab_size.clamp(1, 8);
        s
    }

    pub fn save(&self, root: &Path) -> Result<(), String> {
        let path = root.join(FILE);
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
        }
        let text = serde_json::to_string_pretty(self).map_err(|e| e.to_string())?;
        std::fs::write(path, text).map_err(|e| e.to_string())
    }

    /// Zoom by `steps` (Ctrl+wheel, Ctrl+= / Ctrl+-).
    pub fn zoom(&mut self, steps: f32) {
        self.font_size = (self.font_size + steps).clamp(MIN_FONT, MAX_FONT).round();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_clamp() {
        let root = std::env::temp_dir().join(format!("misty_code_prefs_{}", std::process::id()));
        let mut s = Settings {
            script_editor: ScriptEditor::External,
            word_wrap: true,
            ..Default::default()
        };
        s.zoom(100.0);
        assert_eq!(s.font_size, MAX_FONT);
        s.save(&root).unwrap();
        assert_eq!(Settings::load(&root), s);
        std::fs::remove_dir_all(root).ok();
        assert_eq!(
            Settings::load(Path::new("no/such/dir")),
            Settings::default()
        );
    }
}
