# World Builder: live preview, faster generation, templates

Branch `worldbuilder`. Asked for: a live heightmap preview, faster generation, one-click
templates (desert, forest, mountains, …). Placement brushes (part 4) come later.

## What `live_preview` did before

`WorldBuilder::live_preview` (default on) re-ran the whole pipeline at 129² 350 ms after the last
edit and wrote the result straight into the terrain asset in the store, so the Scene view showed
a low-res terrain. There was no image in the window, and nothing ran while you were still
dragging. Now the field means **Live in Scene view** (default off). The Preview card always
works, whether it is on or off.

## Preview card (`panels/world_builder/preview.rs`, engine `world_gen/preview.rs`)

- A collapsible card at the top of the window: a 256 px top-down image, a mode switch
  (Height / Relief / Biomes) and **Live in Scene view**. Under the image there is a small
  "129² · 24 ms · refining…" badge.
- Change detection: the job signature (settings + stamps + matrices + terrain position) plus the
  modification times of the biome files the builder uses, so editing a biome asset also
  refreshes the preview.
- **Coarse pass** (129²): starts as soon as something changes, unless a pass is already running.
  When it finishes and the settings have changed again, the next one starts right away, so
  while you drag the newest settings always win and there is no queue. The coarse pass runs the
  real pipeline with the real erosion settings. At 129² that is cheap (a few ms in release) and
  it keeps the shape honest.
- **Sharp pass** (257²): starts 300 ms after the last change. A newer edit cancels it.
- Each pass has its own stage cache (see below), so dragging a stamp skips base noise and erosion.
- Images are rendered on the worker thread. Switching modes re-renders the last result on the UI
  thread (about 1 ms at 256²).
  - Height: grayscale, stretched to the map's range.
  - Relief: hillshade (sun from the north-west), hypsometric tint, grey rock on steep ground,
    water blue by depth.
  - Biomes: the painted splat map in each layer's average texture colour, hillshaded, with water.
    Layer colours come from the procedural texture (preset stems) or a 32 px thumbnail of the
    project texture, cached by path.
- Live in Scene view pushes each pass result into the terrain store. Undo and Revert go back to
  the data from before the first push (the existing `preview_base` snapshot). Generate replaces
  it, and a push never happens while Generate is running.
- Preview fidelity: on the WorldBuilderDemo, the 129² pass correlates 0.95 with the full 513²
  result after downsampling both to 48². The 257² pass correlates 0.98. Erosion works in cell
  units, so a coarser grid carves features that are a little bigger. That is fine for a preview,
  and the gap is the reason for the sharp pass.

## Faster generation

