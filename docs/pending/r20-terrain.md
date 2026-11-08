# Round 20B: terrain tools at Unity parity, procedural placement brushes

Branch `r20-terrain`. Asked for: terrain holes, more than four paint layers, several grass
species and detail meshes with density painting, placement brushes for prefabs, rocks and
trees, and World Builder scatter that becomes editable terrain trees, without changing the
MeadowDemo look or frame time.

The Terrain Inspector now has nine tool tabs: Raise or Lower, Set Height, Smooth, Paint
Texture, **Paint Holes**, **Paint Details**, Paint Trees, **Place Objects**, Settings. Every
brush stroke is one undo step. Sample scene: `Assets/Scenes/TerrainToolsDemo.scene`
(generator `crates/misty_editor/src/sample_terrain_tools.rs`).

## 1. Holes (`terrain/data.rs`, `terrain/brush.rs`, `render/terrain.rs`)

- Data: one byte per heightmap cell (`(res - 1)²`), saved in the `.terrain` file as packed bits
  (`"holes"`, left out when there are none, so older files are unchanged). Resizing the
  heightmap carries holes to the nearest cells.
- **Paint Holes** cuts the cells whose centre the brush covers by at least half; Shift fills
  them in; **Fill All Holes** clears them. Holes are all-or-nothing per cell, like the
  reference engine's hole texture.
- Render: chunks with holes get their own index lists: the shared chunk pattern for their
  (LOD, edge mask) with the hole cells' triangles left out. Every pass (camera, depth prepass,
  AO prepass, shadow cascades, virtual shadow map pages) draws them, and no shader changes.
  Those chunks always draw at full detail, and their neighbours step down to them, so hole
  edges follow the cells exactly and the LOD stitching still holds. Terrains without holes take
  the same path as before.
- Collider: hole cells are `CELL_REMOVED` in the rapier heightfield. A ball dropped over a hole
  falls through, and one dropped beside it lands.
- Rays (`terrain::raycast_data`, so brushes and picking) pass through holes. No grass, flowers,
  trees or detail meshes grow over them.

## 2. More than four layers (`terrain/data.rs`, `render/terrain_layers.*`)

- Up to 16 layers, four to a splat (control) map. The first map stays `alphamap`; maps for
  layers 5+ live in `alphamap_extra` (saved as `"alphamap_extra"`). It stays empty with four
  layers or fewer, so those terrains save, paint and draw exactly as before.
- The texel weights over all maps sum to 255. The Paint brush, Auto Paint Rules / Generate,
  Generate in brush, Color Harmony coverage, grass "Grow On" and layer removal all work across
  maps. Removing a layer moves the later layers down a channel, across maps.
- Texture arrays: the layer albedo and normal+height arrays hold one slice per layer, rounded
  up to a whole map (four slices for four layers or fewer, as before).
- Height-based blending: the existing height blend (Cliffs ▸ Height Blend, per-layer height
  map or albedo luminance) now covers every layer.
- Cheap per chunk: the GPU keeps a bit mask per chunk of the layers painted in it (plus a
  one-texel border for the bilinear lookup). With more than four layers, a pixel reads only
  the maps that hold its chunk's layers and keeps the four strongest. It height-blends those
  four exactly like the four-layer path. So a pixel never samples more than four layers, and a
  chunk with two layers costs two layers. The four-layer path is unchanged: identity slot
  indices, and the extra code sits behind a uniform `count > 4` branch.

## 3. Grass species and detail meshes (`terrain/detail_layers.rs`)

**Paint Details** lists the terrain grass (the existing Grass & Details, layer 0) and up to 16
detail layers, each with its own density map (one byte per splat texel):

