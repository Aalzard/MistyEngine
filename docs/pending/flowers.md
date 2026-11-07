# Wildflowers (branch flowers-leaves)

The user: "add some flowers here and there. The flowers could be like purple flowers, red
flowers, white one, yellow ones. And they could be spread around the terrain. But not
excessively. But on some areas."

## Decisions

- **Flowers are grass blades of their own kind**, not a renderer of their own
  (`terrain/flowers.rs` scatters them into the grass chunks: `details::scatter_level` appends
  them). So they are streamed per 16 m chunk on the scatter workers, kept in the grass blade
  pages, culled on the GPU, drawn into the grass visibility buffer and shaded once per
  visible sample by its resolve, shadowed (cascades and the near grass shadow map), and swayed
  by the same wind (`grass_gust()` untouched, still a pure function); the wind's previous-frame
  evaluation gives them correct motion vectors for TAA like any blade. The cost is a few
  thousand more instances among the grass's millions.
- **Encoding** (`Blade`, 72 bytes as before): `shape.z = 2 + kind × 3 + card` (grass keeps
  its dry mix 0..1 there), `extra.x = 2 + key` (the seed-head flag: stem behaviour, no twist),
  with the key scaled into `[0, FLOWER_KEY = 0.004)` so flowers sort first in every chunk and
  survive every LOD level and drawn prefix; the GPU cull records give them key 0. The vertex
  stage thins flowers with their own curve (`grass_flower_keep`: all up to the flowers' LOD
  distance, 25 m, then (lod/dist)², never under 40 %) and grows the ones left up to 3× bigger
  (`grass_flower_grow`) so far patches keep their colour. A flower shrunk away is skipped
  whole (never a flat fan like grass).
- **Three cards per flower**: two crossed plant cards (the atlas's plant from the side) that
  bend along the blade arc like a stem, and a head card (the atlas's head from above) lying
  nearly flat at 92 % of the stem's arc, tilted its own way and turned about its centre — so a
  flower reads from the side and from above. Ten vertices each like a blade (eight far).
- **Textures**: one atlas per kind, 256 × 1024 (head from above in the top square, the plant
  from the side below, its stem at the bottom centre and its head at `HEAD_AT`): albedo,
  normal, roughness, translucency. The renderer packs up to four kinds into two more layers
  of the grass atlas, now a three-layer array (layer 1 albedo, layer 2 = normal x, y,
  roughness, translucency, stored so the sRGB layer decodes them back to linear): no new
  binding, so the lit pass and the visibility resolve stay within their texture limits. The
  flower layers are built on a worker thread while streaming (in place for tests) and cached
  per texture set; until they arrive the flowers' layers are empty (nothing drawn). Card
  coordinates past 64 are flower atlas uv (`grass_atlas_uv` returns the layer), so every grass
  pass samples flowers without changes to its gradients.
- **Shading** (`grass_flower_lit`): the atlas colour × a per-flower tint, the normal map in
  the card's frame (mirrored through the card from the back), roughness (buttercups glossy,
  poppies satin), light through the petals from the translucency map, shade low in the turf
  for stems (canopy height), heads in the light.
- **Placement** (`flowers::scatter`, deterministic, chunks add up): per kind a low-frequency
  noise field (`patch_scale`), thresholded at the quantile that gives the kind its share of
  `coverage` (each kind gathers in its own drifts), with clumps inside a patch; a few lone
  flowers (`scattered`) between patches; more at the meadow's edges (a ring around tree
  trunks and where the grass thins out), fewer on worn spots; times the grass density and an
  optional painted mask.
- **Editing** (Terrain Inspector ▸ Paint Texture ▸ Flowers): Enabled (with no kinds yet it
  adds the four sample wildflowers and writes their textures into `Assets/Flowers` when the
  project lacks them), up to four kinds (name, amount, height range, head size, four texture
  slots; + Add Flower / − Remove), Density, Patches, Patch Size, Lone Flowers, Meadow Edges,
  LOD Distance, and **Paint Flowers** (the Paint brush paints the flower mask; Shift thins it
  out) with Clear Paint. Every change is one undo step. The settings live in the terrain file
  (`flowers`, left out while default so older terrains are unchanged; the mask as base64).
