//! Console log model: entries, filters and counters.

use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LogLevel {
    Info,
    Warning,
    Error,
}

impl LogLevel {
    pub fn icon(self) -> &'static str {
        match self {
            LogLevel::Info => "ℹ",
            LogLevel::Warning => "⚠",
            LogLevel::Error => "⛔",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            LogLevel::Info => egui::Color32::from_gray(200),
            LogLevel::Warning => egui::Color32::from_rgb(255, 200, 60),
            LogLevel::Error => egui::Color32::from_rgb(255, 90, 80),
        }
    }
}

#[derive(Clone, Debug)]
pub struct LogEntry {
    pub level: LogLevel,
    pub message: String,
    /// `HH:MM:SS` wall-clock time (UTC).
    pub time: String,
}

/// All console state shared by the Console panel and the status bar.
pub struct Console {
    pub entries: Vec<LogEntry>,
    pub collapse: bool,
    pub error_pause: bool,
    pub clear_on_play: bool,
    pub show_info: bool,
    pub show_warnings: bool,
    pub show_errors: bool,
    pub search: String,
    pub selected: Option<usize>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            collapse: false,
            error_pause: false,
            clear_on_play: false,
            show_info: true,
            show_warnings: true,
            show_errors: true,
            search: String::new(),
            selected: None,
        }
    }
}

impl Console {
    pub fn log(&mut self, level: LogLevel, message: impl Into<String>) {
        self.entries.push(LogEntry {
            level,
            message: message.into(),
            time: clock_time(),
        });
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Warning, message);
    }

    pub fn clear(&mut self) {
        self.entries.clear();
        self.selected = None;
    }

    pub fn count(&self, level: LogLevel) -> usize {
        self.entries.iter().filter(|e| e.level == level).count()
    }

    pub fn last(&self) -> Option<&LogEntry> {
        self.entries.last()
    }

    /// Whether an entry passes the type toggles and the search box.
    pub fn is_visible(&self, entry: &LogEntry) -> bool {
        let level_ok = match entry.level {
            LogLevel::Info => self.show_info,
            LogLevel::Warning => self.show_warnings,
            LogLevel::Error => self.show_errors,
        };
        level_ok
            && (self.search.is_empty()
                || entry
                    .message
                    .to_lowercase()
                    .contains(&self.search.to_lowercase()))
    }
}

fn clock_time() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let day = secs % 86_400;
    format!("{:02}:{:02}:{:02}", day / 3600, (day / 60) % 60, day % 60)
}