- **Grass species**: own density (blades/m²), height range, blade width, bend, colour (between
  the terrain grass's healthy and dry colours, plus brightness) and seed heads. Their blades are
  scattered into the terrain grass's chunks, so they use the same GPU culling, LOD thinning,
  shadows, wind and visibility-buffer path, and a species costs only its blades. The scatter
  key includes the species settings, and painting re-scatters only the painted region. A species
  draws even when the terrain grass map is empty.
- **Detail meshes** (any primitive or imported mesh + material): scattered from their map with
  random scale and turn, optionally tilted with the ground, and drawn as ordinary mesh draws
  sharing one material, so the renderer's automatic instancing batches them. Each draw is
  culled by its size on screen past the layer's draw distance (a LOD range). Instances are
  cached per terrain version, capped at 20 000 per layer.
- Brush: paints towards **Target Density**; Shift erases. **Fill** / **Clear** apply to the whole
  map.

## 4. Placement brushes

- **Paint Trees** (terrain tree instances): existing spacing and random height/width, plus a
  slope range (min and max), height limits (terrain altitude) and optional random rotation.
  Shift still erases (only the picked tree).
- **Place Objects** (`terrain/placement.rs`, `terrain_place_ui.rs`): a palette of prefabs
  (rocks, bushes, props). The brush scatters instances as scene objects under the terrain's
  "Placed Objects" child, with density (per 100 m²), minimum spacing, random scale range, random
  rotation, align-to-normal, slope range and height limits, never over holes. Shift erases, either
  everything in the circle or only the selected prefab's instances (by prefab link). A stroke
  is one scene undo step, taken at its first change. **Clear All** removes them all.

## 5. Scatter to terrain trees (`world_gen/mod.rs`)

- Scatter Storage ▸ **Terrain Trees (editable with Paint Trees)**: tree items become the
  terrain's own tree instances in `generate()` (on the worker thread), so the World Builder's
  terrain undo step covers them. Each scatter item has a **Terrain Tree** asset. When it is
  empty, tree shapes use the World Builder's own Oak / Birch (`Assets/WorldBuilder/Trees/`,
  written on first use). Other items stay scene objects (Auto rule).
- Regenerating replaces the trees of those prototypes and keeps other trees (painted by hand
  with other assets). Generations with the other storages now keep the terrain's trees too;
  before, they reset the terrain data and dropped painted trees.

## MeadowDemo

- Nothing changes for terrains without holes, extra layers or detail layers: same index
  patterns, four-slice layer arrays, the same shader path (identity slot indices), the same blade
  scatter (species add none), and the same Auto Paint output (quantised over 16 channels with
  zeros: identical bytes).
- Golden shots: all 20 match (`golden_shots`, mean 0.15–0.39, at most 0.03 % of pixels changed).
- Frame time (`meadow_perf_times`, 1080p, GPU ms, baseline main 7d17100 / this branch, mean of
  two alternating rounds under the GPU lock):

  | view | High | Medium | Low |
  | --- | --- | --- | --- |
  | wide | 15.47 / 15.46 | 11.39 / 11.40 | 7.65 / 7.66 |
  | behind | 16.60 / 16.62 | 11.79 / 11.81 | 7.72 / 7.73 |

  Same triangle and draw counts; CPU render time is within noise.
- Found while making the demo: the terrain rule range slider clamped stored values to the
  terrain height when displayed (a rule's default 1000 m top), which rewrote and saved the asset
  just by opening Paint Texture. Fixed with `clamp_existing_to_range(false)`.

## Not done / next

- Holes cut whole heightmap cells (no sub-cell hole texture). A hole in flat ground shows what
  is under the terrain, so caves need their own meshes (the demo puts a pit under its sinkhole).
- Grass species share the terrain grass's colours and wind (they pick a point between healthy
  and dry plus brightness), and need Grass & Details on.
- Detail meshes are plain instanced mesh draws, with no billboards, GPU scatter or wind. Very
  dense mesh layers on very large terrains hit the 20 000-instance cap and are thinned.
- Placed objects are scene objects (like Polybrush), not stored in the terrain asset.
- World Builder generation still rebuilds holes, extra layers and detail layers from its own
  pipeline; only trees are carried over.
