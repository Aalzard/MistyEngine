# Round 64: light mode, custom colour themes, custom fonts (branch r64-themes)

Asked for: theme presets (Dark unchanged, Light, 2–3 more), the whole editor following the
theme, a Theme editor (custom colours, live preview, named custom themes per user, duplicate /
rename / delete / reset, `.mistytheme` import / export, contrast warning), UI and code fonts
from the installed fonts with a size scale and a silent fallback, everything live and saved.

## Design

- **Tokens** (`theme.rs`): one `Palette` struct of 41 colours declared once by the `palette!`
  macro, which also generates `TOKENS` (key, English label, editor group), `Palette::get /
  get_mut` and one function per token (`theme::text()`, `theme::panel_bg()` …). The old
  `theme::TEXT` style constants became these functions (a mechanical rename at every call
  site); the scene gizmo `AXIS_X/Y/Z` stay constants.
- **Active palette**: a thread-local `RefCell<Palette>` (the UI runs on one thread; tests that
  draw under Light don't change the colours other test threads see).
- **`theme::shade(c)`** maps a fixed colour made for the Dark theme onto the active palette:
  its brightness is placed between the palette's matching shades (tab bar 40, field 42, panel
  56, card 63, button 78, hover 92, weak text 130, text 210, strong text 232) and its tint is
  kept. It is the identity under Dark (tested), so wrapping the hard-coded greys of panels
  keeps Dark pixel-identical while Light gets light canvases, dark text and darker accents.
- **`theme::near(token, dark)`**: a colour drawn beside a token (the Shader Graph's node body is
  6 levels lighter than the Script Graph's): the token's colour plus the Dark offset. Exact
  under Dark, follows the token in custom themes.
- **Graphs and Code Editor** use their own tokens (graph background / node body / header /
  selection; code background / text / line numbers / current line / selection).
- **Visuals**: `theme::visuals(&Palette)` builds egui's visuals from the palette (dark or light
  base by the panel luminance, so icon tints via `icons::accent` follow); `dock_style` reads the
  palette every frame.

## Hard-coded colours audit

- Every `theme::CONST` use (507 sites, 77 files) → token function.
- Literal `Color32::from_rgb/from_gray/from_rgba_*` in UI files (≈330 lines, 75 files) wrapped
  in `theme::shade(...)` by a script; local colour constants are wrapped at their use sites.
  Skipped on purpose: scene gizmos and handles drawn over the 3D view (handles, splines, skin,
  sprites, UI rect, light probes, vcam, occlusion, physics debug, mesh builder selection, world
  builder gizmos, the Scene view axis gizmo), the Hub's own palette, user data colours (folder
  colours, gradients, nav areas, lightmap swatches) and colours computed at run time.
- `Color32::LIGHT_RED` error labels → `theme::error()`, `Color32::GRAY` → `theme::text_dim()`.
- White text on coloured fills (status bars, accent buttons) is left white.

## Custom themes (`theme/custom.rs`, `theme/editor.rs`)

- Files: `%APPDATA%\MistyEngine\themes\<name>.mistytheme` (JSON: `format`, `name`, `base`,
  `colors` as `#rrggbb` / `#rrggbbaa`). Tokens a file leaves out come from its base preset;
  unknown tokens are skipped; a colour that doesn't parse is an error.
- The choice is `editor_theme` in `hub.json` (`light`, `custom:My Theme`); `MISTY_THEME=<preset
  id or custom name>` overrides it (captures). Choosing a theme sets the Hub's light / dark by
  the palette's luminance, and the Hub's Dark / Light buttons set the editor's theme.
- Editing a preset's colour copies it to "<Preset> Custom" first. Edits apply the same frame;
  the file is written once the mouse is up.
- Contrast check: text on panel / field / card / popup ≥ 4.5, text on button, weak text on
  panel and selected text on selection ≥ 3, code text on code background ≥ 4.5 (WCAG).

## Fonts (`theme/fonts.rs`)

- Enumeration: Windows `%WINDIR%\Fonts` and `%LOCALAPPDATA%\Microsoft\Windows\Fonts`; Linux
  `/usr/share/fonts`, `/usr/local/share/fonts`, `$XDG_DATA_HOME/fonts`, `~/.local/share/fonts`,
  `~/.fonts`; macOS system / library / user folders. `.ttf/.otf/.ttc/.otc`, recursive, every
  face of collections, names from the `name` table, monospace from `post.isFixedPitch`, faces
  without outlines skipped. Read with `skrifa` 0.44 (already in the tree through egui; MIT /
  Apache-2.0), on a background thread the first time the picker opens.
- Fallback chain: Arabic built-in font first (unchanged, `i18n::shape` relies on it), then the
  chosen font, then Segoe UI / the symbol fonts / egui's fonts; egui picks each glyph from the
  first font that has it.
- A chosen font is checked with `skrifa::FontRef::from_index` (exactly what egui does before it
  panics on a bad font) before it reaches egui; a missing or unreadable file is skipped and a
  note is left (`take_fallback_note`), shown once as a small corner note and a Console warning.
- Settings: `editor_font`, `editor_mono_font` (`path` or `path#index`), `editor_font_scale`
  (80–150 %) in `hub.json`.

## Tests

`theme/tests.rs`, `theme/panel_tests.rs` (see Checks).

## Checks

(filled in at the end)
