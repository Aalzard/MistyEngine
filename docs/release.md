# Releasing Misty Engine and how auto-update works

Misty Engine updates itself from the GitHub releases of
**https://github.com/Aalzard/MistyEngine** (releases page:
https://github.com/Aalzard/MistyEngine/releases). Everyone who has Misty Engine installed
gets the newest release with one click, so people don't end up on different versions.

## How auto-update works

1. **Check.** When the editor or the Hub starts, a background thread asks
   `https://api.github.com/repos/Aalzard/MistyEngine/releases/latest` — at most once every 6
   hours (the time and the answer are cached in `%APPDATA%\MistyEngine\updater.json`).
   Help ▸ **Check for Updates…** always asks again. Short timeouts; nothing blocks the window.
   Offline, a repository without releases (404) or any other error = no message at all.
2. **Compare.** The release tag (`v0.0.2`) is compared with the running version (the
   workspace `version` in `Cargo.toml`) as a semantic version. Drafts and pre-releases are
   ignored. A version the user chose to **Skip** is not offered again automatically (a manual
   check still shows it).
3. **Offer.** A small message box: "New version 0.0.2 is available" with the release notes and
   **Update now / Later / Skip this version**, plus a green **⬆ Update** button in the editor
   toolbar (right side) and in the Hub. *Later* closes the box but keeps the button; *Skip*
   hides both until a newer version comes out.
4. **Install** (Windows). In the editor, modified scenes get the usual Save / Don't Save /
   Cancel prompt first. Then:
   - download `MistyEngine-vX.Y.Z-windows-x64.zip` and its `.sha256.txt` (progress bar);
   - verify the SHA-256 — **a mismatch refuses the install** and nothing changes;
   - extract to a temporary staging folder;
   - swap files into the install folder (the folder of the running exe): each replaced `.exe`
     is renamed to `<name>.old.exe` (Windows lets a running exe be renamed, not overwritten),
     other replaced files are moved to `.misty-update-backup\`, then the new files move in.
     Only files that are in the package are touched; user projects are never touched, and files
     that already exist under `Examples\` are kept (only new example files are added);
   - if anything fails, every step is undone (old exe restored) and a clear message shows;
   - start the new `MistyEngine.exe` with the same arguments and close. On the next start the
     leftover `*.old.exe` and backup folder are deleted.
5. **Can't install in place?** If the install folder isn't writable (e.g. under
   `C:\Program Files` without admin rights), or the release has no Windows zip or no checksum
   file, the updater shows **Download the new version** with a button that opens the release
   page in the browser.

Security: HTTPS only; downloads only from `api.github.com`, `github.com`,
`objects.githubusercontent.com` and `release-assets.githubusercontent.com` (every redirect hop
is checked); the package must match its published SHA-256; the only program ever started is
the `MistyEngine.exe` the package placed in the install folder. Requests send the User-Agent
`MistyEngine-Updater/<version>` and nothing else about the user.

## How to publish a release

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`), e.g. `0.0.2`, and commit.
2. Make the package — one command from the repository root (Windows PowerShell 5.1 or 7):

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\make_release.ps1
   ```

   It builds `misty_editor` and `misty_player` in release, then writes to `..\MistyRelease\`
   (next to the repository; `-OutDir` changes it, `-TargetDir` uses another cargo target folder,
   `-SkipBuild` packages the exes already built). Details in **The package** below.
3. Look at the result: unzip the zip somewhere new and do the smoke test below.
4. Commit, then tag the release commit: `git tag -a v0.0.2 -m "Misty Engine v0.0.2"`.
5. Push the commit and the tag.
6. On https://github.com/Aalzard/MistyEngine/releases click **Draft a new release**, pick the
   tag **`v0.0.2`**, write the release notes (they appear in the message box), upload the zip and its
   `.sha256.txt` as release assets and **Publish** (not as a pre-release). The repository must be public for
   the API to see it.

Within 6 hours (or right away via Help ▸ Check for Updates…) every install older than 0.0.2
offers the update.

## The package

`tools/make_release.ps1` makes, for the workspace version `X.Y.Z` in `Cargo.toml`:

```
MistyRelease\
  MistyEngine-vX.Y.Z-windows-x64\
    MistyEngine.exe          the editor and Hub (misty.exe renamed; the updater relaunches this name)
    misty_player.exe         File > Build copies it into games; it must sit next to MistyEngine.exe
                             (an editor outside the engine source always uses this one)
    Examples\SampleProject\  `git archive HEAD SampleProject` (committed files only: uncommitted
                             edits never ship), without Library\ or Builds\
    README.txt               tools/release/README.txt ({VERSION} filled in)
    CHANGELOG.txt            tools/release/CHANGELOG.txt (add the new version at the top)
    LICENSE.txt              LICENSE + RUNTIME-EXCEPTION.md
  MistyEngine-vX.Y.Z-windows-x64.zip            the folder above inside the zip (bsdtar, deflate)
  MistyEngine-vX.Y.Z-windows-x64.zip.sha256.txt "<sha256>  <zip name>"
