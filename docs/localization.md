# Localization

Two different things are localized in Misty Engine:

- **The editor UI** (menus, panels, the Hub, Preferences…): English, Türkçe and العربية.
- **Your games**: String Tables, Localized Text and `Localization` in Lua (see the scripting
  guide and Window ▸ General ▸ Localization Tables). Any language works; Arabic and other
  right-to-left scripts are shaped and laid out right to left, Turkish casing is handled.

## Editor language

Edit ▸ Preferences ▸ General ▸ **Language** (also on the Hub's Settings page). The choice is
saved per user (`editor_language` in `hub.json`) and applies on the next frame, no restart.
`MISTY_EDITOR_LANGUAGE=tr` (`en`, `tr`, `ar`) overrides it for one run, for screenshots.

Text the editor has no translation for stays English. Object, asset and scene names are never
translated (they are your data), but Arabic names are shown joined and right to left.

### How it works (`crates/misty_editor/src/i18n`)

- Code keys text by its English source: `tr!("Save")`, `tr!("Save changes to {scene}?",
  scene = name)`. Placeholders are filled after translating. The menu helpers (`item`, `sub`,
  `top` in `menu_bar.rs`), `card::sub_header`, `targets::label` and the vector rows translate
  their label themselves and keep the English text as the tutorial target id.
- When one English word needs two translations, the key gets a `#context` suffix that is never
  shown: `tr!("Light#theme")` (the Hub's colour scheme) versus `"Light"` (a light source).
- Translations live in `crates/misty_editor/locales/<code>.toml`, embedded in the binary.
- Arabic: egui (0.36) shapes each run of one font and lays an Arabic run out right to left,
  but has no bidirectional algorithm, so mixed lines come out in the wrong order.
  `i18n::shape` joins the letters (`ar-reshaper`, MIT: the Arabic Presentation Forms), puts
  each line in visual order with the Unicode Bidirectional Algorithm (`unicode-bidi`,
  mirroring brackets), so numbers and Latin words inside Arabic sentences keep reading left to
  right, then hands every stretch of Arabic characters back in logical order for egui to lay
  out right to left. `tr!` does it for translated text and `i18n::display` for names. Shaped
  text is for drawing only: text fields keep the logical text.
- Font: the built-in Noto Sans Arabic subset (`crates/misty_engine/assets/fonts`, SIL OFL 1.1,
  68 KB, shared with games) comes first in the editor's text fonts. It has only Arabic
  characters, so egui's Arabic runs are exactly the stretches of Arabic characters (what
  `i18n::shape` relies on), and its line metrics were set to Segoe UI's so rows keep their
  height. Bold Arabic titles use its regular weight.

### Right-to-left layout (Arabic)

Mirrored:
- the title bar: window buttons on the left, menus from the right (File rightmost), entries
  right-aligned with the shortcut on the left and submenu arrows pointing left;
- the Hub: page links on the right, template cards with the picture on the right;
- Properties panel labels: right-aligned against their values;
- the Default layout (when no saved layout exists, or Window ▸ Layouts ▸ Default): Properties
  on the left, Scene Tree on the right;
- dock tab titles (icon after the title).

Kept left to right on purpose: the Editor Scene and Scene Viewer, gizmos, number fields, vectors
(X Y Z), the Code Editor and the Console text. Not mirrored yet: the Scene Tree and Asset Files
trees (indentation stays on the left), submenus still open to the right, panel toolbars,
the order of label and value columns in the Properties panel, and other built-in layouts.
Long Arabic tooltips are broken into lines of about 64 characters before egui sees them
(egui would wrap visual text from the wrong end); narrow places (Hub cards) break at the
measured width with `i18n::tr_wrapped`.

## Adding a language

1. Add the language to `i18n::Language` (`code`, `native_name`, `is_rtl`) and its file to
   `catalog::source`.
2. Copy `locales/tr.toml` to `locales/<code>.toml` and translate the right-hand sides. Keep
   every `{placeholder}` exactly; leave an entry out (or empty) to fall back to English.
3. Run `cargo test -p misty_editor --release i18n -- --nocapture`:
   - `untranslated_keys_report` prints the coverage of each language and lists untranslated
     and unused entries (a report, it never fails);
   - `placeholders_match_their_english` fails when a translation's placeholders differ;
   - `translation_files_parse_without_conflicting_entries` fails on TOML errors or an English
     text translated two ways.
4. A right-to-left language other than Arabic also needs a font with its glyphs and, for
   scripts with joining (Syriac, N'Ko…), a shaper; Hebrew only needs reordering.

To translate more of the editor, wrap the English literal in `tr!` where it is drawn (never
where it is used as an id, a file name or a lookup key) and add the entries to the files.

## Game text

- Shaping: UI Text with right-to-left characters takes the rich text path, which shapes runs
  with rustybuzz and reorders them (UAX #9). The built-in Noto Sans Arabic is a fallback of
  every Text, so Arabic shows on any machine; project fonts with Arabic glyphs are used first.
- **Mirror for RTL** (Text properties, `UiText::mirror_rtl`): text that starts in a
  right-to-left script swaps Left and Right alignment, so one Text works for every language of
  a Localized Text.
- Turkish casing: Rust's `to_uppercase` turns "istanbul" into "ISTANBUL"; Turkish wants
  "İSTANBUL" (and "KIRMIZI" → "kırmızı"). Use `localization::to_upper` / `to_lower` /
  `to_title`, the smart-string formatters `{name:upper}`, `{name:lower}`, `{name:title}`, or
  `Localization.ToUpper(text[, locale])` / `Localization.ToLower` in Lua (the current locale by
  default). Azerbaijani follows the same rules.
- Sample: `Assets/Scenes/LocalizationDemo2.scene` (English, Türkçe, العربية): casing, plural
  forms, mixed Arabic / numbers / Latin, a rich-text Text with an outline, mirrored alignment.

## Term glossary

Standard, searchable terms: the ones Microsoft's Turkish and Arabic style guides and the
Turkish and Arabic game-dev communities use. Product names stay in Latin script (Misty Hub,
Shader Graph, Misty GI, Misty Mesh, Lua, MCP); `Assets` stays as the folder name it is. Keyboard keys and
shortcuts are never translated.

| English | Türkçe | العربية | Note |
|---|---|---|---|
| Scene | Sahne | المشهد | |
| Editor Scene | Düzenleyici Sahnesi | مشهد المحرر | the editing view |
| Scene Viewer | Sahne Görüntüleyici | عارض المشهد | the play view |
| Scene Tree | Sahne Ağacı | شجرة المشهد | |
| Properties | Özellikler | الخصائص | |
| Asset Files | Varlık Dosyaları | ملفات الأصول | the asset browser |
| Project | Proje | المشروع | a game project |
| Console | Konsol | وحدة التحكم | Microsoft's Arabic term |
| Asset(s) | Varlık(lar) | الأصول | |
| Object | Nesne | كائن | the Object menu |
| Archetype | Arketip | نمط | what objects are built from (Add Archetype) |
| Prefab | Prefab | قالب جاهز | Turkish users say "prefab" |
| Transform | Dönüşüm | التحويل | |
| Position / Rotation / Scale | Konum / Döndürme / Ölçek | الموضع / الدوران / المقياس | |
| Mesh | Örgü | الشبكة | Blender's Turkish term |
| Renderer | İşleyici | عارض | |
| Rendering | İşleme | التصيير | Arabic: not العرض, which is "view" |
| Collider | Çarpıştırıcı | مصادم | |
| Rigidbody | Katı Cisim | جسم صلب | |
| Shader | Gölgelendirici | مظلل | |
| Material | Malzeme | مادة | |
| Texture | Doku | خامة | |
| Script | Betik | البرنامج النصي | Microsoft's Arabic term |
| Sprite | Sprite | سبرايت | the usual Arabic game-dev word |
| Tilemap | Karo Haritası | خريطة البلاطات | |
| Gizmos | Gizmolar | الأدوات المرئية | |
| Baked (lighting) | Pişirilmiş | مُجهّز مسبقًا | "bake" = pişirmek |
| Build | Derleme / Derle | البناء / بناء | |
| Play / Pause / Step | Oynat / Duraklat / Adım | تشغيل / إيقاف مؤقت / خطوة | |
| Layout | Yerleşim | التخطيط | |
| Preferences | Tercihler | التفضيلات | |
| Profiler | Profil Oluşturucu | محلل الأداء | |
| Timeline | Zaman Çizelgesi | المخطط الزمني | |
| Animator | Animatör | أداة التحريك | not المحرّك ("engine") |
| Particle System | Parçacık Sistemi | نظام الجسيمات | |
| Post-processing | Son İşleme | المعالجة اللاحقة | |
| Light / Light (theme) | Işık / Açık | الضوء / فاتحة | `Light#theme` |

Style: Turkish uses title case for menu entries and labels and sentence case for tooltips, with
the polite imperative (-in, "Seçin"). Arabic uses verbal nouns for commands (حفظ, فتح, إغلاق),
"،" and "؟" punctuation, no diacritics, and «» quotes; counts are phrased as "الكائنات: {count}"
so one wording fits every number.
