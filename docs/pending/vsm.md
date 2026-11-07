# Virtual shadow maps for the sun (branch vsm)

Goal: sharper, more stable sun shadows than the cascades at the same or lower cost, with
cached static pages making most frames cheap. Behaviour follows the published virtual shadow
map / clipmapped page table descriptions; the code is our own.

**Status:** Quality ▸ Shadows ▸ **Virtual** works for every lit surface (meshes, terrain, the
grass resolve and lit pass, trees, water, custom shaders, the temporal shadow mask), the fog and
sun shafts. Every preset stays **Cascaded** (see "Default" below).

## Design

| Part | What it does | Where |
|---|---|---|
| Clipmap | Levels of 8192 × 8192 texels (64 × 64 pages of 128 × 128) centred on the camera. Level 0 texel = 1/1024 m × Shadow Resolution's texels per pixel (Very High ½, High 1, Medium 2, Low 4); each level doubles. The last level is the "safety" level: pages ≥ 1/8 of the shadow distance, every page within the distance requested (≈ 70–120 pages), so there is always a coarse fallback. High (150 m): 9 levels, 1 mm … 25 cm. | `vsm/clipmap.rs` |
| Page grid | Fixed in light space (the light view has a fixed origin, as the cascades'); a page's depth never depends on the camera. The page table is toroidal: page `g` lives in slot `g mod 64` of its level (R32Uint 64 × 64 × 12, physical page + 1). | `clipmap.rs`, `vsm_common.wgsl` |
| Depth | One light-space depth range for all pages: the casters' bounds, padded and snapped to 64 m (a change re-renders everything). Camera-relative coordinates in the shaders (no precision loss away from the origin). | `clipmap::depth_range` |
| Page marking | A compute pass over every other pixel of a depth buffer — the screen-space prepass's single-sampled one right after it when it runs (Medium and up), else the main pass's multisampled one after the main pass, then only every other frame (storing and decompressing that depth is most of the cost) — marks, for pixels within the shadow distance, the page at the level the pixel's footprint picks, every coarser level up to the soft-shadow search level and the grass taps' level, and neighbour pages within 12 texels of a border. 1 bit per table slot, read back asynchronously (one frame later in practice). **Guard band**: the 8 neighbours of every marked page are requested too, so a turning or moving view finds its next pages rendered instead of sampling a coarser level for a frame. | `vsm_mark.wgsl` |
| Cache | Wanted = marked within the last 90 frames + the safety disk. Wanted pages get a physical page (coarse levels first); least-recently-wanted unwanted pages are evicted when the pool is full; wanted pages that don't fit are left out (sampled from coarser levels). Pages leaving the window stay cached for a return. | `vsm/cache.rs` |
| Invalidation | New pages; pages under a caster that moved, appeared, vanished or is posed (skinned: every frame) — detected by a signature per caster (mesh fingerprint, model matrix bits, shadow variant), so no ids are needed; pages under a terrain chunk whose level of detail or edge mask changed; everything when the clipmap changes, a terrain is edited or the sun jumps more than 5°. **Turning sun** (time of day): the shown pages stay while the sun is within 0.1° of their light; past that a second set of pages ("set 1") for the new light fills in the background (128 pages a frame, marked by the same pass with the new light's parameters) and takes over once every page it wants is rendered, then the old set is freed. Shadows lag the sun by about 0.1° plus a few frames, never drop out, and the cost is spread over frames. | `vsm/mod.rs` `caster_changes`, `terrain_changes` |
| Rendering | This frame's pages (≤ 384, coarse and never-rendered first) are grouped per level into rectangles of up to 16 × 16 neighbouring pages, shelf-packed into a 2048² scratch depth atlas (≤ 2 fills a frame), each rectangle drawn once with its own view through the ordinary caster pipelines — mesh batches culled per rectangle by the frame lists, terrain chunks culled per rectangle, trees (their own lists per rectangle, detail by the level's texel, drawn past the camera's tree distance too so cached pages never miss a tree), virtual geometry (its shadow views, texel-sized detail; rectangles capped to its 6 views when it is on) — then each page is copied into the pool by one instanced quad. | `vsm/rects.rs`, `vsm/passes.rs`, `vsm_copy.wgsl` |
| Sampling | `sun_shadow` / `sun_shadow_taps` (shadows.wgsl) branch to `vsm_*`: the same level choice as the marking, page lookup with fallback to coarser levels while a page isn't rendered, normal offset and bias in the level's texels. Hard: one bilinear compare. Soft (PCSS): blocker search at the level where the search radius (sun size × 60 m) is ≤ 8 texels, filter at the finest level (≥ the pixel's) where the penumbra radius is ≤ 8 texels — contact edges filter at the pixel's level (1.5 texels = a few mm close up), wide penumbrae at coarser levels; each tap is looked up on its own so kernels cross page borders, through the kernel's centre page when it lands there (most taps). The hardware compare is clamped half a texel inside the tap's page. Grass taps run at the level that holds their spread in ≤ 4 texels. | `shaders/shadows.wgsl` |
| Fog / shafts | The froxel fog and sharp sun shafts read cascade-shaped layers (`shadow_access.rs`); with VSM on, four 1024² layers are filled from the virtual map by a texel lookup each (the same light view, so a cascade texel is a page column), instead of drawing casters. | `vsm/fog.rs`, `vsm_fog.wgsl` |
| Near layer | Casters that move on their own can't be cached in pages: a 2048² **near layer** around the camera (±32 m, or the grass caster range if larger; 3 cm texels) is redrawn every frame with the grass shadow casters past the near grass map (the cascades' widened share, outside its 12 m square — High 20 m, Ultra 32 m) and the trees whose box, seen from the light, lies within 90 % of its square (all of it to leave: hysteresis), whose leaves and limbs sway. Those trees are left out of the cached pages; a tree entering or leaving the layer re-renders the pages under it. Every lit surface multiplies it into the sun's shadow (`vsm_near_shadow`: four hardware-filtered taps for soft shadows, one for the grass's fast taps). Off — not drawn, not sampled — while nothing would go into it. | `vsm/passes.rs`, `trees.rs` `near` |
| Trees | Past the near layer, cached with the wind pose of the frame their page was rendered; detail per page level (full ≤ 3 cm texels, LOD 1 < 20 cm, impostors beyond). | `trees.rs` |
| Settings | Quality ▸ Shadows (Cascaded / Virtual), Shadow Page Pool (512 / 1024 / 2048 / 4096 pages, 32–256 MB, at most 8192 texels a side; presets Low 512, Medium 2048, High 2048, Ultra 4096), Shadow Resolution = texels per pixel in Virtual mode. Cascade options hide in Virtual mode. New fields stay out of project files at their defaults. | `quality.rs`, editor `project_settings_quality.rs` |
| Debug | Scene view draw modes "Virtual Shadow Levels" (level tint, page borders), "Virtual Shadow Cache" (green cached, red rendered this frame, magenta = sampled from a coarser level), "Virtual Shadow Pages" (each physical page its colour = the page table's mapping). `Renderer::vsm_stats()` (wanted, resident, rendered, invalidated, rectangles, evicted, overflow, pending, reset). | `vsm_debug` in shadows.wgsl |

Bindings: lit group 1 gets 10 (page table) and 11 (pool); 1×1 dummies while off. The shadow mask
compute pass gets the same two.

## Measurements

RTX 4060, 1920×1080, GPU timestamps, quietest of 2 alternating rounds (`meadow_perf_vsm`,
`scene_perf_vsm`; "moving" = 4 m/s forward and turning 15°/s at 60 frames a second). Other agents
shared the machine; differences under ~0.15 ms are noise.

MeadowDemo, wide view (GPU frame ms):

| level | cascaded still | **virtual still** | cascaded moving | **virtual moving** |
|---|---|---|---|---|
| High | 15.44 | **15.17** (−0.27) | 15.45 | **15.24** (−0.21) |
| Medium | 11.83 | 12.23 (+0.40) | 12.22 | 12.37 (+0.15) |
| Low | 8.08 | 8.28 (+0.20) | 8.35 | 8.53 (+0.18) |

MeadowDemo, behind view (towards the trees): High 16.87 → **16.60** still, 15.94 → **15.46**
moving; Medium 12.87 → 13.18 still, 12.16 → 12.47 moving.

Where it goes (High, wide, still): cascades 0.48 ms → pages 0.13 ms (the fog layers' fill is in
it; 0 pages rendered) + marking 0.11 ms; sun shafts 1.16 → 0.95 (the marking already
decompressed the MSAA depth they read); grass culling 0.41 → 0.32 (no cascade lanes); terrain
2.63 → 2.68, grass shading 1.45 → 1.58, shadow mask 0.93 → 0.99 (page lookups). Medium's
cascades cost only 0.14 ms and Low's (one 512² cascade) under 0.05 ms, so the lookups and the
marking don't pay for themselves there.

Cached pages: High 257 resident (wide), 0 rendered per frame still, 0.9 per frame moving; Medium
122 / 0 / 0.2; Low 104 / 0 / 0.1. First frame: the safety disk plus ~100–200 pixel pages, within
one frame's budget.

Static-heavy scenes (GPU ms, shadow passes in brackets):

| scene / level | cascaded still | virtual still | cascaded moving | virtual moving |
|---|---|---|---|---|
| StressStatic High | 5.22 (0.68) | **5.16** (0.09) | 5.08 (0.66) | **4.94** (0.07) |
| StressStatic Medium | 3.18 (0.33) | 3.42 (0.08) | 3.14 (0.32) | 3.34 (0.07) |
| StressStatic Low | 2.02 (0.21) | **1.88** (0.08) | 1.94 (0.20) | **1.78** (0.07) |
| ShadowsDemo High | 2.48 (0.06) | **2.39** (0.06) | 2.61 | 2.58 |
| ShadowsDemo Medium | 1.37 (0.02) | 1.54 (0.06) | 1.45 | 1.63 |
| ShadowsDemo Low | 0.79 | 0.85 | 0.80 | 0.87 |

(The Cycles room has no sun; virtual shadow maps are sun-only, so it is unaffected.)

## Look

Screenshots in `docs/screenshots/gfx/` (cascaded | virtual | difference × 8):

- `vsm_shadowsdemo_low_zoom.png`: Low's single 512² cascade (15 cm texels) against 4 px texels —
  the table legs, the figure's legs and arms get their own shadows; the hard edge shows the
  4-pixel texel steps of Low's resolution. `vsm_shadowsdemo_high(_zoom).png`: High, contact
  shadows harden at the feet and poles.
- `vsm_stressstatic_high.png`: the cubes' and spheres' shadows tighten (mean 0.95 levels).
- `vsm_meadow_high_{wide,close,oak}.png`, `vsm_meadow_high_oak_zoom.png`: MeadowDemo High, mean
  0.04 / 0.08 / 0.17 levels; the visible change is the oak's crown, whose sunward lobe is lit in
  VSM where the coarser cascade shadowed it.
- **Stability**: `shadow_edges_do_not_crawl_when_the_camera_slides` (GPU test) slides a top-down
  view by whole pixels and finds the same shadows shifted (mean < 0.25 levels): pages on a
  world-fixed grid don't crawl. A moving perspective camera re-renders only new pages, and a
  page's content is the same whenever it is rendered.
- **Default look**: with Virtual off, the 9 MeadowDemo default shots (High / Medium / Low ×
  wide, behind, oak) against main 1ee5aa9: 8 bit-identical, `high_oak` differs in 2 pixels by
  1 level (the lit shaders compile differently with the new bindings and branch).

## Round 2 (branch vsm2): default-ready

What changed, and why:

1. **Grass past the near grass map** — the near layer takes the grass shadow casters out to their
   range (High 20 m, Ultra 32 m), drawn every frame like the cascades' grass lanes it replaces
   (the culling's first cascade lane), outside the near map's 12 m square.
2. **Swaying trees** — first tried: re-rendering the fine pages under nearby crowns round-robin
   (96 pages a frame). It looked right but cost 8–12 ms GPU and 40–60 ms CPU a frame at High (every
   page re-culls and redraws terrain and trees): dropped. Instead the near layer draws the trees
   near the camera every frame (their real wind pose) and the cached pages leave them out; a tree
   crossing the layer's edge re-renders the pages under it. Cost: the layer's pass is 0.12 ms at
   High with grass in it, more with trees; walking past the oak (it joins the layer, ~5 pages a
   frame re-render around it) High measures 12.78 ms against the cascades' 12.99. A tree joins
   only when its whole box, seen from the light, fits the layer (within roughly 15–20 m in
   MeadowDemo); further trees keep a cached pose.
3. **Turning sun** — background page sets (see Invalidation): no frame re-renders everything,
   shadows never drop out (`a_turning_sun_refills_pages_in_the_background`). At 0.5°/s: High 30
   pages a frame on average, Medium 14, Low 9, Ultra 80 — GPU time within noise of a still sun.
4. **Medium / Low cost** — one page-table load per pixel instead of two to four (the found page is
   the kernels' centre), the grass's four taps laid out in light space (no per-tap transform),
   marking from the screen-space prepass's single-sampled depth when it runs (the main pass then
   no longer stores its multisampled depth for it) and every other frame otherwise, the near layer
   off while empty. Medium went from +0.37 to +0.17 ms, Low from +0.22 to +0.21 ms against the
   cascades (wide view, still); the rest is the page indirection in the lit grass and terrain
   (+0.1 ms each), which their nearly free cascades (0.14 / < 0.05 ms) can't pay back.
5. **Late pages on fast turns** — the guard band (neighbours of marked pages requested too). Turning
   90°/s: pages sampled from a coarser level for a frame went from 8.3 to 0.1 a frame (Ultra), 2.1
   to 0 (High), 0.4 to 0 (Medium), 0.1 to 0 (Low); resident pages +50 %.

Also fixed: Ultra's 4096-page pool was 16384 texels tall (over the 8192 limit; Virtual at Ultra
couldn't start) — pools past 2048 pages now use 64-page rows.

### Measurements (round 2)

RTX 4060, 1920×1080, GPU ms, quietest of 2 alternating rounds (`meadow_perf_vsm` with
`MISTY_VSM_TURN=1`, `scene_perf_vsm`). Walk = 4 m/s turning 15°/s; sun = 0.5°/s; turn = 90°/s.

MeadowDemo, wide view:

| level | cascaded still | **virtual still** | cascaded walk | **virtual walk** | cascaded sun | **virtual sun** | cascaded turn | **virtual turn** |
|---|---|---|---|---|---|---|---|---|
| Ultra | 21.33 | **19.17** | 20.97 | **19.09** | 21.31 | **19.23** | 21.94 | **19.99** |
| High | 15.25 | **15.15** | 15.31 | **15.27** | 15.30 | **15.23** | 16.24 | **16.20** |
| Medium | 11.96 | 12.13 | 12.14 | 12.35 | 11.93 | 12.10 | 12.24 | 12.56 |
| Low | 8.08 | 8.29 | 8.42 | 8.61 | 8.11 | 8.28 | 8.38 | 8.62 |

MeadowDemo, 12 m towards the oak (`oaknear`):

| level | cascaded still | **virtual still** | cascaded walk | **virtual walk** | cascaded sun | **virtual sun** | cascaded turn | **virtual turn** |
|---|---|---|---|---|---|---|---|---|
| Ultra | 18.67 | **16.76** | 19.08 | **17.26** | 18.61 | **16.92** | 19.03 | **16.77** |
| High | 13.37 | **13.22** | 13.76 | **13.65** | 13.42 | **13.27** | 13.77 | **13.19** |
| Medium | 10.36 | 10.69 | 10.79 | 10.80 | 10.23 | 10.66 | 10.25 | 10.43 |
| Low | 6.81 | 7.09 | 7.00 | 7.30 | 6.58 | 7.05 | 6.85 | 6.99 |

Under the oak (`oakunder`, 20 m towards it): Ultra 19.10 → **17.23** still, 18.50 → **16.45**
walking (the oak joins the near layer), 18.99 → **17.28** sun; High 13.65 → **13.52**, 12.99 →
**12.78**, 13.67 → **13.58**; Medium 10.44 → 10.64, 10.04 → 10.27, 10.46 → 10.91; Low 6.56 → 6.86,
6.30 → 6.75, 6.78 → 7.14.

Ultra's cascades are four 4096² maps redrawn every frame (≈2.5 ms with the grass casters); the
virtual map renders 0 pages a frame still, 3–4 walking, ~80 with the sun turning.

Static-heavy scenes:

| scene / level | cascaded still | virtual still | cascaded walk | virtual walk | cascaded sun | virtual sun |
|---|---|---|---|---|---|---|
| StressStatic High | 5.23 | **5.19** | 5.08 | **4.93** | 5.22 | 5.43 |
| StressStatic Medium | 3.18 | 3.41 | 3.02 | 3.20 | 3.18 | 3.41 |
| StressStatic Low | 2.01 | **1.88** | 1.93 | **1.74** | 2.04 | **1.88** |
| ShadowsDemo High | 2.47 | **2.40** | 2.61 | **2.59** | 2.47 | **2.42** |
| ShadowsDemo Medium | 1.37 | 1.56 | 1.44 | 1.61 | 1.37 | 1.56 |
| ShadowsDemo Low | 0.78 | 0.84 | 0.80 | 0.84 | 0.78 | 0.84 |

Look: MeadowDemo shots (cascaded against virtual, mean difference): High wide 0.05, close 0.08,
oak 0.17, behind 0.31; Medium 0.00 / 0.12 / 0.16 / 0.19; Low 0.01 / 0.26 / 0.04 / 0.27; under the oak
0.34 (`vsm2_high_oakunder_zoom.png`: the ground shadow's dappled edge is crisper). The grass between 12 and 20 m shades as the
cascades' grass casters did (band means within 0.3 levels). Screenshots `vsm2_*.png`.

## Recommendation

- **Ultra → Virtual**: 1.7–2.2 ms faster in every MeadowDemo setup (still, walking, sun, fast turn)
  and sharper.
- **High → Virtual**: equal or faster everywhere measured (−0.03 to −0.6 ms), sharper close up
  (1 mm texels against 8 mm), no shimmer, a turning sun handled; StressStatic with a turning sun is
  the one case 0.2 ms slower.
- **Medium, Low → stay Cascaded** for speed (+0.15–0.45 ms MeadowDemo, +0.2 ms StressStatic Medium);
  Virtual is the choice there only for the look (Low's one 512² cascade has 15 cm texels).

Defaults are unchanged: the user decides after the shots.

## Tests

- `vsm::clipmap::tests` (levels, windows, world-fixed grid, wrap, safety level, depth range),
  `vsm::cache::tests` (allocation, caching, invalidation, LRU eviction, overflow, budget),
  `vsm::rects::tests` (grouping, packing), `vsm::tests` (uniform layout, caster / terrain change
  detection).
- GPU: `virtual_shadows_land_where_the_cascades_do`, `a_still_view_stops_rendering_and_a_moving_caster_renders_only_its_pages`,
  `debug_views_render`, `shadow_edges_do_not_crawl_when_the_camera_slides`,
  `a_turning_sun_refills_pages_in_the_background`; cache tests for the second page set; every pool
  within 8192 texels.
- Harnesses (ignored): `meadow_perf_vsm` (still / walk / sun, `MISTY_VSM_TURN=1` adds the fast
  turn with and without the guard band; views `wide`, `oaknear`, `oakunder`, …), `scene_perf_vsm`,
  `scene_shots_vsm`; `meadow_perf_shots` with `MISTY_PERF_OPTS=vsm`.

## Left

- Medium / Low stay 0.15–0.45 ms slower than their nearly free cascades: the page indirection in the
  lit grass and terrain. A GPU-written per-tile level/page hint, or folding the table lookup into
  the visibility-buffer resolve, would be the next things to try.
- Trees only join the near layer when their whole box fits it; a tree too large for it near the
  camera keeps a cached wind pose (its leaves sway in the lit pass, not in its shadow).
- The near layer's soft shadow is a fixed four-tap ring, not PCSS: trees close up cast crisper
  shadows than their cached PCSS shadows further out.
- A sun jumping more than 5° (or dragged quickly in the editor) still re-renders everything at once.
- GPU-driven page allocation and per-page instance culling would remove the readback latency
  and the CPU work; virtual geometry is capped to its 6 shadow views per frame.
- Local lights (spot / point) still use their own maps.
