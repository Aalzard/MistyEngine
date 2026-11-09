# Round 21: World Partition-lite (world streaming)

Branch `r21-streaming`. Asked for: grid cells as sub-scenes with a Convert action, streaming
sources with load / unload radii and hysteresis, asynchronous loads with time-sliced main-thread
integration, HLOD proxies for unloaded cells, stitched streaming terrain tiles, a World
Partition window with a minimap, gizmos and a stats overlay, a Lua API, and a 4 × 4 km
`StreamingDemo` sample, with a flythrough proving no frame's streaming work goes over budget.

Code: `crates/misty_engine/src/world_partition/` (settings and grid in `mod.rs`, `convert.rs`,
`streamer.rs`, `hlod.rs`, `tiles.rs`), `scene/streaming.rs` (scenes added a slice at a time),
`scripting/world_partition_api.rs`, the renderer's cross-tile LOD stitching in
`render/terrain_lod.rs` / `render/terrain.rs`; editor `panels/world_partition.rs`,
`panels/inspector/world_partition_editors.rs`; sample `crates/misty_editor/src/sample_streaming.rs`.

## 1. Cells as sub-scenes

- A **World Partition** component (Add Component ▸ World Partition) on an object of the
  persistent scene holds the setting: cell size, grid origin, folder, default load / unload
  radius, HLOD distance, frame budget (ms), parallel loads, and the list of cells (object count,
  height range, whether it has an HLOD). It is a component rather than a Lighting-style scene
  setting so it lives in the scene file without a format change and shows in the Inspector.
- **Always Loaded** (marker component) keeps an object in the persistent scene. Cameras,
  directional lights, UI, Wind Zones, Post-process Volumes, World Builders, Streaming Sources
  and terrains larger than a cell stay too.
- **Convert Scene** (World Partition window) puts every other top-level object into the cell
  under its anchor (middle of its mesh bounds, middle of a terrain, else its position), writes
  each cell as `<folder>/Cells/Cell_<x>_<z>.scene` (an ordinary `.scene`, compact one-line JSON
  because there are many and they are generated), removes the objects from the persistent scene
  and lists the cells. Converting again (e.g. after changing the cell size) merges the cells back
  (loaded ones from the world, the rest from their files) and splits again; stale cell files are
  deleted. **Save Cells** writes loaded cells back.
- A loaded cell is an additive scene of the multi-scene system (`SceneEntry::streamed`), so it
  shows in the Hierarchy, saves to its file and its objects render, collide and run scripts
  like any other. `SceneEntry::partial` is set while its objects are still arriving.

## 2. Streaming

