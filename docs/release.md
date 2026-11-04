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
2. Build the release: `cargo build --release -p misty_editor -p misty_player`.
3. Make the package folder `MistyEngine-v0.0.2-windows-x64\` with `MistyEngine.exe` (the
   editor, renamed from `misty.exe`), `misty_player.exe` and anything else that ships
   (e.g. `Examples\`). The exe must be called `MistyEngine.exe` at the top of the package.
4. Zip it as **`MistyEngine-v0.0.2-windows-x64.zip`** (the files may sit at the top of the zip
   or inside that one folder; deflate or stored).
5. Write the checksum file **`MistyEngine-v0.0.2-windows-x64.zip.sha256.txt`**. In PowerShell:

   ```powershell
   $zip = "MistyEngine-v0.0.2-windows-x64.zip"
   (Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() + "  $zip" | Out-File -Encoding ascii "$zip.sha256.txt"
   ```

   (`sha256sum MistyEngine-v0.0.2-windows-x64.zip > MistyEngine-v0.0.2-windows-x64.zip.sha256.txt`
   works too — the first 64-hex-digit word is the hash.)
6. On https://github.com/Aalzard/MistyEngine/releases click **Draft a new release**, create the
   tag **`v0.0.2`**, write the release notes (they appear in the message box), upload both files
   as release assets and **Publish** (not as a pre-release). The repository must be public for
   the API to see it.

Within 6 hours (or right away via Help ▸ Check for Updates…) every install older than 0.0.2
offers the update.

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

The Hub owns an `updater::Updater` like the editor (`Updater::from_env()` then `start(ctx)`);
in its sidebar footer it draws `updater::ui::update_button(ui, &updater)` and each frame calls
`updater::ui::windows(ctx, &mut updater)`; when either returns true it calls
`updater.install()` (the Hub has no scenes to save).
