//! Console log model: a bounded store of entries, the counters, the filters and the rows the
//! Console panel draws.
//!
//! The store keeps the newest [`Console::capacity`] lines (a ring buffer); older ones are
//! dropped. Every line has an index that never changes: the first line logged is 0, and
//! neither dropping old lines nor Clear starts the numbering again. The counters count every
//! line logged since the last Clear, dropped ones included.

use std::collections::hash_map::{DefaultHasher, Entry};
use std::collections::{HashMap, VecDeque};
use std::hash::{Hash, Hasher};
use std::time::{SystemTime, UNIX_EPOCH};

/// Lines kept unless Preferences says otherwise.
pub const DEFAULT_CAPACITY: usize = 10_000;
/// The sizes Preferences offers.
pub const CAPACITIES: [usize; 5] = [1_000, 5_000, 10_000, 50_000, 100_000];
const MIN_CAPACITY: usize = 100;
const MAX_CAPACITY: usize = 1_000_000;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
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

    fn slot(self) -> usize {
        match self {
            LogLevel::Info => 0,
            LogLevel::Warning => 1,
            LogLevel::Error => 2,
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

/// One row of the Console list: the line it shows and how many lines it stands for (more than
/// one only with Collapse on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Row {
    /// Index of the line shown (with Collapse: the first of the identical lines still kept).
    pub index: u64,
    pub count: usize,
}

/// What the rows were built for; they are rebuilt when the settings of the panel differ.
#[derive(Clone, Default, PartialEq)]
struct Filter {
    collapse: bool,
    show: [bool; 3],
    search: String,
    /// `search` in lower case.
    needle: String,
}

impl Filter {
    fn passes(&self, e: &LogEntry) -> bool {
        self.show[e.level.slot()]
            && (self.needle.is_empty() || e.message.to_lowercase().contains(&self.needle))
    }
}

/// The filtered (and collapsed) rows, kept up to date line by line.
#[derive(Default)]
struct RowCache {
    valid: bool,
    filter: Filter,
    rows: VecDeque<Row>,
    /// Collapse: hash of level and message → index of the line its row shows.
    seen: HashMap<u64, u64>,
    /// Times the rows were built from scratch (tests).
    rebuilds: u64,
}

fn message_hash(e: &LogEntry) -> u64 {
    let mut h = DefaultHasher::new();
    e.level.hash(&mut h);
    e.message.hash(&mut h);
    h.finish()
}

/// All console state shared by the Console panel, the status bar and the AI tools.
pub struct Console {
    /// The newest lines, oldest first.
    entries: VecDeque<LogEntry>,
    /// Index of `entries[0]`: the lines dropped or cleared before it.
    first_index: u64,
    /// Lines logged since the last Clear, by level (dropped ones included).
    counts: [usize; 3],
    capacity: usize,
    rows: RowCache,
    pub collapse: bool,
    pub error_pause: bool,
    pub clear_on_play: bool,
    pub show_info: bool,
    pub show_warnings: bool,
    pub show_errors: bool,
    pub search: String,
    /// Index of the selected line.
    pub selected: Option<u64>,
}

impl Default for Console {
    fn default() -> Self {
        Self {
            entries: VecDeque::new(),
            first_index: 0,
            counts: [0; 3],
            capacity: DEFAULT_CAPACITY,
            rows: RowCache::default(),
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
        while self.entries.len() >= self.capacity {
            self.drop_oldest();
        }
        self.counts[level.slot()] += 1;
        let index = self.next_index();
        self.entries.push_back(LogEntry {
            level,
            message: message.into(),
            time: clock_time(),
        });
        self.row_added(index);
    }

    pub fn info(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Info, message);
    }

    pub fn warn(&mut self, message: impl Into<String>) {
        self.log(LogLevel::Warning, message);
    }

    /// Empty the Console and zero the counters. Indices go on from where they were.
    pub fn clear(&mut self) {
        self.first_index = self.next_index();
        self.entries = VecDeque::new();
        self.counts = [0; 3];
        self.selected = None;
        self.rows.valid = false;
        self.rows.rows = VecDeque::new();
        self.rows.seen = HashMap::new();
    }

    /// Lines of this level logged since the last Clear, dropped ones included.
    pub fn count(&self, level: LogLevel) -> usize {
        self.counts[level.slot()]
    }

    /// Lines logged since the last Clear, dropped ones included.
    pub fn total(&self) -> usize {
        self.counts.iter().sum()
    }

    /// The lines still kept, oldest first.
    pub fn entries(&self) -> &VecDeque<LogEntry> {
        &self.entries
    }

    pub fn last(&self) -> Option<&LogEntry> {
        self.entries.back()
    }

    /// Index of the oldest line still kept (= `next_index` when there is none).
    pub fn first_index(&self) -> u64 {
        self.first_index
    }

    /// The index the next line will get.
    pub fn next_index(&self) -> u64 {
        self.first_index + self.entries.len() as u64
    }

    /// The line with this index, unless it was dropped or cleared.
    pub fn get(&self, index: u64) -> Option<&LogEntry> {
        let at = index.checked_sub(self.first_index)?;
        self.entries.get(usize::try_from(at).ok()?)
    }

    /// The kept lines from `index` on (from the oldest one when `index` is gone), with their
    /// indices.
    pub fn since(&self, index: u64) -> impl Iterator<Item = (u64, &LogEntry)> {
        let skip = index.saturating_sub(self.first_index);
        let skip = usize::try_from(skip).unwrap_or(usize::MAX);
        let first = self.first_index;
        self.entries
            .iter()
            .enumerate()
            .skip(skip)
            .map(move |(i, e)| (first + i as u64, e))
    }

    /// How many lines are kept before the oldest ones are dropped.
    pub fn capacity(&self) -> usize {
        self.capacity
    }

    /// Change how many lines are kept; the oldest ones beyond it are dropped now.
    pub fn set_capacity(&mut self, lines: usize) {
        self.capacity = lines.clamp(MIN_CAPACITY, MAX_CAPACITY);
        while self.entries.len() > self.capacity {
            self.drop_oldest();
        }
        if self.entries.capacity() > self.capacity * 2 {
            self.entries.shrink_to(self.capacity);
        }
    }

    /// The rows the panel lists, for its current toggles and search box. Kept from frame to
    /// frame: built again only when a toggle or the search changes (or, with Collapse on, when
    /// old lines were dropped); new lines are added to them as they are logged.
    pub fn rows(&mut self) -> &VecDeque<Row> {
        let c = &self.rows.filter;
        let same = self.rows.valid
            && c.collapse == self.collapse
            && c.show == [self.show_info, self.show_warnings, self.show_errors]
            && c.search == self.search;
        if !same {
            self.rebuild_rows();
        }
        &self.rows.rows
    }

    /// The rows as last built by [`Console::rows`] (the panel reads them while it changes the
    /// selection).
    pub fn built_rows(&self) -> &VecDeque<Row> {
        &self.rows.rows
    }

    #[cfg(test)]
    pub fn row_rebuilds(&self) -> u64 {
        self.rows.rebuilds
    }

    fn rebuild_rows(&mut self) {
        let cache = &mut self.rows;
        cache.filter = Filter {
            collapse: self.collapse,
            show: [self.show_info, self.show_warnings, self.show_errors],
            search: self.search.clone(),
            needle: self.search.to_lowercase(),
        };
        cache.rows.clear();
        cache.seen.clear();
        cache.valid = true;
        cache.rebuilds += 1;
        for at in 0..self.entries.len() {
            self.row_added(self.first_index + at as u64);
        }
    }

    /// Add the line `index` (already in `entries`) to the rows, if they are in use.
    fn row_added(&mut self, index: u64) {
        let cache = &mut self.rows;
        if !cache.valid {
            return;
        }
        let entries = &self.entries;
        let first = self.first_index;
        let at = |index: u64| &entries[(index - first) as usize];
        let e = at(index);
        if !cache.filter.passes(e) {
            return;
        }
        if cache.filter.collapse {
            match cache.seen.entry(message_hash(e)) {
                Entry::Occupied(o) => {
                    let shown = at(*o.get());
                    // The same hash for another text (all but impossible) gets its own row.
                    if shown.level == e.level && shown.message == e.message {
                        // Rows are in index order.
                        let pos = cache.rows.partition_point(|r| r.index < *o.get());
                        cache.rows[pos].count += 1;
                        return;
                    }
                }
                Entry::Vacant(v) => {
                    v.insert(index);
                }
            }
        }
        cache.rows.push_back(Row { index, count: 1 });
    }

    fn drop_oldest(&mut self) {
        let Some(old) = self.entries.pop_front() else {
            return;
        };
        let index = self.first_index;
        self.first_index += 1;
        if self.selected == Some(index) {
            self.selected = None;
        }
        let cache = &mut self.rows;
        if !cache.valid || cache.rows.front().is_none_or(|r| r.index != index) {
            // The filter hid it. (It cannot be counted in a row further down: a collapsed row
            // shows the oldest of its lines, and this was the oldest line of all.)
            return;
        }
        let Some(row) = cache.rows.pop_front() else {
            return;
        };
        if !cache.filter.collapse {
            return;
        }
        // Collapse: the row moves to the next identical line still kept, if there is one.
        let hash = message_hash(&old);
        let same = |e: &LogEntry| e.level == old.level && e.message == old.message;
        let next = (row.count > 1)
            .then(|| self.entries.iter().position(same))
            .flatten()
            .map(|at| self.first_index + at as u64);
        let known = cache.seen.get(&hash) == Some(&index);
        match next {
            Some(next) => {
                let pos = cache.rows.partition_point(|r| r.index < next);
                cache.rows.insert(
                    pos,
                    Row {
                        index: next,
                        count: row.count - 1,
                    },
                );
                if known {
                    cache.seen.insert(hash, next);
                }
            }
            None if row.count > 1 => cache.valid = false,
            None => {
                if known {
                    cache.seen.remove(&hash);
                }
            }
        }
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

#[cfg(test)]
#[path = "console_tests.rs"]
mod tests;
