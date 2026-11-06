# Frame-time hitches (branch perf-hitches)

User report on MeadowDemo in the editor: "a lot of lagging sometimes when I turn around or
zooming in/out, and FPS drops".

## Measuring

`render/hitch_tests.rs`: MeadowDemo rendered headless with a scripted camera — 60 frames still,
a 180° turn over 120 frames, then a 12 m dolly in and back out over 120 frames (300 frames in
all, the first 30 left out of the statistics). Every frame records the wall time (render call +
waiting for the GPU), the CPU time of the render call, the GPU time (timestamp queries) and the
profiler scopes, so a slow frame says what it was doing.

- `hitch_profile` (ignored, prints; `MISTY_HITCH_DUMP=<dir>` writes every frame):
  `cargo test --release -p misty_engine hitch_profile -- --ignored --nocapture --test-threads=1`
- `scripted_meadow_run_has_no_hitches`: no frame over 2.5× the median wall time after the
  warmup, and the render thread never spends more than the grass upload budget (plus the chunk
  that crosses it) in a frame.
- `demo_scenes_compile_no_shader_after_their_first_frames`: MeadowDemo, CyclesRoomDemo and
  VfxDemo with the same camera script build no pipeline after frame 1 (`pipelines_built()`
  counts every `create_*_pipeline`).

GPU times on this machine drift by a few ms between runs (the user's editor shares the GPU).

## Before (40a612e, High, 1600×900, RTX 4060)

| | median | p95 | p99 | max | hitches (> 2.5× median) |
|---|---|---|---|---|---|
| wall | 29.1 ms | 39.4 | 91.8 | 349 | 3 |
| CPU (render call) | 1.8 ms | 12.5 | 65.1 | 324 | 29 |
| GPU | 26.1 ms | 27.7 | 28.0 | 28.0 | 0 |

The first 21 frames took 110–1285 ms each (grass scattering on load). Turning alone was
smooth (wall ≤ 35 ms); every hitch came with the dolly, and every one was CPU.

## Causes found

1. **Grass chunks scattered on the render thread** (`Terrain.GrassScatter` 65–324 ms per
   frame when the camera moved into new chunks; 100–350 ms per frame for the first 20 frames
   of a scene). MeadowDemo has 600 blades/m²: a full-density 16 m chunk is 153 000 blades, each
   with five fBm lookups, two patch lookups, a tuft search and a height sample, about 70 ms on
   one core — and up to 24 chunks were built in one frame. Moving 12 m needs several new
   level-0 chunks at once, so a zoom (dolly) stalled the frame for 100–300 ms. The old code also
   dropped chunks that had become too coarse before their replacement existed (holes).
2. **Asset watcher walking the whole `Assets/` folder on the main thread every 2 s**
   (`asset_db::poll`): `read_dir` of every folder plus a stat per entry (786 files in the
   sample project) — a periodic stutter whatever the camera does ("sometimes").
3. **Terrain Inspector walking `Assets/` every frame** (`inspector/terrain.rs::terrain_assets`,
   called by the Settings tab and the Terrain Collider editor): with the terrain selected — the
   natural state while tweaking grass in MeadowDemo — every editor frame walked the project.
4. **LOD cross-fade shader variants compiled on first fade**: a LOD Group's levels fade with the
   cutout variants of the mesh and shadow pipelines, built the first time a fade started (a
   shader compile in the middle of a run, when the camera dollies past a LOD switch distance).
5. Steady, not a hitch: the GPU median itself (26 ms at 1600×900 High). The grass realism
   round measured that cost; nothing here changes the look.

## Fixes

1. `render/terrain_grass.rs`: scattering runs on worker threads (half the cores, 1–4, shared by
   every terrain, fed through a channel). Each frame the render thread takes finished chunks
   off the channel, uploads them nearest first within a 2 ms budget (`UPLOAD_BUDGET`; the chunk
   that crosses the line still goes), then hands out the nearest stale chunks, at most
   `MAX_IN_FLIGHT` = 6 at once so the queue never fills with chunks the camera has left behind.
   Results of an older generation (terrain edited, settings changed) are dropped. A chunk
   coarser than wanted keeps drawing until its finer scatter arrives — no holes, the vertex
   shader draws the coarse blades at full share meanwhile. `set_grass_streaming(false)`
   (`headless()` sets it for every GPU test) waits for the chunks in the frame that needs them,
   so pixel tests and screenshots stay deterministic and identical to before.
   `RenderStats` gained `grass_pending` and `grass_upload_ms`.
2. `asset_db::poll` walks on a thread (`misty asset watcher`); the next poll compares the
   listing. `walk` uses the directory entry's file type instead of a stat per entry.
3. The terrain data picker lists `.terrain` paths from the GUID database (already in memory);
   the walk stays as a fallback without a database.
4. `prepare_assets` builds the cutout (fading) mesh and shadow variants of every LOD'd draw with
   its normal ones.
5. Stats overlay (Game view ▸ Stats): "Hitches: N (> 2× median) worst", a frame-time graph
   (one bar per frame, hitches red, the median as a line), grass chunks pending / upload ms and
   the pipelines-built count (`render::FrameHistory`). The status bar shows "Preparing
   shaders… (N compiled)" in any frame that built pipelines.

## After (same machine, same session)

| | median | p95 | p99 | max | hitches |
|---|---|---|---|---|---|
| wall | 27.6 ms | 29.5 | 29.8 | 31.4 | 0 |
| CPU (render call) | 1.1 ms | 2.1 | 3.7 | 5.3 | — |
| GPU | 26.1 ms | 27.7 | 27.8 | 28.1 | 0 |

Load: frame 0 is 544 ms (pipelines, textures, the grass atlas), then 12–30 ms per frame while
the chunks stream in at 2–5 ms of render-thread time each. Medium: wall median 23.9, p95 26.0,
max 30.4 ms (before: 26.4 / 51.3 / 517).

## Not done / left

- Cascade staggering (far cascades every other frame) was considered and left out: the
  cascades are sphere-fitted with a two-texel margin, so a stale far cascade would miss the
  slice corners while turning (flicker at the screen edges) unless the margin grew, which costs
  resolution. The near grass shadow map can't be cached across frames either: wind moves the
  casters every frame.
- The live-reload polls (textures, models, shaders, effects, scenes, scripts) each stat their
  files twice a second on the main thread; with a few dozen files that is a millisecond or two
  and they all fall in the same frame. Not measured in the editor; worth staggering if the
  Profiler shows them.
- Frame 0 of a scene (~0.5 s) is pipeline compiles and texture decodes; compiling the shader
  modules on a thread would shorten it.
- Scatter latency: a full-density chunk takes ~70 ms on one worker, so after a long jump the
  near field is coarse for a few hundred ms before filling in.
