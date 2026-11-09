//! `git status --porcelain=v1 -z --branch` parsed into per-file badges.

use std::collections::HashMap;

/// What a badge says about a file (or, for a folder, the most urgent of what is inside it).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Badge {
    Untracked,
    Added,
    Deleted,
    Modified,
    Conflicted,
}

impl Badge {
    pub fn letter(self) -> &'static str {
        match self {
            Badge::Untracked => "?",
            Badge::Added => "A",
            Badge::Deleted => "D",
            Badge::Modified => "M",
            Badge::Conflicted => "!",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Badge::Untracked => "Untracked",
            Badge::Added => "Added",
            Badge::Deleted => "Deleted",
            Badge::Modified => "Modified",
            Badge::Conflicted => "Conflicted",
        }
    }

    pub fn color(self) -> egui::Color32 {
        match self {
            Badge::Untracked => egui::Color32::from_rgb(140, 140, 150),
            Badge::Added => egui::Color32::from_rgb(90, 180, 90),
            Badge::Deleted => egui::Color32::from_rgb(200, 90, 80),
            Badge::Modified => egui::Color32::from_rgb(220, 160, 60),
            Badge::Conflicted => egui::Color32::from_rgb(230, 70, 70),
        }
    }
}

/// One changed file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Entry {
    /// Project-relative path ("Assets/Scenes/Main.scene").
    pub path: String,
    /// Staged change (`X` of `XY`): ' ', M, A, D, R, C, U or '?'.
    pub index: char,
    /// Work tree change (`Y`).
    pub worktree: char,
    /// A rename's old project-relative path.
    pub from: Option<String>,
}

impl Entry {
    pub fn is_untracked(&self) -> bool {
        self.index == '?'
    }

    pub fn is_conflicted(&self) -> bool {
        matches!(
            (self.index, self.worktree),
            ('U', _) | (_, 'U') | ('A', 'A') | ('D', 'D')
        )
    }

    /// Something is in the index for the next commit.
    pub fn is_staged(&self) -> bool {
        !self.is_conflicted() && !matches!(self.index, ' ' | '?' | '!')
    }

    /// The work tree differs from the index.
    pub fn has_unstaged(&self) -> bool {
        self.is_untracked() || (!self.is_conflicted() && self.worktree != ' ')
    }

    pub fn badge(&self) -> Badge {
        if self.is_conflicted() {
            return Badge::Conflicted;
        }
        if self.is_untracked() {
            return Badge::Untracked;
        }
        match (self.index, self.worktree) {
            (_, 'D') | ('D', _) => Badge::Deleted,
            ('A', _) | ('R', _) | ('C', _) => Badge::Added,
            _ => Badge::Modified,
        }
    }
}

/// The branch and every changed file under the project.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Status {
    pub branch: String,
    /// The upstream branch (`origin/main`), when there is one.
    pub upstream: Option<String>,
    pub ahead: u32,
    pub behind: u32,
    pub entries: Vec<Entry>,
    /// Badge per file and per folder holding changes (project-relative, no trailing `/`).
    badges: HashMap<String, Badge>,
}

impl Status {
    pub fn entry(&self, rel: &str) -> Option<&Entry> {
        self.entries.iter().find(|e| e.path == rel)
    }

    /// The badge of a file or folder (folders show the most urgent change inside them).
    pub fn badge(&self, rel: &str) -> Option<Badge> {
        self.badges.get(rel.trim_end_matches('/')).copied()
    }

    pub fn conflicts(&self) -> impl Iterator<Item = &Entry> {
        self.entries.iter().filter(|e| e.is_conflicted())
    }

    fn index_badges(&mut self) {
        self.badges.clear();
        for e in &self.entries {
            let b = e.badge();
            self.badges.insert(e.path.clone(), b);
            let mut dir = e.path.as_str();
            while let Some((parent, _)) = dir.rsplit_once('/') {
                let slot = self.badges.entry(parent.to_owned()).or_insert(b);
                *slot = (*slot).max(b);
                dir = parent;
            }
        }
    }
}

