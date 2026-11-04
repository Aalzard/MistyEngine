//! Global search (Ctrl+K): one box that finds scene objects, assets, menu commands, settings
//! pages and guide headings. Matching and filters are shared with the Hierarchy and Project
//! search boxes ([`fuzzy`], [`query`], [`scene_search`]).

pub mod asset_index;
pub mod calc;
pub mod dispatch;
pub mod fuzzy;
pub mod menu_items;
pub mod pages;
pub mod palette;
pub mod query;
pub mod scene_search;

use std::path::{Path, PathBuf};

use misty_engine::hecs::Entity;

use crate::state::EditorState;
use asset_index::AssetIndex;
use fuzzy::{Match, Mode};
use menu_items::{MenuCommand, MenuRun};
use pages::{DocHeading, SettingsPage};
use query::{Query, Scope};

/// Where a result comes from; also the group it is listed under.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Provider {
    Recent,
    Calculator,
    Hierarchy,
    Project,
    Menu,
    Settings,
    Help,
}

impl Provider {
    pub fn title(self) -> &'static str {
        match self {
            Provider::Recent => "Recent searches",
            Provider::Calculator => "Calculator",
            Provider::Hierarchy => "Hierarchy",
            Provider::Project => "Project",
            Provider::Menu => "Menu",
            Provider::Settings => "Settings",
            Provider::Help => "Help",
        }
    }

    pub fn icon(self) -> &'static str {
        match self {
            Provider::Recent => "🕘",
            Provider::Calculator => "🖩",
            Provider::Hierarchy => "☰",
            Provider::Project => "🗁",
            Provider::Menu => "☰",
            Provider::Settings => "⚙",
            Provider::Help => "❓",
        }
    }
}

/// What a result points at.
#[derive(Clone, Debug)]
pub enum Target {
    Object(Entity),
    Asset {
        rel: String,
        is_dir: bool,
    },
    Menu(MenuRun),
    Settings(SettingsPage),
    Doc(&'static str),
    /// Calculator result text.
    Calc(String),
    /// A recent search: Enter puts it back in the box.
    Recent(String),
}

/// One result row.
#[derive(Clone, Debug)]
pub struct Hit {
    pub provider: Provider,
    pub icon: &'static str,
    pub label: String,
    /// Dimmed text after the label (folder, menu path, window).
    pub detail: String,
    /// Right-aligned shortcut (menu commands).
    pub shortcut: &'static str,
    pub score: i32,
    /// Matched char positions in `label`.
    pub indices: Vec<usize>,
    pub target: Target,
}

/// Results per provider when searching everything, and with a provider prefix.
pub const CAP: usize = 6;
pub const CAP_SCOPED: usize = 50;
/// Recent searches kept.
pub const RECENT_MAX: usize = 8;
const RECENT_FILE: &str = "Library/SearchHistory.json";

/// Palette state (lives in [`EditorState::search`]).
pub struct SearchPalette {
    pub open: bool,
    pub query: String,
    /// Highlighted row in `hits`.
    pub selected: usize,
    /// Showing the secondary actions of this row (Alt+Enter / right-click).
    pub actions_of: Option<usize>,
    pub action_selected: usize,
    pub hits: Vec<Hit>,
    /// Query and index generation `hits` were computed for.
    cache_key: Option<(String, u64)>,
    pub index: AssetIndex,
    /// Menu commands, rebuilt each time the palette opens.
    menu: Vec<MenuCommand>,
    docs: Vec<DocHeading>,
    pub recent: Vec<String>,
    root: PathBuf,
    /// Give the text box focus next frame.
    pub focus_text: bool,
}

impl SearchPalette {
    pub fn new(root: &Path) -> Self {
        let recent = std::fs::read_to_string(root.join(RECENT_FILE))
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default();
        Self {
            open: false,
            query: String::new(),
            selected: 0,
            actions_of: None,
            action_selected: 0,
            hits: Vec::new(),
            cache_key: None,
            index: AssetIndex::new(root),
            menu: Vec::new(),
            docs: Vec::new(),
            recent,
            root: root.to_path_buf(),
            focus_text: false,
        }
    }

