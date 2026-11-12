//! Localization tests: CSV, lookups with fallback, smart strings and Localized Text.

use super::format::{Args, format, plural_index};
use super::*;
use crate::scene::{ScriptRef, Scripts};

fn table() -> StringTable {
    let mut t = StringTable::new("UI", &["en", "fr", "ar"]);
    t.set("hello", "en", "Hello, {name}!");
    t.set("hello", "fr", "Bonjour, {name} !");
    t.set("hello", "ar", "مرحبا، {name}!");
    t.set("only.en", "en", "English only");
    t.set("apples", "en", "{0:plural:one apple|{} apples} for {1}");
    t.set("apples", "fr", "{0:plural:une pomme|{} pommes} pour {1}");
    t.set(
        "apples",
        "ar",
        "{0:plural:لا تفاح|تفاحة|تفاحتان|{} تفاحات|{} تفاحة|{} تفاحة}",
    );
    t
}

fn localizer() -> Localizer {
    Localizer::new(
        LocalizationSettings {
            locales: vec![
                Locale::new("en", "English"),
                Locale::new("fr", "Français"),
                Locale::new("ar", "العربية"),
            ],
            startup: StartupLocale::Default,
            ..Default::default()
        },
        vec![table()],
    )
}

#[test]
fn csv_round_trip_keeps_quotes_commas_and_line_breaks() {
    let mut t = table();
    t.set("tricky", "en", "Say \"hi\", then\nleave");
    t.set("tricky", "fr", "  padded ");
    t.entry_mut("tricky").unwrap().comment = "comma, \"quote\"".into();
    let csv = t.to_csv();
    assert!(
        csv.starts_with('\u{feff}'),
        "byte-order mark for spreadsheets"
    );
    assert!(csv.contains("\"Say \"\"hi\"\", then\nleave\""), "{csv}");
    let back = StringTable::from_csv("UI", &csv).unwrap();
    assert_eq!(back.locales, t.locales);
    assert_eq!(back.entries, t.entries);
    assert_eq!(back.to_csv(), csv, "stable");

    // Merging: new keys and columns are added, cells replaced, others kept.
    let mut m = table();
    let (added, changed) = m
        .merge_csv("Key,fr,es\r\nhello,Salut {name},Hola {name}\r\nnew.key,,Nuevo\r\n")
        .unwrap();
    assert_eq!(added, 1);
    assert_eq!(changed, 3);
    assert_eq!(m.get("hello", "fr"), Some("Salut {name}"));
    assert_eq!(m.get("hello", "en"), Some("Hello, {name}!"), "kept");
    assert_eq!(m.get("new.key", "es"), Some("Nuevo"));
    assert!(m.locales.contains(&"es".to_owned()));
    assert!(table::parse_csv("Key,en\n\"open").is_err());
    assert!(m.merge_csv("en,fr\nx,y").is_err(), "needs a Key column");
    // CRLF inside quotes, empty trailing cells.
    let rows = table::parse_csv("a,\"b\r\nc\",\r\nd,,\n").unwrap();
    assert_eq!(rows, vec![vec!["a", "b\nc", ""], vec!["d", "", ""]]);
}

#[test]
fn missing_texts_fall_back_and_unknown_keys_show_the_key() {
    let loc = localizer();
    let args = Args::from_entries(&["name=Ana".to_owned()]);
    assert_eq!(loc.get_in(None, "hello", &args, "fr"), "Bonjour, Ana !");
    assert_eq!(
        loc.get_in(None, "only.en", &args, "fr"),
        "English only",
        "fallback locale"
    );
    assert_eq!(loc.get_in(None, "nope", &args, "fr"), "nope");
    assert_eq!(
        loc.get_in(None, "hello", &args, "fr-CA"),
        "Bonjour, Ana !",
        "same language"
    );
    assert_eq!(loc.get_in(Some("Other"), "hello", &args, "fr"), "hello");
    assert_eq!(loc.resolve_code("FR-be").as_deref(), Some("fr"));
    assert_eq!(loc.resolve_code("de"), None);
    assert_eq!(
        table().missing(),
        vec![
            ("only.en".to_owned(), "fr".to_owned()),
            ("only.en".to_owned(), "ar".to_owned())
        ]
    );
    // No locale list: the tables' columns.
    let open = Localizer::new(LocalizationSettings::default(), vec![table()]);
    let codes: Vec<String> = open.locales().into_iter().map(|l| l.code).collect();
    assert_eq!(codes, ["en", "fr", "ar"]);
}

