# Meadow trees (branch meadow-trees)

Goal: realistic broadleaf trees for the MeadowDemo meadow, in the soft, warm, "one piece" art
direction of the grass reference (`MistyDesign/grass_reference.png`: two lone round trees at
the meadow's edge, everything soft), cheap enough for a demo well over 100 FPS, usable like the
reference engine's terrain trees, and shipped as original, reusable sample assets.

## Decisions

- **A tree is a recipe, not a mesh file.** `.tree` assets (`misty_engine::trees`, JSON):
  seed, height, crown shape / width / base, trunk radius, leader (decurrent oak ↔ excurrent
  birch), trunk bend, limbs (count, angle, curve), side branches, leaf density and cluster
  size, tints, variation, translucency, four textures and three wind multipliers. Presets:
  oak, linden, birch. The engine builds the meshes and the impostor from it on a worker
  thread (`trees::store`; built in place when grass streaming is off, i.e. tests), so the
  Inspector can edit trees live and a project stays small.
- **Generator** (`trees/generate.rs`): a trunk with lean, wobble, taper and root flare; main
  limbs by phyllotaxis, each a cubic curve aimed at a point of the crown's outline (a surface
  of revolution, `CrownShape::profile`); decurrent trees spread their limbs' ends over the
  whole dome by index (no hole on top); side branches towards the outline (marched reach);
  leaf-cluster cards along the outer branches, spaced so the crown shell is covered about
  twice, fewer and smaller deep inside. Cards are folded along their middle (4 triangles).
  Budgets (tested): LOD 0 under 30 k triangles (the presets 7.5–9 k), LOD 1 about a quarter
  (fewer sides, every second ring, only thick side branches, a third of the cards grown to
  cover the same area).
- **Soft, one-piece crowns**: leaf normals lean 70 % to the crown ellipsoid's normal (the
  crown lights like a dome), per-vertex occlusion from the depth inside the crown and the
  height under it (also on the bark), wrapped diffuse, a wide 4-tap sun shadow on leaves
  (dappled, no speckle), light through the leaves (forward scattering against the sun plus
  diffuse through the back) only dimmed by the crown's own shadow.
- **Impostors baked on the CPU** (`trees/impostor.rs`): LOD 0 rasterised from 8 level views
  at 2× and box filtered into a 2048 × 256 atlas: albedo with the same cluster variation +
  coverage, view-frame normal + occlusion. Lit at draw time like the meshes (they follow the
  sun). The billboard turns about Y towards the camera and blends the two nearest views
  (both sampled: smooth derivatives; a dithered pick broke the mip selection and drew
  rectangles). Shadows of far trees: the impostor turned towards the sun.
- **Rendering** (`render/trees.rs`, `tree*.wgsl`): instanced per (prototype, LOD, part) from
  one per-frame instance buffer (32 bytes a tree); CPU sorting of a few hundred trees per
  frame (distance, frustum sphere, LOD with ±10 % dithered cross-fades, a far fade at the
  draw distance). Passes: main (bark opaque, leaves and impostors alpha to coverage, before
  the grass so the grass depth pass skips what they hide), the screen-space prepass (AO sees
  trees, velocity), the sun's cascades (camera LOD in cascade 0, at least LOD 1 further out).
  One texture pair per bind group (bark / leaves / impostor): the lit pass stays under 16
  textures per stage. Leaf atlases and impostors get coverage-keeping mips (per cell).
  Hooks in shared files are one call each (mod.rs prepare / main / shadow, screen_space.rs
  prepass); terrain bounds grow by `MAX_TREE_HEIGHT` (45 m) when a terrain has trees, so
  tree tops are inside the shadow fit.
- **Wind** = the terrain grass's (`terrain_grass::wind`: Grass & Details wind × the Wind
  Zone) and the same `grass_gust()` waves at the tree's foot, so gusts roll through grass
  and trees together. Trunk bend (height share²), limb sway with one phase per limb (side
  branches and their leaves share it: nothing tears), leaf flutter along the normal. The
  vertex stages evaluate the wind a frame ago too (dt from the grass clock, only with TAA),
  so swaying trees write correct motion vectors.
