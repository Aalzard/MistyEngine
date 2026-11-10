# NoonValley: a noon meadow valley (branch r26-noon-meadow)

User: a scene that looks like their reference photo of a noon meadow valley (sun nearly
overhead, deep blue sky with puffy cumulus, strong aerial perspective on layered ranges, thick
uneven grass with cream and yellow flowers, a super-thick broadleaf forest on a steep
hillside, pale limestone faces with heavy trees on top), a forest interior like their second
reference (tall oak and birch trunks, dappled sun on leaf litter and twigs, ferns and broad
groundcover, bushes, light haze), eye adaptation when walking into the shade, and "if it
can't handle 90k trees, what's the point?": real time, fix the engine, don't cut trees.

## The scene

- Generator `crates/misty_editor/src/sample_noon_valley.rs` (ignored test
  `write_noon_valley`; `noon_valley_matches_its_generator`, `noon_valley_plays`); assets in
  `SampleProject/Assets/NoonValley/`, scene `Assets/Scenes/NoonValley.scene`; textures from
  `tools/noon_valley_textures.py` (original, procedural: 2K oak and birch bark with normal,
  height, AO and roughness, 2K forest-floor leaf litter with twigs, fern and groundcover leaf
  atlases).
- 4 km terrain: rolling meadow, a steep forested hillside on the right with limestone ledges
  (terraced in patches), three layered ranges ahead (1.5, 2.2, 2.8 km), a treeline 350 m
  ahead, woods behind. About 110 000 placed trees and plants: oak, linden, beech, birch, edge
  trees and bushes, a lone meadow oak, and within 700 m ferns and groundcover between the
  trunks.
- Look: Volumetric Clouds ▸ Cumulus, Fog ▸ Sky Haze (aerial perspective), Directional Light ▸
  Intensity from Sun Height, Tree ▸ Crown Shading and Clumping, grass Depth Shading, Contact
  Shadows ▸ Distant Shadows, eye adaptation (−3.2 EV down to 0: brightens in the shade).

## Engine changes (each off by default and left out of saved files at its default)

- **GPU-driven tree culling** (`render/trees_cull.rs`, `shaders/trees_cull.wgsl`, fast path
  `tree_gpu_cull`): all placed trees of a terrain live in a GPU buffer; one compute dispatch a
  frame tests each against the camera and the cascades, picks its level of detail and appends
  it to per-(view, prototype, level) lists drawn with indirect draws. The CPU no longer loops
  over trees. Used with cascaded shadows (the virtual shadow map keeps the CPU lists).
- **Small plants** (trees.rs `size_factor`, `casts_into`): prototypes with a bounding radius
  under 2 m (ferns, groundcover) get their LOD and draw distances scaled by their size and
  cast only into the cascades whose texels resolve them. Trees and bushes are unchanged.
- **Terrain ▸ Tree Rendering ▸ Shadow Detail** (`TreeSettings::shadow_detail`, default 1):
  lighter meshes and impostors sooner in the shadow cascades. NoonValley uses 0.34.
- **Tree ▸ Clumping** (`TreeAsset::clumping`): leaf cards gather into rounded lumps near the
  side branches' ends; each lump shades as its own ball (normals and occlusion), giving the
  cauliflower crowns of many sunlit lumps and dark hollows.
- **Misty GI tree proxies** are built once per terrain (cached, bucketed in 64 m cells); a
  frame gathers only the cells near the clipmap levels, and plants under 1.5 m get none.
- Earlier on the branch: terrain layer Occlusion, Volumetric Clouds ▸ Cumulus, Fog ▸ Sky
  Haze, Directional Light ▸ Intensity from Sun Height, Tree ▸ Crown Shading, Contact Shadows
  ▸ Distant Shadows, grass Depth Shading.

## Engine changes since (b3d6e414)

- **Hi-Z occlusion for GPU-culled trees** (fast path `tree_occlusion`, `TreeHiz` per target):
  after the main pass the target's depth becomes a Hi-Z pyramid (the gpu_culling pyramid
  code); next frame the tree cull skips camera-list trees whose sphere is behind it, only
  while the camera has moved < 1 m and turned < ~3.6° (else the test is off for that frame).
  Cascades are never occlusion culled. Only in the unsplit main pass and with MSAA depth.
- **Draw statistics** for GPU-culled trees: the indirect arguments are copied to a staging
  buffer and mapped after submit (never waited for); draws count the latest instance counts.

## Timings (RTX 4060, 1920 × 1080, machine heavily loaded: ±30 % between runs)

Before (f1525da9, CPU per-tree loop): ~2–3 s per frame at 90 000 trees.

t8 (300b9255…20320a86, 110 173 trees, GPU culling, before small plants / Shadow Detail /
occlusion):

