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
5. **Linux.** The same steps with `MistyEngine-vX.Y.Z-linux-x64.tar.gz` and its
   `.sha256.txt` (`updater::release::Platform::HOST` picks the package per OS: Windows takes the
   `-windows-x64.zip`, Linux x64 the `-linux-x64.tar.gz`; never the Setup `.exe`). The tarball
   is unpacked in-process (tar crate: files and folders only, no links, no `..`), executable bits
   are kept, and the running binary (no `.exe`) goes to the backup folder like any other file,
   since Linux lets a running program be renamed. The new `MistyEngine` is started. Other
   systems (macOS, Linux ARM64) get the release-page fallback below.
6. **Can't install in place?** If the install folder isn't writable (e.g. under
   `C:\Program Files` without admin rights, or `/opt`), or the release has no package for this
   OS or no checksum file, the updater shows **Download the new version** with a button that
   opens the release page in the browser.

Security: HTTPS only; downloads only from `api.github.com`, `github.com`,
`objects.githubusercontent.com` and `release-assets.githubusercontent.com` (every redirect hop
is checked); the package must match its published SHA-256; the only program ever started is
the `MistyEngine.exe` the package placed in the install folder. Requests send the User-Agent
`MistyEngine-Updater/<version>` and nothing else about the user.

## How to publish a release

Pushing a version tag is the release: GitHub Actions builds the Windows and Linux packages from
the tag, signs the Windows files (once SignPath is set up, see **Code signing** below) and
uploads everything to a **draft** release. You look at it, then publish. No file from your own
computer goes into a release.

1. Bump `version` in the root `Cargo.toml` (`[workspace.package]`), e.g. `0.0.2`, add the
   version's section at the top of `tools/release/CHANGELOG.txt` (it becomes the release notes)
   and commit.