    /// Remember a search that led somewhere (most recent first, saved in the Library).
    pub fn remember(&mut self, text: &str) {
        let text = text.trim();
        if text.is_empty() {
            return;
        }
        self.recent.retain(|r| r != text);
        self.recent.insert(0, text.to_owned());
        self.recent.truncate(RECENT_MAX);
        let path = self.root.join(RECENT_FILE);
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        if let Ok(json) = serde_json::to_string_pretty(&self.recent) {
            let _ = std::fs::write(path, json);
        }
    }

    /// Replace the text (recent search picked, MISTY_SEARCH) and reset the highlight.
    pub fn set_query(&mut self, text: &str) {
        self.query = text.to_owned();
        self.selected = 0;
        self.actions_of = None;
        self.focus_text = true;
    }

    pub fn selected_hit(&self) -> Option<&Hit> {
        self.hits.get(self.selected)
    }
}

/// Open the palette (Ctrl+K): fresh menu list, text selected for typing over.
pub fn open(s: &mut EditorState, text: &str) {
    s.search.menu = menu_items::all(s);
    if s.search.docs.is_empty() {
        s.search.docs = pages::doc_headings();
    }
    s.search.open = true;
    s.search.set_query(text);
    s.search.cache_key = None;
}

pub fn close(s: &mut EditorState) {
    s.search.open = false;
    s.search.actions_of = None;
}

/// Re-run the providers if the text or the asset index changed since last time.
pub fn update(s: &mut EditorState) {
    let key = (s.search.query.clone(), s.search.index.generation);
    if s.search.cache_key.as_ref() == Some(&key) {
        return;
    }
    let hits = run(s, &s.search.query);
    let p = &mut s.search;
    p.hits = hits;
    p.selected = p.selected.min(p.hits.len().saturating_sub(1));
    p.cache_key = Some(key);
}

/// All results for `text`, grouped by provider; groups with the best match come first.
pub fn run(s: &EditorState, text: &str) -> Vec<Hit> {
    let p = &s.search;
    if text.trim().is_empty() {
        return p.recent.iter().map(|r| recent_hit(r)).collect();
    }
    let q = Query::parse_palette(text);
    if let Some(expr) = &q.calc {
        return calc_hit(expr).into_iter().collect();
    }
    let cap = if q.scope == Scope::All {
        CAP
    } else {
        CAP_SCOPED
    };
    let mut groups: Vec<Vec<Hit>> = Vec::new();
    if q.wants(Scope::Hierarchy) {
        groups.push(scene_hits(s, &q, cap));
    }
    if q.wants(Scope::Project) {
        groups.push(asset_hits(&p.index, &q, cap));
    }
    // Type / tag / layer filters only apply to objects and assets.
    if !q.has_filters() {
        if q.wants(Scope::Menu) {
            groups.push(menu_hits(&p.menu, &q, cap));
        }
        if q.wants(Scope::Settings) {
            groups.push(settings_hits(&q, cap));
        }
        if q.scope == Scope::All {
            groups.push(doc_hits(&p.docs, &q, cap));
        }
    }
    groups.retain(|g| !g.is_empty());
    // Stable: equal best scores keep the provider order above.
    groups.sort_by_key(|g| std::cmp::Reverse(g[0].score));
    groups.into_iter().flatten().collect()
}

fn recent_hit(text: &str) -> Hit {
    Hit {
        provider: Provider::Recent,
        icon: Provider::Recent.icon(),
        label: text.to_owned(),
        detail: String::new(),
        shortcut: "",
        score: 0,
        indices: Vec::new(),
        target: Target::Recent(text.to_owned()),
    }
}

fn calc_hit(expr: &str) -> Option<Hit> {
    let value = calc::format(calc::eval(expr)?);
    Some(Hit {
        provider: Provider::Calculator,
        icon: Provider::Calculator.icon(),
        label: format!("= {value}"),
        detail: expr.to_owned(),
        shortcut: "",
        score: 0,
        indices: Vec::new(),
        target: Target::Calc(value),
    })
}

fn scene_hits(s: &EditorState, q: &Query, cap: usize) -> Vec<Hit> {
    let tl = &s.project_settings.tags_and_layers;
    scene_search::search(&s.scene, tl, q, Mode::Fuzzy, cap)
        .into_iter()
        .map(|(e, m)| {
            let detail = s
                .scene
                .parent(e)
                .map(|p| s.scene.name(p))
                .unwrap_or_default();
            Hit {
                provider: Provider::Hierarchy,
                icon: crate::state::object_icon(&s.scene, e),
                label: s.scene.name(e),
                detail,
                shortcut: "",
                score: m.score,
                indices: m.indices,
                target: Target::Object(e),
            }
        })
        .collect()
}

fn asset_hits(index: &AssetIndex, q: &Query, cap: usize) -> Vec<Hit> {
    index
        .search(q, cap)
        .into_iter()
        .map(|(a, m)| {
            let folder = a.rel.rsplit_once('/').map_or("", |(d, _)| d).to_owned();
            let entry = crate::project::AssetEntry {
                path: PathBuf::from(&a.rel),
                name: a.name.clone(),
                is_dir: a.is_dir,
            };
            Hit {
                provider: Provider::Project,
                icon: entry.icon(),
                label: a.name.clone(),
                detail: folder,
                shortcut: "",
                score: m.score,
                indices: m.indices,
                target: Target::Asset {
                    rel: a.rel.clone(),
                    is_dir: a.is_dir,
                },
            }
        })
        .collect()
}

/// Match the title; failing that, the secondary text (menu path, page keywords) without
/// highlights and with a lower score.
fn title_or_extra(q: &Query, title: &str, extra: &str) -> Option<Match> {
    fuzzy::match_words(&q.words, title, Mode::Fuzzy).or_else(|| {
        let m = fuzzy::match_words(&q.words, extra, Mode::Fuzzy)?;
        Some(Match {
            score: m.score - 40,
            indices: Vec::new(),
        })
    })
}

fn best(mut hits: Vec<Hit>, cap: usize) -> Vec<Hit> {
    hits.sort_by(|a, b| {
        b.score
            .cmp(&a.score)
            .then(a.label.len().cmp(&b.label.len()))
    });
    hits.truncate(cap);
    hits
}

fn menu_hits(menu: &[MenuCommand], q: &Query, cap: usize) -> Vec<Hit> {
    let hits = menu
        .iter()
        .filter_map(|c| {
            let m = title_or_extra(q, &c.title, &c.path)?;
            Some(Hit {
                provider: Provider::Menu,
                icon: "▸",
                label: c.title.clone(),
                detail: c.path.clone(),
                shortcut: c.shortcut,
                score: m.score,
                indices: m.indices,
                target: Target::Menu(c.run.clone()),
            })
        })
        .collect();
    best(hits, cap)
}

fn settings_hits(q: &Query, cap: usize) -> Vec<Hit> {
    let hits = SettingsPage::all()
        .filter_map(|page| {
            let extra = format!("{} {}", page.window(), page.keywords());
            let m = title_or_extra(q, page.title(), &extra)?;
            Some(Hit {
                provider: Provider::Settings,
                icon: Provider::Settings.icon(),
                label: page.title().to_owned(),
                detail: page.window().to_owned(),
                shortcut: "",
                score: m.score,
                indices: m.indices,
                target: Target::Settings(page),
            })
        })
        .collect();
    best(hits, cap)
}

fn doc_hits(docs: &[DocHeading], q: &Query, cap: usize) -> Vec<Hit> {
    let hits = docs
        .iter()
        .filter_map(|d| {
            let m = fuzzy::match_words(&q.words, &d.title, Mode::Fuzzy)?;
            Some(Hit {
                provider: Provider::Help,
                icon: Provider::Help.icon(),
                label: d.title.clone(),
                detail: d.guide.clone(),
                shortcut: "",
                // Guides rank below commands and pages that match as well.
                score: m.score - 10,
                indices: m.indices,
                target: Target::Doc(d.file),
            })
        })
        .collect();
    best(hits, cap)
}

#[cfg(test)]
mod tests;
