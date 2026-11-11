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

## Left (in order)
1. Full `cargo test --workspace --release --no-fail-fast` (log `integrate-scratch/test1.log`).
2. clippy `--workspace --all-targets --release` (run it after the tests), `cargo fmt --all --check`.
3. Golden check under the GPU lock (`golden_shots`, 20 shots). r33 changed `mesh.wgsl`
   (`uv_centroid` for `fs_unlit`), `terrain.wgsl`, the vgeo shaders and the custom-shader wrap.
4. Aurora screenshots: `sample_moon::render_moon_screenshots` (ignored test, GPU lock).
5. Editor sanity run on `integrate-scratch/project/SampleProject` with `shots.py` into
   `docs/screenshots/integrate-1/`.
6. "Integration 1" section in docs/progress.md.
7. Delete `SampleProject/Assets/Models/Crate_*` left by the test runs.
