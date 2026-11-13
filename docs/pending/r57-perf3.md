# Round 57: performance round 3 (branch r57-perf3)

Goal: faster and leaner without changing the picture: renderer memory (NoonValley 460 MB,
view target 213 MB at 1080p), the NoonValley hero's GPU time (sum of parts ~12.5 ms, goal
12), main-thread CPU in big scenes (edit and Play), and the ~280 ms first-load frames.
Forked from main 39c3f6dd (round 55 merged).

Disks: target `E:\MistyTargets\r57-perf3` (binaries in `bin\`), baseline worktree
`MistyWork\r57-base` (main 39c3f6dd, same target dir, so the baseline editor is never
rebuilt by this branch's edits), TMP `C:\MistyTry\r57tmp`, editor runs on
`C:\MistyTry\r57\SampleProject`. Scripts in the session scratchpad `r57/` (round 55's,
re-pointed; `round.sh` = interleaved `noon_valley_times` rounds under the lock,
`memround.sh` = `noon_valley_memory`).

## Memory (NoonValley hero, High 1080p, `noon_valley_memory`)

| item | before | after |
|---|---|---|
| renderer allocations | 460.2 MB | 460.2 MB |
| view target (`RenderTarget::memory_bytes`) | 212.9 MB (shadow mask not counted) | 181.3 MB (shadow mask counted) |
| of which 4× MSAA emission image | 63.3 MB | 0 (glowless frames) |
| shadow mask buffers (2 per view) | 63.3 MB (16 B/px) | 31.6 MB (8 B/px, ≤ 2 lights) |
| view total incl. shadow mask | 276.2 MB | 181.3 MB (−94.9 MB, −34 %) |

- **Glowless main pass** (render/glowless.rs, 3f0407dc): see the module docs. Frames without
  anything that writes glow (emissive materials, blended surfaces, particles, water, VFX,
  custom shaders, virtual geometry, wireframe, depth prepass, Frame Debugger) for 30 frames in
  a row swap in main-pass pipelines built with an empty glow slot (`Spec::glowless`,
  `PipelineKey::glowless` for the mesh variants; `MainSet` in pipelines.rs, `TreeMain` in
  trees.rs and the grass composite swap with a kept copy of the other mode), drop the MSAA
  glow image and write the resolved glow from the sky in one full-screen pass (the sky's glow ×
  the share of the pixel's samples still at the far plane: what the resolve averaged). The
  glowless set is built on a worker the first time (the frames keep the glow target
  meanwhile); switching back is a swap. `MISTY_FAST_OFF=glowless` turns it off.
- **Shadow mask** (af8ce5ef): buffers sized by the light count.
- **Tree textures** (160 MB "made textures: Trees" in the test): all BC7 (checked with the
  `MISTY_TREE_TEXTURES=1` diagnostic in `GpuTree::new`): 12 tree kinds; 2048² bark / bark
  normal maps for 8 of them (5.3 MB each with mips), 1024² for the rest, leaves 1024² (2048²
  for 2), impostors 2048 × 256. Already 1 byte per texel; the mips are sampled at the
  distances NoonValley shows (trees from a few metres to kilometres; the leaf / impostor mips
  keep alpha-test coverage). Smaller bark maps would change close-ups: not done. Note the
  tree textures are not part of the "renderer 460 MB" sum (they are reported separately).

Images (noon_valley_shots, 64 frames, vs main): hero mean 0.001/255; down 0.19, inside 0.39 with
no pixel more than 1 apart — a uniform +0.2–0.4 shift that is Auto Exposure (NoonValley's is
Progressive and adapts on the wall clock, clamped to 0.25 s per frame): the test builds the
glowless set in place on frame 30 (no background loads in tests), that frame takes longer,
and exposure adapts further. Base vs base: 0.001 / 0.020 / 0.051. The GPU test
`glowless_frames_match_the_glow_target` (no auto exposure, sun in view) differs by ≤ 2.

## GPU (NoonValley High 1080p, `noon_valley_times`, medians of 3 interleaved runs, ms)

| view | base sum of parts | glowless | grass composite base → glowless |
|---|---|---|---|
| hero | 12.71 | 12.69 | 0.42 → 0.32 |
| down | 11.70 | 11.57 | 0.75 → 0.54 |
| inside | 11.51 | 11.51 | 0.06 → 0.06 |

## CPU (editor capture runs, 1100 × 650 logical, VSync off, 600 frames, last 400 profiled)

The editor is GPU-bound in both scenes (frame median = GPU median, 7–11 ms; whole-frame times
move ±30 % with the other agents' builds, so per-scope medians are compared). Main-thread work
in scopes, NoonValley Play: Render 2.1 ms (recording 0.45, `Render.Submit` 0.9 of which the
last part's finish 0.6, culling 0.3), PlayerLoop 0.74, Asset Files panel 0.27, draw cache 0.14.

| change | measured (median, before → after) |
|---|---|
| Navigation, rigs, Event System and Player Input skip their walk of the whole hierarchy when no object has the component (1d89c6c1) | MeadowDemo Play `PlayerLoop` 1.21 / 1.25 → 0.74 / 0.78 ms (Navigation.Update 0.20 → 0.003, Animation.Rigging 0.10 → 0.001, Input.Update 0.13 → 0.02, UI.Events 0.12 → 0.075) |

## First-load frames

New profiler scopes (fb50f204 and the next commit) split opening a scene and entering Play.
With a warm project copy, opening the scene itself is not the long frame:

| frame | NoonValley | MeadowDemo |
|---|---|---|
| `Scene.Open` (editor start, `MISTY_SCENE`) | 82 ms: read 3 (parse 1.8, spawn 1.3), reset 6, prefabs 3, keep the view above ground 68 (the terrain data's first load, needed by the first render anyway), remember 1 | 47 ms: read 11, reset 12, prefabs 10, view 12 |
| `Play.Start` (`MISTY_PLAY_AT=200`, 3 runs) | 293–384 ms: physics 243–337 (adding ~100 000 tree-trunk capsules 120–196 of which the trunk list 17–41; the zero-length step that fills the broad phase 119–141), scripts 42–48 (Awake) | 162–240 ms: physics 50–83, scripts 71–187 (Awake 56–65) |

The ~280 ms frames of round 55's runs were entering Play (the capture runs press Play), and
most of it is the physics world: one static rapier collider per tree trunk. What would cut it
(not done, it changes physics behaviour): the trunks as a few compound colliders (one
broad-phase leaf per terrain chunk instead of one per tree); contact events would then come
per compound, not per trunk.

## Tried and dropped

- **Tree-trunk colliders built on rayon** (trunk list and `ColliderBuilder::build` in
  parallel, inserted in order): `Physics.TreeTrunks` 17–41 → 74–262 ms (the global pool is
  busy with the frame's loops and the machine with builds); reverted.
- **Rg11b10 emission target** (instead of dropping it): would halve it everywhere but
  quantises the bloom input; not needed once the glowless pass existed.
- **TAA sharpen folded into the composite**: weather, depth of field and motion blur read the
  sharpened image, so it only works without them; ~0.1 ms at best; not done.
- **Bloom skipped when the glow is black**: the High Quality sun halo is non-zero over half
  the sky, so it never is; not done.
- **Clouds traced only where the scene depth leaves them visible**: changes the temporal
  accumulation and the upsampling at edges; not done.

## Status (paused 2026-11-13 ~17:55 by the user)

Committed: af8ce5ef shadow mask sizing; f54acae0 + 3f0407dc glowless main pass; 1d89c6c1
per-frame walks; fb50f204 + next commit profiler scopes; aad450e3 rustfmt. The parallel
trunk-collider experiment was reverted (not committed).

Still to do on resume:
- Checks: `cargo fmt --all --check` passed on the final tree; clippy and the full test run
  never got a build slot before the pause (scratchpad `r57/checks.sh`). Then golden shots
  under the GPU lock (copy the fresh `misty_engine-*.exe` from `release/deps`, run
  `golden_shots --ignored` from `crates/misty_engine`).
- Remove the baseline worktree `MistyWork57-base` (`git worktree remove`) and the scratch
  project copies in `C:\MistyTry57` when done.

