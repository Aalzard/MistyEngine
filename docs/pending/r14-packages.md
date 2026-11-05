# Round 14 — Asset Groups (addressables) and Packages

## Progress

### Asset Groups (addressable assets)

- **Settings** (`misty_engine::asset_groups::settings`, `ProjectSettings/AssetGroups.json`):
  groups (the *Default Local Group* plus user groups) of entries (GUID, path, address, labels);
  per group Bundle Mode (pack together / separately / by label), Compression (none / LZ4 /
  zlib), Include in Build, Load Path (local / remote + URL template with `{Platform}`); the
  editor's Play Mode Script.
- **Asset Inspector header**: an *Addressable* checkbox above every asset's inspector, with the
  address field, group and labels.
- **Asset Groups window** (Window ▸ Asset Management ▸ Asset Groups): groups with their entries
  (filterable), drag entries between groups or assets in from the Project window, entry details
  (address, labels, group, remove), group settings, New ▸ Group, Remove Group, Play Mode Script,
  Build ▸ Asset Bundles / Clean Build Cache, Analyze (duplicate dependencies listed below).
- **Catalog + bundle build** (`asset_groups::catalog`, `asset_groups::build`): `plan` assigns
  entries to bundles, packs the non-addressable files they use into each user bundle, turns
  references to another bundle's addressable into a bundle dependency (and leaves out what that
  bundle brings along). Bundles are pak archives (new zlib codec next to LZ4), hashed with
  SHA-256; `catalog.json` lists bundles (hash, size, compression, remote URL, dependencies,
  files) and entries (address, path, labels, bundle, file dependencies). Output:
  `Builds/Bundles/<platform>/` (remote bundles in `Remote/`). Player builds build the bundles
  and copy the catalog + local bundles into `<Product>_Data/Bundles/`.
- **Runtime** (`asset_groups::runtime::AssetSystem`): handles, per-bundle reference counts,
  dependency-first loading (a bundle mounts only after its dependencies), unloading when the last
  handle is released. Loaded bundles are mounted into the VFS as extra paks
  (`vfs::mount_bundle`), so every existing loader reads bundle content unchanged. Providers:
  asset database, simulated groups, built bundles (editor Play Mode Script), and a built game's
  `Bundles/catalog.json` (`AssetSystem::auto`).
- **Remote bundles** (`asset_groups::fetch`): downloaded on a worker thread over HTTPS / HTTP /
  `file://` with ureq, SHA-256 checked against the catalog (mismatch = rejected, not cached),
  cached per user (`Library/BundleCache` in the editor).
- **Lua** (`scripting::assets_api`): `Assets.LoadAsync(address or label)`,
  `Assets.InstantiateAsync(address, parent)`, `Assets.Release(handle or object)`,
  `Assets.Run(fn)` (engine-resumed coroutine), `Assets.GetStats()`; handles with `isDone`,
  `status`, `error`, `result`, `results`, `OnCompleted(fn)`, `Await()`, `Release()`.

### Packages

- **Files** (`misty_editor::packages::manifest`): `package.json` (name, displayName, version,
  description, dependencies, author string or object, license, samples),
  `Packages/manifest.json` (dependencies as range / `file:` / `git+url#rev`, registry URL),
  `Packages/packages-lock.json` (version, source, resolved, hash, indirect, dependencies).
- **Semver** (`packages::semver`): versions with pre-releases; ranges `1.2.3`, `^`, `~`, `x`/`*`,
  comparators, hyphen ranges, `||`; npm pre-release rule.
- **Resolution** (`packages::resolve`): newest version satisfying every range, project folders
  pinned, conflicts listing each requirement and its requester and the available versions.
- **Install / remove** (`packages::install`): from a folder, a zip (top-level package.json or a
  single top folder; `enclosed_name` guards paths), a git URL (`git clone --depth 1 [--branch]`,
  commit recorded in the lock; disabled when git isn't on PATH), the registry (zip download,
  SHA-256 check); remove (refused for built-ins and packages others need); import samples into
  `Assets/Samples/<title>/<version>/<sample>`.
- **Registry** (`packages::registry`): JSON index from the URL set in Project Settings ▸
  Package Manager (stored in the manifest; default none → "No registry configured"), fetched in
  the background; relative archive URLs.
- **Built-in packages** (`packages::builtin`): the engine modules at the engine version,
  listable and depend-able, not removable.
- **Package Manager window** (Window ▸ Package Manager…): In Project / Built-in / Registry lists
  with search and update badges; details with dependencies (resolved or broken), users, samples
  (Import Sample / Reimport), Install / Update / Remove; ＋ from disk / zip / git URL; conflicts
  and broken package.json files shown at the top.
