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
- Golden check (GPU lock): 20/20 ok, the same numbers as main (mean 0.00-0.39, changed at
  most 0.03 %). The round 33 shader change moves no shot.
- Aurora screenshots re-rendered (`MISTY_MOON_SHOTS=night`): the three aurora shots and
  `night_sky_demo.jpg` changed, the others came out byte-identical.
- Editor sanity run on a project copy under D: (`shots.py`): `docs/screenshots/integrate-1/`
  (MoonDemo, NightSkyDemo, WeatherDemo in Play under Light Rain and in Edit mode, the Lighting
  window's Moon and Night Sky sections). Turkish and Arabic checked by eye (scratch `shots/`).
- "Integration 1" section in docs/progress.md.

## Left
- Nothing blocking the merge into main (not pushed, main untouched).
- Small: in a narrow Lighting tab the phase name after the Phase slider is clipped
  ("Waxing Cre..."); as in r27, not fixed here.
- When done with this branch: delete `D:\MistyTargets\integrate`, `integrate-scratch` and
  `tmp\integrate`.
