# Round 34 — Weather (rain, snow, storms) — branch r34-weather

Status: work in progress (resumable). Target dir `D:/MistyTargets/r34-weather`; build only through
`/d/MistyTargets/r34-scratch/cargo.sh <log> <cargo args>` (takes a shared build slot, `-j 4`).

## Design (written)
- `crates/misty_engine/src/weather/`: `WeatherParams` (every value) + 13 presets (`mod.rs`), the
  `Weather` component = weather volume, Global (follows camera) or Area Box/Sphere with blend
  distance, priority, weight, timeline (Sequence/Random weighted), surfaces, effects, audio,
  Preview in Edit Mode (`component.rs`); runtime `WeatherWorld` on `Scene.weather` (timelines,
  priority blend at the camera, script layer for `Weather.SetState`, wind with gusts, wetness /
  puddles / snow cover accumulation and decay, lightning, events) (`state.rs`); lightning strikes,
  branching bolts, thunder delay = distance / 343 m/s (`lightning.rs`); renderer input
  `WeatherFrame` + post adjustments (Drive Clouds, fog) (`frame.rs`); generated rain/wind/thunder
  WAVs and the Play-mode audio driver through mixer groups (`audio.rs`); CPU tests (`tests.rs`).
- `crates/misty_engine/src/render/weather/`: view group bindings 24/25 (globals + top-down
  occlusion depth map), occlusion pass after the shadow maps, splash compute from the scene-copy
  depth, composite after TAA into the HDR image (haze, far sheets, storm curtains, mist, sky flash,
  procedural GPU rain streaks with refraction, snow flakes with DoF/tumble/swirl, splashes, bolts
  (+ glow image for bloom), lens drops), sun/sky dimming of the lights uniform.
  Lit-shader hook: `weather_surface` in `mesh.wgsl` `shade()`, `weather_foliage` in
  `shade_grass` / `shade_tree`. Quality ▸ `weather: Tier` (counts, splash slots, occlusion size).
- Lua `Weather` table (`scripting/weather_api.rs`) and events OnLightning / OnThunder /
  OnWeatherChanged as broadcast custom events (graphs + Lua). Runtime updates weather before
  scripts; audio driver after.
- Editor: `weather_preview.rs` (edit-mode tick), `panels/inspector/weather_editors.rs` (Inspector).

## State
- Engine + editor compile; weather tests (22 engine incl. GPU, 2 editor) pass; WeatherDemo
  generated and committed; screenshots in `docs/screenshots/r34/`; golden shots 20/20 within
  tolerance; GPU costs in docs/progress.md (Heavy Rain +0.75 ms, Downpour +1.3 ms, Thunderstorm
  +0.95 ms, Blizzard + snow cover +0.75 ms at 1080p High on the RTX 4060).

## Left to do
- clippy 0 warnings and the full workspace test run (in progress at the time of writing).
- Regenerate docs/api (build misty, `misty.exe --api-docs docs/api`).

## Known limits / next steps
- Terrain trees (the trees renderer) are not in the occlusion map: only mesh shadow casters
  shelter (mesh trees do). Add the tree crowns' shadow pass to the map.
- Area volumes beyond 4 near the camera are left out of per-pixel blending.
- Wetness / snow cover is per volume, not painted per surface (no footprints or drip lines).
- Thunder and rain use generated sounds unless clips are set; no reverb / occlusion of sound
  under roofs. No snow puffs on landing, no under-shelter drips.
- Lightning lights surfaces as an unshadowed sky flash (no shadows from the bolt).
- The weather is skipped while the Frame Debugger captures.
