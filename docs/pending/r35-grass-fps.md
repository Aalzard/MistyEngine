# Round 35 — FPS drops after walking through the grass (branch r35-grass-fps)

Report: "plays smoothly, then when I walk forwards in the grass field FPS drops from 60 to 20
until I reopen the scene; it also happens when I look backward after a while."

## Root cause (proven by reading the code; numbers below)
- A grass chunk kept the finest density level it ever had until it was 1.5 draw distances
  away (`Grass::update` only dropped far chunks; `stale_chunks` only asked for finer ones).
  Walking leaves a trail of full-density chunks behind the camera. MeadowDemo: 800 blades/m²,
  LOD distance 6.5 m, draw distance 160 m, so a full chunk is 204 800 blades (14.7 MB) where
  its distance needs 3 200 from 37 m on: 64 times too many, kept for 240 m.
- The blade pages hold 4 × 1.86 M blades (128 MB storage binding each). Standing still takes
  3.0 M; the trail fills all four pages within about 150 m of walking.
- Past that a chunk got a buffer of its own (`Store::Own`). Such a chunk can't be named by the
  grass visibility ids, so every frame it was in view the lit grass fell back from the
  visibility buffer to the old lit pass (`Grass::vis_ready`). Looking back at the trail did
  the same. And when the cull arena (8.39 M records) filled too, a chunk got no cull records
  and every pass of every chunk dropped to the CPU path (`plan`: "a full arena falls back").
- Nothing ever gave the memory back short of dropping the terrain: reopening the scene.
- v0.3.2 has the same code path (`keep_chunks(|k| near(k, camera, range * 1.5))`).

## Fix (in this branch)
- `Grass::trim`: a chunk more than one level finer than its nearest remembered camera needs
  keeps only the prefix of that level (blades are in drop-out key order; `level_counts` on the
  worker), moved by GPU copy into a best-fit free run (`BladePool::trim`, `Arena::trim`), the
  old run freed whole. At most 128 k blades moved per frame. Nothing drawn changes.
- No buffers outside the pages any more: a chunk with no room is refused and asked for again
  one level coarser (`Pressure`: bias on every chunk's level; rises on a refusal or when the
  blades held pass 75 % of the room, falls after 10 s with room for the finer level).
- Best-fit free lists; an empty last page is dropped after 10 s; no grass = no pages.
- Stats: `RenderStats::grass_chunks / grass_blades / grass_coarser`, Stats overlay line.

## Harness
- `crates/misty_engine/src/render/grass_walk_tests.rs`: `grass_walk_report` (ignored):
  MeadowDemo, 1920×1080 High, the camera walks 500 m out, looks back, rounds a 500 × 250 m
  block (2 km), looks back, 300 m back, three circles; one frame per 1/60 s; logs per second
  and at each mark (frame times, GPU parts, grass held, memory).
  `cargo test --release -p misty_engine --lib grass_walk_report -- --ignored --nocapture --test-threads=1`
  (env vars at the top of the file).