```

Nothing else is needed at runtime: shaders, fonts, icons, templates, tutorials and the Ctrl+K
guides are built into the exes (the guides are written to the temp folder when opened). Both exes
carry the Misty Engine icon and version info (`crates/misty_editor/build_icon.rs`, needs the
Windows SDK's `rc.exe`; check Properties ▸ Details of `MistyEngine.exe` shows the version).

### Smoke test

From the unzipped folder, somewhere outside the repository, with a throwaway Hub config
(`$env:MISTY_HUB_CONFIG = "$env:TEMP\misty-smoke\hub.json"`, so your recent list is untouched):

1. `MISTY_SCREENSHOT=hub.png MistyEngine.exe` — the Hub.
2. `MISTY_SCREENSHOT=new.png MistyEngine.exe --new Standard %TEMP%\misty-smoke\SmokeProject` —
   the editor on a new Standard project.
3. `MISTY_SCREENSHOT=clouds.png MISTY_SCENE=Assets/Scenes/CloudsDemo.scene MistyEngine.exe
   Examples\SampleProject` — a demo scene renders.
4. Build the SmokeProject: File ▸ Build Settings ▸ Build (or, without clicking, the ignored test
   `real_build` with `MISTY_BUILD_PROJECT` / `MISTY_BUILD_OUT`; it uses the repository's player), then
   run the game exe from the build folder with `MISTY_SCREENSHOT=game.png`.


### The installer

`make_release.ps1` also writes `MistyEngine-Setup-v<version>-windows-x64.exe` with Inno Setup 6
(`tools/installer/MistyEngine.iss`; install Inno Setup from https://jrsoftware.org/isinfo.php, or pass
`-NoInstaller`). It installs per user to `%LOCALAPPDATA%\Programs\Misty Engine` (no admin prompt) with a
Start menu shortcut, an optional desktop shortcut and an uninstaller. Upload it to the release next to the
zip; the updater keeps using the zip.

## Changing the repository / testing

- The repository is the constant `REPO` in `crates/misty_editor/src/updater/mod.rs`
  (`"Aalzard/MistyEngine"`). Set it to another `owner/name` if the project moves; a placeholder
  like `OWNER/MistyEngine` turns the updater off silently.
- `MISTY_UPDATE_REPO=owner/name` follows another repository without rebuilding.
- `MISTY_UPDATE_URL=<url>` reads a JSON shaped like GitHub's latest release from anywhere
  (plain `http://` is allowed only for `127.0.0.1` / `localhost` test servers); its host is
  then allowed for downloads too.
- `MISTY_UPDATER_DIR=<folder>` keeps `updater.json` there instead of `%APPDATA%\MistyEngine`.
- `MISTY_FAKE_UPDATE=0.0.2` pretends that version is available (screenshots; no network, and
  Update now only shows the release-page fallback).

## Hub hook

There is one `updater::Updater` per process: the launcher makes it at startup
(`Launcher::updater`: `Updater::from_env()` then `start(ctx)`) and hands it Hub → editor → Hub.
The Hub draws `updater::ui::update_button` next to the version in its sidebar footer and calls
`updater::ui::windows` each frame; either one returning true calls `updater.install()` (the Hub
has no scenes to save).
