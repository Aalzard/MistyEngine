# Integration 1: r33-qa + r27-moon + r34-weather (branch integrate-1)

Worktree `MistyWork\integrate`, from main d1703eb3. Cargo target `D:/MistyTargets/integrate`
(`CARGO_INCREMENTAL=0`, `TMP`/`TEMP` = `D:\MistyTargets\tmp\integrate`), scratch
`D:\MistyTargets\integrate-scratch\` (`cargo.sh <log> <cargo args>` takes a build slot;
`pipeline1.sh` = build misty, regenerate docs/api, full tests; `shots.py` = editor screenshots).

## Done
- Merged in order: r33-qa (052eda68), r27-moon (e1de2fec), r34-weather (536dee5e). Conflicts:
  `docs/progress.md` (every section kept, Round 27 moved before Round 28), the locale files
  (both new tables kept), `render/volumetrics/mod.rs` (the view group takes the sky's five
  entries, moon texture at binding 11, and the weather's two, bindings 24 and 25).
- r27 loose ends: two clippy warnings fixed; GPU cost numbers in docs/progress.md; MoonDemo /
  NightSkyDemo / WeatherDemo rewritten with their write tests (no change: already current).
- Moon and Night Sky sections wrapped in `tr!()` with Turkish and Arabic entries (`[night_sky]`
  tables); the weather tooltip says Editor Scene / Scene Viewer.
- `MISTY_LIGHTING_SCROLL=<px>` (screenshots of the Lighting window's lower sections).
- `api_docs::SOURCES` reads `weather_api.rs`; docs/api regenerated.

- Checks on af2b1523 (2026-11-11, before the second power loss; logs in the scratch folder:
  `test2.log`, `clippy.log`, `fmt.log`): full `cargo test --workspace --release
  --no-fail-fast`: editor 868 passed / 0 failed / 84 ignored, engine 1466 / 0 / 98 ignored,
  player 11 / 0, doc tests ok; clippy `--workspace --all-targets --release` 0 warnings;
  `cargo fmt --all --check` clean.
- Golden check: cut by the power loss after 13 of 20 shots, all 13 ok with the same numbers
  as main (meadow low/medium/high/ultra wide, close, behind; low oak). Run it again whole.

## Left (in order)
Use `-j 4` (two shared build slots now). `gpu1.sh` in the scratch folder prebuilds, takes the
GPU lock, runs the golden check and the night screenshots.
1. Golden check under the GPU lock (`golden_shots`, 20 shots). r33 changed `mesh.wgsl`
   (`uv_centroid` for `fs_unlit`), `terrain.wgsl`, the vgeo shaders and the custom-shader wrap.
2. Aurora screenshots: `MISTY_MOON_SHOTS=night`, `sample_moon::render_moon_screenshots`
   (ignored test, GPU lock).
3. Editor sanity run on `integrate-scratch/project/SampleProject` with `shots.py` into
   `docs/screenshots/integrate-1/`.
4. "Integration 1" section in docs/progress.md.
5. Delete `SampleProject/Assets/Models/Crate_*` left by the test runs.
