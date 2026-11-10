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

## Timings

(see the progress section)

## Not done / differences from the references

(see the progress section)