#[test]
fn smart_strings_fill_arguments_and_plurals() {
    let a = |p: &[&str]| Args {
        positional: p.iter().map(|s| s.to_string()).collect(),
        ..Default::default()
    };
    assert_eq!(format("{0} + {1}", &a(&["2", "3"]), "en"), "2 + 3");
    assert_eq!(format("{{literal}} {0}", &a(&["x"]), "en"), "{literal} x");
    assert_eq!(format("{5} {missing}", &a(&[]), "en"), "{5} {missing}");
    let loc = localizer();
    assert_eq!(
        loc.get_in(None, "apples", &a(&["1", "Bo"]), "en"),
        "one apple for Bo"
    );
    assert_eq!(
        loc.get_in(None, "apples", &a(&["3", "Bo"]), "en"),
        "3 apples for Bo"
    );
    assert_eq!(
        loc.get_in(None, "apples", &a(&["0", "Bo"]), "en"),
        "0 apples for Bo"
    );
    assert_eq!(
        loc.get_in(None, "apples", &a(&["0", "Bo"]), "fr"),
        "une pomme pour Bo"
    );
    assert_eq!(
        loc.get_in(None, "apples", &a(&["2", "Bo"]), "fr"),
        "2 pommes pour Bo"
    );
    assert_eq!(loc.get_in(None, "apples", &a(&["2"]), "ar"), "تفاحتان");
    assert_eq!(loc.get_in(None, "apples", &a(&["5"]), "ar"), "5 تفاحات");
    assert_eq!(loc.get_in(None, "apples", &a(&["11"]), "ar"), "11 تفاحة");
    assert_eq!(plural_index("ru", 21.0, 3), 0);
    assert_eq!(plural_index("ru", 23.0, 3), 1);
    assert_eq!(plural_index("ru", 25.0, 3), 2);
    assert_eq!(plural_index("ja", 5.0, 1), 0);
    assert_eq!(plural_index("en", 1.5, 2), 1);
    assert_eq!(plural_index("de", 0.0, 3), 0, "zero|one|other");
    // Nested placeholders inside forms.
    assert_eq!(
        format(
            "{n:plural:one of {total}|{} of {total}}",
            &Args::from_entries(&["n=4".into(), "total=9".into()]),
            "en"
        ),
        "4 of 9"
    );
    assert!(is_right_to_left("ar-EG") && !is_right_to_left("fr"));
    // Case formatters follow the locale's rules.
    let city = Args::from_entries(&["city=istanbul".into()]);
    assert_eq!(format("{city:upper}", &city, "tr"), "İSTANBUL");
    assert_eq!(format("{city:upper}", &city, "en"), "ISTANBUL");
    assert_eq!(format("{city:title}", &city, "tr-TR"), "İstanbul");
    assert_eq!(format("{0:lower}", &a(&["IŞIK"]), "tr"), "ışık");
}

#[test]
fn startup_modes_pick_a_locale() {
    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let _prefs = crate::prefs::TEST_LOCK
        .lock()
        .unwrap_or_else(|e| e.into_inner());
    let mut loc = localizer();
    loc.settings.default_locale = "fr".into();
    assert_eq!(loc.startup_locale(), "fr");
    crate::prefs::set(PREF_KEY, crate::prefs::PrefValue::String("ar".into()));
    loc.settings.startup = StartupLocale::SavedPreference;
    assert_eq!(loc.startup_locale(), "ar");
    crate::prefs::delete_key(PREF_KEY);
    loc.settings.startup = StartupLocale::SystemLanguage;
    let sys = system_language().and_then(|s| loc.resolve_code(&s));
    assert_eq!(loc.startup_locale(), sys.unwrap_or_else(|| "fr".into()));
    reset_session();
    let mut loc = localizer();
    assert_eq!(loc.locale(), "en", "first use picks the startup locale");
    assert!(
        !loc.take_changed(),
        "picking the startup locale isn't a change"
    );
    assert!(loc.set_locale("de").is_err());
    assert_eq!(loc.set_locale("fr-FR").unwrap(), "fr");
    assert!(loc.take_changed());
    assert_eq!(crate::prefs::get_string(PREF_KEY, ""), "fr", "remembered");
    reset_session();
    crate::prefs::delete_key(PREF_KEY);
}

