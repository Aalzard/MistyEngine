# Round 13 — Occlusion Culling

## Progress

Occlusion culling like the reference engine's Occlusion Culling window:

- **Static Editor Flags** (`scene::StaticFlags`): Contribute GI, Occluder Static, Batching Static,
  Navigation Static, Occludee Static, Off Mesh Link Generation, Reflection Probe Static, set from a
  "▾" menu beside the Inspector's Static checkbox (Nothing / Everything / each flag). A flag only
  counts while Static is ticked; all are set by default and left out of files then, so existing
  scenes are unchanged and ticking Static still means "everything".
- **Occlusion Area** component (Rendering): box size / centre, Is View Volume; wire box gizmo.
- **Occlusion Culling window** (Window ▸ Rendering ▸ Occlusion Culling): *Object* (Occluder /
  Occludee Static of the selection, the scene's Occlusion Areas, Create New Occlusion Area),
  *Bake* (Smallest Occluder, Smallest Hole, Backface Threshold, Dynamic Occlusion, Clear / Bake,
  progress bar with Cancel, baked data summary and an out-of-date warning) and *Visualization*
  (camera cell, view volumes, hide culled objects; live cell and visible / culled counts).
- **Bake** (`misty_engine::occlusion::bake`): uniform view cells (side = Smallest Occluder) inside
  the View Volume areas (or the static bounds). From every cell's 27 lattice points (corners,
  edges, faces, centre — shared between neighbours) rays are cast against the Occluder Static
  triangles through the reflection-probe BVH into a depth cube (rays per side from Smallest
  Hole); each Occludee Static box is tested against the cube. Points run in parallel (rayon) with
  progress and cancel; cells whose points are all inside geometry (Backface Threshold) are marked
  invalid; each cell's set is widened by its 26 neighbours; equal sets are shared. Saved as
  `Assets/Scenes/<Scene>/OcclusionData.occ` (binary bit sets, see `occlusion/data.rs`) and named
  in the scene's Lighting settings (`occlusion.data`), so builds pack it.
- **Runtime**: `GameView` (editor Game view and player) finds the camera's cell; draws of
  occludees outside its set are skipped by the main pass via `Environment::occlusion` (shadow maps
  still draw them). Occludee Static objects stay out of static batches while occlusion is used.
  Objects changed since the bake (box differs) are always drawn. **Dynamic Occlusion** turns on
  the renderer's existing two-phase GPU Hi-Z culling for that view, which culls non-static
  renderers too. Camera component **Occlusion Culling** toggle (on by default).
- **Stats**: `RenderStats::occlusion_culled`, a new "Occlusion culled" line in the Game view
  Stats; the Scene view shows a summary box and hides culled objects while visualizing.
- **Lua**: `Camera.main.useOcclusionCulling` (get/set).
- **OcclusionDemo** (`sample_occlusion.rs`): 5 × 5 walled city blocks with courtyard statues,
  parked cars, floating drones (dynamic, Hi-Z) and an Occlusion Area; `OcclusionTour.lua` drives
  the camera round the streets (O toggles culling). Its bake: 35 × 1 × 35 cells of 4 m, 940
  distinct sets for 380+ occludees, about 3 s on 16 threads; along the tour most of the city is
  culled (test: mean culled fraction > 50 %).

## Decisions

1. **PVS from depth cubes, not per-pair rays.** Casting rays from each cell to each occludee costs
   cells × occludees × rays and is slowest exactly where culling pays off (hidden pairs never stop
   early). Instead each sample point casts one cube of rays (6 × (n+1)², n from Smallest Hole) and
   every occludee box is tested against it: cost is independent of the occludee count. Each texel
   keeps the *farthest* of its four corner rays, so a gap that a corner ray passes through counts
   for the whole texel (conservative at edges and holes down to the texel size).
2. **Samples on the cell boundary, then a one-cell dilation.** Any line from inside a cell to an
   object leaves the cell through its boundary, so visibility from the boundary covers the
   interior; the 27 lattice points sample that boundary and are shared with neighbours (about a
   third of the points). Sampling still leaves gaps between points, so each cell's set is the
   union of its neighbours' sets: popping would be worse than drawing a few extra objects. A
   randomised test checks that anything a brute-force ray sees is never culled.
3. **Smallest Occluder is the cell size and the occluder size filter**: occluders smaller than it
   (largest box side) are ignored, terrains always occlude. Smallest Hole sets the ray density.
   Backface Threshold drops sample points that see more back faces than the threshold (inside
   geometry); cells with no valid point are *invalid* and, like positions outside every View
   Volume, cull nothing (conservative; the reference engine removes such data).
4. **Boxes are clipped per cube face.** A box straddling a face's plane (an object at eye
   height crossing the horizontal plane through the sample point) is cut to the part in front of
   the face before projecting; using the whole face instead made every such object visible
   through the open +Y (sky) face. The farthest-corner texels are conservative but coarse: with
   a large Smallest Hole (few rays) gaps under or beside walls can widen a set; the cost is extra
   draws, never a wrong cull.
5. **Sample points are nudged by a few millimetres** off the exact cell planes: floors and wall
   faces often lie on them and rays grazing along a surface slipped between its triangles.
6. **Occludees are matched by Hierarchy order and box.** Scenes have no stable object ids, so
   the bake stores each occludee's world box; at runtime the n-th Occludee Static renderer uses
   bit n only while its box still matches (1 mm tolerance). Moved, added or removed objects are
   drawn — stale data never hides something wrongly. The map is rebuilt when the draw cache
   changed.
7. **The mask travels in `Environment`, not `Frame`.** `Frame` is built in ~30 places; the
   environment is per view already (fog volumes, glows and 2D lights travel the same way), so
   the player and every caller of `GameView` get occlusion culling without changes.
8. **Occludee Static objects are not static-batched while occlusion is used** (a merged chunk
   mixes objects from all over the scene and can't be hidden per object). Occluder-only statics
   still batch.
9. **Settings live in the Lighting settings** (`RenderSettings::occlusion`, left out while
   default): they get multi-scene handling, undo snapshots and saving for free, and the data path
   is a string the build's dependency walk already follows.
10. **Dynamic occlusion reuses the GPU Hi-Z pass.** The renderer already had two-phase GPU culling
   (last frame's visible set, depth pyramid, retest); a view with baked data and Dynamic
   Occlusion on simply runs it (when the device supports indirect first-instance draws and no
   AO/contact-shadow prepass or custom shader is active). PVS-hidden draws are removed from its
   candidates. GPU-culled instance counts stay on the GPU, so Stats count them as visible.
11. **Static flags default to all**, stored as a number only when not all, so the Navigation
   window and static batching keep reading the Static checkbox; only occlusion reads the new
   flags for now.

## Not done

- Portals / adaptive (octree) cells: cells are uniform; large open scenes need a View Volume.
- Per-renderer Dynamic Occlusion checkbox (it is per scene).
- Occlusion data is not split per scene for additive multi-scene setups (each scene keeps its own
  file; only the active scene's data is used).
