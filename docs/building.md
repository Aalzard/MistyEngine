# Building and shipping your game

This guide takes you from "it plays in the editor" to "people can download it". You don't need
to have shipped a game before.

## The short version

1. **File ▸ Build Settings…** (Ctrl+Shift+B). Click **Add Open Scenes** for every scene the game
   uses. The first scene in the list is the one the game starts with.
2. Pick a **Platform** on the left (your own computer is marked "this PC").
3. Click **Build**, choose an empty folder *outside* your project (for example `Builds/Windows`),
   and wait for the progress bar. The first build compiles the engine and takes a few minutes;
   later builds are much faster.
4. Open the folder and double-click the game. **Build And Run** does steps 3–4 in one go, using
   the last folder.

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

The dropdown in the Game view's toolbar previews a level in the editor.

## What ends up in the build folder

```
MyGame.exe              the game (named after Product Name, with your icon)
MyGame_Data/
    data.pak            your scenes and every asset they use, plus the settings
    icon.png / .ico     the icon
Player.log              only for Development Builds (see below)
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