Profiled with `cargo test --release -p misty_engine world_gen_profile -- --ignored --nocapture`
(the demo's request rebuilt in the engine, at 513² and at 1025², on 16 threads). The machine is
shared with other builds, so I took the best of several runs. Times are in ms.

Release:

| stage | 513² before | 513² after | 1025² before | 1025² after |
|---|---|---|---|---|
| Base | 41 | 22–27 | 135 | 72–96 |
| Hydraulic erosion | 56 | 45–49 | 439–564 | 304–356 |
| Thermal erosion | 57 | 13–14 | 161–167 | 43–47 |
| Stamps | 6 | 4 | 22 | 17 |
| Water | 25 | 3 | 99 | 13 |
| Classify | 22 | 13 | 72 | 50–57 |
| Textures | 5 | 4 | 16–21 | 14 |
| Scatter | 5 | 5 | 9 | 9 |
| **total** | **219** | **113–122** | **956–1101** | **527–605** |

Dev profile (opt-level 1 for workspace crates), best runs: 513² 1311 → 329 ms, 1025² 3944 →
1173 ms. Thermal is still the largest stage in dev builds: bounds checks and closures that are
not inlined at opt-level 1.

Changes:
- `par_rows` now uses rayon's pool instead of spawning one thread per core on every call. This
  was the biggest single win, because thermal erosion called it 40 times and each call spawned
  16 threads. Rows are still independent, so the result does not depend on the thread count.
- Hydraulic: droplet chunks run on rayon. The per-batch merge adds and clears the 16 chunk
  deltas in one parallel pass instead of an add pass plus 16 `fill`s. Brush erosion away from
  the edges skips per-cell bounds checks and uses a weight sum computed once, in the same order,
  so results are bit-identical to the old code.
- Thermal: pass 1 also stores how much each cell sheds (`k·most`), so pass 2 no longer
  recomputes the outflow, and neighbours of interior cells are plain index offsets. This is the
  same maths with a different summation order, so it is not bit-identical (see "Demo" below).
- Noise: the per-octave rotation `sin_cos` comes from a table built once (bit-identical).
- Water: the wet-texel stamping of rivers only visits texels inside the river's bounding box,
  in parallel (bit-identical).
- No GPU erosion. Hydraulic erosion is the biggest stage at 1025², but it now takes about
  0.3 s in release. A wgpu port would need fixed-point atomics to stay deterministic, a
  readback, and it would compete with the editor's renderer for the device. It is not worth it
  until terrains reach 2049²+. That is noted for later.

### Incremental regeneration (`generate::run_cached` + `Cache`)

Every stage's output is cached under a key hash of its inputs plus the previous stage's key. The
inputs are the `Debug` text, where floats print exactly.

base(res, size, base settings) → hydraulic(+ erosion settings, seed) → thermal(+ talus…) →
stamps(+ height-shaping stamps only) → water(+ water/river stamps, splat res) →
classify(+ zones / rock areas, breakup, seed, biome count) → textures(+ biomes) → scatter(+ biomes,
seed).

- When Generate uses heights from the current terrain (Textures / Scatter only), the heights are
  hashed by bits (fast) instead.
- `Output.reused` lists the stages that came from the cache. The Generate section shows
  "reused N", with the stage list on hover.
- The editor keeps one cache for full runs and one each for the coarse and sharp preview passes,
  because keys include the resolution.
- Each stage is a pure function, so a cached result equals a fresh run. A test checks this after
  moving a zone, a mountain and changing erosion.
- Typical wins: moving a Biome Zone skips 5 of 8 stages. Moving a stamp skips base + erosion,
  which is about 80% of the cost at 1025².

### Progress, ETA, Cancel

`Progress` now carries a plan (the stages that will run, weighted by a cost model fitted to the
release timings). `overall()` gives the fraction of the whole run. The Generate bar shows
"Generate World: Hydraulic erosion · 3 s left" with Cancel, which already existed. The ETA is
elapsed × (1 − f) / f, shown after 0.4 s. The editor's Apply step (store, save, materials, scene
objects) is timed and shown too.

## Templates (`world_gen/templates.rs`, editor `panels/world_builder/templates.rs`)

There are eight templates: Desert, Forest, Mountains, Plains, Islands, Canyon, Tundra and
Volcanic. Each sets a generator, max height, erosion, default biome, breakup and a few stamps:

| template | base | stamps | biomes |
|---|---|---|---|
| Desert | Dunes, no hydraulic, thermal at 32° | 3 Mesa stamps (Max) | Desert, Rocky Badlands zone |
| Forest | Rolling Hills | river, meadow, rock area | Forest, Grassland zone |
| Mountains | Mountains 260 m, hydraulic ×8 | ridge, valley river, crags | Alpine (new) |
| Plains | low Rolling Hills | 2 woods zones, river, village flatten | Grassland, Forest zones |
| Islands | Islands | hill, sea at 6.5 %, inland forest | Coast (new), Forest zone |
| Canyon | Canyons 180 m, talus 55° | river gorge (the bed never rises, so it cuts through mesas), sand flats | Rocky Badlands, Desert zone |
| Tundra | Rolling Hills | ridge, basin flattened to 0 m + lake at 2.5 % (always below the lowest ground, so the lake cannot leak), snowy peaks | Tundra (new), Snowy Mountains zone |
| Volcanic | Islands | volcano (Replace), lava rock area, sea | Volcanic (new), Coast zone |

- New biome presets: Alpine, Tundra, Volcanic and Coast. New procedural textures: Basalt and Ash.
  They are written to `Assets/WorldBuilder/` like the other presets, and SampleProject now has
  them too.
