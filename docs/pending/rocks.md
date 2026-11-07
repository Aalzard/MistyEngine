# Meadow rocks (branch rocks)

The user: "start making rocks. We might make 5 to 7 type of rocks. Most of them are big. Some
of them are small. And spread them around the landscape."

## What was made

Six procedural rock types in `SampleProject/Assets/Rocks` (CC0, README + LICENSE), generated
by `crates/misty_editor/src/sample_rocks` (`write_rocks`) from tileable material sets made by
`tools/rock_textures.py` (Codex-assisted):

| Prefab | Stone | Size (w × visible h) | LOD 0 (virtual) | LOD 1 / 2 / 3 | Albedo / normal + mask |
| --- | --- | --- | --- | --- | --- |
| Boulder Large | weathered granite, rounded, peeling sheets, a crack | 5.2 × 2.1 m | 81 920 | 12 288 / 3 276 / 818 | 2048 / 1024 |
| Boulder Split | granite block, six joint faces, two deep splits | 3.4 × 1.7 m | 81 920 | same | 2048 / 1024 |
| Boulder Round | granite, egg-shaped, mossy top | 2.3 × 1.2 m | 81 920 | same | 1024 / 512 |
| Outcrop Layered | sandstone slab, stepped dipping layers, joints | 8.4 × 1.5 m | 81 920 | same | 2048 / 1024 |
| Rock Small | granite fragment | 0.85 × 0.4 m | 20 480 | 4 096 / 1 228 / 408 | 1024 / 512 |
| Cobbles | seven river cobbles in one mesh | ~1.2 × 0.3 m | 35 840 | 7 168 / 2 150 / 1 132 | 1024 / 512 |

Each type: `Meshes/<Name>.glb` (dense LOD 0, imported with **Virtual Geometry** on, lightmap
UVs off), `Meshes/<Name> LODs.glb` (three levels by the virtual geometry simplifier, Virtual
Geometry off), `Textures/<Name>_Albedo.jpg` / `_Normal.png` / `_Mask.png` (G = occlusion,
A = smoothness), `Materials/<Name>.mat` (Standard) and `Prefabs/<Name>.prefab` (LOD Group over
LOD0–LOD3; Mesh Collider on LOD2 except Cobbles). Every asset has its `.meta`.

## Decisions

- **Geometry** (`shape.rs`): an icosphere (subdivision 6 for the big rocks) pushed out along
  each direction by a radial function, so the surface is always closed, manifold and
  star-shaped: a superellipsoid (boxier for the outcrop), broad lumps, joint planes cut with a
  smooth minimum (rounded, weathered edges), exfoliation ledges (the radius steps where a broad
  noise crosses a level), cracks (gaussian grooves along wobbling planes, fading out), ridged
  fine detail; the outcrop gets terraced, dipping layers whose edges are pulled in by a
  per-layer amount (softer layers erode further back). The underside is squashed (settled) and
  the origin put on a **ground line** about a quarter of the height up: placed on the terrain,
  the rest is buried. Normals are computed on the welded surface, so UV seams don't crease.
- **UVs**: one layout per stone that spends the texture on what is seen. The directions are
  bent upwards (`normalize(d + lift·Y)`, a smooth bijection, `lift` chosen per stone so
  everything above the ground line lands in the upper half), mapped octahedrally, and the
  upper half (a diamond) turned 45° to fill the whole square; the buried lower half folds into a
  4 % strip along the border, the only place with seams. (A plain octahedral map wasted half
  the texture on the buried underside; the first bake showed it.)
- **Texture bake** (`bake.rs`), no shader changes: every texel of a rock's map is a point on
  its dense mesh (rasterised in UV space). Colour, relief, roughness and occlusion come from
  the tileable sets projected triplanar in object space (mip level picked from the texel size),
  then: broad weathering blotches and warm/grey patches, dirt in hollows and bleached ridges
  (from two cavity scales: distance to a Laplacian-smoothed copy of the mesh), iron and water
  stains running down the sides, lichen crusts on exposed faces, moss on top faces and in
  hollows (height-blended, so it fills crevices first), a damp, darker, grass-tinted band at
  the ground line scaled to the rock's size, soil below it. The normal map is the slope of
  the combined relief (plus pits and grain bumps between the mesh's and the texel's
  resolution) in the mesh's **own interpolated tangent frame**, i.e. exactly what
  `mapped_normal` rebuilds on LOD 0 (LODs reuse the same UVs; their slightly different frames
  don't show at their distances). The moss and lichen are "world space" in the sense the task
  meant: placed by the object's up axis and the ground line, which stay up in the meadow (rocks
  only turn about Y and tilt a little).
- **Material colour = the bake's 99.5th percentile albedo**, the texture holds albedo divided
  by it: Misty GI bounces the material colour (it doesn't read textures), so a white tint would
  have made rocks bounce like snow; this way it bounces roughly the rock's colour (still a
  little bright) and the 8-bit texture uses its full range.
