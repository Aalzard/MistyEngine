//! User Guide tests: both languages have the same chapters, links and pictures resolve, the
//! guides name only nodes that exist and every node of their example graphs, and the reader
//! draws every chapter (English and Arabic) without trouble.

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use egui_kittest::Harness;

use super::content::{self, CHAPTERS};
use super::markdown::{self, Block, Span};
use super::view::{self, GuideView};
use crate::i18n::{self, Language};

fn docs() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../docs/guide")
}

fn md_files(dir: &Path) -> BTreeSet<String> {
    std::fs::read_dir(dir)
        .unwrap()
        .filter_map(|e| {
            let name = e.unwrap().file_name().to_string_lossy().into_owned();
            name.strip_suffix(".md").map(str::to_owned)
        })
        .collect()
}

#[test]
fn every_chapter_exists_in_both_languages() {
    let en = md_files(&docs().join("en"));
    let ar = md_files(&docs().join("ar"));
    assert_eq!(en, ar, "English and Arabic have the same chapter files");
    let built_in: BTreeSet<String> = CHAPTERS.iter().map(|c| c.id.to_owned()).collect();
    assert_eq!(en, built_in, "every chapter file is built into the editor");
    for c in CHAPTERS {
        // The built-in text is the file's text (rebuilt when a file changes).
        let file = std::fs::read_to_string(docs().join("en").join(format!("{}.md", c.id))).unwrap();
        assert_eq!(file.replace("\r\n", "\n"), c.en.replace("\r\n", "\n"));
        assert!(
            i18n::shape::needs_shaping(c.ar),
            "{}: the Arabic chapter is in Arabic",
            c.id
        );
    }
}

/// Counts of each kind of block: the translation keeps the chapter's structure.
fn shape_of(blocks: &[Block]) -> [usize; 6] {
    let mut n = [0; 6];
    for b in blocks {
        let i = match b {
            Block::Heading { .. } => 0,
            Block::Code { .. } => 1,
            Block::Image { .. } => 2,
            Block::Table { .. } => 3,
            Block::Note { .. } => 4,
            Block::List { .. } => 5,
            Block::Paragraph(_) => continue,
        };
        n[i] += 1;
    }
    n
}

#[test]
fn translations_keep_the_structure_and_the_code() {
    for c in CHAPTERS {
        let en = markdown::parse(c.en);
        let ar = markdown::parse(c.ar);
        assert_eq!(
            shape_of(&en),
            shape_of(&ar),
            "{}: headings, code, images, tables, notes, lists",
            c.id
        );
        let code = |blocks: &[Block]| -> Vec<String> {
            fn walk(b: &[Block], out: &mut Vec<String>) {
                for x in b {
                    match x {
                        Block::Code { text, .. } => out.push(text.clone()),
                        Block::Note { blocks, .. } => walk(blocks, out),
                        _ => {}
                    }
                }
            }
            let mut out = Vec::new();
            walk(blocks, &mut out);
            out
        };
        // Node diagrams and Lua stay the same (comments aside).
        let strip = |v: Vec<String>| -> Vec<String> {
            v.into_iter()
                .map(|t| {
                    t.lines()
                        .filter(|l| !l.trim_start().starts_with("--"))
                        .collect::<Vec<_>>()
                        .join("\n")
                })
                .collect()
        };
        assert_eq!(strip(code(&en)), strip(code(&ar)), "{}: code blocks", c.id);
    }
}

