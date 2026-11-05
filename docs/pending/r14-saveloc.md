# Round 14: Save System + Localization

## Decisions

1. **Names.** `SaveSystem` (Lua), the **Saveable** component, **Save Data** window; **Localization
   Settings** (Project Settings page), **String Table** assets (`.strings`), **Localized Text**
   component, **Localization Tables** window, Lua `Localization`. Behaviour follows the reference
   engine's save-game add-ons and its localization package; no code, assets or names copied.
2. **Load = reload the saved scene file, then apply.** `SaveSystem.Load` queues the save; after the
   frame, `Runtime` reads the scene the save names (`info.scene_path`), starts it again (physics,
   Awake) and applies the save: destroyed objects go, spawned prefabs come back, states are set,
   bodies teleported and velocities restored, then `OnLoad(data)` (after Awake, before Start).
   Reason: the only way to bring back objects destroyed after the save, and it doesn't need the
   player or editor to know about saves (both already call `Runtime::update`; the editor only
   clears its selection via `take_scene_reloaded`). Unsaved Edit-mode changes aren't in the file, like
   the reference engine's scene loads. Without a scene file (untitled scene) the save is applied in
   place (this session's spawned objects are removed first) and a warning says so.
3. **Synchronous `Save`.** `OnSave` must run on every script instance from inside a Lua call, where
   the host's instance list isn't reachable; instances are therefore also tracked in a Lua registry
   table (`misty.save.instances`, removed on destroy) so `Save` writes the file before returning
   and `Exists(slot)` is true right after.
4. **What identifies objects.** Saveable's Save ID (GUID string). `Scene::duplicate`,
   `instantiate_prefab` and runtime `Instantiate(gameObject)` give copies fresh IDs; the editor also
   fixes empty/repeated IDs every Edit-mode frame (paste, presets). Save IDs are not prefab
   overrides. Spawned prefab instances are recorded by `Instantiate("….prefab")` (path + GUID from
   the asset database) with the Save IDs of their Saveables in Hierarchy order, so their states and
   script data map back after recreation. Destroyed objects = Save IDs present when the scene
   started but missing at save time; a reloaded scene starts with them, so they stay destroyed
   across save → load → save. `Instantiate(gameObject)` copies are not recreated (no prefab).
5. **Script data key** = script path (`path#2`… for repeats on one object); only scripts on
   Saveable objects get `OnSave`/`OnLoad` (they need an ID). Lua ↔ JSON: arrays for 1..n tables,
   Vector3 as `{"$type":"Vector3",…}`, functions dropped, depth capped at 64.
6. **Files.** `<slot>.save` (pretty JSON, or gzip when `SaveSystem.compress`, detected on load;
   written via a temp file + rename), `<slot>.info.json` (metadata so `ListSlots` doesn't parse whole
   saves), `<slot>.png` thumbnail. Folder: the editor's `Library/Saves` (outside version control);
   built games call `save::set_folder_override(player_folder(company, product))` at startup, next to
   PlayerPrefs. The folder is looked up at each call, so the first scene (started before the player
   opens prefs) uses the right folder too. Dates are formatted in UTC (no time-zone database).
7. **Versioning.** `format` (engine layout, newer = refused) and `version` (the game's,
   `SaveSystem.version`). Older saves run Rust hooks (`save::register_migration`) then
   `SaveSystem.OnMigrate(data, fromVersion)`, and are stamped with the current version; newer ones
   load with a warning.
8. **Thumbnails** are requested (`save::request_thumbnail`) and written by whoever renders: the
   player captures its next frame, the editor reads back the Game view's target. Headless runs
   simply don't produce one.
9. **Session locale is process-wide** (`localization::session_locale`), String Tables and settings
   live with each game (`World.localization`). Reason: the locale must survive scene loads (the
   player and editor start a new `Runtime` per scene) while tables per game keep tests isolated.
   Editor Play calls `reset_session` so each session picks its startup locale like a launch; the
   player resets after opening PlayerPrefs so "Saved Preference" can work. Changing locale writes
   PlayerPrefs `Misty.Locale`.
10. **Startup modes**: Saved Preference (pref → system → default), System Language (Windows
    `GetUserDefaultLocaleName` via a direct kernel32 import, `LANG`/`LC_*` elsewhere), Default.
    Locale codes resolve exactly, else by language (`fr-CA` → `fr`). Empty locale list = every
    column of the tables (works without touching Project Settings).
11. **Lookup chain**: current locale → fallback → default; a key with no text anywhere returns the
    key (visible in the UI, never an empty label).
12. **Smart strings** are our own small format: `{0}`, `{name}`, `{{ }}`, `{n:plural:a|b|…}` with
    `{}` = the number and nested placeholders; plural rules per language (en/es/de…: one|other,
    fr/pt: 0–1 singular, ar: six forms, ru/uk/pl/cs…: three, ja/zh/ko…: one), generic two/three-form
    rules otherwise. Unknown formatters print the value.
13. **CSV**: RFC 4180 (quotes, doubled quotes, commas, CR/LF inside quotes), `Key,<locales…>[,Comment]`,
    exported with a UTF-8 BOM so spreadsheet apps read accents and Arabic; import merges (new keys
    and columns added, cells replaced, others kept).
14. **Localized Text only writes `UiText.text`** (refreshed after Awake, at the start and end of
    every frame, and immediately when Lua changes its key/args). It knows nothing about rendering,
    so the parallel rich-text work isn't affected. In Edit mode the Inspector writes the default
    locale's string when the key changes.
15. **Asset Tables** are the same `.strings` format with `"kind": "assets"`; `Localization.GetAsset`
    returns the path. `.strings` files are packed into builds like `Resources` (looked up by name at
    runtime) and scanned as JSON for the asset paths they reference.
16. **Windows are floating** (like Project Settings / Grid and Snap) instead of dock tabs, so
    `Tab::ON_DEMAND` and the layout files stay untouched (parallel branches add tabs there).
    Window ▸ General ▸ Save Data / Localization Tables; double-clicking a `.strings` opens the table.
17. **Right-to-left**: the UI Text renderer has no bidi/shaping and the built-in font (Ubuntu Light)
    has no Arabic glyphs, so the Arabic option shows the right strings but not rendered RTL —
    `Localization.isRightToLeft` is there for when the text renderer supports it.

## Progress

- Engine: `save` (Saveable, capture/apply, slots, compression, migration, thumbnails, play time),
  `localization` (settings, session locale, Localizer, String Tables + CSV, smart format, Localized
  Text), Lua `SaveSystem` / `Localization` / `GetComponent("LocalizedText"/"Saveable")`,
  `OnSave`/`OnLoad`/`OnLocaleChanged`, `Runtime::save_game`/`load_game`/`take_scene_reloaded`.
- Editor: Save Data window (slots, metadata, thumbnail, contents, delete with confirm), Localization
  Tables window (table picker, create, add key/locale, editable grid with missing cells highlighted,
  search, only-missing, remove, CSV import/export), Project Settings ▸ Localization, Inspectors for
  Saveable and Localized Text (key picker, args, per-locale preview), save thumbnails from the Game
  view, `MISTY_WINDOW` / `MISTY_LOCALE` / `MISTY_SAVE_SLOT` screenshot hooks.
- Player: saves folder per Player Settings, thumbnails, startup locale after PlayerPrefs.
- Sample: `SaveLocDemo.scene` (generator `sample_saveloc.rs`), `SaveLocMenu.lua`, `CoinCounter.lua`,
  `CubeMover.lua`, `Localization/SaveLocDemo.strings`, `Prefabs/SaveLocCoin.prefab`.
- Docs: `docs/scripting.md` ▸ Save games, Localization.
- Screenshots (`docs/screenshots/round14/`, all with `MISTY_SCENE=Assets/Scenes/SaveLocDemo.scene`):
  `saveloc_tables.png` (`MISTY_WINDOW=Localization Tables`), `saveloc_save_data.png`
  (`MISTY_PLAY=1 MISTY_SAVE_SLOT=demo MISTY_WINDOW=Save Data MISTY_CAPTURE_FRAME=90`),
  `saveloc_play_fr.png` (`MISTY_PLAY=1 MISTY_LOCALE=fr`).

### Left for later
- RTL layout/shaping and a font with Arabic glyphs (text rendering work).
- Multi-scene saves (only the active scene is saved/reloaded), `Instantiate(gameObject)` copies.
- Local time zone for slot dates; a Localized Asset component (Asset Tables are Lua-only).
