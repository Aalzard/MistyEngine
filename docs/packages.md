# Packages

A package is a folder of reusable content (scripts, prefabs, scenes, any asset) with a
`package.json` at its root. Packages live in the project's `Packages/<name>` folder and show up
in the Project window under the **Packages** root, next to Assets.

## package.json

```json
{
  "name": "com.studio.dialogue",
  "displayName": "Dialogue",
  "version": "1.2.0",
  "description": "Branching conversations.",
  "author": { "name": "Studio", "email": "hi@studio.example" },
  "license": "MIT",
  "dependencies": { "com.misty.example-tools": "^1.0.0" },
  "samples": [
    { "displayName": "Demo", "description": "A short talk.", "path": "Samples~/Demo" }
  ]
}
```

- `name`: lower-case reverse-domain id (`com.company.name`); letters, digits, `-`, `_`, `.`.
- `version`: semantic version (`1.2.0`, `2.0.0-preview.1`).
- `author` may be a string or an object.
- `samples`: folders copied into `Assets/Samples/<displayName>/<version>/<sample>` by
  **Import Sample**. Without a list, every folder in `Samples~` is a sample.

## Version ranges

| range           | means                              |
|-----------------|------------------------------------|
| `1.2.3`         | exactly 1.2.3                      |
| `^1.2.3`        | `>=1.2.3 <2.0.0` (`^0.2.3`: <0.3.0) |
| `~1.2.3`        | `>=1.2.3 <1.3.0`                   |
| `1.x`, `1.2.*`  | any 1.x / 1.2.x                    |
| `>=1.0 <2`      | both                               |
| `1.x \|\| 3.x`  | either                             |
| `*`             | any                                |

Pre-releases only match ranges that name a pre-release of the same version.

## The Package Manager window

Window ▸ Package Manager…

- **Packages: In Project / Built-in / Registry** picks the list.
  - *Built-in*: the engine's modules (Physics, Audio, UI, ...), with versions and descriptions.
    Other packages can depend on them by name; they can't be removed.
  - *In Project*: the packages in `Packages/`. A package with a newer registry version shows `↑`.
  - *Registry*: the packages of the registry configured in Project Settings ▸ Package Manager
    (none by default: "No registry configured").
- The details pane shows the version, source, author, license, description, dependencies (with
  the resolved version, or what's wrong), the packages that use it, samples (**Import Sample**)
  and **Install** / **Update to x** / **Remove**.
- **＋** adds a package **from disk** (a folder with a package.json), **from zip** (package.json at
  the top or inside a single top folder) or **from git URL** (`https://host/repo.git#v1.0.0`;
  needs `git` on PATH, otherwise the item is disabled and says so).
- Dependency conflicts (no version satisfies every range) are listed at the top with who asked
  for what.

## Files

- `Packages/manifest.json`: the packages the project asked for, and the registry URL.
  Values: a version range (registry), `file:<folder>` (a folder in Packages/) or
  `git+<url>[#rev]`.
- `Packages/packages-lock.json`: the resolved version, source (`local`, `git`, `registry`,
  `builtin`), where it came from, a hash (download SHA-256 / git commit), whether it is only an
  indirect dependency, and its dependencies. Rewritten after every change.

Resolution picks, for every package needed directly or through other packages, the newest
version every range allows. Packages copied into the project (disk, zip, git) are pinned to the
version they hold. Registry packages are read-only in the Project window (they are replaced on
update).

## Registries

A registry is a JSON index at a URL (https://, http:// or file://):

```json
{ "packages": [ {
    "name": "com.studio.dialogue", "displayName": "Dialogue", "description": "...",
    "author": "Studio", "license": "MIT",
    "versions": {
      "1.2.0": { "url": "dialogue-1.2.0.zip", "sha256": "<hex>",
                 "dependencies": { "com.misty.modules.ui": "*" } } } } ] }
```

Relative `url`s are relative to the index. Downloads whose SHA-256 doesn't match are refused.

## Examples

The SampleProject has two local packages: `com.misty.example-tools` (the Bobber Lua utility,
a Beacon prefab, a Tools Demo sample scene) and `com.misty.example-effects` (a Pulse script and
an Effects Demo sample; depends on example-tools `^1.0.0`).
