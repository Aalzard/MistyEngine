# Build Profiles, dedicated server, achievements, build report (round 21A, branch r21-buildprofiles)

Goal: build the same project several ways without editing settings between builds, ship a
headless server for multiplayer games, unlock achievements from Lua and see what makes a build
big.

## Build Profiles (File ▸ Build Profiles…, Ctrl+Shift+B)

`misty_engine::build_profile`. The old Build Settings window became a Unity 6 style Build
Profiles window (`crates/misty_editor/src/build_settings.rs`):

- Left: **Scene List** (the shared "Scenes In Build"), **Platforms** (Windows, Linux, macOS,
  Dedicated Server) and **Build Profiles** (custom). `● Active` marks the active profile.
  **＋ Add Build Profile** (pick a platform); right-click: Duplicate / Rename / Delete
  (platform profiles: Copy to New Profile); double-click renames.
- Right: the profile. **Switch Profile** makes it active (Ctrl+B and Play mode use the active
  one). Sections: Platform (Target combo + toolchain status), Scene List (Override Global Scene
  List, its own list with Add Open Scenes / reorder / remove), Scripting Defines, Texture
  Overrides (Compression, Max Texture Size), Quality (level the game starts on), Build Options
  (Development Build, Compress Data, Create .zip, Output Folder + Browse, shared Steam App ID).
  Build / Build To… / Build And Run, progress, log, and the build report.

Storage: platform profiles `ProjectSettings/BuildProfiles/<Windows|Linux|macOS|DedicatedServer>.buildprofile`,
custom ones `Assets/Settings/Build Profiles/<Name>.buildprofile` (JSON, scene paths saved as
GUIDs like Build Settings). A platform profile without a file starts from the old Build Settings
options (target, development, compression, zip). `BuildSettings.active_profile` names the active
profile; `BuildSettings.target` follows it so code that only knows Build Settings (asset bundles,
tests) keeps working. Double-clicking a `.buildprofile` in the Project window opens it.

How each override reaches the build (`build_pipeline/package.rs`):

| Override | In the build |
|---|---|
| Scene list | `BuildProfile::apply` gives the packed `BuildSettings.json` and the pack roots |
| Defines | `BuildInfo.defines` (profile + built-in `MISTY_STANDALONE_*`, `MISTY_SERVER`, `DEVELOPMENT_BUILD`); the player installs them at startup |
| Texture compression / max size | written into each packed texture's `.meta` as the platform's override (GUID kept, project files untouched), and the compressed blocks baked with those settings (`compress::bake_with`), so players find the same cache keys |
| Quality level | packed `ProjectSettings.quality.default_level` |
| Development / Compress / Zip | `BuildInfo.development`, pak LZ4, zip |
| Output folder | where Build goes; a picked folder is remembered by the profile |

Scripting defines in Lua: `--#if NAME` / `--#elif` / `--#else` / `--#endif` (with `!`, `&&`,
`||`) are resolved when a script loads (`build_profile::preprocess_lua`, lines left out become
blank so line numbers stay), plus `Application.HasDefine(name)`, `Application.isDedicatedServer`,
`Application.isEditor`. Play mode uses the active profile's defines plus `MISTY_EDITOR`.

