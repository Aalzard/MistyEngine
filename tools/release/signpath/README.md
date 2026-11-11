# SignPath settings for the Windows release

The Windows release is signed through [SignPath](https://about.signpath.io/) with a certificate
from [SignPath Foundation](https://signpath.org/) (free for open source projects). The signing
itself happens in `.github/workflows/release-windows.yml`; this folder holds the settings that
live **in SignPath**, so they are reviewed and versioned like the rest of the source.

Policy for users: [docs/code-signing-policy.md](../../../docs/code-signing-policy.md).
Setting it up for the first time: [docs/pending/r36-signing.md](../../../docs/pending/r36-signing.md).

| File | What it is | Where it goes in SignPath |
| --- | --- | --- |
| `executables.xml` | Artifact configuration for signing pass 1: a zip with `MistyEngine.exe` and `misty_player.exe`. Both are signed with Authenticode, and both must have the product name "Misty Engine" and the release version. | Project ▸ Artifact configurations ▸ add, name and slug `executables`, paste the XML. Make it the default. |
| `installer.xml` | Artifact configuration for signing pass 2: a zip with the Inno Setup installer `MistyEngine-Setup-vX.Y.Z-windows-x64.exe`, with the same product name and version check. | Project ▸ Artifact configurations ▸ add, name and slug `installer`, paste the XML. |
| `pipeline-policy.yml` | Optional pipeline policy: every job must run on a GitHub-hosted runner. | Pipeline Policies ▸ add (internal definition), then add it to the `release-signing` signing policy. Skip it if SignPath Foundation already set one. |

## Why two passes

SignPath can sign files nested inside zip, MSI and similar containers in one request, but it
cannot look inside an Inno Setup installer. So the workflow:

1. builds the exes and uploads them as the workflow artifact `unsigned-executables`
   (SignPath only signs what GitHub stores as a workflow artifact of the run);
2. asks SignPath to sign them (`executables`) and waits for the approval;
3. builds the zip and the installer from the **signed** exes (`tools/make_release.ps1 -ExeDir`);
4. uploads the installer as `unsigned-installer`, asks SignPath to sign it (`installer`) and
   waits for the second approval;
5. writes the `.sha256.txt` files last (`tools/make_release.ps1 -HashOnly`) and uploads to the
   GitHub release.

## The `version` parameter

Both configurations need a `version` parameter. The workflow passes the workspace version from
`Cargo.toml` (which a release tag must equal: `v0.3.2` ↔ `0.3.2`). The exes get their version
info from `crates/misty_editor/build_icon.rs`, the installer from
`tools/installer/MistyEngine.iss`. SignPath Foundation requires the product name and product
version restrictions (<https://signpath.org/terms>, "Artifact configuration"), so do not remove
them.

## Names the workflow uses

| Setting | Default | Change it with the repository variable |
| --- | --- | --- |
| Project slug | `MistyEngine` | `SIGNPATH_PROJECT_SLUG` |
| Signing policy slug | `release-signing` | `SIGNPATH_SIGNING_POLICY_SLUG` |
| Artifact configuration, pass 1 | `executables` | `SIGNPATH_EXES_CONFIG_SLUG` |
| Artifact configuration, pass 2 | `installer` | `SIGNPATH_INSTALLER_CONFIG_SLUG` |

`SIGNPATH_ORGANIZATION_ID` (variable or secret) and the secret `SIGNPATH_API_TOKEN` have no
default: signing is on only when both are set.

## When you change something

- Changed an XML file here: paste it into SignPath too (and the other way round). SignPath
  keeps its own copy; the workflow refers to it by slug only.
- New exe or dll in the package: add it to `executables.xml` (an `<include>`), to the "Collect
  the executables to sign" step of the workflow and to `tools/make_release.ps1`.
- Only files built from this repository may be signed. Third-party dlls may be shipped inside
  the package unsigned, never signed with this certificate.

## Not checked yet

These files were written from SignPath's documentation
(<https://docs.signpath.io/artifact-configuration/syntax>,
<https://docs.signpath.io/artifact-configuration/reference>,
<https://docs.signpath.io/trusted-build-systems/github>) before the project had a SignPath
account, so no signing request has run against them. Things to watch on the first trial run:

- whether `product-version="${version}"` compares with the text version (`0.3.2`, what the
  files carry) or wants four numbers;
- the documentation's pipeline policy example says `require_github_hosted`, its table says
  `required_github_hosted`; use the spelling SignPath's editor accepts.
