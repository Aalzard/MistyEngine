//! Theme tokens, custom theme files, contrast warnings and font choice / fallback.

use super::custom::{self, Choice, CustomTheme, ThemeStore};
use super::fonts::{self, FaceRef, FontPrefs};
use super::presets::{self, Preset};
use super::*;

fn temp_dir(name: &str) -> std::path::PathBuf {
    let d = std::env::temp_dir().join(format!("misty_theme_{name}_{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&d);
    std::fs::create_dir_all(&d).unwrap();
    d
}

#[test]
fn every_token_resolves_in_every_preset() {
    for p in Preset::ALL {
        let pal = p.palette();
        for t in TOKENS {
            let c = pal
                .get(t.key)
                .unwrap_or_else(|| panic!("{:?}: {}", p, t.key));
            assert_eq!(c.a(), 255, "{p:?}.{} is opaque", t.key);
        }
        assert_eq!(Preset::from_id(p.id()), Some(p));
        // The active-theme functions follow the palette.
        set_current(pal);
        assert_eq!(text(), pal.text);
        assert_eq!(panel_bg(), pal.panel_bg);
        assert_eq!(graph_header(), pal.graph_header);
    }
    set_current(presets::DARK);
    assert!(presets::DARK.is_dark() && !presets::LIGHT.is_dark());
    // Keys are unique.
    let mut keys: Vec<_> = TOKENS.iter().map(|t| t.key).collect();
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), TOKENS.len());
}

/// Dark keeps the editor's original colours exactly.
#[test]
fn dark_preset_keeps_the_original_colours() {
    let d = presets::DARK;
    assert_eq!(d.panel_bg, Color32::from_rgb(56, 56, 56));
    assert_eq!(d.tab_bar_bg, Color32::from_rgb(40, 40, 40));
    assert_eq!(d.text, Color32::from_rgb(210, 210, 210));
    assert_eq!(d.text_dim, Color32::from_rgb(130, 130, 130));
    assert_eq!(d.selection, Color32::from_rgb(44, 93, 135));
    assert_eq!(d.warn, Color32::from_rgb(230, 180, 60));
    // `shade` is the identity under Dark, for greys, tints and translucent colours.
    for c in [
        Color32::from_rgb(34, 35, 38),
        Color32::from_gray(200),
        Color32::from_rgb(196, 110, 36),
        Color32::from_rgb(255, 214, 110),
        Color32::WHITE,
        Color32::BLACK,
        Color32::from_rgba_unmultiplied(90, 160, 230, 70),
    ] {
        let s = d.shade(c);
        let diff = |a: u8, b: u8| (a as i32 - b as i32).abs();
        assert!(
            diff(s.r(), c.r()) <= 1 && diff(s.g(), c.g()) <= 1 && diff(s.b(), c.b()) <= 1,
            "{c:?} -> {s:?}"
        );
    }
}

#[test]
fn shade_maps_dark_colours_onto_light() {
    let l = presets::LIGHT;
    // A dark canvas becomes light, light text becomes dark.
    assert!(luminance(l.shade(Color32::from_rgb(34, 35, 38))) > 0.6);
    assert!(luminance(l.shade(Color32::from_gray(200))) < 0.05);
    // Panel greys land on the Light panel colours.
    assert_eq!(l.shade(Color32::from_gray(56)), l.panel_bg);
    assert_eq!(l.shade(Color32::from_gray(210)), l.text);
    // A tint keeps its hue.
    let orange = l.shade(Color32::from_rgb(196, 110, 36));
    assert!(orange.r() > orange.b() + 60, "{orange:?}");
}

#[test]
fn custom_theme_round_trips_through_its_file() {
    let mut t = CustomTheme::from_preset("Sunset", Preset::WarmDark);
    t.palette.accent = Color32::from_rgb(1, 2, 3);
    t.palette.graph_header = Color32::from_rgba_unmultiplied(10, 20, 30, 128);
    let text = t.to_text();
    let back = CustomTheme::from_text(&text).unwrap();
    assert_eq!(back.name, "Sunset");
    assert_eq!(back.base, Preset::WarmDark);
    assert_eq!(back.palette.accent, t.palette.accent);
    assert_eq!(
        back.palette.graph_header.to_srgba_unmultiplied(),
        [10, 20, 30, 128]
    );
    assert_eq!(back.palette.panel_bg, t.palette.panel_bg);
}

