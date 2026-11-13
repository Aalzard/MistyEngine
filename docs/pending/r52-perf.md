# Round 52: performance (branch r52-perf)

Goal: the same picture for less GPU time in the NoonValley hero view (High, 1920 × 1080; 12 ms
goal), then CPU frame time in the editor and the player, then VRAM / RAM. Grass is left to
`r45-grass-gpu`. Forked from main f101d418.

Disks: target `E:\MistyTargets\r52-perf` (baseline test binary kept as `bin\base.exe`, the
baseline editor as `bin\misty-base.exe`), TMP `C:\MistyTry\r52tmp`, editor runs on the project
copy `C:\MistyTry\r52\SampleProject`, shots `E:\MistyTargets\r52-shots`. Scripts in the
session scratchpad `r52/`: `times.sh` (`noon_valley_times`, High 1080p), `stats.py` (median
per pass over runs), `shots.sh` (`noon_valley_shots`), `diff.py` (mean difference and the
golden test's measure), `ed.sh` / `ed_round.sh` (editor capture runs with `MISTY_PROFILE`),
`prof.py` / `prof2.py` (profiler capture by scope / by scope path), `mkpatch.py` (a
`MISTY_SHADER_PATCH` file from a source edit, so a shader change is timed on the baseline
binary without a rebuild). Every timing under the `gpu-timing.lock`; RTX 4060, other agents'
builds running most of the time.

## Method

- Per-pass GPU times are the medians of `noon_valley_times` (60 frames timed per pass after 60
  to settle); "sum of parts" is the sum of those medians, which is steadier than the whole-frame
  timer (that one also counts the gaps between passes and moves ±0.5 ms with the machine's load).
- Where a pass's time goes was found by switching one thing off at a time with shader patches
  (`MISTY_SHADER_PATCH`, test builds only; diagnostics, not kept), see below.
- Images: `noon_valley_shots` (64 frames, High 1080p) of hero, down and inside before and after,
  compared by `diff.py` (mean absolute difference per channel, share of pixels more than 8
  apart, and the golden test's 5 × 5 measure).

## Where the hero's GPU time goes (before, ms)

| pass | hero | inside | down |
|---|---|---|---|
| Grass culling + near grass shadow maps | 0.91 | – | 0.86 |
| Grass visibility + shading + composite | 4.78 | 0.21 | 5.42 |
| Screen-space prepass | 1.30 | 0.76 | 0.80 |
| GTAO | 0.63 | 0.94 | 0.96 |
| Shadow mask / denoise | 0.78 | 1.54 | 0.97 |
| Sun shafts (sharp) | 0.95 | 0.84 | 0.88 |
| Main: terrain | 0.95 | 1.32 | 1.02 |
| Main: trees | 0.71 | 1.77 | – |
| Clouds | 0.64 | 0.67 | 0.10 |
| Shadow maps (cascades, local) | 0.34 | 2.56 | 0.37 |
| TAA | 0.46 | 0.43 | 0.51 |
| Main pass (rest), sky, meshes | 0.66 | 0.24 | 0.45 |
| Bloom, tonemap, finish | 0.14 | 0.14 | 0.16 |
| **sum of parts** | **13.44** | **11.70** | **12.76** |

Diagnostics (hero, one thing off at a time):

| switched off | effect |
|---|---|
| tree impostors in the screen-space prepass | prepass 1.30 → 0.78 (trees were 0.52 of it) |
| mesh trees in the prepass | no change |
| terrain in the prepass | prepass −0.11 |
| (r45: grass in the prepass) | prepass −0.6 |
| impostor normal maps read only after the coverage test | prepass 1.30 → 1.16 (kept) |
| GTAO trace / denoise | GTAO 0.63 → 0.21 / 0.43: denoise 0.20 at half resolution |
| shadow-mask PCSS taps / blur | 0.78 → 0.46 / 0.75 |
| TAA neighbourhood depth taps | 0.46 → 0.43 |
| sun shafts: all | 0.95 → 0.07 |
| sun shafts: composite / accumulate | −0.15 / −0.04 |
| sun shafts: cascade shadow taps in the march | 0.95 → 0.57 |
| sun shafts: half the steps (32 → 16) | 0.95 → 0.67 |
| sun shafts: cloud shadow taps / fog boxes / exp per step | −0.04 / −0.07 / 0 |

## Changes

| change | commit | GPU ms saved (hero / inside) | image (mean /255, golden measure) |
|---|---|---|---|
| GTAO denoise reads its 14 × 14 tile of AO texels and normals once (shared memory) | d1f556b0 | GTAO 0.63 → 0.51 / 0.94 → 0.76 | 0.00–0.04, no pixel > 8 |
| Prepass impostors: normal maps after the coverage test | cf758537 | prepass 1.30 → 1.16 / 0.76 → 0.75 | 0.00, no pixel > 8 |
| Local light shadow array made on first use (VRAM) | 4140de5e | – (NoonValley renderer allocations 520 → 460 MB) | none |
| Profiler trigger counter recounted on change / once a second (CPU) | 7d128905 | – (editor, NoonValley Play, profiler on: PlayerLoop 2.4–2.9 → 0.67 ms) | none |

## Final (base f101d418 vs this branch, 3 interleaved runs each, ms)

| pass | hero before | hero after | inside before | inside after | down before | down after |
|---|---|---|---|---|---|---|
| Screen-space prepass | 1.30 | 1.16 | 0.76 | 0.75 | 0.80 | 0.80 |
| GTAO | 0.63 | 0.51 | 0.94 | 0.76 | 0.96 | 0.79 |
| everything else | 11.52 | 11.47 | 9.99 | 10.01 | 11.02 | 11.00 |
| **sum of parts** | **13.45** | **13.14** | **11.69** | **11.52** | **12.78** | **12.59** |
| whole-frame timer | 14.69 | 14.41 | 11.68 | 11.48 | 14.02 | 13.86 |

Images, final vs base (64 frames, High 1080p): hero mean 0.001/255, down 0.020, inside 0.043
(inside's run-to-run noise is ~0.04–0.08), no pixel more than 8 apart. Golden shots: main
f101d418 itself drifts on all 16 MeadowDemo shots against `docs/golden` (mean 32–78, since
round 49 merged the user's MeadowDemo look edits: auto exposure, depth of field, clouds) and
passes CyclesRoom; this branch's golden output against main's own output: every MeadowDemo
shot ≤ 0.07 mean, 0 % changed; CyclesRoom 0.00 against the references.

## Look trade-offs not taken (for the user)

- Sun shafts on Medium (High's tier) at 24 steps instead of 32: 0.95 → 0.82 ms in the hero,
  0.84 → 0.73 ms looking at the sun; still shots differ by 0.01–0.02/255 (temporal accumulation
  hides it), motion not checked. Half the steps (16): 0.67 ms.
- Grass out of the screen-space prepass (r45's measurement): −0.6 ms, changes AO and contact
  shadows on the near turf (grass branch).

## Tried and dropped

- **Impostors depth-first in the prepass** (coverage-only depth draw, then the prepass
  impostors at equal depth, as the main pass does): prepass 1.16 → 1.18 ms (the extra vertex
  work eats the saved overdraw once the normal maps are read after the test). Reverted.
- **Sun shafts: cascade coordinates stepped along the ray** (M·eye + s·M·ray instead of a
  matrix per step): 0.95 → 1.14 ms (the per-cascade arrays spill). Reverted.

## Memory (NoonValley hero, High 1080p, `noon_valley_memory`)

Renderer allocations after 60 frames, before → after: **520 → 460 MB** (the local light shadow
array, 16 × 1024² = 64 MB, was allocated with the renderer; NoonValley has no shadowed local
light). Largest items now: "Terrain meshes and layers" 325 MB, of which made textures are
trees 160 MB (bark, leaves, impostor atlases), terrain layers 24 MB, grass and flower atlases
8 MB, the rest the grass system's buffers (grass: left to r45); sun cascades 64 MB (4 × 2048²);
fog and shafts 25 MB; clouds 15 MB. The view target is 213 MB at 1080p (4× MSAA HDR colour
and emission 132 MB, depth 33 MB): the emission target could be dropped in scenes without
emissive surfaces, but every main-pass pipeline declares it (not attempted).

## CPU (editor capture runs, `MISTY_WINDOW_SIZE=1100x650`, VSync off, 600 frames, last 300 timed)

Whole editor frames (median / p99 / worst in the timed window), base build, second run of each
(the first run of a fresh project copy imports textures and bakes Misty GI: NoonValley edit
took 126 s to settle, MeadowDemo edit ran at 40 ms a frame while baking):

| scene | edit: median / p99 / worst | play: median / p99 / worst |
|---|---|---|
| NoonValley | 7.24 / 15.13 / 151 (frame 1) | 8.56 / 15.68 / 67 |
| MeadowDemo | 7.52 / 14.67 / 178 | 8.78 / 14.03 / 122 |
| WeatherDemo | 5.06 / 8.86 / 170 | 6.53 / 12.15 / 134 |
| StreamingDemo | 2.71 / 5.70 / 152 | 10.56 / 15.49 / 103 |

Where a frame goes (NoonValley edit, profiler scopes, medians): Editor Scene panel 4.1 ms, of
which `Render` 3.6 ms: `Render.Submit.Finish` 1.7 ms (wgpu 30 encodes every recorded command
at `CommandEncoder::finish`, so this is the cost of the frame's passes and draws), queue
submit 0.3, culling 0.4, recording 0.45; Asset Files 0.35, Scene Tree 0.09, draw cache 0.18;
~1.6 ms outside any scope (egui tessellation, paint, present). In Play the game update
(`PlayerLoop`) was 2.5 ms, of which only ~0.5 ms in its child scopes: the gap was the
Profiler's own physics counters (`Physics::report_profiler`, only while recording) walking
NoonValley's ~100 000 tree-trunk colliders to count triggers.

Editor A/B for 7d128905 (NoonValley Play, profiler recording, base and new interleaved, runs 2
and 3; run 1 of each was a cold start): `PlayerLoop` median 2.42 / 2.87 → 0.68 / 0.67 ms,
main-thread work in scopes 5.55 / 6.42 → 3.83 / 3.96 ms (p99 12.9 / 23.4 → 9.2 / 7.9 ms);
whole frames 7.04 / 7.15 → 6.96 / 6.94 ms median, p99 15.6 / 17.6 → 12.6 / 9.3 ms (the editor
is GPU-bound there: 6.8 ms of GPU per frame, so the median frame barely moves).