CLI: `misty --build <project> [<out>] [--profile <name>]` (name case-insensitive; "server" and
"mac" work; without `<out>` the profile's Output Folder). Prints the report at the end.

## Dedicated server

`misty_player/src/server.rs`. A build of the Dedicated Server profile has `BuildInfo.server`;
the player (same binary) then never creates a winit event loop, a wgpu device or an audio device
(`Audio::new(root, None)`), opens build scene 0, starts the network server on the scene's Network
Manager (`--port` overrides the port) and calls `Game::frame` at the Target Frame Rate (60 Hz by
default) until `Application.Quit()`. Logs to stderr and Player.log, attaching to the parent
terminal or opening a console of its own (release players are windowed apps). `--headless`
(also `-batchmode`, `-nographics`) runs any build or project this way, e.g. as a headless client
with `--net-client`.

Stripped from server builds: textures, audio, video and their `.meta` files
(`build_profile::server_strips`), compressed textures, virtual geometry caches, icon and splash.
Meshes stay (colliders).

## Achievements

`misty_engine::achievements`: `AchievementBackend` trait (unlock / is_unlocked / clear /
unlocked), `set_backend`, a `LocalAchievements` JSON backend (editor: `Library/Achievements.json`;
built games: `achievements.json` in the per-user folder next to PlayerPrefs). Lua
`Achievements.Unlock(id)` (true the first time, logs "Achievement unlocked"), `IsUnlocked`,
`Clear`, `GetUnlocked`, `GetBackend`.

Steamworks: not done. The Rust options (`steamworks` / `steamworks-sys`) are FFI wrappers around
Valve's proprietary C++ SDK and need `steam_api64.dll` shipped; there is no pure-Rust,
permissively licensed client. The pluggable backend is where one would go, behind a cargo
feature.

## Build report

`misty_engine::build_report`. Every packed entry is counted with its raw and stored size
(`PakWriter::add_counted`) by type (Textures, Compressed Textures, Meshes, Audio, Video, Scenes,
Prefabs, Materials, Shaders, Scripts, Animation, Terrain, Fonts, Settings, Import Settings,
Other); the 20 largest are kept, plus executable / pak / folder totals, files stripped for a
server, warnings and time. Shown in the window after a build (types with a share bar, 10 largest
assets), saved as `<out>/BuildReport.txt` (left out of the zip) and
`Library/BuildReports/<Profile>.json` (the window reloads it), printed by the CLI.

## Samples

`SampleProject/Assets/Settings/Build Profiles/Demo.buildprofile` (Windows, FirstPerson + Vehicle
demos, `DEMO`, 512 px textures, Low quality, zip, `Builds/Demo`) and `Net Server.buildprofile`
(Dedicated Server, NetworkDemo, `Builds/Server`).

## Tests

- engine `build_profile::tests`: save / load of platform and custom profiles with every override,
  duplicate / rename / delete, apply, texture override metas, Lua `--#if`, server stripping.
- engine `build_report::tests`, `achievements::tests`, `scripting::tests::achievements_and_scripting_defines`.
- editor `build_pipeline::package::tests::profile_overrides_reach_the_build_output` (scenes,
  defines, BC7 at ≤ 64 px with the overridden meta, quality level, compression off, report) and
  `server_builds_strip_visuals_and_audio`.
- editor `build_settings::tests` (window: list, switch, add / edit / duplicate / rename / delete,
  own scene list), `cli_tests`.
- player `tests/dedicated_server.rs`: lays out a server build around the real `misty_player`,
  starts it with no arguments, reads its port, connects a second, headless client process and
  checks both sides (and that no renderer or window started).

## Checked by hand

- `misty --build SampleProject <out> --profile "net server"`: 22 assets from NetworkDemo, 2
  visual / audio files stripped, defines `MISTY_STANDALONE_WIN;MISTY_SERVER;DEVELOPMENT_BUILD`,
  report printed. Running the result with `--port 0`, then a second copy with
  `--headless --net-client 127.0.0.1:<port>`:

  ```
  Dedicated server: …\Misty Sample_Data (pak) 1.0 · no window, audio or GPU
  Server listening on port 60284
  Network: client 1 joined
  ```
  and on the client `Network: connected as client 1`.
- `misty --build SampleProject <out> --profile demo`: the Demo profile's two scenes, `Texture
  overrides: … max size 512`, `Quality level: Low`, defines `MISTY_STANDALONE_WIN;DEMO`, zip.

## Screenshots

`docs/screenshots/r21/build-profiles-demo.png` (the Demo profile), `build-profiles-report.png`
(its options and build report, `MISTY_BUILD_SCROLL`), `build-profiles-server.png` (Net Server).
Editor env: `MISTY_BUILD=1 MISTY_BUILD_PROFILE=<name> [MISTY_BUILD_SCROLL=<px>]
[MISTY_BUILD_REPORT=<report.json>]`.

## Not done

- Steam achievements backend (see above).
- The server is the regular player binary run headless (winit / wgpu / kira are linked but never
  started), not a separately compiled player without them.
- Server builds still copy local asset bundles (they may hold prefabs scripts load).
- Texture overrides don't reach textures the renderer assembles from several images (terrain
  layer arrays, tree and grass atlases); those keep the textures' own settings.
- No per-profile Player Settings (product name, icon) overrides.