- **Why baked, not a world-space blend shader**: a top-layer feature in the Standard shader
  would change the draw uniform and group 2 of every pipeline, the visibility-buffer resolve
  and every other agent's shaders; the bake gives the same look through every path (ordinary,
  virtual geometry lit and visibility buffer, shadows, GI) with zero renderer risk. Cost: unique
  textures (16 MB on disk for the six rocks; ~170 MB of GPU memory at High with mips, a quarter
  at Medium/Low's half-size textures).
- **Virtual geometry and LOD Groups together** (engine change, `render/lod.rs`,
  `draw_cache.rs`, `draw_prep.rs`): a LOD Group whose LOD 0 renderers are all virtual meshes
  gets `LodRange::virtual_lod`; while virtual geometry draws that mesh this frame (Quality
  setting on, supported, build ready), LOD 0 stays at every distance down to the group's cull
  height and LOD 1–3 are skipped, like Nanite meshes ignore their LODs. Otherwise (setting off,
  unsupported adapter, build still running) the group switches levels as usual. The virtual
  geometry meshes are now prepared before the LOD states are computed.
- **Placement** (`meadow_rocks`): 21 groups given from the camera (bearing, distance, spread,
  members), like the tree groves: the close-up group just left of the first view, groups behind
  and beside the player, a few in the open field's sides in the middle distance, outcrops and
  boulders along the foot of the mountains. Small rocks and cobbles gather round each group's
  first big rock. Rules: nothing within 16° of straight ahead closer than 450 m, 8 m clear of the
  player, slopes up to 30° (outcrops 34°, small 26°), clear of tree trunks and of each other;
  uniform scale 0.8–1.25 (small 0.75–1.35); tilted 45 % (small 80 %) towards the terrain normal
  plus a little random lean; sunk until no point of the footprint ring floats. The scene keeps
  the user's objects: `write_meadow_rocks` replaces only the "Rocks" object of the saved scene,
  and `demo_scene` adds the same rocks so `meadow_demo_matches_its_generator` still holds.

## Rendering: LOD Groups by default, virtual geometry ready

`rock_cost` (ignored GPU test, release, 1920 × 1080, RTX 4060, `gpu-timing.lock` held, median
GPU time of frames 40–160): MeadowDemo with no rocks, the rocks through their LOD Groups (70 at the time; 64 after trimming small ones, 34 big and 30 small),
and the same rocks through virtual geometry (Quality ▸ Virtual Geometry on, 1 px). GPU ms, the
difference to "no rocks" in brackets:

| view | quality | no rocks | LOD Groups | Virtual Geometry |
| --- | --- | --- | --- | --- |
| close (7.5 m from the near boulder) | Low | 10.30 | 9.71 (−0.58) | 12.43 (+2.14) |
| | Medium | 15.02 | 14.48 (−0.54) | 15.52 (+0.49) |
| | High | 20.00 | 19.45 (−0.55) | 20.37 (+0.38) |
| | Ultra | 25.63 | 25.03 (−0.60) | 26.12 (+0.49) |
| wide (the start view) | Low | 8.65 | 8.65 (+0.01) | 12.71 (+4.06) |
| | Medium | 12.63 | 12.66 (+0.04) | 14.03 (+1.40) |
| | High | 16.93 | 17.07 (+0.14) | 18.45 (+1.52) |
| | Ultra | 22.62 | 22.86 (+0.24) | 24.20 (+1.58) |
| behind (turned round) | Low | 9.36 | 14.95 (+5.59 outlier) | 13.28 (+3.92) |
| | Medium | 13.83 | 13.88 (+0.06) | 15.26 (+1.44) |
| | High | 18.88 | 19.13 (+0.25) | 20.33 (+1.45) |
| | Ultra | 24.84 | 25.21 (+0.37) | 26.24 (+1.41) |
| gallery (six rocks 13–23 m ahead) | Low | 10.83 | 10.79 (−0.04) | 12.61 (+1.78) |
| | Medium | 15.75 | 15.84 (+0.09) | 17.05 (+1.30) |
| | High | 21.01 | 21.46 (+0.45) | 22.24 (+1.23) |
| | Ultra | 26.85 | 27.53 (+0.68) | 28.02 (+1.17) |

(The close view gets *cheaper* with rocks: the boulder hides grass. Triangles drawn by the
LOD path: +0.07–1.6 M; virtual geometry draws 0.03–0.06 M main + 0.01–0.22 M shadow triangles
for 51–66 visible instances, 370–710 clusters.)

**Verdict: virtual geometry is not neutral here, so the Quality presets keep it off and the
rocks draw through their LOD Groups.** With only ~65 instances of six meshes, an ideal LOD
chain costs almost nothing (0–0.4 ms on High), while virtual geometry pays its fixed per-frame
cost (per-view culling passes, the Hi-Z pyramid it builds when GPU culling is off, the
visibility buffer's full-screen resolve, the extra main-pass split) of about 1.0–1.4 ms over the
LOD path on Medium–Ultra and 2–4 ms on Low (whose small, no-MSAA frame makes the fixed passes a
bigger share). The meshes are still imported with Virtual Geometry on, so turning Quality ▸
Virtual Geometry on (the showcase) draws them as clusters with no other change: the LOD Groups
then keep LOD 0 (see Decisions) and the cluster views work (`rocks_after_vgeo_close_clusters`,
`_levels`). Shadows (cascades, the virtual shadow map's views), Misty GI (traces LOD 0's mesh
SDF, with either path) and the visibility buffer (`VgeoStats::visibility`) all take them.
Virtual geometry pays off with many more dense instances (the `vgeo_field` rock field of 3000)
— the fixed cost is the thing to cut before it can be on by default (virtual-geometry.md "Left
to do" 1 and 7).

## Codex

`tools/rock_textures.py` (NumPy, SciPy, Pillow; deterministic, ~40 s) was written by Codex
(`codex exec`, model gpt-5.6-sol; the configured default model is rejected for this account)
from a spec (maps, conventions, tile sizes, art direction), in a scratch folder, then one review
round: (1) granite was flat grey paint with uniform black dots on a visible cell network,
lichen identical round blobs, moss soft blobs; (2) multi-scale mineral grains without cell
outlines, lobed merging lichen colonies of many sizes with rims and cracked centres, olive moss
cushions. Reviewed on its contact sheets and lit crops before copying. The bake only reads the
albedo, height, roughness, AO and (lichen) alpha maps; the normal maps it writes are for
previews.

## Screenshots (`docs/screenshots/gfx/`, 1600 × 900, High)

- `rocks_before_{close,wide,behind}.png`: the same views without the rocks.
- `rocks_after_{close,wide,behind}.png`: LOD Groups (the default path). Close = 7.5 m from the
  near Boulder Large, Rock Small and Cobbles around it; wide = the player's start view (an
  outcrop at the right edge, a boulder at the left, the view ahead open); behind = turned round.
- `rocks_after_gallery.png`: the six types in a row (Cobbles, Rock Small, Boulder Round,
  Boulder Split, Boulder Large, Outcrop Layered), front-lit.
- `rocks_after_vgeo_close.png` (+ `_clusters`, `_levels`): Quality ▸ Virtual Geometry on: the
  same image through clusters and the visibility buffer; the boulder at level 2 at 7.5 m.

`cargo test --release -p misty_engine --lib rock_ -- --ignored --nocapture --test-threads=1`
(`MISTY_GFX_STEP`, `MISTY_ROCKS=0`, `MISTY_VGEO=1`, `MISTY_GFX_SHOT`).

## Golden shots

The meadow references were updated on purpose (`MISTY_GOLDEN=update MISTY_GOLDEN_SHOTS=meadow`,
lock held): all 16 meadow shots changed (rocks in view, their shadows and GI). CyclesRoom
compared unchanged at every level (mean 0.00, 0.00 % changed).

## What still looks off / left to do

- Granite boulders still read a little like rounded loaves at a distance (the radial shape
  can't make overhangs or deep clefts); Boulder Split's lichen spots are dense and regular
  (polka dots); the outcrop's layers are a bit uniform (stacked slabs of one thickness).
- Moss is in the bake, so it follows the object's up axis, not the world's: tilting a rock far
  (more than ~30°) would put moss on a side. A real world-space top layer needs a shader
  feature (draw uniform + textures in group 2, the visibility resolve too).
- Misty GI bounces the material colour (a bright percentile of the albedo), still brighter than
  the rock's mean albedo; GI reading the albedo texture's mean would fix it engine-wide.
- Unique textures: 16 MB on disk, ~170 MB GPU at High. A shared tileable detail map (a second
  UV scale in the Standard shader) would allow 1024 bakes with the same close-up detail.
- Virtual geometry stays off by default: its fixed per-frame cost (~1–1.4 ms here) outweighs
  65 rocks; per-tile material classification and a persistent instance buffer come first.
- No placement tool yet (the optional part): a prefab brush or rocks as terrain "trees" with
  colliders would be the Unity-like way; the meadow's rocks are placed by the generator.
- The rock prefabs are not static (no static batching of LOD 1–3, no lightmap baking).

## Checks

Release profile, `CARGO_INCREMENTAL=0`: `cargo fmt --all`, `cargo clippy --workspace
--all-targets --release` (0 warnings), `cargo test --workspace --release --no-fail-fast`:
engine 957 passed, editor 595 passed, player 11 passed, 0 failed. New tests:
`rocks_match_their_generator` (files, metas, import settings, materials, prefabs, LOD sizes),
`shape::tests` (closed outward grounded rock, LOD sizes),
`meadow_rocks_are_lod_groups_over_a_virtual_mesh`,
`virtual_lod_zero_draws_at_every_distance_down_to_the_cull_height`; `meadow_demo_plays` now
builds the rocks' trimesh colliders too (mesh paths made absolute: other tests move the global
project root), `meadow_demo_matches_its_generator` covers the rocks.