- **Unity-like use**: terrain data gains `trees` (prototypes = `.tree` paths, instances =
  position 0..1, prototype, rotation, height/width scale, colour; base64, 20 bytes each; left
  out of the file while empty so older terrains are unchanged) and draw settings (draw
  distance, detail distance, billboard start × the quality level's LOD bias, cast shadows).
  The Terrain Inspector's **Paint Trees** tab: prototype list (+ Add Tree / − Remove, double
  click opens the asset), brush (density, height / width ranges, spacing, max slope; Shift
  removes), Mass Place Trees, Clear Trees, Tree Rendering. Every change is one undo step.
  Trees always stand on the heightmap (only xz is stored), so sculpting keeps them grounded.
  **Assets ▸ Create ▸ Tree** makes an oak (and writes the oak textures into
  `Assets/Trees/Textures` when the project has none). A Terrain Collider also gives every
  tree a static trunk capsule.
- **Textures** (`tools/tree_textures.py`, numpy + Pillow, deterministic): bark (oak, linden,
  birch; tileable 512 × 1024 albedo + OpenGL normal) and leaf atlases (2 × 2 leafy twigs
  per species, dilated colour under alpha 0, normal maps). Written with OpenAI Codex
  (several rounds, each reviewed against preview sheets); see below.
- **MeadowDemo**: five prototypes (Oak, Oak Broad, Young Oak, Linden, Birch) placed by
  groves given from the camera (bearing, distance, count, spread, kinds): the pair on the left
  in the middle distance like the reference, a few near trees to the sides and behind the
  player, small groves to the sides, many trees along the foot of the mountains far away;
  nothing within 22° of the view ahead closer than 450 m, nothing on slopes over 20°, 9 m
  apart at least. The user's player, head bob values and scene stay as committed: the
  generator reproduces them (Main Camera off, the First Person Player prefab instance at the
  user's position) and only writes the scene file when it is missing; the prefab test
  compares without the editor's GUID table.

## Codex

Codex (`codex exec`, model gpt-5.6-sol; the configured default model was rejected for this
account) wrote `tools/tree_textures.py` in a scratch folder from a detailed spec plus the
reference image, then three review rounds: (1) leaf clusters far too sparse with stick-like
twigs, holly-like oak leaves, birch marks too big, oak bark like cracked mud; (2) oak bark
too plain; outlines too round. Every round was checked against preview sheets before the
script and its output were copied into the repo.

## Measurements

`cargo test --release -p misty_engine tree_ -- --ignored --nocapture --test-threads=1`:
`tree_shots` (1600 × 900, High, 48 frames, wind clock fixed; `MISTY_GFX_STEP=<name>`,
`MISTY_TREES=0` for the "before" shots) writes `docs/screenshots/gfx/trees_<step>_{wide,close,
behind,pair,vs_reference}.png`; `tree_cost` renders MeadowDemo at 1920 × 1080 per quality level
with and without its trees (160 frames, median GPU time of the last 120, the method of
`grass_cost`; `MISTY_GFX_SHOT=all` adds the close shot). RTX 4060, the user's editor sharing
the GPU (±0.3 ms between runs):

| view | High | Medium | Low |
| --- | --- | --- | --- |
| wide (the player's start view: the far groves) | 29.88 vs 29.65 ms → **0.2 ms** | 21.83 vs 21.87 → ~0 | 13.52 vs 13.55 → ~0 |
| close (towards the broad oak, 42 m) | 28.43 vs 27.78 → **0.65 ms** | 20.52 vs 20.17 → 0.35 | 12.24 vs 12.11 → 0.13 |
| behind (turned round: ~15 trees within 60–150 m) | 34.13 vs 32.86 → **1.3 ms** | 25.46 vs 24.90 → 0.6 | 16.03 vs 15.82 → 0.2 |

So the trees cost 0.2–1.3 ms on High (0.1–0.15 M triangles); the frame is still the grass's
(≈ 30 ms on High, the performance branch's job). 184 trees in MeadowDemo; CPU per frame: the
sort and upload of a few hundred instances (well under a millisecond, no profiler number
taken). Triangles of the sample trees (LOD 0 / LOD 1): oak ~10 k / 3 k, linden ~11 k / 2.5 k,
birch ~10 k / 2.3 k; far: 2 triangles.

## Step log

- before: MeadowDemo without trees (`trees_before_*`).
- t01 (`trees_t01_{wide,close}`): first render: impostors drew dark rectangles (a dithered
  pick between two views broke the mip selection), leaves stippled, crowns sparse, far too
  dark.
- t02: crowns fuller (decurrent limbs over the whole dome), light through leaves kept in the
  crown's shadow. Still black: the textures were converted from sRGB twice.
- t03: sRGB fixed, Codex's dense leaf atlases: trees read green and lit.
- t04–t05: twice the cards per crown (and bigger), clusters pushed out from the twigs, leaf
  tints 0.9; the per-frame fade noise follows TAA's jitter.
- t06: leaf coverage nearly binary (the hardware's alpha-to-coverage dither showed as a
  checker in broad half-covered areas).
- t07 / final: the remaining hatch around crowns was the LOD cross-fade's dither (TAA's
  clipping keeps it): fades practically off, LOD switches instant.

## Final (`trees_final_*`)

Honest comparison with the reference (`trees_final_vs_reference.png`): the start view
looks like the reference's composition (open field, a few small round trees at the field's
edge, groves at the foot of the mountains), but the reference's two trees are darker, more
muted olive and hazier; ours are a little brighter and more saturated, and the far groves
read as a row of small dots rather than soft clumps. Close up (`trees_final_close.png`) the
broad oak reads as a believable broadleaf tree with a soft, lit crown and a long soft shadow,
but its crown is still somewhat clumpy (cards visible as puffs at the outline), the limbs
below the crown look thin and young for an old oak, and the bark texture is stylised (dark
outlined ridges). `trees_final_behind.png` (front-lit) shows the soft "one piece" crowns best.

## Look round 2 (branch tree-look)

The user: the trees looked ugly, the bark rendered inside-out, the bark material looked
unrealistic; asked for Codex-made PBR textures, better leaves and a little more detail.

- **Bark inside-out (the cause).** The engine's front faces are those where
  `cross(b - a, c - a)` points out (clockwise on screen, `FrontFace::Cw`, see `render/mesh.rs`),
  and the bark pipelines cull back faces. The tube triangles in `generate.rs` were wound
  `[a, b, a+1]` (ring i, ring i+1, next side): `cross(b - a, c - a)` = along × around =
  `dir × (dir × n)` = `-n`, so their face normal pointed into the tube. Culling therefore removed the outside and drew the
  inner walls: the dark "seams" and lit back walls of the screenshot. Fixed by winding
  `[a, a+1, b]`; the vertex normals were already outward. Leaf cards were wound backwards too
  (front face away from `facing`), so the leaf shader's `front_facing` flipped the leaf normal
  map when seen from outside; fixed the same way. Not affected: shadows (no culling), impostor
  bake (depth test, no culling), prepass (same index buffer), the derivative tangent frame
  (built from the outward normal; checked analytically for the outside view), the trunk
  capsule (radius only). Test `bark_faces_point_out_of_the_limbs`: every bark triangle of both
  LODs and all presets winds with its normals, trunk faces point away from the trunk axis, leaf
  cards face out of the crown.
- **Geometry.** Decurrent trees (oaks): limbs 0.72 × the trunk radius (was 0.62), taper
  `u^1.2` (stay thick further out), a slow gnarl (sideways kinks); side branches up to 13 cm
  (was 9); root buttresses (4–6 lobes at the foot, darker hollows), trunk rings closer
  together at the foot; bark normals now come from the surface (flare, buttresses and taper
  tilt them). Bark sides 16 / 8 / 5 (LOD 0). Bark texture: square, one repeat per
  0.9 m × √(trunk radius / 0.4) round the trunk, a whole number of times round each limb
  (coarser on old trunks, finer on twigs). Leaf cards near the crown outline 35 % smaller and
  closer together (same cover): the outline breaks into small sprays. Triangles (LOD 0 / 1):
  oak 11.7 k / 3.4 k, linden 13.6 k / 2.7 k, birch 11.8 k / 2.1 k (were ~10 k / 3 k).
- **PBR textures** (`tools/tree_textures.py`, rewritten by Codex, NumPy + SciPy + Pillow,
  deterministic, ~45 s): per species 1024² tileable bark albedo, normal, roughness, AO and
  height, and 1024² leaf atlases with normal, roughness and translucency. New optional
  recipe slots (`bark_roughness`, `bark_occlusion`, `bark_height`, `leaf_roughness`,
  `leaf_translucency`; empty = plain values; in the Inspector). The renderer packs them into
  the spare channels so each draw still binds one texture pair: bark albedo alpha = height,
  normal maps = (x, y, roughness, occlusion / translucency), z rebuilt in the shader. Bark
  shader: roughness, occlusion (ambient, plus a little cavity darkening), one-step offset
  parallax (2.5 cm, fades out by 20 m, derivative tangents); leaves: roughness (a soft sheen)
  and translucency (veins and twigs pass less light); softer light through the leaves (wider
  forward lobe, wrapped diffuse through the back). The cache key and the live reload cover every
  texture.
- **Codex rounds** (scratch folder, model gpt-5.6-sol, each reviewed on preview sheets and
  100 % lit crops): (1) rewrite to the full PBR spec: still vector-like strokes, leaf sprays
  sparse fans of sticks; (2) field-based bark brief: plates from anisotropic Worley, dense but
  ball-shaped leaf clusters; (3) remove the "crocodile" hairline network, irregular sprays:
  bark regressed to strokes, leaves improved (stopped after ~55 min); (4) back to plates,
  leaves accepted; (5) the oak rebuilt on a reviewer prototype (zero-crossings of
  domain-warped anisotropic gradient fBm for the furrows, ridged fibres, cross breaks, pits),
  linden from the same machinery, birch peel bands and dark patches. Then the oak albedo was
  brightened by hand (linear mean 0.045 → 0.10: shaded trunks went black in the filmic toe) and
  the texture AO and the crown occlusion count less on bark (0.5 and 0.6 + 0.4 × crown).
- **Cost** (`tree_cost`, 1920 × 1080, trees vs no trees, GPU shared with the user's editor,
  ±0.3 ms): High wide 0.07 → 0.35, close 0.42 → 0.44, behind 1.40 → 1.28 ms; Medium wide 0.29
  → 0.02, behind 0.66 → 0.62; Low close 0.08 → 0.20, behind 0.15 → 0.40 (one before-run
  Medium close sample was a 420 ms outlier). Within noise of before.
- **Shots**: `treelook_before_*` (main) and `treelook_after_*` (this branch, after merging main
  with GI option C): trunk side (towards the player), trunk sun (sunlit side, after only), trunk
  low, crown, wide, behind; `trees_treelook_final_*` are the usual tree shots.

## What's left

- Crowns: more natural massing (bigger, overlapping clumps; fewer visible card edges), older
  oaks with thicker, more gnarled limbs; a few more species (field maple, pine for the
  World Builder's Pine Tree shape).
- LOD transitions: a dither that survives TAA without a pattern (or a short alpha fade) so
  the instant switches can fade again; impostor views from above (octahedral impostors) for
  hilltop views.
- Colour matching against the reference (darker, hazier far trees; aerial perspective).
- The World Builder's Broadleaf / Pine scatter still makes primitive objects: route it to
  terrain trees.
- Tree colliders are always on with the Terrain Collider (no per-prototype toggle yet).
- The screenshots and timings ran in release; the E: drive had only 2–8 GB free during the work.

## Look round 3 (branch flowers-leaves)

The user: "the tree's leaves still look ugly. So decrease the roughness. Make their shape more
visible."

- **Leaf atlases redrawn** (`tools/tree_textures.py`, leaf part only; the bark output is byte for
  byte the same), with Codex (model gpt-5.6-sol, scratch folder, two review rounds on preview
  sheets with 100 % cells, 128 / 64 px downsamples, a normal-lit and a backlit view):
  - round 1 (from a brief: distinct outlines, veins, per-leaf variation, folded and curved
    leaves, lower roughness, crisp alpha, 45–60 % coverage): individual leaves with midribs and
    veins and crisp edges, but the oak lobes were a regular square wave and linden and birch
    were heaps of oval "bay" leaves with comb teeth;
  - round 2: oak obovate with 4–6 unequal round lobes per side, auricles and a lobe vein in
    every lobe; linden broad heart-shaped leaves (notched cordate base, short drawn-out tip,
    irregular fine teeth) on long petioles along zig-zag twigs; birch small deltoid leaves with
    double-serrated edges on drooping twigs; leaves overlap at their edges only (42–45 %
    coverage), each its own hue and value, some undersides, rare blemishes; midrib and
    secondary veins in albedo and normal map; translucency 0.75–0.9 on the blades with darker
    veins (the vein network shows against the sun); roughness 0.30–0.42 on the upper sides,
    0.6–0.7 underneath.
- **Shader** (`fs_tree_leaves`): smoothness = 0.85 × (1 − roughness) close up (was 0.35 ×), 0.5 ×
  far away (no sparkle): a waxy sheen and glints where single leaves face the sun; the leaves'
  own normal map counts 60 % close up (was 35 %; 30 % far away), so each leaf's fold and tilt
  catch the light differently and the leaf shapes read.
- **Curved cards**: on LOD 0 each card has a third row of vertices, the top curling back into
  the crown and drooping (8 triangles instead of 4); LOD 1 keeps flat folded cards.
- Alpha: the atlases' coverage-keeping mips (per cell) and the alpha-to-coverage rim as before;
  the crisper alpha of the new atlases keeps far crowns from turning to mush.
