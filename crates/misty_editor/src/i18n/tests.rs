//! Translation checks: the files parse, placeholders match their English, the language
//! switches, and a coverage report (untranslated keys per language; a report, not a failure).

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

use regex::Regex;

use super::catalog::{self, Entry};
use super::*;

fn src_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("src")
}

fn rust_files(dir: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            rust_files(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The text of a Rust string literal body (escapes and `\` line continuations resolved).
fn unescape(body: &str) -> String {
    let mut out = String::new();
    let mut chars = body.chars().peekable();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        match chars.next() {
            Some('n') => out.push('\n'),
            Some('t') => out.push('\t'),
            Some('"') => out.push('"'),
            Some('\'') => out.push('\''),
            Some('\\') => out.push('\\'),
            Some('0') => out.push('\0'),
            Some('u') => {
                let mut hex = String::new();
                for d in chars.by_ref() {
                    match d {
                        '{' => {}
                        '}' => break,
                        _ => hex.push(d),
                    }
                }
                if let Some(ch) = u32::from_str_radix(&hex, 16).ok().and_then(char::from_u32) {
                    out.push(ch);
                }
            }
            Some('\n') | Some('\r') => {
                while chars.peek().is_some_and(|c| c.is_whitespace()) {
                    chars.next();
                }
            }
            Some(other) => out.push(other),
            None => {}
        }
    }
    out
}

/// A string literal (group 1) after `prefix`.
fn literal_after(prefix: &str) -> Regex {
    Regex::new(&format!(r#"{prefix}\s*"((?:[^"\\]|\\[\s\S])*)""#)).expect("valid pattern")
}

/// English texts the sources translate: every `tr!("…")` literal, the labels given to the
/// menu helpers (which translate them while keeping the English as tutorial target ids) and
/// the names translated where they are shown (tab titles, component names, …).
fn source_keys() -> BTreeSet<String> {
    let patterns = [
        literal_after(r"\btr!\("),
        literal_after(r"tr_wrapped\(\s*ui,"),
        literal_after(r"\b(?:item|todo)\(\s*ui,\s*s,"),
        literal_after(r"\b(?:top|sub)\(\s*ui,"),
        literal_after(r"targets::label\(\s*ui,"),
        literal_after(r"\b(?:grid_)?sub_header\(\s*ui,"),
        literal_after(r"\bvec3_(?:row|row_mixed|field)\(\s*ui,"),
        literal_after(r"\bnumber\(\s*ui,"),
        literal_after(r"\bentry_button\("),
    ];
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let mut keys = BTreeSet::new();
    for f in files {
        if f.ends_with("i18n/tests.rs") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        for re in &patterns {
            for c in re.captures_iter(&text) {
                let k = unescape(&c[1]);
                if !k.trim().is_empty() {
                    keys.insert(k);
                }
            }
        }
    }
    keys.extend(dynamic_keys());
    keys
}

/// Names translated where they are drawn (from enums and registries rather than literals).
fn dynamic_keys() -> Vec<String> {
    use misty_engine::scene::{
        AudioRolloff, ClearFlags, LightMode, LightType, PrimitiveMesh, Shadows,
    };
    let mut v: Vec<String> = Vec::new();
    let mut add = |names: &mut dyn Iterator<Item = &str>| v.extend(names.map(str::to_owned));
    add(&mut PrimitiveMesh::ALL.iter().map(|p| p.name()));
    add(&mut LightType::ALL.iter().map(|l| l.label()));
    add(&mut LightType::ALL
        .iter()
        .map(|l| crate::actions::Create::Light(*l).default_name()));
    add(&mut ClearFlags::ALL.iter().map(|c| c.label()));
    add(&mut LightMode::ALL.iter().map(|m| m.label()));
    add(&mut Shadows::ALL.iter().map(|s| s.label()));
    add(&mut AudioRolloff::ALL.iter().map(|r| r.label()));
    add(&mut crate::ui_create::UiElement::ALL.iter().map(|e| e.name()));
    {
        use crate::actions::AssetKind as K;
        let shader_kinds = [
            K::SurfaceShader,
            K::UnlitShader,
            K::MaterialFromShader,
            K::LitShaderGraph,
            K::UnlitShaderGraph,
            K::DecalShaderGraph,
            K::ShaderSubGraph,
            K::EffectGraph,
            K::EffectSubgraph,
            K::Test(misty_engine::testing::TestMode::EditMode),
            K::Test(misty_engine::testing::TestMode::PlayMode),
        ];
        add(&mut K::ALL.iter().chain(shader_kinds.iter()).map(|k| k.label()));
    }
    add(&mut crate::state::Tool::ALL.iter().map(|t| t.tooltip()));
    add(&mut crate::panels::scene::DRAW_MODES.iter().copied());
    add(&mut crate::menu_bar::COMPONENT_CATEGORIES.iter().copied());
    add(&mut ["Physics 2D", "Video"].into_iter());
    add(&mut crate::panels::project::TYPE_FILTERS.iter().copied());
    add(&mut ["Lock View to Selected", "✔ Lock View to Selected"].into_iter());
    // Light Inspector rows shown through `targets::label` (fog_editors::light_rows).
    add(&mut ["Volumetric Multiplier", "Volumetric Shadow Dimmer"].into_iter());
    add(&mut misty_engine::light2d::Light2DType::ALL
        .iter()
        .map(|k| k.label()));
    for t in crate::tutorials::list() {
        v.push(t.title.to_owned());
        v.push(t.description.to_owned());
    }
    for t in crate::layout::Tab::ALL {
        v.push(t.title().into());
    }
    for t in crate::layout::Tab::ON_DEMAND {
        v.push(t.title().into());
    }
    for b in crate::layout::BuiltinLayout::ALL {
        v.push(b.name().into());
    }
    for c in crate::panels::inspector::translated_components() {
        v.push((*c).into());
    }
    for t in crate::templates::Template::ALL {
        v.push(t.name().into());
        v.push(t.description().into());
    }
    v
}

/// UI calls with a plain English literal, editor-wide (a rough count of what is left).
fn untranslated_call_sites() -> (usize, usize) {
    let call = Regex::new(
        r#"\.(?:label|button|heading|strong|weak|small_button|on_hover_text|selectable_label|selectable_value\([^,]+,[^,]+,|checkbox\([^,]+,|menu_button)\(\s*"[A-Za-z]"#,
    )
    .unwrap();
    let wrapped = Regex::new(r#"tr!\(\s*"[A-Za-z]"#).unwrap();
    let mut files = Vec::new();
    rust_files(&src_dir(), &mut files);
    let (mut plain, mut done) = (0, 0);
    for f in files {
        let name = f.to_string_lossy().replace('\\', "/");
        if name.contains("/tests") || name.ends_with("tests.rs") || name.contains("/sample_") {
            continue;
        }
        let text = std::fs::read_to_string(&f).unwrap_or_default();
        plain += call.find_iter(&text).count();
        done += wrapped.find_iter(&text).count();
    }
    (plain, done)
}

fn entries(lang: Language) -> Vec<Entry> {
    catalog::parse(catalog::source(lang).expect("translated language")).unwrap_or_else(|e| {
        panic!("locales/{}.toml: {e}", lang.code());
    })
}

/// Each file parses, and an English text has one translation per file.
#[test]
fn translation_files_parse_without_conflicting_entries() {
    for lang in [Language::Turkish, Language::Arabic] {
        let mut seen: BTreeMap<String, (String, String)> = BTreeMap::new();
        for e in entries(lang) {
            if let Some((table, before)) = seen.get(&e.english) {
                assert_eq!(
                    before,
                    &e.translation,
                    "locales/{}.toml: \"{}\" is translated differently in [{table}] and [{}]",
                    lang.code(),
                    e.english,
                    e.table
                );
            }
            seen.insert(e.english.clone(), (e.table.clone(), e.translation.clone()));
        }
        assert!(!seen.is_empty(), "{lang:?} has translations");
    }
}

/// A translation must use exactly the `{placeholders}` of its English text.
#[test]
fn placeholders_match_their_english() {
    let mut problems = Vec::new();
    for lang in [Language::Turkish, Language::Arabic] {
        for e in entries(lang) {
            if e.translation.is_empty() {
                continue;
            }
            let (want, got) = (placeholders(&e.english), placeholders(&e.translation));
            if want != got {
                problems.push(format!(
                    "{}: \"{}\" has {want:?}, the translation {got:?}",
                    lang.code(),
                    e.english
                ));
            }
            let balanced = |s: &str| s.matches('{').count() == s.matches('}').count();
            if balanced(&e.english) && !balanced(&e.translation) {
                problems.push(format!(
                    "{}: unbalanced braces in \"{}\"",
                    lang.code(),
                    e.translation
                ));
            }
        }
    }
    assert!(
        problems.is_empty(),
        "placeholder mismatches:\n{}",
        problems.join("\n")
    );
}

/// Coverage of each language: the editor's translatable texts (see [`source_keys`]) with and
/// without a translation, and entries no source uses any more. A report: never fails.
#[test]
fn untranslated_keys_report() {
    let keys = source_keys();
    let mut report = String::new();
    for lang in [Language::Turkish, Language::Arabic] {
        let table = catalog::table(lang);
        let missing: Vec<&String> = keys.iter().filter(|k| !table.contains_key(*k)).collect();
        let stale: Vec<&String> = table.keys().filter(|k| !keys.contains(*k)).collect();
        let done = keys.len() - missing.len();
        report.push_str(&format!(
            "{} ({}): {done}/{} translated ({:.1} %), {} untranslated, {} unused entries\n",
            lang.native_name(),
            lang.code(),
            keys.len(),
            100.0 * done as f64 / keys.len().max(1) as f64,
            missing.len(),
            stale.len(),
        ));
        for k in missing {
            report.push_str(&format!("  untranslated: {k:?}\n"));
        }
        for k in stale {
            report.push_str(&format!("  unused: {k:?}\n"));
        }
    }
    // Component names: the common ones are in the files; all registered ones for reference.
    let all: Vec<&str> = misty_engine::scene::registry::components()
        .iter()
        .map(|c| c.name)
        .collect();
    for lang in [Language::Turkish, Language::Arabic] {
        let done = all
            .iter()
            .filter(|n| catalog::get(lang, n).is_some())
            .count();
        report.push_str(&format!(
            "{}: {done}/{} registered component names translated\n",
            lang.code(),
            all.len()
        ));
    }
    let (plain, wrapped) = untranslated_call_sites();
    report.push_str(&format!(
        "editor-wide: {wrapped} tr!() call sites, about {plain} UI calls with English literals \
         not yet wrapped ({:.0} % of call sites wrapped)\n",
        100.0 * wrapped as f64 / (wrapped + plain).max(1) as f64
    ));
    println!("{report}");
}

#[test]
fn english_is_the_source_and_the_fallback() {
    set_language(Language::English);
    assert_eq!(tr("File"), "File");
    assert_eq!(tr("No such text anywhere"), "No such text anywhere");
    assert_eq!(
        tr("Assets/Scenes/Level 1.scene"),
        "Assets/Scenes/Level 1.scene"
    );
    assert_eq!(
        tr!("Save changes to {scene}?", scene = "Level 1"),
        "Save changes to Level 1?"
    );
    set_language(Language::Turkish);
    assert_eq!(tr("No such text anywhere"), "No such text anywhere");
    set_language(Language::English);
}

#[test]
fn switching_language_translates_on_the_next_call() {
    set_language(Language::Turkish);
    assert_eq!(tr("File"), "Dosya");
    assert_eq!(tr("Inspector"), "Denetçi");
    assert!(!is_rtl());
    set_language(Language::Arabic);
    assert!(is_rtl());
    // Shaped for egui: "ملف" (file) joined and in visual order.
    assert_eq!(tr("File"), shape::visual("ملف", shape::Base::Rtl));
    assert_eq!(tr("File"), "\u{FEE3}\u{FEE0}\u{FED2}");
    // Translating twice (a label passed through `tr` again) changes nothing.
    assert_eq!(tr(&tr("File")), tr("File"));
    assert_eq!(tr(&tr("Inspector")), tr("Inspector"));
    set_language(Language::English);
    assert_eq!(tr("File"), "File");
}

#[test]
fn placeholders_are_filled_after_translating() {
    set_language(Language::Turkish);
    let t = tr!("Save changes to {scene}?", scene = "Level 1");
    assert!(t.contains("Level 1") && !t.contains("{scene}"), "{t}");
    set_language(Language::Arabic);
    // The Latin scene name stays readable inside the Arabic sentence.
    let t = tr!("Save changes to {scene}?", scene = "Level 1");
    assert!(t.contains("Level 1"), "{t}");
    set_language(Language::English);
    assert_eq!(
        fill("{a} and {b} {c}", &[("a", &1), ("b", &"x")]),
        "1 and x {c}"
    );
    assert_eq!(placeholders("{b} {a}"), ["a", "b"]);
}

#[test]
fn language_codes_and_names() {
    assert_eq!(Language::from_code("tr-TR"), Some(Language::Turkish));
    assert_eq!(Language::from_code("Türkçe"), Some(Language::Turkish));
    assert_eq!(Language::from_code("arabic"), Some(Language::Arabic));
    assert_eq!(Language::from_code("ar"), Some(Language::Arabic));
    assert_eq!(Language::from_code("EN"), Some(Language::English));
    assert_eq!(Language::from_code("fr"), None);
    for l in Language::ALL {
        assert_eq!(Language::from_code(l.code()), Some(l));
    }
}

#[test]
fn user_content_is_shaped_only_when_it_needs_it() {
    assert!(matches!(display("Main Camera"), Cow::Borrowed(_)));
    // An Arabic object name: joined, whatever the UI language.
    assert_eq!(
        display("\u{645}\u{631}\u{62d}\u{628}\u{627}"),
        "\u{FEE3}\u{FEAE}\u{FEA3}\u{FE92}\u{FE8E}"
    );
}

#[test]
fn literal_unescaping_follows_rust() {
    assert_eq!(unescape(r#"a \"b\" \u{e7}"#), "a \"b\" ç");
    assert_eq!(unescape("one \\\n        two"), "one two");
}

#[test]
fn context_suffix_is_never_shown() {
    assert_eq!(source_text("Light#theme"), "Light");
    assert_eq!(source_text("C#"), "C#");
    assert_eq!(source_text("Item #2"), "Item #2");
    set_language(Language::English);
    assert_eq!(tr("Light#theme"), "Light");
    set_language(Language::Turkish);
    assert_eq!(tr("Light#theme"), "Açık");
    assert_eq!(tr("Light"), "Işık");
    set_language(Language::English);
}