#[test]
fn store_saves_renames_and_deletes_themes() {
    let dir = temp_dir("store");
    let mut s = ThemeStore::load(Some(dir.clone()));
    assert!(s.themes.is_empty());
    let mut t = CustomTheme::from_preset("Mine", Preset::Light);
    t.palette.text = Color32::from_rgb(5, 6, 7);
    s.save(t).unwrap();
    assert!(dir.join("Mine.mistytheme").is_file());
    // A fresh load reads it back.
    let s2 = ThemeStore::load(Some(dir.clone()));
    assert_eq!(
        s2.get("Mine").unwrap().palette.text,
        Color32::from_rgb(5, 6, 7)
    );
    assert_eq!(s.unique_name("Mine"), "Mine 2");
    assert_eq!(s.unique_name("light"), "light 2", "preset ids are taken");
    let new = s.rename("Mine", "Ours").unwrap();
    assert_eq!(new, "Ours");
    assert!(!dir.join("Mine.mistytheme").exists());
    assert!(dir.join("Ours.mistytheme").is_file());
    s.delete("Ours").unwrap();
    assert!(!dir.join("Ours.mistytheme").exists());
    assert!(ThemeStore::load(Some(dir.clone())).themes.is_empty());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn import_and_export_share_a_theme() {
    let dir = temp_dir("share");
    let mut t = CustomTheme::from_preset("Shared", Preset::Midnight);
    t.palette.error = Color32::from_rgb(200, 0, 0);
    let file = dir.join("out").join("Shared.mistytheme");
    t.export(&file).unwrap();
    let back = CustomTheme::import(&file).unwrap();
    assert_eq!(back, t);
    // A hand-written file: missing tokens come from its base, unknown ones are skipped, and
    // a nameless file is named after the file.
    let hand = dir.join("Paper.mistytheme");
    std::fs::write(
        &hand,
        r##"{ "base": "light", "colors": { "accent": "#f80", "future_token": "#123456" } }"##,
    )
    .unwrap();
    let p = CustomTheme::import(&hand).unwrap();
    assert_eq!(p.name, "Paper");
    assert_eq!(p.palette.accent, Color32::from_rgb(255, 136, 0));
    assert_eq!(p.palette.panel_bg, presets::LIGHT.panel_bg);
    // Bad colours and bad files are errors, not panics.
    assert!(CustomTheme::from_text(r#"{ "colors": { "text": "pink" } }"#).is_err());
    assert!(CustomTheme::from_text("not json").is_err());
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn hex_colours_parse_and_print() {
    assert_eq!(
        custom::from_hex("#ff8000"),
        Some(Color32::from_rgb(255, 128, 0))
    );
    assert_eq!(
        custom::from_hex("0a0b0c"),
        Some(Color32::from_rgb(10, 11, 12))
    );
    assert_eq!(custom::from_hex("#fff"), Some(Color32::WHITE));
    assert_eq!(custom::from_hex("#12345"), None);
    assert_eq!(custom::from_hex("#ééé"), None);
    assert_eq!(custom::to_hex(Color32::from_rgb(1, 2, 255)), "#0102ff");
}

#[test]
fn low_contrast_text_is_reported() {
    for p in Preset::ALL {
        assert!(
            custom::low_contrast(&p.palette()).is_empty(),
            "{p:?}: {:?}",
            custom::low_contrast(&p.palette())
        );
    }
    let mut p = presets::DARK;
    p.text = Color32::from_gray(70);
    let low = custom::low_contrast(&p);
    assert!(
        low.iter()
            .any(|(fg, bg, r)| *fg == "Text" && *bg == "Panel" && *r < 4.5)
    );
    assert!(contrast(Color32::BLACK, Color32::WHITE) > 20.9);
    assert!((contrast(Color32::RED, Color32::RED) - 1.0).abs() < 1e-4);
}

#[test]
fn theme_choice_settings() {
    for c in [
        Choice::Preset(Preset::Light),
        Choice::Preset(Preset::HighContrast),
        Choice::Custom("My Theme".into()),
    ] {
        assert_eq!(Choice::from_setting(&c.to_setting()), c);
    }
    assert_eq!(
        Choice::from_setting("nonsense"),
        Choice::Preset(Preset::Dark)
    );
    assert_eq!(custom::clean_name("  a/b:c?.  "), "abc");
}

/// A chosen font that is gone falls back to the defaults (with a note), never a panic.
#[test]
fn missing_font_falls_back_without_panicking() {
    let prefs = FontPrefs {
        ui: Some(FaceRef {
            path: "Z:/no/such/font.ttf".into(),
            index: 0,
        }),
        mono: Some(FaceRef {
            path: "Z:/no/such/mono.ttf".into(),
            index: 0,
        }),
        scale: 1.25,
    };
    let (defs, loaded) = fonts::definitions(&prefs);
    assert_eq!(loaded.missing.len(), 2);
    assert!(!defs.font_data.contains_key("user_ui"));
    // The built-in Arabic font still leads the text family.
    assert_eq!(
        defs.families[&egui::FontFamily::Proportional]
            .first()
            .map(String::as_str),
        Some(ARABIC_FONT)
    );
    // A file that is not a font is refused the same way.
    let dir = temp_dir("badfont");
    let bad = dir.join("bad.ttf");
    std::fs::write(&bad, b"definitely not a font").unwrap();
    assert!(
        fonts::read_face(&FaceRef {
            path: bad,
            index: 0
        })
        .is_none()
    );
    // Applying it to a running UI works and leaves the note once.
    let ctx = egui::Context::default();
    let _ = fonts::take_fallback_note();
    let loaded = fonts::apply(&ctx, &prefs);
    assert_eq!(loaded.missing.len(), 2);
    let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
        ui.label("Hello ğ مرحبا");
    });
    assert!(fonts::take_fallback_note().is_some_and(|n| n.contains("font.ttf")));
    assert!(fonts::take_fallback_note().is_none());
    let _ = std::fs::remove_dir_all(dir);
}

/// The built-in fonts serve as a fake fonts folder: enumeration finds them by name, a broken
/// file is skipped, and a found font loads with the per-glyph fallbacks behind it.
#[test]
fn fonts_are_listed_from_a_fonts_folder() {
    let dir = temp_dir("fontdir");
    let sub = dir.join("nested");
    std::fs::create_dir_all(&sub).unwrap();
    std::fs::write(
        dir.join("Arabic.ttf"),
        misty_engine::ui::rich::fonts::ARABIC_FONT_DATA,
    )
    .unwrap();
    let hack = egui::FontDefinitions::default();
    let mono_name = hack.families[&egui::FontFamily::Monospace][0].clone();
    let mono_bytes: &[u8] = &hack.font_data[&mono_name].font;
    std::fs::write(sub.join("Mono.TTF"), mono_bytes).unwrap();
    std::fs::write(dir.join("broken.otf"), b"nope").unwrap();
    std::fs::write(dir.join("readme.txt"), b"not a font").unwrap();
    let list = fonts::enumerate(std::slice::from_ref(&dir));
    assert_eq!(list.len(), 2, "{list:#?}");
    assert!(list.iter().any(|e| e.family.contains("Arabic")));
    let mono = list.iter().find(|e| e.monospace).expect("a monospace face");
    // The chosen font goes first, the built-in ones stay behind it.
    let prefs = FontPrefs {
        ui: None,
        mono: Some(mono.face.clone()),
        scale: 1.0,
    };
    let (defs, loaded) = fonts::definitions(&prefs);
    assert!(loaded.missing.is_empty());
    let fam = &defs.families[&egui::FontFamily::Monospace];
    assert_eq!(fam[0], "user_mono");
    assert!(fam.iter().any(|n| n == ARABIC_FONT));
    assert!(fam.len() > 2, "built-in fallbacks remain: {fam:?}");
    let _ = std::fs::remove_dir_all(dir);
}

#[test]
fn font_scale_is_clamped_and_applied() {
    assert_eq!(fonts::clamp_scale(9.0), fonts::MAX_SCALE);
    assert_eq!(fonts::clamp_scale(f32::NAN), 1.0);
    let ctx = egui::Context::default();
    fonts::apply_scale(&ctx, 1.5);
    let body = ctx.global_style().text_styles[&egui::TextStyle::Body].size;
    let d = egui::Style::default().text_styles[&egui::TextStyle::Body].size;
    assert!((body - d * 1.5).abs() < 1e-3);
}

/// Theme editor actions on a store in a temp folder: editing a preset copies it, duplicate,
/// rename, reset and delete.
#[test]
fn theme_editor_actions() {
    let dir = temp_dir("editor");
    let mut a =
        editor::Appearance::with_store(ThemeStore::load(Some(dir.clone())), dir.join("hub.json"));
    a.choose(Choice::Preset(Preset::Light)).unwrap();
    a.set_color("accent", Color32::from_rgb(9, 9, 9)).unwrap();
    let Choice::Custom(name) = a.choice.clone() else {
        panic!("editing a preset makes a custom theme");
    };
    assert_eq!(a.palette().accent, Color32::from_rgb(9, 9, 9));
    assert_eq!(a.palette().panel_bg, presets::LIGHT.panel_bg);
    let copy = a.duplicate().unwrap();
    assert_ne!(copy, name);
    a.rename_current("Renamed").unwrap();
    assert_eq!(a.choice, Choice::Custom("Renamed".into()));
    a.reset_to(Preset::HighContrast).unwrap();
    assert_eq!(a.palette(), presets::HIGH_CONTRAST);
    a.delete_current().unwrap();
    assert_eq!(a.choice, Choice::Preset(Preset::HighContrast));
    assert!(a.store.get("Renamed").is_none());
    // The Hub's light / dark follows the editor's theme.
    let hub = crate::hub::config::HubConfig::load(&dir.join("hub.json"));
    assert_eq!(hub.editor_theme.as_deref(), Some("high_contrast"));
    assert_eq!(hub.theme, crate::hub::config::HubTheme::Dark);
    let _ = std::fs::remove_dir_all(dir);
}
