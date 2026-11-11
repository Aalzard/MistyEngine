//! The editor's own names for its windows and menus, and the names they replaced.
//!
//! Round 31 renamed what the editor shows (Inspector → Properties, Hierarchy → Scene Tree,
//! Project → Asset Files, Scene → Editor Scene, Game → Scene Viewer, the GameObject menu →
//! Object, the Component menu and "Add Component" → Archetype, World Partition → World
//! Streaming, Virtual Geometry → Misty Mesh). Nothing stored changed: saved layouts keep the
//! tab variant names, scenes keep their component names, and the Lua and MCP names stay. This
//! module keeps the old *shown* names working where something outside the editor may still
//! use them: menu paths given to `editor.run_menu_item`, editor scripts adding `MenuItem`s
//! under the old top menus, and the screenshot variables (`MISTY_TAB`, `MISTY_OPEN_MENU`,
//! `MISTY_DRAW_MODE`).

/// Top menus that were renamed: (old, new).
pub const TOP_MENUS: [(&str, &str); 2] = [("GameObject", "Object"), ("Component", "Archetype")];

/// Words inside menu entries and draw modes that were renamed: (old, new).
const PHRASES: [(&str, &str); 2] = [
    ("Virtual Geometry", "Misty Mesh"),
    ("World Partition", "World Streaming"),
];

/// Names stored in files (component and category names) that the editor shows under another
/// English name: (stored, shown). Translations stay keyed by the stored name.
const STORED_NAMES: [(&str, &str); 2] = [
    ("World Partition", "World Streaming"),
    ("Hierarchy Folder", "Scene Tree Folder"),
];

/// The English name the editor shows for a stored name (itself for most names).
pub fn shown_name(stored: &str) -> &str {
    STORED_NAMES
        .iter()
        .find(|(s, _)| *s == stored)
        .map_or(stored, |(_, shown)| shown)
}

/// The current name of a top menu ("GameObject" → "Object"; others unchanged).
pub fn top_menu(name: &str) -> &str {
    TOP_MENUS
        .iter()
        .find(|(old, _)| old.eq_ignore_ascii_case(name))
        .map_or(name, |(_, new)| new)
}

/// `text` with the renamed phrases replaced (ASCII case-insensitive).
pub fn current_phrase(text: &str) -> String {
    let mut out = text.to_owned();
    for (old, new) in PHRASES {
        let mut from = 0;
        while let Some(at) = out[from..]
            .to_ascii_lowercase()
            .find(&old.to_ascii_lowercase())
        {
            let at = from + at;
            out.replace_range(at..at + old.len(), new);
            from = at + new.len();
        }
    }
    out
}

