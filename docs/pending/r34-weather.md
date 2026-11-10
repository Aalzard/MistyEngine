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
- Engine + editor compile clean (`cargo check --release -p misty_engine -p misty_editor --tests`).
- Editor wiring done (Inspector, preview tick, Create menu, MCP kind, Quality row, Lua API table,
  tr/ar strings). WeatherDemo generator written (`sample_weather.rs`), scene not generated yet.
- GPU tests written (`render/weather/gpu_tests.rs`), plus ignored `weather_gpu_timings` and
  `weather_screenshots` (MISTY_WEATHER_SHOTS=name filters).

## Left to do
1. Run the tests: engine `weather` filter (CPU, naga validation, GPU, Lua), then editor
   `write_weather_demo` (ignored, writes the scene/materials/script + metas) and `weather`.
2. GPU timings at 1080p High and screenshots to `docs/screenshots/r34/` (under the
   gpu-timing.lock), tune looks.
3. Golden check under the lock, docs/api regeneration, fmt / clippy / full tests, progress notes
   (timings, screenshots), final commit.