#[test]
fn links_and_images_resolve() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(docs().join("images"))
        .unwrap()
        .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
        .collect();
    let built_in: BTreeSet<String> = content::IMAGES
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect();
    assert_eq!(
        on_disk, built_in,
        "every picture is built in, and only those"
    );
    let mut used = BTreeSet::new();
    for c in CHAPTERS {
        for lang in [Language::English, Language::Arabic] {
            let blocks = markdown::parse(c.text(lang));
            let (links, images) = markdown::references(&blocks);
            for img in images {
                assert!(img.starts_with("../images/"), "{}: {img}", c.id);
                assert!(content::image(&img).is_some(), "{} ({lang:?}): {img}", c.id);
                used.insert(img.trim_start_matches("../images/").to_owned());
            }
            for link in links {
                if link.starts_with("http") {
                    continue;
                }
                let (file, anchor) = link.split_once('#').unwrap_or((&link, ""));
                let target = if file.is_empty() {
                    c
                } else {
                    content::chapter(file.trim_end_matches(".md"))
                        .unwrap_or_else(|| panic!("{} ({lang:?}): no chapter {link}", c.id))
                };
                if !anchor.is_empty() {
                    let found = markdown::parse(target.text(lang)).iter().any(|b| {
                        matches!(b, Block::Heading { text, .. } if markdown::anchor(text) == anchor)
                    });
                    assert!(found, "{} ({lang:?}): no heading for {link}", c.id);
                }
            }
        }
    }
    assert_eq!(used, built_in, "every built-in picture is used");
}

#[test]
fn markdown_parses_the_guide_subset() {
    let blocks = markdown::parse(
        "# Title\n\nSome **bold** and `code` with a [link](02-performance.md#the-profiler).\n\
         \n- one\n  continued\n- two\n\n1. first\n2. second\n\n```lua\nprint(1)\n```\n\n\
         ![Alt](../images/stats.jpg)\n\n| A | B |\n|---|---|\n| 1 | 2 |\n\n\
         > [!PROGRAMMERS]\n> Deep stuff.\n",
    );
    assert_eq!(blocks.len(), 8, "{blocks:#?}");
    assert!(matches!(&blocks[0], Block::Heading { level: 1, text } if text == "Title"));
    let Block::Paragraph(p) = &blocks[1] else {
        panic!()
    };
    let spans = markdown::inline(p);
    assert!(spans.contains(&Span::Bold("bold".into())));
    assert!(spans.contains(&Span::Code("code".into())));
    assert!(spans.contains(&Span::Link {
        text: "link".into(),
        target: "02-performance.md#the-profiler".into()
    }));
    assert!(
        matches!(&blocks[2], Block::List { ordered: false, items } if items[0].1 == "one continued")
    );
    assert!(matches!(&blocks[3], Block::List { ordered: true, items } if items.len() == 2));
    assert!(matches!(&blocks[4], Block::Code { lang, .. } if lang == "lua"));
    assert!(matches!(&blocks[5], Block::Image { path, .. } if path == "../images/stats.jpg"));
    assert!(matches!(&blocks[6], Block::Table { rows, .. } if rows.len() == 1));
    assert!(matches!(
        &blocks[7],
        Block::Note {
            kind: markdown::NoteKind::Programmers,
            ..
        }
    ));
    assert_eq!(markdown::anchor("The Profiler"), "the-profiler");
    assert_eq!(markdown::anchor("Step 3: look left"), "step-3-look-left");
}

/// Bold `Owner: Member` names in a chapter (the engine nodes it mentions).
fn engine_nodes_named(text: &str) -> BTreeSet<String> {
    let mut out = BTreeSet::new();
    let mut scan = |t: &str| {
        for s in markdown::inline(t) {
            if let Span::Bold(b) = s
                && let Some((owner, _)) = b.split_once(": ")
                && !b.contains('/')
                && owner.chars().all(char::is_alphanumeric)
                && owner.chars().next().is_some_and(char::is_uppercase)
            {
                out.insert(b);
            }
        }
    };
    fn walk(blocks: &[Block], scan: &mut dyn FnMut(&str)) {
        for b in blocks {
            match b {
                Block::Heading { text, .. } | Block::Paragraph(text) => scan(text),
                Block::List { items, .. } => items.iter().for_each(|(_, t)| scan(t)),
                Block::Table { header, rows } => header
                    .iter()
                    .chain(rows.iter().flatten())
                    .for_each(|c| scan(c)),
                Block::Note { blocks, .. } => walk(blocks, scan),
                _ => {}
            }
        }
    }
    walk(&markdown::parse(text), &mut scan);
    out
}