/// Parse porcelain v1 `-z` output; paths outside `prefix` (the project folder) are dropped and the
/// rest made project-relative.
pub fn parse(out: &str, prefix: &str) -> Status {
    let mut s = Status::default();
    let mut fields = out.split('\0').filter(|f| !f.is_empty());
    while let Some(f) = fields.next() {
        if let Some(head) = f.strip_prefix("## ") {
            parse_branch(head, &mut s);
            continue;
        }
        let mut chars = f.chars();
        let (Some(x), Some(y)) = (chars.next(), chars.next()) else {
            continue;
        };
        let path = f.get(3..).unwrap_or("");
        let from = matches!(x, 'R' | 'C').then(|| fields.next()).flatten();
        let Some(rel) = path.strip_prefix(prefix) else {
            continue;
        };
        s.entries.push(Entry {
            path: rel.to_owned(),
            index: x,
            worktree: y,
            from: from.and_then(|p| p.strip_prefix(prefix)).map(str::to_owned),
        });
    }
    s.entries.sort_by(|a, b| a.path.cmp(&b.path));
    s.index_badges();
    s
}

/// `main...origin/main [ahead 1, behind 2]`, `No commits yet on main`, `HEAD (no branch)`.
fn parse_branch(head: &str, s: &mut Status) {
    let head = head
        .strip_prefix("No commits yet on ")
        .or_else(|| head.strip_prefix("Initial commit on "))
        .unwrap_or(head);
    let (names, counts) = match head.split_once(" [") {
        Some((n, c)) => (n, c.trim_end_matches(']')),
        None => (head, ""),
    };
    match names.split_once("...") {
        Some((b, up)) => {
            s.branch = b.to_owned();
            s.upstream = Some(up.to_owned());
        }
        None => s.branch = names.to_owned(),
    }
    for part in counts.split(", ") {
        if let Some(n) = part.strip_prefix("ahead ") {
            s.ahead = n.parse().unwrap_or(0);
        } else if let Some(n) = part.strip_prefix("behind ") {
            s.behind = n.parse().unwrap_or(0);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_branch_files_renames_and_folders() {
        let out = "## main...origin/main [ahead 2, behind 1]\0 M Proj/Assets/a.lua\0\
                   A  Proj/Assets/New.mat\0?? Proj/Assets/Scenes/x.scene\0\
                   UU Proj/Assets/Scenes/Main.scene\0R  Proj/Assets/b.lua\0Proj/Assets/old.lua\0\
                   ?? Other/outside.txt\0";
        let s = parse(out, "Proj/");
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream.as_deref(), Some("origin/main"));
        assert_eq!((s.ahead, s.behind), (2, 1));
        assert_eq!(s.entries.len(), 5, "outside the project is dropped");
        assert_eq!(s.badge("Assets/a.lua"), Some(Badge::Modified));
        assert_eq!(s.badge("Assets/New.mat"), Some(Badge::Added));
        assert_eq!(s.badge("Assets/Scenes/x.scene"), Some(Badge::Untracked));
        assert_eq!(s.badge("Assets/Scenes/Main.scene"), Some(Badge::Conflicted));
        let renamed = s.entry("Assets/b.lua").unwrap();
        assert_eq!(renamed.from.as_deref(), Some("Assets/old.lua"));
        assert_eq!(renamed.badge(), Badge::Added);
        // Folders take the most urgent badge inside them.
        assert_eq!(s.badge("Assets/Scenes"), Some(Badge::Conflicted));
        assert_eq!(s.badge("Assets/"), Some(Badge::Conflicted));
        assert_eq!(s.badge("Assets/Textures"), None);
        assert!(s.entry("Assets/New.mat").unwrap().is_staged());
        assert!(!s.entry("Assets/a.lua").unwrap().is_staged());
        assert!(s.entry("Assets/a.lua").unwrap().has_unstaged());
        assert_eq!(s.conflicts().count(), 1);
    }

    #[test]
    fn parses_a_repo_without_commits() {
        let s = parse("## No commits yet on main\0?? a.txt\0", "");
        assert_eq!(s.branch, "main");
        assert_eq!(s.upstream, None);
        assert_eq!(s.badge("a.txt"), Some(Badge::Untracked));
    }
}
