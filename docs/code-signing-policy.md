# Code signing policy

> **Status: applied for, not active yet.** Misty Engine is applying to the SignPath Foundation
> open source program. Until the application is accepted and the first signed release is out,
> the Windows downloads are **not signed**. This page is the policy that applies from the first
> signed release on.

Free code signing provided by [SignPath.io](https://about.signpath.io/), certificate by
[SignPath Foundation](https://signpath.org/).

The Windows downloads of Misty Engine on the
[Releases page](https://github.com/Aalzard/MistyEngine/releases) are signed with a certificate
issued to SignPath Foundation, so Windows shows **SignPath Foundation** as the publisher. The
signature says: this file was built by GitHub Actions from the source code in
<https://github.com/Aalzard/MistyEngine>, and a member of the team approved the release.

## What is signed

| File | Signed |
| --- | --- |
| `MistyEngine.exe` (the editor and Misty Hub) | yes |
| `misty_player.exe` (the player that File ▸ Build copies into your game) | yes |
| `MistyEngine-Setup-vX.Y.Z-windows-x64.exe` (the installer) | yes |
| `MistyEngine-vX.Y.Z-windows-x64.zip` | the two programs inside are signed; the zip has a `.sha256.txt` |
| Linux packages | not signed; each has a `.sha256.txt` |

Only files built from this repository are signed. Misty Engine is free software under the GNU
GPL v3 or later with a runtime exception ([LICENSE](../LICENSE),
[RUNTIME-EXCEPTION.md](../RUNTIME-EXCEPTION.md)); every release is built from a public tag.

Games you build with Misty Engine are yours and are **not** covered by this policy: SignPath
Foundation signs Misty Engine, not your game. File ▸ Build copies `misty_player.exe` into your
game; the signature on it only says that this player is the unchanged one from a Misty Engine
release, and nothing about your game's data. When a build sets its own icon, Windows removes the
signature from that copy (changing the icon changes the file). Sign your own game with your own
certificate if you need to.

## How a release is built and signed

1. A team member pushes a version tag (`vX.Y.Z`) to this repository.
2. GitHub Actions builds the tag on a GitHub-hosted Windows runner
   ([`.github/workflows/release-windows.yml`](../.github/workflows/release-windows.yml)). No file
   from a developer's computer goes into the release.
3. The workflow sends the two programs to SignPath. SignPath checks that they come from that
   workflow run of this repository and that they carry the product name "Misty Engine" and the
   release version ([`tools/release/signpath/`](../tools/release/signpath/)).
4. An approver (see below) approves the signing request by hand.
5. The workflow builds the zip and the installer from the signed programs, has the installer
   signed the same way (a second approval), writes the SHA-256 files and uploads everything to
   the GitHub release.

The full steps are in [release.md](release.md).

## Team roles

<!-- TO CONFIRM (maintainer): the names below must be the people who really hold these roles,
     each with two-factor authentication on GitHub and on SignPath. -->

| Role | What it means | Members |
| --- | --- | --- |
| Committers (authors) | May change the source code in this repository without another review | [Aalzard](https://github.com/Aalzard) |
| Reviewers | Review every change that comes from someone else (pull requests) before it is merged | [Aalzard](https://github.com/Aalzard) |
| Approvers | Approve each signing request, which means each release | [Aalzard](https://github.com/Aalzard) |

All team members use multi-factor authentication for GitHub and for SignPath. Build scripts and
workflow files count as source code and are reviewed like it.

## Privacy policy

Misty Engine has no accounts, no analytics, no telemetry and no crash reporting. Apart from the
update check described first (which you can turn off), this program will not transfer any
information to other networked systems unless specifically requested by the user or the person
installing or operating it.

What the editor and the Hub (`MistyEngine.exe`) do on the network:

- **Update check (automatic, can be turned off).** When the editor or the Hub starts, and at most once every 6
  hours, it asks GitHub for the newest release:
  `https://api.github.com/repos/Aalzard/MistyEngine/releases/latest`. The request carries the
  User-Agent `MistyEngine-Updater/<version>` and nothing about you or your projects. As with any
  web request, GitHub sees your IP address;
  [GitHub's privacy statement](https://docs.github.com/en/site-policy/privacy-policies/github-general-privacy-statement)
  applies. To turn it off, untick **Check for updates at startup** in Edit ▸ Preferences ▸
  General (or in the Hub: Settings ▸ Updates). The choice is saved per user, and from then on
  no request is made at startup; Help ▸ Check for Updates… still checks when you click it.
- **Update download (only when you click Update now).** The Windows zip or Linux tarball and its
  SHA-256 file are downloaded from GitHub (`github.com`, `objects.githubusercontent.com`,
  `release-assets.githubusercontent.com`) over HTTPS and checked before anything is installed.
- **Package Manager (only when you use it).** A project can name a package registry URL in
  Project Settings; the editor then downloads that registry's index and the packages you install
  from it. Installing a package from a git URL runs your own `git`. No registry is set by default.
- **Version control window (only when you use it).** Fetch, pull and push run your own `git`
  with the remote you configured.
- **AI tools (MCP), off by default.** When you turn on Edit ▸ Preferences ▸ AI Tools, the editor
  listens on `127.0.0.1` only (this computer), behind a random token. See [mcp.md](mcp.md).
- **Profiler, Player target (only when you choose it).** The editor listens on
  `127.0.0.1:54998` for a player on the same computer.
- **Links.** Buttons such as "open the release page" open your web browser.

What the player (`misty_player.exe`) and games built with Misty Engine do on the network depends
on the game:

- A game that uses the networking features sends and receives UDP packets with the addresses its
  scripts name, and can look for or announce games on the local network (UDP broadcast).
- A game that uses remote Asset Groups downloads asset bundles from the URLs its developer set.
- A player started with `MISTY_PROFILER_CONNECT` or `--profiler-connect` sends profiling data to
  that address.

The engine itself adds nothing else: a game that uses none of these features makes no network
connections. The people who make a game are responsible for that game's own privacy policy.

The installer does not use the network.

## Installing and uninstalling

The installer installs for the current user only, by default without administrator rights, and
changes nothing else on the system: it copies the program folder, adds a Start menu entry and,
if you tick it, a desktop shortcut. **Uninstall** it under Windows Settings ▸ Apps ▸ Installed
apps ▸ Misty Engine, or with "Uninstall Misty Engine" in the Start menu. The zip version is
removed by deleting its folder. Your projects are never touched.

## Checking a signature

Right-click the file ▸ **Properties** ▸ **Digital Signatures**: the signer is "SignPath
Foundation". Or, in PowerShell:

```powershell
Get-AuthenticodeSignature .\MistyEngine-Setup-vX.Y.Z-windows-x64.exe | Format-List Status, SignerCertificate
```

`Status` must be `Valid`. The SHA-256 of each download is in the `.sha256.txt` file next to it:
`Get-FileHash <file> -Algorithm SHA256`.

## Reporting a problem

If you think a file signed for Misty Engine is harmful or was not built from this repository,
please open an [issue](https://github.com/Aalzard/MistyEngine/issues) and report it to SignPath
Foundation at <support@signpath.io>.