- Stamp positions are fractions of the terrain's width and length. Radii and river points scale
  with width / 600 m, and noise scale does too. A template keeps the builder's terrain asset,
  width / length, resolutions, scatter storage and Live in Scene view.
- Applying a template is one undo step ("Apply Desert Template"). It replaces the settings,
  deletes the builder's stamps and the Generated child, and spawns the template's stamps.
- The editor asks first ("Apply the X template? This replaces … N stamps") when the setup is
  *edited*: it has stamps, or settings that are neither the defaults nor any template's
  untouched setup. Switching between untouched templates never asks.
- The picker card has 96 px thumbnails, committed as
  `crates/misty_editor/assets/world_templates/*.png`. They are rendered headless on the CPU
  (Biomes preview of a 257² run on a 600 m terrain) by
  `cargo test --release -p misty_engine write_template_thumbnails -- --ignored`. Hovering a
  tile shows its description. The tile's accessible name is "<Name> template". The card starts
  open for a fresh builder and collapsed once the setup has been edited.
- Discoverability:
  - **GameObject ▸ 3D Object ▸ World ▸ <template>** makes a terrain and an "<Name> World" builder,
    applies the template, opens the World Builder and starts Generate. This is one undo step.
  - The Terrain Inspector's tool row has a **⛰ World Builder** button. It opens the builder that
    generates this terrain, or creates one for it.
  - The window follows a selected terrain to its builder.
  - The empty window offers the template grid under the Create button.

## Demo regenerated

The thermal change (summation order) moves heights by float noise. The demo check compares scene
JSON exactly, and scatter positions follow the heights, so I regenerated WorldBuilderDemo
(`.terrain` + `.scene`) with `write_world_builder_demo`. The demo's `live_preview` is now false.
SampleProject also gets the new presets and textures.

## Tests

- Engine:
  - preview correlation (129² > 0.93, 257² > 0.96, and sharp beats coarse)
  - images in every mode
  - sea shows blue
  - incremental cache: everything reused when nothing changed; zone / stamp / erosion edits
    reuse the right prefix and match a fresh run exactly
  - progress plan maths
  - every template is deterministic, has relief, is painted, scatters and has the water it
    promises; template pairs differ (16-bin height histograms, max height or layers)
  - templates keep the asset and scale with terrain size
- Editor:
  - coarse → sharp passes, an edit restarts the coarse pass, the terrain is untouched with Live
    off
  - Live in Scene view pushes and reverts
  - Preview card kittest (modes, Live toggle)
  - template picker kittest (apply, no prompt between untouched templates, prompt after an
    edit, Cancel keeps the edit, Replace, one undo step back)
  - thumbnails are committed
  - GameObject ▸ 3D Object ▸ World menu
  - Terrain Inspector button

## In the editor (dev build, 513², templates via GameObject ▸ 3D Object ▸ World)

The status line reports the whole Generate, including Apply: Desert 0.2 s, Mountains 0.3–0.8 s,
Islands 1.1 s, Volcanic 1.4 s. The preview badge read 26–166 ms for the sharp 257² pass and
under 50 ms for the coarse pass.

## Screenshots (`docs/screenshots/worldbuilder/`)

- `preview_Height.png`, `preview_Relief.png`, `preview_Biomes.png`: the WorldBuilderDemo
  (`MISTY_SCENE=… MISTY_TAB="World Builder" MISTY_WB_PREVIEW=<mode>`).
- `template_picker_empty.png`: the empty window with the template grid.
- `scene_Mountains.png`, `scene_Islands.png`, `scene_Desert.png`, `scene_Volcanic.png`: generated
  in the Scene view (`MISTY_WORLD_TEMPLATE=<name>` creates and generates the world; the picker
  card is open beside it).
- New capture hooks: `MISTY_WORLD_TEMPLATE` and `MISTY_WB_PREVIEW`.

## Left for later

- Part 4: placement brushes for prefabs and objects.
- GPU erosion, for 2049²+ terrains.
- Thermal erosion is still slow in dev builds.
- The preview ignores scatter, so trees are not drawn on the image.