#[test]
fn guides_name_only_nodes_that_exist() {
    let known: BTreeSet<String> = crate::panels::script_graph::search::entries(
        &misty_engine::visual_script::ScriptGraph::default(),
        &[],
    )
    .into_iter()
    .map(|e| e.title)
    .collect();
    // The Script Graph chapters (04 to 08); the others name UI labels such as "CPU: main".
    for c in CHAPTERS
        .iter()
        .filter(|c| ("04".."09").contains(&&c.id[..2]))
    {
        for name in engine_nodes_named(c.en) {
            assert!(known.contains(&name), "{}: no node called {name}", c.id);
        }
    }
}

/// Each guide's text names every node and variable of its example graph.
#[test]
fn guides_describe_their_example_graphs() {
    use misty_engine::visual_script::{ScriptGraph, nodes};
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../SampleProject");
    for c in CHAPTERS {
        let Some(ex) = content::example(c.id) else {
            continue;
        };
        let g = ScriptGraph::load(&root.join(ex.graph)).unwrap();
        for v in &g.variables {
            assert!(
                c.en.contains(&format!("| {} |", v.name))
                    || c.en.contains(&format!("**{}**", v.name)),
                "{}: variable {} is not described",
                c.id,
                v.name
            );
        }
        for n in &g.nodes {
            let title = nodes::title(n, &g);
            assert!(
                c.en.contains(&title),
                "{}: node {title} is not described",
                c.id
            );
        }
        // The built-in copies are the sample project's files.
        for (rel, bytes) in ex.files {
            let on_disk = std::fs::read(root.join(rel)).unwrap();
            assert_eq!(&on_disk[..], *bytes, "{rel}");
        }
    }
}

#[test]
fn search_finds_text_in_the_reading_language() {
    let mut v = GuideView::default();
    let hits = view::search(&mut v, "frame debugger", Language::English);
    assert!(
        hits.iter()
            .any(|(c, _, _)| CHAPTERS[*c].id == "02-performance")
    );
    let hits = view::search(&mut v, "مصحح الإطارات", Language::Arabic);
    assert!(
        hits.iter()
            .any(|(c, _, _)| CHAPTERS[*c].id == "02-performance")
    );
    assert!(view::search(&mut v, "  ", Language::English).is_empty());
    assert!(v.open("06-guide-door"));
    assert_eq!(CHAPTERS[v.chapter].id, "06-guide-door");
    assert!(!v.open("nope"));
}

/// The reader draws every chapter, in English, Turkish (English text) and Arabic.
#[test]
fn the_reader_draws_every_chapter_in_every_language() {
    for lang in Language::ALL {
        let mut h = Harness::builder()
            .with_size([1100.0, 800.0])
            .build_ui_state(
                move |ui, v: &mut GuideView| {
                    i18n::set_language(lang);
                    let _ = view::show(ui, v, true);
                },
                GuideView::default(),
            );
        for i in 0..CHAPTERS.len() {
            h.state_mut().chapter = i;
            h.run_steps(2);
        }
        // Searching draws the results list.
        h.state_mut().search = "Profiler".into();
        h.run_steps(2);
    }
    i18n::set_language(Language::English);
}

/// "Open example" in a project without the guide files copies them in and opens the graph and
/// the scene.
#[test]
fn open_example_copies_the_files_and_opens_them() {
    let root = std::env::temp_dir().join(format!("misty_guide_example_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("Assets")).unwrap();
    let mut s = crate::state::EditorState::new(crate::project::ProjectBrowser::open(root.clone()));
    super::open_example(&mut s, "08-guide-animation");
    let ex = content::example("08-guide-animation").unwrap();
    for (rel, _) in ex.files {
        assert!(root.join(rel).is_file(), "{rel} copied");
    }
    assert_eq!(s.script_graph.path.as_deref(), Some(ex.graph));
    assert!(
        s.actions
            .iter()
            .any(|a| matches!(a, crate::actions::Action::OpenScenePath(p) if p == ex.scene))
    );
    let _ = std::fs::remove_dir_all(&root);
}