- Scratch: `D:\MistyTargets\r35-scratch\` (`walk.sh <exe> <tag>` runs it under the GPU lock,
  `main-base.exe` = main d1703eb3 + harness, `v032-base.exe` = v0.3.2 + harness from the
  worktree `MistyWork\r35-v032`, target `D:/MistyTargets/r35-v032`).

## Numbers
Before (main d1703eb3 + harness, `base-main-high.log/.csv`; machine shared with other agents'
builds, so absolute times are high; MeadowDemo, High, 1920×1080, 5 m/s):

| Mark | wall ms (FPS) | GPU ms | blades held | pages | own buffers | no cull records | grass GPU MB | device MB |
|---|---|---|---|---|---|---|---|---|
| start | 22.3 (45) | 16.5 | 2.96 M | 2 | 0 | 0 | 354 | 1441 |
| 500 m | 55.7 (18) | 40.7 | 10.21 M | 4 | 22 | 17 | 953 | 2041 |
| 500 m, looking back | 49.0 (20) | 39.9 | 10.21 M | 4 | 22 | 17 | 953 | 2041 |
| 2 km | 52.0 (19) | 40.6 | 10.21 M | 4 | 22 | 17 | 967 | 2055 |
| 2 km, looking back | 44.8 (22) | 36.3 | 10.21 M | 4 | 22 | 17 | 967 | 2055 |
| end (2771 m) | 37.6 (27) | 34.1 | 13.54 M | 4 | 37 | 32 | 1163 | 2251 |

- Fourth page at 70 m; first chunk in a buffer of its own at 115 m (visibility buffer off);
  first chunk without cull records at 150 m (GPU culling off for every chunk): GPU 17 → 42 ms.
- Peak: 16.0 M blades, grass 1382 MB, device allocator 2470 MB.
- GPU parts at 500 m: "Main: grass lit" 29.7 ms (start: Grass visibility 2.8 + shading 1.8 +
  composite 0.5). Everything else flat: terrain 1.9 → 2.2, shadow maps 0.5 → 1.1, Misty GI,
  clouds, TAA, GTAO, sun shafts unchanged; CPU render 4.0 → 3.7 ms; textures 294 MB throughout.
  So not VSM, Misty GI, trees, TAA history or a CPU leak.

After, first version of the fix (3bafb96c, `fix2-main-high.log/.csv`), same walk:

| Mark | wall ms (FPS) | GPU ms | blades held | pages | grass GPU MB | device MB |
|---|---|---|---|---|---|---|
| start | 19.2 (52) | 14.9 | 2.96 M | 2 | 354 | 1441 |
| 500 m | 19.5 (51) | 15.4 | 3.85 M | 4 | 592 | 1679 |
| 500 m, looking back | 18.5 (54) | 14.2 | 3.85 M | 4 | 592 | 1679 |
| 2 km | 19.0 (53) | 15.2 | 3.85 M | 4 | 684 | 1771 |
| 2 km, looking back | 18.4 (54) | 14.1 | 3.85 M | 4 | 684 | 1771 |
| end (2771 m) | 18.9 (53) | 14.5 | 4.21 M | 4 | 684 | 1771 |

- Frame time flat, visibility buffer and GPU culling on throughout, nothing refused or
  thinned. But all four pages grew to their limit (512 MB for 4 M blades: short runs landed in
  the room long runs need). Second version (ba5eeea2): short and long runs in pages of their
  own kind, cull records mirror the pages (one free list).

After, final version (d646fa47, `fix4-main-high.log/.csv`): GPU 15.0 / 15.3 / 14.2 / 15.2 /
14.1 / 14.4 ms at the six marks, 3.85-4.21 M blades (peak 4.6 M), grass 592-628 MB, device
allocator 1679-1715 MB, 4 pages (456 MB), nothing refused or thinned. The full table is in
`docs/progress.md` (Round 35).

v0.3.2 (`base-v032-high.log`, walk at 0.4 scale): 15.1 ms at the start, 41.3 ms after 200 m,
39.9 ms at the end; own buffers from 115 m, no cull records from 150 m: the same fault.

## Status
- Done: harness, fix, tests, numbers before / after / v0.3.2, clippy (0 warnings), fmt,
  `docs/progress.md` section.
- Running or next: golden shots (`golden.log`), full workspace tests (`fulltest.log`) in
  `D:\MistyTargets35-scratch\`.
- The worktree `MistyWork35-v032` (v0.3.2 + harness, uncommitted, detached) and
  `D:\MistyTargets35-v032`, `r35-grass-fps`, `r35-scratch` can be deleted when the round
  is merged.

## Found, not fixed
- The editor Console keeps every message and lays out every row each frame
  (`panels/console.rs`): a script printing every frame slows the editor the longer it plays.
- Pages still grow to four for 4 M blades (two kinds, each a little over one page).
