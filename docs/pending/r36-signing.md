# Round 36: Windows code signing through SignPath Foundation

Goal: the Windows installer and exes stop showing "Windows protected your PC / unknown
publisher". Route: the free open source program of SignPath Foundation. The repository side is
ready on branch `r36-signing`; **nothing has been applied for, no account exists, no secret is
set, and nothing was pushed.** Everything below marked **You** is yours to do.

Researched on 2026-11-11. Sources are quoted with their URLs; what could not be checked is
listed at the end.

## 1. What SignPath Foundation requires

From <https://signpath.org/terms> (the page calls itself a draft code of conduct):

| Requirement | Where Misty Engine stands |
| --- | --- |
| No malware or unwanted programs | Fine. |
| "OSI-approved Open Source license without commercial dual-licensing for all components" | GPL-3.0-or-later. The runtime exception is an *additional permission* under GPL section 7, not a second license. Say so in the application. |
| "may not contain any proprietary, non open-source component" | **You confirm:** every bundled asset (SampleProject textures, HDRIs, models, sounds, fonts, template content) is under an open license. The Rust dependencies are open source. |
| "actively maintained" | Yes. |
| "must already be released in the form that should be signed" | Yes: v0.3.2 has the Setup exe and the zip on GitHub Releases. |
| "functionality must be described on its download page" | README (the repository home page) describes it; release notes come from the CHANGELOG. |
| Sign only your own project and binaries built from your own source | The two exes are built from this repository. The installer is made by Inno Setup from them. |
| No hacking tools | Fine. |
| Software that "collects user data and transfers it to systems not specified by the user" needs a privacy policy shown during installation and an option to turn it off | Misty Engine collects no user data. The one automatic request is the update check to GitHub (no data about the user; see the policy page). It can be turned off: Edit ▸ Preferences ▸ General ▸ **Check for updates at startup** (also Hub ▸ Settings ▸ Updates; on by default, saved per user). Off, no request is made at startup; Help ▸ Check for Updates… still works on request. |
| "must not modify the user's system configuration without proper warnings" | The installer is per-user: files, Start menu entry, optional desktop shortcut. |
| Installation must come with uninstallation | Inno Setup uninstaller; described on the policy page. |
| "All team members must use multi-factor authentication for both SignPath and source code repository access" | **You:** turn on 2FA on GitHub (and on SignPath after approval). |
| Team roles: authors, reviewers, approvers | On the policy page: `Aalzard` in all three. **You confirm.** |
| A code signing policy on the home page; the term "Code signing policy" on the home page and download/release pages | `docs/code-signing-policy.md`, linked from README under **Code signing policy**. The release workflow adds a "Code signing policy" line to the notes of signed releases. |
| The sentence "Free code signing provided by SignPath.io, certificate by SignPath Foundation" | On the policy page, in README and in signed releases' notes. |
| Privacy policy, or the sentence "This program will not transfer any information to other networked systems unless specifically requested by the user or the person installing or operating it" | On the policy page, with the update check named as the one exception, and the rest of the network use listed. |
| "All signed binaries must have metadata attributes set and enforced": product name = project name, the same product version on all files of a build | Exes: `crates/misty_editor/build_icon.rs`. Installer: `tools/installer/MistyEngine.iss`. Enforced in `tools/release/signpath/*.xml`. |
| "Binary artifacts must be built from source code in a verifiable way" | `.github/workflows/release-windows.yml` on a GitHub-hosted runner. |
| "Every release needs manual approval for signing" | Two approvals per release (exes, installer). |

From <https://docs.signpath.io/trusted-build-systems/github>: SignPath checks that the build was
really done by a GitHub workflow, that the origin data comes from GitHub, that "the artifact is
stored as a GitHub workflow artifact before it is submitted for signing", and, for open source,
that "all jobs of the GitHub workflow leading up to the signing request were executed on
GitHub-hosted agents". The artifact must be uploaded with `actions/upload-artifact` v4 or newer;
the action is `signpath/github-action-submit-signing-request@v3`.

From <https://docs.signpath.io/origin-verification> and <https://docs.signpath.io/projects>:
origin verification and a trusted build system are required for open source signing, and with
them "interactive users cannot be declared as Submitters": only CI can ask for a signature.

Acceptance is their decision. The terms say: "For executable programs that may be downloaded
and executed based on our signature, we require a certain verifiable reputation" and "We're
under no obligation to accept your project". Misty Engine is a young project, so a refusal or a
"come back later" is possible. If that happens the workflow simply keeps releasing unsigned.