- **Streaming Source** component (own load / unload radius, 0 = the partition's). Without one,
  the main camera (tagged MainCamera, else the highest-depth camera) is the source.
- A cell loads when a source is within its load radius (distance on the x/z plane to the cell's
  rectangle) and unloads once every source is past its unload radius; the gap is the hysteresis.
  Loading by hand (window, `WorldPartition.LoadCell`) pins a cell.
- `Streamer::update` (called by `Runtime::update` before the scripts, so it runs in Play mode
  and the player; the editor runs its own in Edit mode):
  1. picks cells for the sources; cells that should load get a job on the **background pool**
     (`background::pool()`, below-normal priority) nearest first, up to *Parallel Loads* at once;
  2. each job reads the file (through the VFS, so paks work), parses it into object entries
     (`scene::streaming::parse`) and reads ahead what the objects name: terrain data (and the
     tree models its trees use), models (into the model cache);
  3. on the main thread, within the **frame budget**: objects of unloading cells are removed one
     top-level subtree at a time, then parsed cells are added nearest first, one whole top-level
     subtree at a time (`Scene::continue_streamed`), checking the clock after each. Entity ids of
     a cell are all reserved up front, so references between objects work across slices; a
     subtree is never left half made, so physics (`add_subtree`) and scripts (Awake / Start) only
     see complete objects. Terrain data read on the pool goes into the terrain store first.
  4. A cell that should unload while being read or added is dropped or removed part way.
- Unloading frees the cell's terrain data from the store (unless it has unsaved edits); the
  renderer drops GPU data of terrains not drawn for 600 frames and placed trees of terrains that
  are gone.
- Stats (`StreamingStats`): loaded / reading / waiting / adding / removing / failed cells, a
  memory estimate (file size × 3 plus terrain arrays), last and worst main-thread ms, frames over
  budget, loads, unloads, and the last 240 frames' times.

## 3. HLOD proxies

- **Build HLODs** reads every cell and merges what it shows into one mesh with vertex colours,
  drawn with one vertex-colour material (`<folder>/HLOD/HLOD.mat` + `HLOD.shader`, written if
  missing), so an unloaded cell is one draw:
  - meshes (only the coarsest level of a LOD Group, objects of 1.5 m or more) welded and
    simplified to ≤ 48 triangles with the virtual-geometry simplifier, coloured by material
    colour × albedo texture mean;
  - terrains as a 32 m grid coloured from the splat layers' mean texture colours, darker under
    trees, sunk 0.6 m so a cell whose tile is arriving shows the tile;
  - terrain trees as crowns (stored as 10 bytes each, expanded into diamonds when loaded) — the
    tree impostors' job at mid range, done by the merged mesh.
- `<folder>/HLOD/Cell_x_z.hlod`: JSON with quantised positions, octahedral normals, colours and
  crowns in base64.
- At run time `hlod::draws` (in every view's draw cache) adds one draw per cell with an HLOD
  that is not fully loaded (within the HLOD distance, if set). The file is read and its mesh
  registered (a new runtime-mesh key, so no GPU cache is invalidated) on the background pool the
  first time a proxy is wanted. A proxy hides once its cell's last object exists.
- Virtual Geometry: proxies are plain merged meshes; dense meshes that already draw as virtual
  geometry are included through their coarsest LOD level like the rest.

## 4. Terrain tiles

- `tiles::from_fn` (tiles from a world height function), `tiles::split` (an existing terrain:
  heights, splat maps, grass and detail density, holes, trees), `tiles::stitch` (shared edge
  samples averaged; each tile's **border** filled with the neighbours' samples one step past the
  edge, or its own slope continued at the world edge). `split_object` is the editor action
  (World Partition Inspector ▸ *Split into Tiles* for terrains larger than a cell): one tile
  terrain per cell under `<folder>/Terrain`, each a top-level object with Terrain and Terrain
  Collider that converts into its cell.
- `TerrainData::border` (new, saved as `"border"` in the `.terrain` file and left out when empty,
  so other terrains are unchanged): `vertex_normal` uses it past the edges, so edge normals are
  the same on both sides of a seam (no lighting line).
- Renderer: neighbouring tiles (same size and chunk grid, corners touching) are found each frame
  and their chunk LODs limited across the shared edge (at most one level apart, then within each
  tile again); a chunk next to a coarser tile snaps its edge vertices onto the coarse grid like
  chunks inside one terrain. No cracks.

## 5. Editor

- **World Partition window** (Window ▸ World ▸ World Partition; docks by the Inspector):
  Convert Scene, Build HLODs, Save Cells, Load All / Unload All, *Stream Around Scene Camera*
  (Edit mode), Gizmos and Stats Overlay toggles; a **minimap** of the grid (north up) coloured by
  cell state (grey unloaded, amber reading / waiting, light green adding with a progress bar,
  green loaded, red failed), a blue outline where an HLOD stands in, a dot on cells loaded by
  hand, the sources with their load (green) and unload (orange) circles and the Scene camera;
  click selects a cell (details, Load / Unload), double-click loads or unloads; the stats and a
  graph of the last 240 frames' main-thread streaming time against the budget. In Play mode it
  shows and steers the game's streamer. "Add World Partition" when the scene has none.
- **Inspectors** for World Partition (settings, cell summary, Split into Tiles for big terrains,
  Open window), Streaming Source and Always Loaded.
- **Scene view gizmos**: every cell outlined at its ground height in its state's colour, the
  sources' load and unload circles.
- **Stats overlay** (Scene and Game views, top left): loaded / total cells, pending (reading,
  waiting, adding), unloading, memory, loads / unloads, main-thread ms (last, max, budget, frames
  over) and the frame graph.
- `MISTY_WORLD_PARTITION=around|all` for screenshots.

## 6. Lua (`WorldPartition`)

Fields `enabled`, `loadedCells`, `pendingLoads`, `cellCount`, `memory`, `lastFrameMs`,
`maxFrameMs`, `budgetMs`, `overBudgetFrames`; `IsAreaLoaded(position, radius)`,
`CellAt(position)`, `GetCellState(x, z)`, `LoadCell(x, z)`, `UnloadCell(x, z)`;
`WaitForArea(position, radius)` (yields; inside `WorldPartition.Run(function() … end)`, a
coroutine the engine resumes every frame) and `OnAreaLoaded(position, radius, fn)`.

## 7. StreamingDemo

`Assets/Scenes/StreamingDemo.scene` (generator `sample_streaming.rs`, regenerate with
`cargo test --release -p misty_editor -- --ignored write_streaming_demo`;
`streaming_demo_matches_its_generator` checks the files). 4096 × 4096 m, 8 × 8 cells of 512 m:
rolling meadow hills, a winding river valley, mountains to the north-east with snow; 64 stitched
terrain tiles (65 × 65 samples, 32² splat, four layers painted from slope and height in world
space so there are no seams); forests of the four sample trees as terrain trees; the sample
boulders and outcrops as prefab instances; an HLOD per cell. The camera flies a loop in Play mode
(`Assets/Scripts/StreamingFlyover.lua`: waits with `WorldPartition.OnAreaLoaded`, then flies).

Sizes: see the end of this file.

## Measurements

(see below)

## Not done yet

(see below)
