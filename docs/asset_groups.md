# Asset Groups (addressable assets)

Asset Groups let a game load assets **by address or label** while it runs, instead of placing
them in a scene or a `Resources` folder. Addressable assets are packed into **bundle files** that
ship with the game or are downloaded later from a server.

## Making an asset addressable

- Select it in the Project window: the Inspector's first row has an **Addressable** checkbox.
  Ticking it adds the asset to the *Default Local Group* with its path as the address; edit the
  address (any string, e.g. `Enemies/Ghost`) and the comma-separated **Labels** right there.
- Or drag assets from the Project window onto a group in the **Asset Groups** window
  (Window ▸ Asset Management ▸ Asset Groups).

Everything is stored in `ProjectSettings/AssetGroups.json` (entries keep the asset's GUID, so a
renamed or moved asset stays addressable). Package assets (`Packages/...`) can be addressable too.

## The Asset Groups window

- **Groups**: the *Default Local Group* plus your own (New ▸ Group). Drag entries between groups.
  Select an entry to edit its address, labels and group, or remove it.
- **Group settings** (select the group header):
  - *Bundle Mode*: Pack Together (one bundle), Pack Separately (one per entry) or Pack Together
    By Label (one per first label).
  - *Compression*: Uncompressed, LZ4 (fast to load) or Zlib (smaller).
  - *Include in Build*.
  - *Load Path*: Local (shipped in the player build) or Remote with a *Remote URL* template;
    `{Platform}` becomes the build platform (`windows-x64`, `linux-x64`, ...).
- **Play Mode Script** (how Play mode in the editor loads):
  - *Use Asset Database*: straight from the project files, nothing to build.
  - *Simulate Groups*: project files, but bundles, dependency order and reference counts are
    tracked as if built (Assets.GetStats shows bundles).
  - *Use Built Bundles*: the bundles of the last build for the Build Settings platform.
- **Build ▸ Asset Bundles** writes:
  ```text
  Builds/Bundles/<platform>/catalog.json
  Builds/Bundles/<platform>/<group>_<suffix>.bundle         local groups
  Builds/Bundles/<platform>/Remote/<group>_<suffix>.bundle  remote groups: upload these
  ```
  **Build ▸ Clean Build Cache** removes them and the download cache.
- **Analyze** reports assets that would be packed into more than one bundle (a file that is
  not addressable itself is copied into every bundle whose addressables use it). Fix it by
  making the shared asset addressable or packing its users together.

Player builds (File ▸ Build Settings) build the bundles for the target platform and copy the
catalog and local bundles into `<Product>_Data/Bundles/`.

## The catalog

`catalog.json` lists every bundle (file name, group, SHA-256, size, compression, remote URL,
the bundles it depends on, the files inside) and every entry (address, path, labels, its bundle
and the files it depends on). An addressable that references another bundle's addressable makes
its bundle depend on that one; dependencies load first.

## Scripting

```lua
-- Callback
local h = Assets.LoadAsync("Enemies/Ghost")          -- address or label
h:OnCompleted(function(h)
    if h.status == "Succeeded" then Instantiate(h.result) end
end)

-- Coroutine: Assets.Run starts a coroutine the engine resumes every frame.
Assets.Run(function()
    local all = Assets.LoadAsync("props"):Await()     -- every asset with the label
    for _, path in ipairs(all.results) do Instantiate(path) end

    local ghost = Assets.InstantiateAsync("Enemies/Ghost", self.transform):Await()
    ghost.result.transform.position = Vector3.new(0, 1, 0)
    Assets.Release(ghost)     -- destroys the instance; bundles nothing uses unload
    Assets.Release(all)
end)

local s = Assets.GetStats()   -- loaded, released, handles, bundles, bundlesLoaded, bundlesUnloaded
```

Handle fields: `id`, `key`, `isDone`, `status` (`Loading` / `Succeeded` / `Failed`), `error`,
`result` (first path, or the GameObject for `InstantiateAsync`), `results` (all paths),
`progress`. Methods: `OnCompleted(fn)`, `Await()` (only inside a coroutine), `Release()`.
`Assets.Release` also accepts a GameObject created by `InstantiateAsync`.

Bundles are reference counted: each handle holds its bundles (and their dependencies); a bundle
unloads when the last handle using it is released.

## Remote bundles

Remote bundles are fetched from `<Remote URL>/<bundle>` over HTTPS (http:// and file:// work
too), checked against the SHA-256 in the catalog and cached (per user:
`%LOCALAPPDATA%/MistyEngine/BundleCache`, `~/.cache/MistyEngine/BundleCache`; the editor uses
`Library/BundleCache`). A bundle whose hash doesn't match is rejected and not cached; the handle
fails with the reason. A cached copy whose hash still matches is used without a download.

See the **AddressablesDemo** scene in the SampleProject.