## 2. What is in this branch

- `.github/workflows/release-windows.yml`: tag push → build on `windows-latest` → sign (when
  set up) → zip, installer, checksums → upload to a draft release. Unsigned when SignPath is
  not set up. "Run workflow" on a branch is a trial run.
- `tools/make_release.ps1`: new `-ExeDir`, `-Strict`, `-HashOnly`; a `.sha256.txt` for the
  installer too; uses Windows' own `tar.exe`; checks the exes' version info.
- `tools/installer/MistyEngine.iss`: version info on the Setup exe; publisher "Misty Engine"
  (was "Aalzard"), the same as the exes' CompanyName. Change both if you prefer another name.
- `crates/misty_editor/build_icon.rs`: adds LegalCopyright, OriginalFilename, InternalName.
- `tools/release/signpath/`: the two artifact configurations, an optional pipeline policy, README.
- `docs/code-signing-policy.md`, linked from README; `docs/release.md` describes the new flow.

Checked locally: the script's three modes against the exes in `D:/MistyTargets/verify2`
(zip, installer and both checksum files made; the installer shows product name "Misty Engine",
version 0.3.2); the resource script compiles with `rc.exe`; the workflow parses as YAML and
every script block parses as PowerShell. **The workflow has never run on GitHub** (nothing was
pushed), and no signing request has ever been made.

## 3. Your checklist

### Before applying

1. [ ] Turn on two-factor authentication for the GitHub account `Aalzard`
       (GitHub ▸ Settings ▸ Password and authentication).
2. [ ] Read `docs/code-signing-policy.md`. Confirm the team roles and the privacy section; fix
       anything that is not true.
3. [ ] Confirm every bundled asset is under an open license (see the table above).
4. [x] A Preferences switch for the automatic update check: done in round 37 (Edit ▸
       Preferences ▸ General ▸ Check for updates at startup), and the policy page says so.
5. [ ] Merge `r36-signing` into main and push, so SignPath can see the policy page, the README
       link and the workflow.
6. [ ] Optional but useful: Actions ▸ **Release (Windows)** ▸ Run workflow on `main`. It builds
       and packages unsigned and uploads nothing to a release; it proves the workflow works on
       GitHub's runner before anyone at SignPath looks at it.

### Apply

7. [ ] Open <https://signpath.org/apply> and fill in the form. The form is loaded by script and
       could not be read from here, so the labels below are a best guess; the answers are ready
       to paste wherever they fit.

| Likely field | Answer |
| --- | --- |
| Project name | Misty Engine |
| Repository URL | https://github.com/Aalzard/MistyEngine |
| Project home page | https://github.com/Aalzard/MistyEngine |
| Download / release page | https://github.com/Aalzard/MistyEngine/releases |
| License | GNU General Public License v3.0 or later (GPL-3.0-or-later), file `LICENSE`. `RUNTIME-EXCEPTION.md` is an additional permission under section 7 of the GPL for games built with the engine; there is no commercial or dual license. |
| Code signing policy URL | https://github.com/Aalzard/MistyEngine/blob/main/docs/code-signing-policy.md |
| Privacy policy URL | https://github.com/Aalzard/MistyEngine/blob/main/docs/code-signing-policy.md#privacy-policy |
| Build system | GitHub Actions, GitHub-hosted runners: `.github/workflows/release-windows.yml` |
| What should be signed | Windows x64: `MistyEngine.exe` (editor), `misty_player.exe` (game player), both built from this repository with Rust, and the Inno Setup installer `MistyEngine-Setup-vX.Y.Z-windows-x64.exe` |
| Maintainer / contact | **You:** your name and an email you read. GitHub user: Aalzard |
| Your name, email, country | **You** |

   Project description (adapt freely):

   > Misty Engine is a free, open source game engine and editor for Windows and Linux, written
   > in Rust (wgpu, egui, rapier, Lua). It has a scene editor, a project hub, prefabs, Lua
   > scripting, terrain and foliage tools, and builds standalone games. It is licensed under
   > GPL-3.0-or-later; an additional permission lets people ship the games they make under any
   > license. Releases are published on GitHub Releases as a Windows installer, a Windows zip
   > and a Linux tarball. We would like to sign the two Windows executables and the installer.
   > They are built from a version tag by GitHub Actions on GitHub-hosted runners. One
   > maintainer (GitHub: Aalzard) is committer, reviewer and approver, with two-factor
   > authentication.

   Two things worth telling them up front, because they will see them anyway:
   - `misty_player.exe` is the runtime that the editor copies into games people build. A game
     built without its own icon therefore ships that signed, unchanged file; a game with its
     own icon gets an unsigned copy (Windows drops the signature when the icon is replaced).
   - The editor checks GitHub for a new release when it starts (no user data).