2. Make a local test build and do the smoke test below — one command from the repository root
   (Windows PowerShell 5.1 or 7):

   ```powershell
   powershell -ExecutionPolicy Bypass -File tools\make_release.ps1
   ```

   It builds `misty_editor` and `misty_player` in release, then writes to `..\MistyRelease\`
   (next to the repository; `-OutDir` changes it, `-TargetDir` uses another cargo target folder,
   `-SkipBuild` packages the exes already built). Details in **The package** below. This build
   is unsigned and is **not** what gets published.
   Optional: Actions ▸ **Release (Windows)** ▸ **Run workflow** on the branch is a trial run of
   the release build (nothing is uploaded to a release), and pushing the commit to a `ci/*`
   branch (`git push github HEAD:ci/linux`) or running **CI (Linux)** by hand builds and runs
   `cargo test --workspace --release` on Linux (the GPU tests skip there: no adapter).
3. Tag the release commit: `git tag -a v0.0.2 -m "Misty Engine v0.0.2"`. Push the commit and
   the tag. The tag must equal `v<Cargo.toml version>`; both workflows stop otherwise.
4. **Windows** — `.github/workflows/release-windows.yml` (windows-latest, GitHub-hosted):
   - creates the release for the tag as a **draft**, with the tag's CHANGELOG section as notes
     (if you already made the release by hand, it is used as it is);
   - builds with `cargo build --release --locked`, then, when SignPath is set up: sends
     `MistyEngine.exe` and `misty_player.exe` to SignPath and **waits for you to approve the
     signing request** in SignPath; builds the zip and the installer from the signed exes with
     `tools/make_release.ps1 -ExeDir`; sends the installer to SignPath and waits for the
     **second approval**; writes the `.sha256.txt` files last;
   - uploads the zip, the installer and their `.sha256.txt` files to the release
     (`gh release upload --clobber`). The run's summary page lists every file with its
     signature and checksum.

   Without the SignPath settings the same job runs without the signing steps and uploads
   unsigned files. An approval that is not given within 2 hours (repository variable
   `SIGNPATH_WAIT_SECONDS`) fails the job: re-run it, or Actions ▸ Release (Windows) ▸
   **Run workflow** on the tag.
5. **Linux** — `.github/workflows/release.yml` (ubuntu-22.04) checks the tag, builds, packages
   with `tools/make_release_linux.sh`, waits up to 45 minutes for the release of that tag (the
   draft from step 4 counts) and uploads `MistyEngine-vX.Y.Z-linux-x64.tar.gz` and its
   `.sha256.txt` with `gh release upload --clobber`. If no release appears in time it creates a
   published one with the tag's CHANGELOG section as notes. Actions ▸ Release (Linux) ▸
   **Run workflow** with the tag re-uploads for an existing tag.
6. When both workflows are green: on https://github.com/Aalzard/MistyEngine/releases open the
   draft, download the Windows zip or installer and do the smoke test once more on the real
   files (and check the signature, see below), tidy the notes (they appear in the update message
   box) and **Publish** (not as a pre-release). The repository must be public for the API to
   see it. Nobody is offered the update while the release is a draft.

Within 6 hours of publishing (or right away via Help ▸ Check for Updates…) every install older
than 0.0.2 offers the update.

## Code signing

The Windows exes and the installer are signed through SignPath with a certificate from SignPath
Foundation; Windows shows "SignPath Foundation" as the publisher. The public policy is
[code-signing-policy.md](code-signing-policy.md); the SignPath settings are in
`tools/release/signpath/` (with a README); first-time setup is in
[pending/r36-signing.md](pending/r36-signing.md). **Until that setup is done, releases are
unsigned.**

- Signing is **on** when the repository has the secret `SIGNPATH_API_TOKEN` and the variable
  `SIGNPATH_ORGANIZATION_ID`; the other names and their defaults are listed at the top of
  `release-windows.yml`. With neither set the release is unsigned; with only one the job stops.
- Only GitHub Actions can ask for a signature, and only for files built in that same run on a
  GitHub-hosted runner; SignPath checks this. A local build cannot be signed.
- Every signing request needs a manual approval in SignPath: two per release (the exes, then
  the installer). If signing fails, the job fails and no Windows file is uploaded: there is
  never a half-signed release.
- Order: sign the exes → zip and installer from the signed exes → sign the installer →
  checksums. Changing a file after it is signed breaks its signature, so nothing touches the
  files afterwards.
- Check a signed file: Properties ▸ Digital Signatures, or

  ```powershell
  Get-AuthenticodeSignature .\MistyEngine.exe | Format-List Status, SignerCertificate
  signtool verify /pa /v .\MistyEngine-Setup-vX.Y.Z-windows-x64.exe   # Windows SDK
  ```
- Game builds: File ▸ Build copies the signed `misty_player.exe`. A build with its own icon
  rewrites the exe's resources, which removes the signature (the game exe is then unsigned, as
  before); a build without an icon keeps the Misty Engine signature.

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
  MistyEngine-Setup-vX.Y.Z-windows-x64.exe            the installer (see below)
  MistyEngine-Setup-vX.Y.Z-windows-x64.exe.sha256.txt "<sha256>  <installer name>"
```

Other switches, used by the release workflow: `-ExeDir <folder>` packages ready
`MistyEngine.exe` and `misty_player.exe` (the signed ones) instead of building; `-Strict` turns
warnings into errors (no Inno Setup, wrong version info); `-HashOnly` only rewrites the
`.sha256.txt` files (after the installer is signed).

The Linux package (`tools/make_release_linux.sh`, run by the release workflow; it also works on
any Linux machine with the system libraries below) has the same layout without `.exe`:

```
MistyEngine-vX.Y.Z-linux-x64/
  MistyEngine              the editor and Hub (misty renamed), mode 755
  misty_player             mode 755; must sit next to MistyEngine
  Examples/SampleProject/  as above
  README.txt               tools/release/README-linux.txt ({VERSION} filled in)
  CHANGELOG.txt, LICENSE.txt
MistyEngine-vX.Y.Z-linux-x64.tar.gz             GNU tar + gzip, owner root:root, sorted names
MistyEngine-vX.Y.Z-linux-x64.tar.gz.sha256.txt  "<sha256>  <tar.gz name>" (sha256sum)
```

Building it needs `pkg-config libasound2-dev libudev-dev` (alsa-sys and libudev-sys link
against them); winit, xkbcommon and Vulkan are loaded at runtime. The binaries link
`libasound.so.2`, `libudev.so.1` and glibc 2.35 (ubuntu-22.04), so they run on Ubuntu 22.04,
Debian 12, Fedora 36 and newer.

Nothing else is needed at runtime: shaders, fonts, icons, templates, tutorials and the Ctrl+K
guides are built into the exes (the guides are written to the temp folder when opened). Both exes
carry the Misty Engine icon and version info (`crates/misty_editor/build_icon.rs`, needs the
Windows SDK's `rc.exe`; check Properties ▸ Details of `MistyEngine.exe` shows the version). The
product name "Misty Engine" and the product version are what the signing service checks: keep
them as they are.

### Smoke test

From the unzipped folder, somewhere outside the repository, with a throwaway Hub config
(`$env:MISTY_HUB_CONFIG = "$env:TEMP\misty-smoke\hub.json"`, so your recent list is untouched):

1. `MISTY_SCREENSHOT=hub.png MistyEngine.exe` — the Hub.
2. `MISTY_SCREENSHOT=new.png MistyEngine.exe --new "Floating Platform" %TEMP%\misty-smoke\SmokeProject`
   — the editor on a new Floating Platform project.
3. `MISTY_SCREENSHOT=clouds.png MISTY_SCENE=Assets/Scenes/CloudsDemo.scene MistyEngine.exe
   Examples\SampleProject` — a demo scene renders.
4. Build the SmokeProject: File ▸ Build Settings ▸ Build (or, without clicking, the ignored test
   `real_build` with `MISTY_BUILD_PROJECT` / `MISTY_BUILD_OUT`; it uses the repository's player), then
   run the game exe from the build folder with `MISTY_SCREENSHOT=game.png`.


### The installer

`make_release.ps1` also writes `MistyEngine-Setup-v<version>-windows-x64.exe` with Inno Setup 6
(`tools/installer/MistyEngine.iss`; install Inno Setup from https://jrsoftware.org/isinfo.php, or pass
`-NoInstaller`). It installs per user to `%LOCALAPPDATA%\Programs\Misty Engine` (no admin prompt) with a
Start menu shortcut, an optional desktop shortcut and an uninstaller. The release workflow uploads it next
to the zip; the updater keeps using the zip. The uninstaller Inno Setup writes into the install folder is
not signed (it is made on the user's computer, not downloaded).

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
