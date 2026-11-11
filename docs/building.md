# Building and shipping your game

This guide takes you from "it plays in the editor" to "people can download it". You don't need
to have shipped a game before.

## The short version

1. **File ▸ Build Profiles…** (Ctrl+Shift+B). Click **Scene List** at the top left, then **Add
   Open Scenes** for every scene the game uses. The first scene in the list is the one the game
   starts with.
2. Pick a platform profile on the left (Windows, Linux, macOS, Dedicated Server) and its
   **Target** (your own computer is marked "this PC").
3. Click **Build**, choose an empty folder *outside* your project (for example `Builds/Windows`),
   and wait for the progress bar. The profile remembers the folder as its **Output Folder**.
   The first build compiles the engine and takes a few minutes; later builds are much faster.
4. Open the folder and double-click the game. **Build And Run** does steps 3–4 in one go.
   Ctrl+B builds and runs the **active** profile (**Switch Profile** makes the shown one active).

## Before your first real build

Open **Edit ▸ Project Settings…** and look at two pages.

### Player

| Setting | What it does |
|---|---|
| Product Name | The game's name: window title, `.exe` name, save-data folder. |
| Company Name | Your name or studio. Saved data goes in `<Company>/<Product>` in the player's user folder. |
| Version | Shown in the build log and zip name, e.g. `1.0`. Bump it for every release. |
| Icon | A square PNG from your Assets, 256×256 or bigger. On Windows it becomes the `.exe` icon; on every platform it is the window icon. |
| Screen Mode | **Fullscreen Window** (recommended: borderless, quick Alt+Tab), **Exclusive Fullscreen** (the monitor switches resolution), **Windowed** or **Maximized Window**. |
| Default Resolution | The window size in Windowed mode, and the resolution Exclusive Fullscreen uses. |
| Resizable Window | Players can drag the window's edges. |
| Run In Background | Keep playing when the window isn't focused. Off = the game pauses (good for single-player games). |
| VSync | Wait for the monitor's refresh: smooth, no tearing. Leave it on unless you have a reason. |
| Target Frame Rate | Only when VSync is off: a frame cap (e.g. 60). `-1` = as fast as possible. |
| Splash Screen | Optional: your logo for a couple of seconds before the first scene. |

Players can change the screen mode and resolution from your own options menu with
`Screen.SetResolution(width, height, FullScreenMode.Windowed)`; their choice is remembered.

### Quality

Quality levels (Low, Medium, High, Ultra) are graphics presets. Players start on the
**Default** level; your options menu can switch with `QualitySettings.SetQualityLevel(i)` and the
choice is remembered. Each level has:

- **Shadows** — resolution, distance, cascades, soft shadows.
- **Rendering** — anti-aliasing, render scale (draw at a lower resolution on slow PCs), LOD bias.
- **Textures** — texture quality (half resolution saves ¾ of the texture memory), anisotropic
  filtering, a maximum texture size.
- **Effects** — ambient occlusion, volumetric fog, reflection probes.

The dropdown in the Scene Viewer's toolbar previews a level in the editor.

## Build Profiles

Each profile is a set of build options saved as an asset: the platform profiles in
`ProjectSettings/BuildProfiles/`, your own in `Assets/Settings/Build Profiles/` (commit them).
**＋ Add Build Profile** makes one for a platform; right-click a profile to **Duplicate**,
**Rename** (or double-click it) or **Delete** it. A "Demo" profile could build only the first
levels with a `DEMO` define, smaller textures and the Low quality level.