/// A menu path written with the old names, in today's names: "GameObject/3D Object/Cube" →
/// "Object/3D Object/Cube", "Window/General/Inspector" → "Window/General/Properties",
/// "Hierarchy/Level 1/Save Scene" → "Scene Tree/Level 1/Save Scene",
/// "Tools/Rendering/Convert Scene to Virtual Geometry..." → "… to Misty Mesh...".
/// Current paths come back unchanged.
pub fn menu_path(path: &str) -> String {
    let mut segs: Vec<String> = path.split('/').map(str::to_owned).collect();
    let Some(first) = segs.first().cloned() else {
        return path.to_owned();
    };
    if first.eq_ignore_ascii_case("Hierarchy") {
        segs[0] = "Scene Tree".into();
    } else {
        segs[0] = top_menu(&first).to_owned();
    }
    let in_window = segs[0].eq_ignore_ascii_case("Window");
    for seg in segs.iter_mut().skip(1) {
        if in_window
            && let Some(tab) = crate::layout::Tab::every().find(|t| {
                t.legacy_title()
                    .is_some_and(|l| l.eq_ignore_ascii_case(seg))
            })
        {
            *seg = tab.title().to_owned();
            continue;
        }
        *seg = current_phrase(seg);
    }
    segs.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn old_menu_paths_map_to_the_new_names() {
        assert_eq!(
            menu_path("GameObject/3D Object/Cube"),
            "Object/3D Object/Cube"
        );
        assert_eq!(
            menu_path("gameobject/3d object/cube").to_lowercase(),
            "object/3d object/cube"
        );
        assert_eq!(
            menu_path("Component/Physics/Rigidbody"),
            "Archetype/Physics/Rigidbody"
        );
        assert_eq!(
            menu_path("Window/General/Inspector"),
            "Window/General/Properties"
        );
        assert_eq!(
            menu_path("Window/General/Game"),
            "Window/General/Scene Viewer"
        );
        assert_eq!(
            menu_path("Window/General/Scene"),
            "Window/General/Editor Scene"
        );
        assert_eq!(
            menu_path("Window/General/Project"),
            "Window/General/Asset Files"
        );
        assert_eq!(
            menu_path("Window/World/World Partition"),
            "Window/World/World Streaming"
        );
        assert_eq!(
            menu_path("Hierarchy/Level 1/Save Scene"),
            "Scene Tree/Level 1/Save Scene"
        );
        assert_eq!(
            menu_path("Tools/Rendering/Convert Scene to Virtual Geometry..."),
            "Tools/Rendering/Convert Scene to Misty Mesh..."
        );
        assert_eq!(
            menu_path("GameObject/Enable Virtual Geometry"),
            "Object/Enable Misty Mesh"
        );
        // "Scene" outside the Window menu is an asset kind, not the tab.
        assert_eq!(menu_path("Assets/Create/Scene"), "Assets/Create/Scene");
        // Current paths are unchanged.
        for p in ["Object/Create Empty", "File/New Scene", "Edit/Undo History"] {
            assert_eq!(menu_path(p), p);
        }
    }

    #[test]
    fn stored_names_keep_their_files_and_show_the_new_name() {
        assert_eq!(shown_name("World Partition"), "World Streaming");
        assert_eq!(shown_name("Rigidbody"), "Rigidbody");
        assert_eq!(top_menu("GameObject"), "Object");
        assert_eq!(top_menu("Tools"), "Tools");
        assert_eq!(
            current_phrase("Virtual Geometry Clusters"),
            "Misty Mesh Clusters"
        );
    }

    /// Other engines' names and their trademarked feature names, which the editor never shows
    /// ("Lumen" only capitalised: "lumen" is also the unit of light).
    fn banned_word(text: &str) -> Option<&'static str> {
        const ANY_CASE: [&str; 4] = ["Unity", "Unreal", "Nanite", "Blueprint"];
        text.split(|c: char| !c.is_alphanumeric()).find_map(|word| {
            let lower = word.to_lowercase();
            ANY_CASE
                .into_iter()
                .find(|b| lower.starts_with(&b.to_lowercase()))
                .or_else(|| (word == "Lumen" || word == "Lumens").then_some("Lumen"))
        })
    }

    fn check(problems: &mut Vec<String>, place: &str, text: &str) {
        if let Some(word) = banned_word(text) {
            problems.push(format!("{place}: \"{word}\" in {text:?}"));
        }
    }

    #[test]
    fn banned_words_are_recognised() {
        assert_eq!(banned_word("a Unity-like Inspector"), Some("Unity"));
        assert_eq!(banned_word("Blueprints"), Some("Blueprint"));
        assert_eq!(banned_word("UnrealEngine"), Some("Unreal"));
        assert_eq!(banned_word("Lumen GI"), Some("Lumen"));
        assert_eq!(banned_word("1000 lumens"), None);
        assert_eq!(banned_word("community"), None);
        assert_eq!(banned_word("Misty GI and Misty Mesh"), None);
    }

    /// Nothing the editor shows names another engine or its features: tab titles, menu
    /// commands, tutorials, the translation files, every string literal of the editor's
    /// sources and the user guides.
    #[test]
    fn ui_text_and_user_docs_use_mistys_own_names() {
        let mut problems = Vec::new();
        for tab in crate::layout::Tab::every() {
            check(&mut problems, "tab", tab.title());
        }
        let root = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let sample = root.join("../../SampleProject");
        let s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(sample));
        for c in crate::search::menu_items::all(&s) {
            check(&mut problems, "menu", &format!("{} ({})", c.title, c.path));
        }
        for t in crate::tutorials::data::all() {
            let place = format!("tutorial {}", t.id);
            check(&mut problems, &place, &t.title);
            check(&mut problems, &place, &t.description);
            for step in &t.steps {
                check(&mut problems, &place, &step.title);
                check(&mut problems, &place, &step.text);
                for target in &step.targets {
                    check(&mut problems, &place, target);
                }
            }
        }
        for lang in [
            crate::i18n::Language::Turkish,
            crate::i18n::Language::Arabic,
        ] {
            let text = crate::i18n::catalog::source(lang).unwrap_or_default();
            for e in crate::i18n::catalog::parse(text).expect("translation file parses") {
                let place = format!("locales/{}.toml", lang.code());
                check(&mut problems, &place, &e.english);
                check(&mut problems, &place, &e.translation);
            }
        }
        // String literals of the editor's sources (comments may cite other engines).
        let literal = regex::Regex::new(r#""((?:[^"\\\n]|\\.)*)""#).unwrap();
        let mut files = Vec::new();
        let mut dirs = vec![root.join("src")];
        while let Some(dir) = dirs.pop() {
            for e in std::fs::read_dir(&dir).into_iter().flatten().flatten() {
                let p = e.path();
                if p.is_dir() {
                    dirs.push(p);
                } else if p.extension().is_some_and(|x| x == "rs") && !p.ends_with("renames.rs") {
                    files.push(p);
                }
            }
        }
        for f in files {
            let text = std::fs::read_to_string(&f).unwrap_or_default();
            for line in text.lines().filter(|l| !l.trim_start().starts_with("//")) {
                for c in literal.captures_iter(line) {
                    check(&mut problems, &f.display().to_string(), &c[1]);
                }
            }
        }
        // User guides (developer history such as progress.md and decisions.md is not one).
        let repo = root.join("../..");
        let mut guides = vec![
            repo.join("README.md"),
            repo.join("tools/release/README.txt"),
            repo.join("tools/release/README-linux.txt"),
        ];
        for e in std::fs::read_dir(repo.join("docs"))
            .into_iter()
            .flatten()
            .flatten()
        {
            let p = e.path();
            let name = p.file_name().and_then(|n| n.to_str()).unwrap_or("");
            let internal = ["progress.md", "decisions.md", "unity-notes.md"].contains(&name);
            if p.extension().is_some_and(|x| x == "md") && !internal {
                guides.push(p);
            }
        }
        guides.push(repo.join("docs/api/lua-api.md"));
        for g in guides {
            let text = std::fs::read_to_string(&g).unwrap_or_default();
            for line in text.lines() {
                check(&mut problems, &g.display().to_string(), line);
            }
        }
        assert!(problems.is_empty(), "{}", problems.join("\n"));
    }
}
