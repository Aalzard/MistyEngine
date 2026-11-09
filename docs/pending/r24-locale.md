# Round 24: Turkish and Arabic (branch r24-locale)

Asked for: "start working on Turkish and Arabic". The editor UI was English-only (literals in
egui code); games already had String Tables, Localized Text and `MISTY_LOCALE`. Build a small
i18n layer for the editor with one readable file per language, a live language picker,
translate what a new user sees in the first ten minutes, make Arabic render correctly (shaping,
right to left, a font) in the editor and in games, make Turkish casing locale-aware, add a
three-language sample, screenshots and docs.

## Result

| | Türkçe | العربية |
|---|---|---|
| Keys the code translates (`tr!` literals, menu helpers, shown enum names) | 792 / 792 (100 %) | 792 / 792 (100 %) |
| Registered component names | 60 / 124 | 60 / 124 |
| Editor-wide UI call sites wrapped in `tr!` (rough count) | 412 of ~1 918 (21 %) | same |

Numbers from `cargo test -p misty_editor --release untranslated_keys_report -- --nocapture`.
The 792 keys cover menus (all eight, every submenu), toolbar, tab titles, Hierarchy, Project,
Console, Scene view toolbar and camera settings, the Inspector header, Add Component and the
common components (Transform, Camera, Light incl. shadows and volumetrics, Mesh Filter / Mesh
Renderer, Rigidbody, Box / Sphere / Capsule / Mesh Collider, Audio Source, Audio Listener,
scripts), the Hub (Projects, New Project with the template cards, Learn, Settings), Preferences
(incl. the Code Editor and AI tools sections), the Code Editor's toolbar and find bar, the save
prompts and the status bar.

Screenshots (`docs/screenshots/r24/`): `editor_tr_gameobject_menu.png`,
`editor_tr_inspector.png`, `editor_ar_file_menu.png`, `editor_ar_inspector.png`,
`editor_ar_preferences.png`, `hub_tr_new_project.png`, `hub_tr_settings.png`,
`hub_ar_new_project.png`, `hub_ar_projects.png`, `game_ar_localization_demo2.png`,
`game_tr_localization_demo2.png`.

## Decisions

- **Keys are the English text** (`tr!("Save")`), so untranslated text falls back to English by
  itself and English never changes. Placeholders are named (`{scene}`) and filled after
  translating; a `#context` suffix (`"Light#theme"`) separates the two meanings of one word and
  is never shown. The menu helpers, `card::sub_header`, `targets::label` and the vector rows
  translate their label and keep the English text as the tutorial target id, so tutorials keep
  working in every language.
- **TOML files** (`crates/misty_editor/locales/tr.toml`, `ar.toml`), tables per area, embedded
  with `include_str!`, parsed once with the `toml` crate (already in the lock file).
- **The language is per thread** (`thread_local`): the UI runs on one thread, and tests (one
  thread each) can switch language without disturbing each other. Saved in `hub.json`
  (`editor_language`), shared by the Hub and the editor, applied on the next frame.
- **Arabic in egui.** egui 0.36 shapes each run of one font with harfrust and lays a run of
  Arabic out right to left, but has no bidirectional algorithm and guesses a run's direction
  from its first letter, so mixed lines (Arabic + numbers + Latin) came out in the wrong order
  (first attempt, presentation forms in visual order, showed every Arabic word reversed). The
  fix: the built-in Arabic font goes first in the text fonts and contains only Arabic
  characters, so egui's Arabic runs are exactly the stretches of Arabic characters;
  `i18n::shape` joins the letters (`ar-reshaper`, presentation forms), applies UAX #9 with
  `unicode-bidi` (brackets mirrored) and hands every Arabic stretch back in logical order for
  egui to reverse. The result has no logical-form letters left, so shaping twice is a no-op.
- **Font**: Noto Sans Arabic Regular subset (68 KB, SIL OFL 1.1) in
  `crates/misty_engine/assets/fonts`, with Segoe UI's line metrics so the editor's rows keep
  their height with it first in the list; shared by games (a built-in fallback of every UI
  Text, shaped with rustybuzz there).
- **RTL layout**: the title bar is mirrored (window buttons left, File rightmost, entries
  right-aligned with shortcuts left, submenu arrows left), Inspector labels right-aligned, Hub
  navigation on the right and template cards mirrored, the Default layout mirrored (Inspector
  left, Hierarchy right), tab icons after the title. Scene/Game views, gizmos, numbers, vectors
  and the Code Editor stay left to right.
- **Long Arabic text**: egui wraps visual text from the left (the end of an RTL sentence would
  land on the first line), so Arabic longer than 64 characters (tooltips) is broken into lines
  first; narrow places measure (`i18n::tr_wrapped`, Hub cards).
- **Turkish casing** lives in `localization::case` (to_upper / to_lower / to_title, Turkish and
  Azerbaijani rules); smart strings gained `{x:upper}`, `{x:lower}`, `{x:title}` and Lua
  `Localization.ToUpper` / `ToLower`. The runtime had no locale case conversion before.
- **Game text**: `UiText::mirror_rtl` (Text ▸ Mirror for RTL, off by default so existing scenes
  don't change) swaps Left/Right alignment for text that starts right to left; brackets in RTL
  runs are mirrored (was missing: "(2×)" showed as ")2×(").
- Translation choices and the glossary: `docs/localization.md`.

## Checks

- `cargo fmt --all`: clean.
- `cargo clippy --workspace --all-targets --release`: 0 warnings.
- `cargo test --workspace --release --no-fail-fast`: all pass (editor 1 383, engine 795, player
  and integration tests 14; 171 ignored as usual).
- Golden shots (`golden_shots -- --ignored`): pass.

## Not done yet

- The rest of the editor is English: Project Settings, the graph editors, Animation, Timeline,
  Lighting, the Game view toolbar, most component editors beyond the common ones (64 of 124
  component names), tutorial steps, console messages.
- Not mirrored: the Hierarchy and Project trees, submenus (they still open to the right),
  panel toolbars, Preferences rows, the Inspector's label / value column order, built-in
  layouts other than Default (and a layout saved before switching keeps its sides).
- Text fields edit Arabic in logical order: words are shaped and laid out by egui, but a line
  mixing Arabic and Latin is not reordered while editing.
- Bold Arabic titles use the regular weight (no bold Arabic face embedded).
- Hebrew and other RTL scripts: reordered, but no font or shaping work beyond what the system
  fonts give.
- The editor does not follow the system language on first start (English until chosen).