8. [ ] Wait for their email. Answer questions from the same address.

### After approval, in SignPath

SignPath's screens could not be seen from here; the names follow their documentation
(<https://docs.signpath.io/projects>, <https://docs.signpath.io/trusted-build-systems>). They may
have created some of this for you already: check first, then fill the gaps.

9.  [ ] Accept the invitation, set a password and **turn on multi-factor authentication** for
        your SignPath user.
10. [ ] Trusted Build Systems ▸ **Add predefined** ▸ GitHub.com.
11. [ ] Install the SignPath GitHub App on `Aalzard/MistyEngine` only (the link is on
        <https://docs.signpath.io/trusted-build-systems/github>, "Prerequisites").
12. [ ] Project: name `Misty Engine`, slug **`MistyEngine`**, repository URL
        `https://github.com/Aalzard/MistyEngine`. In the project, **link** the GitHub.com
        trusted build system.
13. [ ] Artifact configurations: add **`executables`** (paste
        `tools/release/signpath/executables.xml`, make it the default) and **`installer`**
        (paste `tools/release/signpath/installer.xml`).
14. [ ] Signing policies: **`release-signing`** with the SignPath Foundation certificate,
        approval process on, approver = you, 1 approval, trusted build system verification and
        origin verification on. If there is a **`test-signing`** policy (test certificate), keep
        it for trial runs.
15. [ ] Users ▸ add a **CI user** (for example `github-actions`), create its **API token** and
        copy it once. Make the CI user a **submitter** of both signing policies.
16. [ ] Note the **organization id** (shown in SignPath; it is also in the address
        `app.signpath.io/Web/<organization id>/...`).

### After approval, on GitHub

17. [ ] Repository ▸ Settings ▸ Secrets and variables ▸ Actions:

| Kind | Name | Value |
| --- | --- | --- |
| Secret | `SIGNPATH_API_TOKEN` | the CI user's API token |
| Variable | `SIGNPATH_ORGANIZATION_ID` | the organization id |
| Variable, only if different | `SIGNPATH_PROJECT_SLUG` | default `MistyEngine` |
| Variable, only if different | `SIGNPATH_SIGNING_POLICY_SLUG` | default `release-signing` |
| Variable, only if different | `SIGNPATH_EXES_CONFIG_SLUG` | default `executables` |
| Variable, only if different | `SIGNPATH_INSTALLER_CONFIG_SLUG` | default `installer` |

   From a terminal, if you prefer: `gh secret set SIGNPATH_API_TOKEN --repo Aalzard/MistyEngine`
   (it asks for the value) and
   `gh variable set SIGNPATH_ORGANIZATION_ID --repo Aalzard/MistyEngine --body "<id>"`.
18. [ ] Trial run: Actions ▸ Release (Windows) ▸ Run workflow on `main` with signing policy
        `test-signing`. Approve the two requests if the policy asks. The files in the run's
        artifact are signed with a **test** certificate (Windows does not trust it; that is
        expected). This shows the artifact configurations and the version check work.

### The first signed release

19. [ ] Release as in `docs/release.md`: bump the version, tag, push the tag.
20. [ ] The workflow makes the draft release, builds, then stops at
        "Sign the executables". **Approve the signing request in SignPath.** A few minutes
        later it stops at "Sign the installer": **approve again.** Each wait is 2 hours at most.
21. [ ] When the run is green, its summary lists each file with `Valid: CN=SignPath Foundation…`
        and the checksums. Download the installer from the draft release and check it (below).
22. [ ] Publish the release.
23. [ ] Then update the texts that still say "not signed": the status box at the top of
        `docs/code-signing-policy.md`, the Download section of `README.md`
        ("Windows may say…"), and the same paragraph in `tools/release/README.txt`.

## 4. Checking a signature

- Right-click the file ▸ Properties ▸ **Digital Signatures**: signer "SignPath Foundation",
  with a timestamp. Details ▸ View Certificate shows the chain.
- PowerShell: `Get-AuthenticodeSignature .\MistyEngine-Setup-v0.4.0-windows-x64.exe | Format-List`
  → `Status : Valid`.
- Windows SDK: `signtool verify /pa /v .\MistyEngine-Setup-v0.4.0-windows-x64.exe` →
  "Successfully verified". `/pa` uses the normal Authenticode rules; `/v` prints the signer and
  the timestamp. Do the same for `MistyEngine.exe` and `misty_player.exe` from the zip.
- The checksum: `Get-FileHash <file> -Algorithm SHA256` equals the `.sha256.txt` next to it.

## 5. What to expect from SmartScreen (honest version)

- **The publisher shown is "SignPath Foundation", not Misty Engine or your name.** The terms:
  "The code signing certificate is issued to SignPath Foundation. This means SignPath Foundation
  is the publisher of the OSS project."
- **"Unknown publisher" goes away at once** (UAC prompts, file properties, the SmartScreen box
  names the publisher).
- **The blue "Windows protected your PC" box may still appear at first.** Microsoft:
  "Even when signed, a newly created binary could still show a SmartScreen warning until its
  hash or publisher certificate accumulates sufficient evidence of positive reputation", and for
  any OV or EV certificate the first-download behaviour is "Warning — app flagged as
  unrecognized until reputation accumulates; verified publisher name is displayed"
  (<https://learn.microsoft.com/en-us/windows/apps/package-and-deploy/smartscreen-reputation>).
  The same page: "it can take several weeks and hundreds of clean installs", and there is no
  way to submit a file to speed it up for home users.
- **Signing still helps even then:** "Signing files using a trusted certificate can allow
  certificate reputation to build, potentially avoiding warnings on new files signed by the
  same trusted certificate. Unsigned files must build reputation anew with every update."
  Today every Misty Engine release starts from zero.
- **Not known:** whether the certificate SignPath Foundation gives a project already has
  reputation (from other projects, if it is shared, or from earlier use), and so whether the
  first signed release warns or not. Neither SignPath's nor Microsoft's pages say. Plan for a
  warning on the first signed releases and be pleased if there is none.
- SignPath's own page says ordinary (OV) certificates build reputation gradually and that
  "renewed standard (OV) certificates do not inherit reputation"
  (<https://signpath.io/knowledge-base/windows-platform>), so warnings can come back for a
  while when the Foundation's certificate is renewed.
- The in-app updater is not affected: it downloads the zip itself and checks the SHA-256.
- Smart App Control (Windows 11) blocks unsigned programs that have no reputation; signed
  releases are the way through it.

## 6. Things that changed for you

- **A release now starts with the tag push**, not with a local build. The local script still
  works and makes unsigned test builds.
- The Windows workflow creates the release as a **draft**. Nothing is offered to users until
  you publish it. (Before: you created the release by hand; if you still do, the workflow
  uploads into yours.)
- If you push a tag **before** SignPath is set up, the workflow uploads **unsigned** Windows
  files, built by GitHub instead of on your computer.
- The installer now has its own `.sha256.txt`.
- Publisher in Windows' app list is "Misty Engine" (was "Aalzard").

## 7. Not verified

- The application form's real fields (script-loaded form; third-party write-ups mention
  repository URL, license, download URL and a description).
- Every SignPath screen and menu name in section 3; whether the Foundation creates the project,
  policies and certificate for you; the exact GitHub App link.
- Whether SignPath's `product-version` check compares with the text version `0.3.2` (what our
  files carry) or with four numbers. The trial run (step 18) will show it.
- How a signing policy's "allowed branch names" treats a build started by a **tag** push. If
  release signing is refused for the tag, ask SignPath support how they want tag builds set up.
- Whether SignPath accepts the cargo cache in a signed build. Their origin verification page
  lists "prevent caching from previous (possibly unverified) builds" as a typical check; the
  GitHub page does not. The repository variable `RELEASE_CARGO_CACHE=off` builds without it.
  (A cache saved by a tag build is only visible to that same tag, so the cache mostly speeds
  up re-runs and branch trial runs anyway.)
- The workflow on a real runner: build time and disk space on `windows-latest`, whether Inno
  Setup is preinstalled there (it is installed with Chocolatey if not), the `gh` steps. Do
  step 6.
- SmartScreen behaviour with the Foundation's certificate (section 5).
- What replacing the icon does to a signed player was tested here with another signed program
  (a copy of `gh.exe`): after `UpdateResource` the file was reported as not signed and still
  ran. It was not tested with a SignPath-signed `misty_player.exe`.