- **MeadowDemo**: the four wildflowers (`SampleProject/Assets/Flowers`, CC0, README), density
  40 /m² in the middle of patches covering 7 % of the grass, 16 m patches with clumps, a few
  lone flowers (0.1 % of the patch density), more around the trees (Meadow Edges 0.6). The generator writes the same settings (`meadow_demo_matches_its_generator`
  checks them and the textures); the user's scene and player are untouched.

## Codex

`tools/flower_textures.py` was written by OpenAI Codex (`codex exec`, model gpt-5.6-sol) in a
scratch folder from a spec (species, layout, anchors, maps, value ranges) plus the grass
reference image, then reviewed on its preview sheets and rendered in the game:

- Round 1: layout and species right, but flat vector clip-art (uniform fills, polygon petals,
  weak normal maps), an angular "butterfly" poppy side view, pinwheel buttercups, a flat
  knapweed disc, odd bracket marks on the buttercup.
- Round 2 (feedback: volume and surface, real normal relief, a cup-shaped poppy side view
  with a hairy nodding bud, glossy buttercup cups, a shaggy knapweed tuft with frayed florets
  and scaly bracts, a domed daisy disc, thicker round stems, more basal leaves; interrupted
  once by the full E: drive and resumed on C:): accepted. Soft petal gradients, crinkled
  poppy petals with black basal blotches, layered knapweed florets, domed daisy discs, soft
  waxy buttercup sheen. All maps validated by the script (sizes, anchors, margins, defaults
  under alpha 0, normal relief), deterministic (SHA-256), ~53 s for all four.

## Measurements

RTX 4060, other agents' GPU work and builds on the machine; GPU timing lock held.

- **Flower cost** (`flower_cost`: MeadowDemo 1920 × 1080, 160 frames, median of the last 120,
  with vs without flowers, best of 2 interleaved rounds):

  | view | High | Medium | Low |
  | --- | --- | --- | --- |
  | wide (the player's start view) | 17.34 vs 17.24 → **0.10 ms** | 12.95 vs 12.91 → 0.04 | 8.95 vs 8.95 → 0.00 |
  | patch_far (towards the densest patch 40–60 m out) | 19.21 vs 19.05 → **0.16 ms** | 14.66 vs 14.51 → 0.15 | 10.16 vs 10.07 → 0.08 |

  Under the 0.5 ms target everywhere.
- **Golden shots**: with the grass atlas turned into an array and the flower code in, but no
  flowers in the terrain and the old leaves, all 20 shots were identical to the references
  (mean 0.00, 0 % changed): the grass path is unchanged. The meadow references were then
  updated for the flowers and leaves (see the progress note); CyclesRoom unchanged.
- **Shots** (`flower_shots`, 1600 × 900, High; `before` = `MISTY_FLOWERS=0`):
  `flowers_{before,after}_{wide,behind,patch_close,patch_poppies,patch_far}.png`.

## What still looks off

- The player's start view (`wide`) shows only a few specks of colour: the patches nearest the
  view ahead are 40 m+ away and flowers that far are a few pixels. Turned round (`behind`)
  the patches round the trees read as soft colour.
- Seen from the side at grazing angles the head cards of daisies read a little like coins
  (a flat disc on a thin stem); the plant cards are mostly hidden in the turf.
- Beyond the grass draw distance (160 m) there are no flowers and no flower colour in the
  terrain's far ground tint.
- No per-kind wind stiffness or per-kind density; flower kinds share the patch settings.
- The flower layers are built once per set of texture paths: editing a flower PNG on disk is
  only picked up when the paths change or the scene reloads (no live reload of them yet).
