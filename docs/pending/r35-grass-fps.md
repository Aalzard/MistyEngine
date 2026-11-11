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

## Status
- See "Numbers" below (filled in as runs finish).

## Next steps
- Run the walk on the fixed build, compare; run fmt, clippy, full tests, golden shots.