- **Project window**: a *Packages* root under Assets with every package (by display name) and its
  folders; breadcrumbs start at Packages; package assets drag as `Packages/...` paths; registry
  packages are read-only (no rename / delete / move / drop).

### Samples

- `SampleProject/Packages/com.misty.example-tools` (Bobber.lua, Beacon prefab, Tools Demo
  sample scene) and `com.misty.example-effects` (Pulse.lua, Effects Demo sample; depends on
  example-tools `^1.0.0`), with manifest and lock.
- **AddressablesDemo** (`sample_addressables.rs`): three project prefabs and the package's
  Beacon are addressable (`Creatures/Orb` in a pack-separately *Creatures* group; `props` label
  on the rest); Spawn by Address / Spawn by Label / Release All buttons and a stats line driven
  by `AddressablesSpawner.lua`. Tests play it from the asset database, simulated groups and
  freshly built bundles.

## Decisions

1. **Our names**: "Asset Groups" for the feature/window, "addressable"/"address"/"label" for
   entries, `Assets.*` in Lua, "Packages" / "Package Manager…" for packages; file names
   `AssetGroups.json`, `catalog.json`, `*.bundle`, `manifest.json`, `packages-lock.json`.
   Reason: copy behaviour, not names or formats.
2. **Addressable settings in one project file keyed by GUID (+ path), not in `.meta` files.**
   Reason: texture import settings rewrite meta files wholesale (`meta::write_settings`), and
   one file gives the window, the build and the player a single source; GUIDs still follow
   renames.
3. **Bundles are pak archives mounted into the VFS.** Reason: reuses the tested pak reader and
   every asset loader keeps working unchanged (prefabs, materials, meshes, scripts read through
   `vfs`); loading a bundle = mounting, unloading = unmounting. Added a zlib codec to pak for the
   group "Zlib" option.
4. **Non-addressable dependencies are copied into each bundle that uses them** (and reported by
   Analyze), while references to another bundle's addressable become bundle dependencies.
   Reason: matches the reference behaviour users know, and keeps bundles self-contained.
5. **Lua handles are tables built in a Lua prelude over Rust primitives.** Reason: `Await` must
   `coroutine.yield`, which can't cross a Rust callback; `Assets.Run` gives coroutines that the
   engine resumes once a frame after the loads advance.
6. **Loads finish on the next frame even when local** (and immediately-failing keys fail on the
   next update). Reason: one code path for callbacks; scripts can't depend on synchronous
   completion that would break with remote bundles.
7. **Remote downloads in the engine with ureq + sha2** (the editor's updater versions, rustls).
   Reason: the player must fetch remote bundles; same crates already in the lock file.
   `file://` support doubles as the test transport; a tiny local HTTP server tests http://.
8. **Player builds rebuild bundles automatically** when the project has addressables. Reason:
   avoids shipping stale bundles; Build ▸ Asset Bundles is still there for remote content.
9. **Packages are always copied into `Packages/<name>`** (disk, zip, git, registry); the manifest
   records the source. Reason: the task's storage rule; projects stay self-contained.
   Project-folder packages are pinned in resolution; registry-installed ones compete with the
   registry's other versions so updates resolve.
10. **The registry URL lives in `Packages/manifest.json`**, edited from Project Settings ▸ Package
    Manager. Reason: keeps package configuration with the packages and avoids growing the shared
    `ProjectSettings` struct.
11. **Package files are not in the GUID database** (it scans `Assets/` only); package assets are
    referenced by `Packages/...` paths. Reason: smallest change to the asset database; packages
    are replaced wholesale on update, so path references are stable.
12. **Lock file is only written when it changed** (or created when a manifest exists). Reason:
    opening the window must not dirty projects without packages.

## Tests

- Engine: settings edits/round-trip, catalog (addresses, labels, deps, bundle modes, exclusion),
  analyze duplicates, bundle build round-trip (hashes, codecs, stale removal, player copy),
  load/release reference counts + VFS unmounting, dependency load order, database / simulated /
  unavailable providers and editor modes, remote file:// download + cache + hash mismatch
  rejection, HTTP download hash check, pak zlib round-trip, Lua `Assets` API (callbacks, Await,
  InstantiateAsync with parent, stats, release by object).
- Editor: semver parse/order/ranges, resolver (newest, conflicts, pinning), package.json
  validation, manifest/lock round-trip, install/remove folder + zip, conflicts + samples,
  registry install with indirect deps + hash refusal, Project window Packages root + read-only,
  kittests for the Asset Groups window (groups, entry selection, New Group, Play Mode Script,
  Build, Analyze), the Inspector Addressable header, the Package Manager window (details, sample
  import, built-in, registry message, ＋ menu, remove) and the Project Settings page;
  AddressablesDemo generator check and play tests (database, simulated, built bundles).