| Setting | What it does |
|---|---|
| Target | Operating system and CPU. |
| Override Global Scene List | Build this profile's own scenes instead of the shared **Scene List**. |
| Scripting Defines | Names scripts can test: `--#if DEMO` … `--#else` … `--#endif` blocks in Lua (with `!`, `&&`, `\|\|`), and `Application.HasDefine("DEMO")`. Built in: `MISTY_STANDALONE_WIN` / `_LINUX` / `_OSX`, `MISTY_SERVER`, `DEVELOPMENT_BUILD` (builds) and `MISTY_EDITOR` (Play mode, which uses the active profile's defines). |
| Texture Overrides | **Compression** for every compressed texture (None turns compression off everywhere) and a **Max Texture Size** cap. The project's textures are not changed; only the build's copy. |
| Quality Level | The level the built game starts on. |
| Development Build, Compress Data, Create .zip | As below, per profile. |
| Output Folder | Where Build puts the game, absolute or relative to the project. |

From a terminal or CI:

```
misty --build <project folder> <output folder> --profile "Dedicated Server"
misty --build <project folder> --profile Demo       # into the profile's Output Folder
```

Without `--profile` the active profile builds. The build report is printed at the end.

## Dedicated servers

The **Dedicated Server** profile builds a headless player for Windows or Linux: no window, no
audio device and no GPU. It opens the first scene, starts the network server on the scene's
Network Manager and updates at the Target Frame Rate (60 per second when none is set). Textures,
audio and video are left out of the build, and so are the icon and compressed textures.

```
MyGame_Server --port 7777          # override the Network Manager's port
MyGame --headless --net-client 203.0.113.5:7777   # a headless client, for bots and tests
```

The server logs `Server listening on port N` and every client that joins, to the terminal (or a
console window of its own) and `Player.log`. `Application.isDedicatedServer` and the
`MISTY_SERVER` define let scripts skip work only players need. `Application.Quit()` stops it.

## Build report

After every build the window shows the build's size by asset type (textures, compressed
textures, meshes, audio, scenes, scripts…) and its 10 largest assets. The full report is saved
as `BuildReport.txt` in the build folder (left out of the .zip) and as
`Library/BuildReports/<Profile>.json` in the project, so the window shows it again later.

## Achievements

```lua
Achievements.Unlock("FIRST_WIN")       -- true the first time
if Achievements.IsUnlocked("FIRST_WIN") then ... end
Achievements.Clear("FIRST_WIN")        -- lock it again while testing
```

Unlocks are stored locally: `Library/Achievements.json` in the editor, `achievements.json` in the
player's user folder (next to PlayerPrefs) in a built game. The storage is pluggable
(`misty_engine::achievements::AchievementBackend`), so a store integration can replace it;
there is no Steam backend yet, because no pure-Rust Steamworks client exists to build it on.

## What ends up in the build folder

```
MyGame.exe              the game (named after Product Name, with your icon)
MyGame_Data/
    data.pak            your scenes and every asset they use, plus the settings
    icon.png / .ico     the icon
Player.log              only for Development Builds (see below)
BuildReport.txt         sizes by asset type and the largest assets
```

Only what the game needs is packed: the scenes in the build list, everything they reference
(materials, textures, models, sounds, scripts, prefabs, animations, nav meshes…), and anything
inside a folder called **Resources** (for files a script finds by name). Unused assets stay out,
so the download stays small. The build log lists references to files that no longer exist.

Ship the **whole folder**: the `.exe` alone won't run.

## Build options

- **Development Build** — the game opens a console window with its log, and writes `Player.log`
  next to the `.exe`. Use it while testing; turn it **off** for anything you publish. Release
  builds write their log to the user's folder instead (see "Where are saves and logs?").
- **Compress Data** — makes `data.pak` smaller (LZ4). Leave it on.
- **Create .zip** — also writes `MyGame-1.0-windows-x64.zip` next to the build folder, ready to
  upload to itch.io, Game Jolt or a website.
- **Steam App ID** — see "Shipping on Steam".

## Platforms

Misty compiles the game with Rust's `cargo`. Each platform is a Rust *target*. The Build window
shows ✔ when a target is ready and ⚠ with the exact fix when it isn't.