| view, level | GPU ms | CPU render ms |
|---|---|---|
| hero High, GPU cull on | 26.0 | 6.0 |
| hero High, GPU cull off | 19.0 | 16.1 |
| hero Ultra | 36.8 | 6.1 |
| inside High | 34.2 | 6.1 |
| inside Ultra | 38.1 | 7.1 |
| stress, 250 000 trees, hero High | 22.1 | 7.8 |

The on/off GPU difference is mostly load noise: every part (grass, sky, post) was ~30 %
slower in the "on" run, trees included. The interleaved A/B test (`noon_valley_cull_ab`)
was added to measure it properly. The triangle counter showed ~45 M in both: that is the
grass (indirect tree draws counted 0 before b3d6e414; the CPU path's trees were ~0.2 M).
Inside the wood the shadow cascades cost 15–19 ms: High's cascades 0–2 have texels ≤ 3 cm,
so they drew full-detail trees; Shadow Detail 0.34 and the small-plant rule address that.

### After small plants, Shadow Detail, occlusion (engine 5e8edfd0, scene 3a254653)

`noon_valley_cull_ab`, 3 interleaved rounds, medians, High 1920 × 1080, 107 089 trees:

| view | mode | GPU ms | CPU render ms | triangles | shadow maps | main: trees |
|---|---|---|---|---|---|---|
| hero | GPU cull + occlusion | 20.0 | 4.0 | 45.6 M | 0.48 | 2.89 |
| hero | GPU cull | 20.1 | 3.9 | 45.6 M | 0.48 | 3.10 |
| hero | CPU lists | 19.7 | 14.4 | 45.6 M | 0.46 | 2.87 |
| inside | GPU cull + occlusion | 15.6 | 3.9 | 11.5 M | 2.59 | 2.92 |
| inside | GPU cull | 15.8 | 3.9 | 11.6 M | 2.55 | 3.28 |
| inside | CPU lists | 15.7 | 14.6 | 11.6 M | 2.52 | 3.29 |

The GPU path draws exactly what the CPU lists draw (same triangle counts; parity test mean
difference 0.000) and costs the same on the GPU; it takes the CPU from ~14.5 ms to ~4 ms.
Inside the wood the frame went from 34 ms to 15.6 ms (shadow cascades 15–19 → 2.6 ms).
Occlusion saves 0.2–0.4 ms of tree drawing here (big conservative spheres; the canopy has
gaps) and building its pyramid costs ~0.5 ms ("Tree Hi-Z"): about even in these views (the
A/B totals include both). The meadow view's 45 M triangles are the grass (all passes).

Stress, 250 000 trees (hero copied and jittered), High: GPU 22.8 ms, CPU render 4.3 ms
(t8: 22.1 / 7.8 ms; the CPU drop is the cached Misty GI proxies). Trees draw in 5.4 ms.

## Status / how to resume

- Last build round: `scratchpad/r26/tools/t11.sh r9 …` (engine-only, `SKIP_REGEN=1`), steps
  shots, unit, parity, ab, stress, aeoff from a copy of the engine test binary
  (`D:/MistyTargets/tmp/r26-noon-meadow/bin/engine_r9.exe`), shots into `scratchpad/r26/shots/r9`.
- The target dir had corrupt artifacts (machine overload: rustc metadata panics, LNK1127 /
  LNK1183): serde_core, serde, ash, image were cleaned; hashbrown, libm, zerocopy rmeta
  replaced from other agents' identical builds. Build in the shared slot
  (`/d/MistyTargets/build-slot.sh`, `-j 4`).
- Still to do: run the round (parity, A/B with occlusion, stress), check the inside view
  numbers, golden shots (MeadowDemo must not drift), fmt (rustfmt wants changes in
  `sample_noon_valley.rs`, `lightmap/bake/input.rs`), clippy, full tests, API docs check, final
  shots into `docs/screenshots/r26/`.
- Scene content (generator, textures, NoonValley assets) moved to branch `r26-level`; new
  engine settings for it: Tree ▸ Clumping, Terrain ▸ Tree Rendering ▸ Shadow Detail (already
  used by the generator at 80e185bb).

## Not done / differences from the references

- GPU time on High at 1080p is far above the 12 ms goal in the meadow view (grass ~8 ms,
  screen-space and post ~6 ms, trees ~3–4 ms).
- Look (r8 shots): inside views were near black (closed low canopy, camera in a bush,
  exposure limit); fixed in the generator at 80e185bb but not yet rendered. Far ranges showed
  pale, snow-like limestone patches; forest crowns read as dark smooth domes; too many pale
  seed heads; flowers hidden in the taller grass. All addressed in 80e185bb, unverified.