/// A Localized Text follows `Localization.locale` changes made by a script, which also get
/// `OnLocaleChanged` events; `Localization.Get` and the component API work from Lua.
#[test]
fn localized_text_updates_on_locale_change() {
    use crate::audio::Audio;
    use crate::runtime::{Runtime, Time};
    use crate::scene::UiText;
    use crate::scripting::{Input, LogKind};
    use crate::settings::ProjectSettings;

    let _lock = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
    let n = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let root = std::env::temp_dir().join(format!("misty_loc_{n}"));
    std::fs::create_dir_all(root.join("Assets/Scripts")).unwrap();
    table()
        .save(&root.join("Assets/Localization/UI.strings"))
        .unwrap();
    std::fs::write(
        root.join("Assets/Scripts/Switcher.lua"),
        r#"
local Switcher = {}
function Switcher:Awake()
    Localization.OnLocaleChanged:AddListener(function(code) Debug.Log("listener", code) end)
end
function Switcher:OnLocaleChanged(code) Debug.Log("method", code) end
function Switcher:Update()
    if Time.frameCount == 2 then
        Localization.locale = "fr"
        Debug.Log("get", Localization.Get("apples", { 2, "Zoé" }), Localization.locale)
        Debug.Log("case", Localization.ToUpper("istanbul", "tr"),
            Localization.ToLower("KIRMIZI", "tr"), Localization.ToUpper("istanbul"))
        local lt = GameObject.Find("Label"):GetComponent("LocalizedText")
        Debug.Log("key", lt.key, lt.text)
    elseif Time.frameCount == 4 then
        GameObject.Find("Label"):GetComponent("LocalizedText").args = { name = "Lea" }
    end
end
return Switcher
"#,
    )
    .unwrap();
    let mut scene = Scene::new("t");
    let label = scene.spawn("Label", None);
    scene.add(label, UiText::default());
    scene.add(
        label,
        LocalizedText {
            key: "hello".into(),
            args: vec!["name=Ana".into()],
            ..Default::default()
        },
    );
    let s = scene.spawn("Switcher", None);
    scene.add(
        s,
        Scripts {
            list: vec![ScriptRef {
                path: "Assets/Scripts/Switcher.lua".into(),
                ..Default::default()
            }],
        },
    );
    let mut settings = ProjectSettings::default();
    settings.localization = localizer().settings;
    reset_session();
    let (audio, _) = Audio::null(root.clone());
    let mut rt = Runtime::start_with_audio(&mut scene, root.clone(), &settings, audio);
    let text = |scene: &Scene| scene.world.get::<&UiText>(label).unwrap().text.clone();
    assert_eq!(text(&scene), "Hello, Ana!", "set before the first frame");
    let mut time = Time::default();
    let mut logs = Vec::new();
    for _ in 0..5 {
        time.advance(1.0 / 60.0);
        rt.update(&mut scene, &time, Input::default());
        logs.extend(rt.take_logs());
        if time.frame == 2 {
            assert_eq!(text(&scene), "Bonjour, Ana !", "same frame as the change");
        }
    }
    let problems: Vec<_> = logs.iter().filter(|(k, _)| *k != LogKind::Info).collect();
    assert!(problems.is_empty(), "{problems:?}");
    assert!(
        logs.iter().any(|(_, t)| t == "get 2 pommes pour Zoé fr"),
        "{logs:?}"
    );
    assert!(logs.iter().any(|(_, t)| t == "key hello Bonjour, Ana !"));
    assert!(
        logs.iter()
            .any(|(_, t)| t == "case İSTANBUL kırmızı ISTANBUL"),
        "locale-aware case from Lua: {logs:?}"
    );
    assert_eq!(
        logs.iter().filter(|(_, t)| t == "listener fr").count(),
        1,
        "{logs:?}"
    );
    assert_eq!(logs.iter().filter(|(_, t)| t == "method fr").count(), 1);
    assert_eq!(text(&scene), "Bonjour, Lea !", "args from Lua");
    reset_session();
    crate::prefs::delete_key(PREF_KEY);
    std::fs::remove_dir_all(root).ok();
}

#[test]
fn deeply_nested_plurals_do_not_overflow_the_stack() {
    let n = 100_000;
    let text = format!("{}x{}", "{0:p:".repeat(n), "}".repeat(n));
    let args = Args::from_entries(&["1".to_owned()]);
    // Run on a small stack: unbounded recursion would overflow it.
    let out = std::thread::Builder::new()
        .stack_size(256 * 1024)
        .spawn(move || format(&text, &args, "en"))
        .unwrap()
        .join()
        .expect("formatting overflowed the stack");
    assert!(
        out.ends_with('x') || out.contains("{0:p:"),
        "{}",
        &out[..out.len().min(80)]
    );
    // Ordinary nesting still works.
    let args = Args::from_entries(&["2".to_owned(), "total=5".to_owned()]);
    assert_eq!(format("{0:p:one|{0} of {total}}", &args, "en"), "2 of 5");
}