| Platform | Rust target | Build it on |
|---|---|---|
| Windows x64 | `x86_64-pc-windows-msvc` | Windows. Needs Visual Studio's "Desktop development with C++" (or the Build Tools). |
| Windows ARM64 | `aarch64-pc-windows-msvc` | Windows, plus the Visual Studio component "MSVC … ARM64 build tools". |
| Linux x64 | `x86_64-unknown-linux-gnu` | Linux (or WSL). From Windows: install zig and `cargo install cargo-zigbuild`. |
| Linux ARM64 | `aarch64-unknown-linux-gnu` | Linux on ARM, or with cargo-zigbuild. Covers Raspberry Pi 4/5 (64-bit OS). |
| macOS (Apple Silicon) | `aarch64-apple-darwin` | A Mac with Xcode. |
| macOS (Intel) | `x86_64-apple-darwin` | A Mac with Xcode (either kind of Mac builds both). |

If a target is missing, the window shows the command, e.g.:

```
rustup target add aarch64-pc-windows-msvc
```

Run it in a terminal, then press **Check again**.

**Why can't I build a Mac game on Windows?** Apple's SDK and code signing only exist on macOS.
Borrow a Mac, or use a cloud CI service with Mac machines. Linux builds from Windows work with
cargo-zigbuild, or build inside WSL.

macOS builds are an app bundle (`MyGame.app`). To share them without warnings, Apple requires
signing and notarizing with a paid developer account (`codesign` and `xcrun notarytool` on the
Mac). Unsigned apps open with right-click ▸ Open. On Linux, make sure the game file is
executable (`chmod +x MyGame`); zips made by Misty already mark it so.

## Shipping on itch.io (easiest)

1. Make a release build (Development Build off) with **Create .zip** on.
2. Create a project on itch.io, upload the zip, and tick the platform it's for
   (Windows / Linux / macOS).
3. One zip per platform. That's it. (itch's `butler` tool can upload new versions from the
   command line: `butler push MyGame-1.0-windows-x64.zip you/mygame:windows`.)

## Shipping on Steam

You need a Steamworks partner account (one-time fee per game) and an App ID from it.

1. Type the App ID into **Steam App ID** in the Build window and build.
2. The build folder gets a `steam/` folder with:
   - `app_build_<AppID>.vdf` and `depot_build_<DepotID>.vdf` — upload scripts already pointing at
     your build folder,
   - `README_STEAM.md` — the exact `steamcmd` command and the clicks in Steamworks.
3. Development Builds also get `steam_appid.txt` next to the `.exe`, which lets Steam features
   find your App ID when you start the game yourself. Release builds leave it out (Steam starts
   them) and the upload scripts never upload it.
4. Install SteamCMD, run the command from the README, then set the build live in Steamworks
   (try a private `beta` branch first).

Misty doesn't include the Steamworks SDK yet, so achievements, the overlay API and
workshop aren't available from scripts. You can still sell and update the game on Steam.

## Where are saves and logs?

`PlayerPrefs` (and the player's chosen quality and resolution) are saved per user:

- Windows: `%APPDATA%\<Company>\<Product>\prefs.json`
- macOS: `~/Library/Application Support/<Company>/<Product>/prefs.json`
- Linux: `~/.config/<Company>/<Product>/prefs.json`

Release builds write `Player.log` in the same folder. When a player reports a crash, ask for it.

## Troubleshooting

- **"the Rust target … isn't installed"** — run the `rustup target add …` command shown.
- **Linker errors (`link.exe` not found, `cc` failed)** — install the C++ build tools for that
  platform (Visual Studio Build Tools on Windows, `build-essential` on Linux, Xcode on Mac).
- **The game starts with a black screen** — check that the first scene in Build Settings has a
  camera, and read `Player.log`.
- **Something is missing in the build** — look for "is missing (used by …)" warnings in the build
  log. Files loaded by name from scripts must be inside a `Resources` folder.
- **I want to play the project without building** — run
  `cargo run -p misty_player --release -- path/to/Project`; loose project folders still work.
